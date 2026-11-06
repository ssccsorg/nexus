// nexus-storage-ref: the smallest consumer of the neXus storage key space.
//
// A volume is read through the storage contract: a listing hands over the keys,
// a record's address is the key its bytes live at, and a question over several
// axes is answered from the keys without reading a record.
//
// A read never creates the volume it reads. A reader that has to tell "the
// record is not here" from "the question was wrong" gets exit code 2 for the
// first and 1 for the second.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use clap::{Parser, Subcommand};
use nex_client::NexClient;
use nex_fih::{Content, CoordId, Fact};
use nex_io::FileIo;
use nex_io::fs_io::FsIo;
use nexus_storage_ref::FactRecord;
use std::cell::RefCell;
use tagma_core::DIGITS_PER_AXIS;
use tokio::runtime::Runtime;

/// Keys one listing call hands over.
const KEY_PAGE: usize = 32;

#[derive(Parser)]
#[command(
    name = "nexus-storage-ref",
    about = "Read a neXus volume through the storage contract"
)]
struct Cli {
    /// Read through the hub at this socket instead of opening a volume.
    #[arg(long, env = "NEX_HUB_SOCKET", global = true)]
    hub: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the keys a prefix holds.
    Keys(KeysArgs),
    /// List the records a box over coordinate axes admits.
    Box(BoxArgs),
    /// Read the bytes at the key a record's address implies.
    Read(ReadArgs),
    /// Recompute a record's address from what it holds.
    Verify(VerifyArgs),
    /// Place a sample volume beside an existing application file.
    Sidecar(SidecarArgs),
    /// Run the read path in a temporary volume.
    SelfCheck,
}

#[derive(clap::Args)]
struct KeysArgs {
    #[arg(long)]
    volume: Option<PathBuf>,
    /// The key prefix to keep. An empty prefix lists the whole key space.
    #[arg(long, default_value = "facts/")]
    prefix: String,
}

#[derive(clap::Args)]
struct BoxArgs {
    #[arg(long)]
    volume: Option<PathBuf>,
    /// An axis range, as `<axis>=<from>-<until>` with one axis window a bound.
    /// An axis the box does not name is free.
    #[arg(long = "axis", value_parser = parse_axis)]
    axes: Vec<Axis>,
}

#[derive(clap::Args)]
struct ReadArgs {
    #[arg(long)]
    volume: Option<PathBuf>,
    /// The record address: 60 base-36 digits, three an axis.
    #[arg(long)]
    address: String,
}

#[derive(clap::Args)]
struct VerifyArgs {
    #[arg(long)]
    volume: Option<PathBuf>,
    /// The record address, as 60 base-36 digits.
    #[arg(long)]
    address: String,
}

#[derive(clap::Args)]
struct SidecarArgs {
    #[arg(long)]
    app: PathBuf,
    #[arg(long)]
    volume: PathBuf,
}

/// One axis constrained to the axis windows it admits, inclusive.
///
/// A bound is the fixed-width text of one axis value. The coordinate being tested
/// is in the same form, so a bound and the text it is compared against are the
/// same width.
#[derive(Clone)]
struct Axis {
    at: usize,
    from: String,
    until: String,
}

impl Axis {
    fn admits(&self, coordinate: &str) -> bool {
        let width = self.from.chars().count();
        let value: String = coordinate
            .chars()
            .skip(self.at * width)
            .take(width)
            .collect();
        value.chars().count() == width && value >= self.from && value <= self.until
    }
}

fn parse_axis(text: &str) -> Result<Axis, String> {
    let (at, range) = text
        .split_once('=')
        .ok_or_else(|| format!("{text}: expected <axis>=<from>-<until>"))?;
    let (from, until) = range
        .split_once('-')
        .ok_or_else(|| format!("{text}: expected <from>-<until>"))?;
    let at: usize = at
        .parse()
        .map_err(|_| format!("{text}: the axis is a number"))?;
    let from = bound(text, from)?;
    let until = bound(text, until)?;
    if from.chars().count() != until.chars().count() {
        return Err(format!("{text}: the bounds are not the same width"));
    }
    if from > until {
        return Err(format!("{text}: the lower bound is above the upper one"));
    }
    Ok(Axis { at, from, until })
}

fn bound(text: &str, part: &str) -> Result<String, String> {
    if part.chars().count() == DIGITS_PER_AXIS {
        Ok(part.to_string())
    } else {
        Err(format!(
            "{text}: a bound is {DIGITS_PER_AXIS} base-36 digits, one axis window"
        ))
    }
}

/// Why a command did not finish, in the two forms a caller has to tell apart.
enum Failure {
    /// The request or the volume is wrong, so the question was never asked.
    Refused(String),
    /// The question was well formed and the volume holds nothing at that address.
    Missing(String),
}

fn main() -> ExitCode {
    // A usage error is a refused question, not an absent record, so it must not
    // land on exit 2. `--help` and `--version` are not failures.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let asked_for_help = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            let _ = error.print();
            return if asked_for_help {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
    };
    let result = match cli.command {
        Command::Keys(args) => open_source(args.volume.as_deref(), cli.hub.as_deref())
            .and_then(|source| keys(&source, &args.prefix)),
        Command::Box(args) => open_source(args.volume.as_deref(), cli.hub.as_deref())
            .and_then(|source| boxed(&source, &args.axes)),
        Command::Read(args) => resolve_address(&args.address).and_then(|coordinate| {
            open_source(args.volume.as_deref(), cli.hub.as_deref())
                .and_then(|source| read(&source, &coordinate))
        }),
        Command::Verify(args) => resolve_address(&args.address).and_then(|coordinate| {
            open_source(args.volume.as_deref(), cli.hub.as_deref())
                .and_then(|source| verify(&source, &coordinate))
        }),
        Command::Sidecar(args) => sidecar(&args),
        Command::SelfCheck => self_check(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Refused(message)) => {
            eprintln!("refused: {message}");
            ExitCode::FAILURE
        }
        Err(Failure::Missing(message)) => {
            eprintln!("absent: {message}");
            ExitCode::from(2)
        }
    }
}

fn sample_volume() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo/volume")
}

/// Where the bytes come from: a volume this process opens, or a node reached through the hub.
///
/// The reader asks either one the same two questions, which is why a box and an address
/// check are answered the same way over a socket as over a directory.
enum Source {
    Volume(FsIo),
    Hub {
        runtime: Runtime,
        client: RefCell<NexClient>,
    },
}

impl Source {
    fn list(&self, prefix: &str) -> Result<Vec<String>, Failure> {
        match self {
            Source::Volume(io) => {
                let mut keys = Vec::new();
                let mut cursor: Option<Vec<u8>> = None;
                loop {
                    let (page, next) = futures_executor::block_on(io.list_page(
                        prefix,
                        cursor.as_deref(),
                        KEY_PAGE,
                    ))
                    .map_err(|e| Failure::Refused(format!("list {prefix}: {e}")))?;
                    keys.extend(page);
                    match next {
                        Some(next) => cursor = Some(next),
                        None => return Ok(keys),
                    }
                }
            }
            Source::Hub { runtime, client } => {
                let value = runtime
                    .block_on(async {
                        client
                            .borrow_mut()
                            .call("storage.keys", serde_json::json!({ "prefix": prefix }))
                            .await
                    })
                    .map_err(|e| Failure::Refused(format!("the hub: {e}")))?;
                Ok(value
                    .get("keys")
                    .and_then(|keys| keys.as_array())
                    .map(|keys| {
                        keys.iter()
                            .filter_map(|key| key.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default())
            }
        }
    }

    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, Failure> {
        match self {
            Source::Volume(io) => futures_executor::block_on(io.read(key))
                .map_err(|e| Failure::Refused(format!("read {key}: {e}"))),
            Source::Hub { runtime, client } => {
                let value = runtime
                    .block_on(async {
                        client
                            .borrow_mut()
                            .call("storage.read", serde_json::json!({ "key": key }))
                            .await
                    })
                    .map_err(|e| Failure::Refused(format!("the hub: {e}")))?;
                match value.get("data") {
                    Some(serde_json::Value::String(hex)) => Ok(Some(from_hex(hex)?)),
                    _ => Ok(None),
                }
            }
        }
    }
}

fn from_hex(text: &str) -> Result<Vec<u8>, Failure> {
    if !text.len().is_multiple_of(2) {
        return Err(Failure::Refused("a hex frame has an even length".into()));
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        let text = std::str::from_utf8(pair)
            .map_err(|_| Failure::Refused("a hex frame is text".into()))?;
        bytes.push(
            u8::from_str_radix(text, 16)
                .map_err(|_| Failure::Refused(format!("{text:?} is not hex")))?,
        );
    }
    Ok(bytes)
}

/// Open the volume a subcommand names, or reach the node a hub names.
fn open_source(volume: Option<&Path>, hub: Option<&str>) -> Result<Source, Failure> {
    if let Some(socket) = hub {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Failure::Refused(format!("start a runtime for the hub: {e}")))?;
        let client = runtime
            .block_on(NexClient::connect(socket))
            .map_err(|e| Failure::Refused(format!("connect the hub at {socket}: {e}")))?;
        return Ok(Source::Hub {
            runtime,
            client: RefCell::new(client),
        });
    }
    let volume = volume
        .ok_or_else(|| Failure::Refused("a volume is required unless --hub names a node".into()))?;
    Ok(Source::Volume(open_volume(volume)?))
}

/// Open a volume that has to exist. A read creates nothing.
fn open_volume(volume: &Path) -> Result<FsIo, Failure> {
    if !volume.is_dir() {
        return Err(Failure::Refused(format!(
            "{} is not a directory; a read opens a volume and does not create one",
            volume.display()
        )));
    }
    FsIo::new(volume)
        .map_err(|e| Failure::Refused(format!("open volume {}: {e}", volume.display())))
}

/// Visit the keys the source names.
fn for_each_key(source: &Source, mut visit: impl FnMut(&str) -> bool) -> Result<(), Failure> {
    for key in source.list("")? {
        if !visit(&key) {
            return Ok(());
        }
    }
    Ok(())
}

/// The coordinate a record key carries, or nothing when the key is not a record's.
fn record_coordinate(key: &str) -> Option<&str> {
    key.strip_prefix("facts/f_")?.strip_suffix(".fact")
}

fn refused<T>(what: &str, result: Result<T, std::io::Error>) -> Result<T, Failure> {
    result.map_err(|e| Failure::Refused(format!("{what}: {e}")))
}

fn keys(source: &Source, prefix: &str) -> Result<(), Failure> {
    let mut matched = 0;
    for_each_key(source, |key| {
        if key.starts_with(prefix) {
            println!("{key}");
            matched += 1;
        }
        true
    })?;
    println!("{matched} key(s) under {prefix:?}");
    Ok(())
}

fn boxed(source: &Source, axes: &[Axis]) -> Result<(), Failure> {
    let mut matched = 0;
    // The coordinate is in the key, so the whole box is answered by the listing
    // and no record is read.
    for_each_key(source, |key| {
        let Some(coordinate) = record_coordinate(key) else {
            return true;
        };
        if axes.iter().all(|axis| axis.admits(coordinate)) {
            println!("{coordinate}");
            matched += 1;
        }
        true
    })?;
    println!("{matched} record(s) in the box");
    Ok(())
}

fn read(source: &Source, coordinate: &CoordId<20>) -> Result<(), Failure> {
    let native = coordinate.to_string();
    let key = FactRecord::fact_key(&native);
    let bytes = source.read(&key)?;

    println!("coordinate: {native}");
    println!("key:        {key}");
    match bytes {
        Some(bytes) => {
            println!("bytes:      {}", bytes.len());
            println!("present:    yes");
            Ok(())
        }
        None => Err(Failure::Missing(format!("{key} holds no record"))),
    }
}

/// The coordinate an address names.
///
/// The text names the sequence or it names nothing: a length that is not the
/// depth's and a byte outside the alphabet are both refused, so a caller cannot
/// address a record by a spelling of it.
fn resolve(address: &str) -> Option<CoordId<20>> {
    CoordId::<20>::from_string(address)
}

/// The coordinate an address names, refused before a volume or a hub is opened.
fn resolve_address(address: &str) -> Result<CoordId<20>, Failure> {
    resolve(address).ok_or_else(|| {
        Failure::Refused(format!(
            "{address:?} is not a record address; an address is {DIGITS_PER_AXIS} base-36 digits an axis, twenty axes"
        ))
    })
}

fn verify(source: &Source, coordinate: &CoordId<20>) -> Result<(), Failure> {
    let name = coordinate.to_string();

    // The address implies the key, so the record is one read and the check needs nothing
    // that trusts the medium.
    let record_key = FactRecord::fact_key(&name);
    let bytes = source
        .read(&record_key)?
        .ok_or_else(|| Failure::Missing(format!("the volume holds no record at {coordinate}")))?;
    let framing: FactRecord = postcard::from_bytes(&bytes)
        .map_err(|e| Failure::Refused(format!("{record_key} is not a fact record: {e}")))?;
    let payload = source
        .read(&framing.blob_key())?
        .ok_or_else(|| Failure::Refused("the record's payload is missing".to_string()))?;

    // The address follows from what the record holds, so holding the record is
    // enough to check it. Nothing above this trusts the storage.
    // The media type does not enter the address: `Fact::new` hashes the payload
    // alone, so a record written under another type still verifies here.
    let rebuilt = Fact::new(
        framing.origin.clone(),
        Content {
            mime_type: "application/octet-stream".to_string(),
            data: payload.clone(),
        },
        framing.creator.clone(),
    );
    let address_ok = rebuilt.id.to_string() == framing.id;

    println!("coordinate: {name}");
    println!("key:        {}", FactRecord::fact_key(&name));
    println!("origin:     {}", framing.origin);
    println!("creator:    {}", framing.creator);
    println!("written:    {} ns", framing.submitted_at);
    println!("payload:    {} bytes", payload.len());
    println!(
        "address:    {}",
        if address_ok {
            "valid, recomputed from the origin, the creator, and the payload"
        } else {
            "invalid, the record does not derive the address it is stored at"
        }
    );

    if address_ok {
        Ok(())
    } else {
        Err(Failure::Refused(
            "the address does not follow from the record".to_string(),
        ))
    }
}

fn sidecar(args: &SidecarArgs) -> Result<(), Failure> {
    let before = fingerprint(&args.app)?;

    let existed = args.volume.exists();
    if existed {
        let mut entries = refused(
            &format!("inspect {}", args.volume.display()),
            fs::read_dir(&args.volume),
        )?;
        if entries.next().is_some() {
            return Err(Failure::Refused(format!(
                "{} already holds content; the sample volume is placed in a new directory",
                args.volume.display()
            )));
        }
    }

    copy_volume(&sample_volume(), &args.volume)?;

    let after = fingerprint(&args.app)?;
    if after != before {
        return Err(Failure::Refused(format!(
            "{} changed while the volume was placed",
            args.app.display()
        )));
    }

    println!(
        "app:       {} (unchanged, {} bytes and modification time)",
        args.app.display(),
        before.0
    );
    println!(
        "volume:    {} ({})",
        args.volume.display(),
        if existed {
            "filled an empty directory"
        } else {
            "created"
        }
    );
    Ok(())
}

/// What a copy must not change, read without loading a file that may be large.
fn fingerprint(path: &Path) -> Result<(u64, Option<SystemTime>), Failure> {
    let metadata = refused(&format!("app {}", path.display()), fs::metadata(path))?;
    Ok((metadata.len(), metadata.modified().ok()))
}

fn copy_volume(from: &Path, to: &Path) -> Result<(), Failure> {
    // A volume's top level may hold a file, so the destination cannot wait for a
    // nested entry to create it.
    refused(&format!("create {}", to.display()), fs::create_dir_all(to))?;
    for entry in refused(&format!("read {}", from.display()), fs::read_dir(from))? {
        let entry = refused(&format!("read {}", from.display()), entry)?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            refused(
                &format!("create {}", target.display()),
                fs::create_dir_all(&target),
            )?;
            copy_volume(&source, &target)?;
        } else {
            refused(
                &format!("copy {}", source.display()),
                fs::copy(&source, &target),
            )?;
        }
    }
    Ok(())
}

fn self_check() -> Result<(), Failure> {
    let root = std::env::temp_dir().join(format!("nexus-storage-ref-{}", std::process::id()));
    // A run that was killed leaves its directory behind, and a container can
    // reuse the pid, so the check starts from an empty one every time.
    let _ = fs::remove_dir_all(&root);
    refused(
        &format!("create {}", root.display()),
        fs::create_dir_all(&root),
    )?;
    let app = root.join("app.db");
    let volume = root.join("records.vol");

    let result = (|| -> Result<(), Failure> {
        const APP: &[u8] = b"existing application state";
        refused(&format!("write {}", app.display()), fs::write(&app, APP))?;
        copy_volume(&sample_volume(), &volume)?;

        let source = Source::Volume(open_volume(&volume)?);

        // The address is in the key, so the coordinate comes from the listing
        // rather than from a constant, and the key has to follow from it again.
        let mut records: Vec<String> = Vec::new();
        for_each_key(&source, |key| {
            if key.starts_with("facts/") {
                records.push(key.to_string());
            }
            true
        })?;
        let key = match records.as_slice() {
            [only] => only,
            other => {
                return Err(Failure::Refused(format!(
                    "the sample volume holds {} record keys; the check expects one",
                    other.len()
                )));
            }
        };
        let coordinate = key
            .strip_prefix("facts/f_")
            .and_then(|rest| rest.strip_suffix(".fact"))
            .ok_or_else(|| Failure::Refused(format!("{key} is not a record key")))?;
        if FactRecord::fact_key(coordinate) != *key {
            return Err(Failure::Refused(
                "the key does not follow from the address".to_string(),
            ));
        }

        // The box is answered from the keys, so the record's own coordinate has
        // to select it.
        let own_axis = coordinate
            .chars()
            .next()
            .ok_or_else(|| Failure::Refused("the sample record has no coordinate".to_string()))?;
        let mut found = 0;
        for_each_key(&source, |key| {
            let Some(other) = record_coordinate(key) else {
                return true;
            };
            if other.starts_with(own_axis) {
                found += 1;
            }
            true
        })?;
        if found != 1 {
            return Err(Failure::Refused(format!(
                "the record's own axis selected {found} records"
            )));
        }

        let bytes = source.read(key)?;
        if bytes.is_none() {
            return Err(Failure::Refused(
                "the key the address implies holds no bytes".to_string(),
            ));
        }

        let after = refused(&format!("read {}", app.display()), fs::read(&app))?;
        if after != APP {
            return Err(Failure::Refused(
                "the volume changed the app file".to_string(),
            ));
        }

        println!("self-check: PASS");
        println!("coordinate: {coordinate}");
        println!("key:        {}", FactRecord::fact_key(coordinate));
        Ok(())
    })();

    let _ = fs::remove_dir_all(&root);
    result
}
