// nexus-storage-ref: the smallest consumer of the neXus storage key space.
//
// A volume is read through the storage contract alone: a listing hands over the
// keys, and a record's address is the key its bytes live at. This crate writes
// no record, opens no database, keeps no index, and decodes no record.
//
// A read never creates the volume it reads. A reader that has to tell "the
// record is not here" from "the question was wrong" gets exit code 2 for the
// first and 1 for the second.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use clap::{Parser, Subcommand};
use nex::io::{FileIo, fs_io::FsIo};
use nex_fih::{CoordId, FactRecord};

#[derive(Parser)]
#[command(
    name = "nexus-storage-ref",
    about = "Read a neXus volume through the storage contract"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the keys a prefix holds.
    Keys(KeysArgs),
    /// Read the bytes at the key a record's address implies.
    Read(ReadArgs),
    /// Place a sample volume beside an existing application file.
    Sidecar(SidecarArgs),
    /// Run the read path in a temporary volume.
    SelfCheck,
}

#[derive(clap::Args)]
struct KeysArgs {
    #[arg(long)]
    volume: PathBuf,
    /// The key prefix to keep. An empty prefix lists the whole key space.
    #[arg(long, default_value = "facts/")]
    prefix: String,
}

#[derive(clap::Args)]
struct ReadArgs {
    #[arg(long)]
    volume: PathBuf,
    /// The record address: 20 coordinate characters.
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

/// Why a command did not finish, in the two forms a caller has to tell apart.
enum Failure {
    /// The request or the volume is wrong, so the question was never asked.
    Refused(String),
    /// The question was well formed and the volume holds nothing at that address.
    Missing(String),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Keys(args) => keys(&args.volume, &args.prefix),
        Command::Read(args) => read(&args.volume, &args.address),
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples/volume")
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

fn list_all(io: &FsIo) -> Result<Vec<String>, Failure> {
    futures_executor::block_on(io.list(""))
        .map_err(|e| Failure::Refused(format!("list the volume: {e}")))
}

fn refused<T>(what: &str, result: Result<T, std::io::Error>) -> Result<T, Failure> {
    result.map_err(|e| Failure::Refused(format!("{what}: {e}")))
}

fn keys(volume: &Path, prefix: &str) -> Result<(), Failure> {
    let io = open_volume(volume)?;
    let mut matched = 0;
    for key in list_all(&io)? {
        if key.starts_with(prefix) {
            println!("{key}");
            matched += 1;
        }
    }
    println!("{matched} key(s) under {prefix:?}");
    Ok(())
}

fn read(volume: &Path, address: &str) -> Result<(), Failure> {
    let coordinate = CoordId::<20>::from_string(address).ok_or_else(|| {
        Failure::Refused(format!(
            "{address:?} is not a record address; an address is 20 coordinate characters"
        ))
    })?;
    let io = open_volume(volume)?;
    let key = FactRecord::fact_key(&coordinate.to_string());
    let bytes = futures_executor::block_on(io.read(&key))
        .map_err(|e| Failure::Refused(format!("read {key}: {e}")))?;

    println!("coordinate: {coordinate}");
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

        let io = open_volume(&volume)?;
        let listing = list_all(&io)?;

        // The address is in the key, so the coordinate comes from the listing
        // rather than from a constant, and the key has to follow from it again.
        let records: Vec<&String> = listing
            .iter()
            .filter(|key| key.starts_with("facts/"))
            .collect();
        let key = match records.as_slice() {
            [only] => *only,
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

        let bytes = futures_executor::block_on(io.read(key))
            .map_err(|e| Failure::Refused(format!("read {key}: {e}")))?;
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
        println!("key:        {key}");
        Ok(())
    })();

    let _ = fs::remove_dir_all(&root);
    result
}
