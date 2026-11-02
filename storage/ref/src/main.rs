// nexus-storage-ref: the reference implementation of the neXus storage domain.
//
// One volume, one record, one coordinate. No resident index, no database
// replacement. The host and a smaller target derive the same coordinate from
// the same payload bytes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nex::io::fs_io::FsIo;
use nex_fih::{AsyncFactCapable, AsyncFilterCapable, Content, Fact, FihStorage, StateFilter};
use serde::Deserialize;

const PROJECT: &str = "nexus-storage-ref";

#[derive(Parser)]
#[command(
    name = "nexus-storage-ref",
    about = "Reference implementation of the neXus storage domain"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write one record to a volume.
    Write(WriteArgs),
    /// Read one record by coordinate.
    Read(ReadArgs),
    /// List records in a volume.
    Query(QueryArgs),
    /// Recompute a record's coordinate and content hash.
    Verify(VerifyArgs),
    /// Add a volume beside an existing application file.
    Sidecar(SidecarArgs),
    /// Print the host-side coordinate for a record.
    DeviceId(DeviceIdArgs),
    /// Run the full host path in a temporary volume.
    SelfCheck,
}

#[derive(clap::Args)]
struct WriteArgs {
    #[arg(long)]
    volume: PathBuf,
    #[arg(long)]
    record: Option<PathBuf>,
}

#[derive(clap::Args)]
struct ReadArgs {
    #[arg(long)]
    volume: PathBuf,
    #[arg(long)]
    id: String,
}

#[derive(clap::Args)]
struct QueryArgs {
    #[arg(long)]
    volume: PathBuf,
    #[arg(long)]
    origin: Option<String>,
    #[arg(long)]
    creator: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(clap::Args)]
struct VerifyArgs {
    #[arg(long)]
    volume: PathBuf,
    #[arg(long)]
    id: String,
}

#[derive(clap::Args)]
struct SidecarArgs {
    #[arg(long)]
    app: PathBuf,
    #[arg(long)]
    volume: PathBuf,
    #[arg(long)]
    record: Option<PathBuf>,
}

#[derive(clap::Args)]
struct DeviceIdArgs {
    #[arg(long)]
    record: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RecordSpec {
    origin: String,
    creator: String,
    #[serde(default = "default_mime")]
    mime_type: String,
    payload: serde_json::Value,
}

fn default_mime() -> String {
    "application/json".to_string()
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Write(args) => write(args),
        Command::Read(args) => read(args),
        Command::Query(args) => query(args),
        Command::Verify(args) => verify(args),
        Command::Sidecar(args) => sidecar(args),
        Command::DeviceId(args) => device_id(args),
        Command::SelfCheck => self_check(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn open_store(volume: &Path) -> Result<FihStorage<FsIo>, String> {
    fs::create_dir_all(volume).map_err(|e| format!("create volume {}: {e}", volume.display()))?;
    let io = FsIo::new(volume).map_err(|e| format!("open volume {}: {e}", volume.display()))?;
    Ok(FihStorage::new(io, PROJECT))
}

fn default_spec_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples/asset-event.json")
}

fn load_spec(path: &Path) -> Result<RecordSpec, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn fact_from_spec(spec: &RecordSpec) -> Result<Fact, String> {
    let data = serde_json::to_vec(&spec.payload).map_err(|e| format!("serialize payload: {e}"))?;
    let content = Content {
        mime_type: spec.mime_type.clone(),
        data,
    };
    Ok(Fact::new(
        spec.origin.clone(),
        content,
        spec.creator.clone(),
    ))
}

fn write(args: WriteArgs) -> Result<(), String> {
    let path = args.record.unwrap_or_else(default_spec_path);
    let spec = load_spec(&path)?;
    let fact = fact_from_spec(&spec)?;
    let store = open_store(&args.volume)?;
    futures_executor::block_on(async {
        AsyncFactCapable::submit_fact(&store, &fact)
            .await
            .map_err(|e| format!("submit: {e:?}"))?;
        store.flush_pending().await
    })?;
    println!("record:    {}", fact.id);
    println!("hash:      {}", fact.content_hash);
    println!("origin:    {}", fact.origin);
    println!("creator:   {}", fact.creator);
    println!("volume:    {}", args.volume.display());
    Ok(())
}

fn read(args: ReadArgs) -> Result<(), String> {
    let store = open_store(&args.volume)?;
    let record = futures_executor::block_on(async {
        store.rebuild_cache().await?;
        Ok::<_, String>(store.get_fact_by_id(&args.id).await)
    })?;
    let (content, hash, origin, creator) =
        record.ok_or_else(|| format!("record not found: {}", args.id))?;
    println!("record:    {}", args.id);
    println!("hash:      {hash}");
    println!("origin:    {origin}");
    println!("creator:   {creator}");
    println!("mime:      {}", content.mime_type);
    println!("bytes:     {}", content.data.len());
    println!("content:   {content}");
    Ok(())
}

fn query(args: QueryArgs) -> Result<(), String> {
    let store = open_store(&args.volume)?;
    let filter = StateFilter {
        origin: args.origin,
        creator: args.creator,
        limit: args.limit,
        ..Default::default()
    };
    let state = futures_executor::block_on(async {
        store.rebuild_cache().await?;
        Ok::<_, String>(AsyncFilterCapable::read_state_filtered(&store, &filter).await)
    })?;
    println!(
        "{:<24} {:<20} {:<20} {:<18} {:>6}",
        "COORDINATE", "ORIGIN", "CREATOR", "MIME", "BYTES"
    );
    for fact in &state.facts {
        println!(
            "{:<24} {:<20} {:<20} {:<18} {:>6}",
            fact.id.to_string(),
            fact.origin,
            fact.creator,
            fact.content.mime_type,
            fact.content.data.len()
        );
    }
    println!("{} record(s)", state.facts.len());
    Ok(())
}

fn verify(args: VerifyArgs) -> Result<(), String> {
    let store = open_store(&args.volume)?;
    let record = futures_executor::block_on(async {
        store.rebuild_cache().await?;
        Ok::<_, String>(store.get_fact_by_id(&args.id).await)
    })?;
    let (content, hash, origin, creator) =
        record.ok_or_else(|| format!("record not found: {}", args.id))?;
    let rebuilt = Fact::new(origin.clone(), content.clone(), creator.clone());
    let address_ok = rebuilt.id.to_string() == args.id;
    let hash_ok = rebuilt.content_hash == hash;
    println!("record:    {}", args.id);
    println!("hash:      {hash}");
    println!("origin:    {origin}");
    println!("creator:   {creator}");
    println!(
        "address:   {} ({})",
        rebuilt.id,
        if address_ok { "valid" } else { "invalid" }
    );
    println!(
        "content:   {} ({})",
        rebuilt.content_hash,
        if hash_ok { "valid" } else { "invalid" }
    );
    println!(
        "seal:      {}",
        if address_ok && hash_ok {
            "valid"
        } else {
            "invalid"
        }
    );
    if address_ok && hash_ok {
        Ok(())
    } else {
        Err("seal check failed".to_string())
    }
}

fn sidecar(args: SidecarArgs) -> Result<(), String> {
    let metadata =
        fs::metadata(&args.app).map_err(|e| format!("app {}: {e}", args.app.display()))?;
    let path = args.record.unwrap_or_else(default_spec_path);
    let spec = load_spec(&path)?;
    let fact = fact_from_spec(&spec)?;
    let store = open_store(&args.volume)?;
    futures_executor::block_on(async {
        AsyncFactCapable::submit_fact(&store, &fact)
            .await
            .map_err(|e| format!("submit: {e:?}"))?;
        store.flush_pending().await
    })?;
    println!(
        "app:       {} (untouched, {} bytes)",
        args.app.display(),
        metadata.len()
    );
    println!("volume:    {} (added)", args.volume.display());
    println!("record:    {}", fact.id);
    println!("hash:      {}", fact.content_hash);
    Ok(())
}

fn device_id(args: DeviceIdArgs) -> Result<(), String> {
    let path = args.record.unwrap_or_else(default_spec_path);
    let spec = load_spec(&path)?;
    let fact = fact_from_spec(&spec)?;
    println!("record:    {}", fact.id);
    println!("hash:      {}", fact.content_hash);
    println!("origin:    {}", fact.origin);
    println!("creator:   {}", fact.creator);
    println!(
        "note:      run the MCU verifier with the same payload bytes and compare the coordinate"
    );
    Ok(())
}

fn self_check() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("{PROJECT}-{}", std::process::id()));
    fs::create_dir_all(&root).map_err(|e| format!("create {}: {e}", root.display()))?;
    let app = root.join("app.db");
    let volume = root.join("records.vol");
    let spec_path = default_spec_path();
    let result = (|| -> Result<(), String> {
        fs::write(&app, b"existing application state").map_err(|e| format!("write app: {e}"))?;
        let spec = load_spec(&spec_path)?;
        let fact = fact_from_spec(&spec)?;
        {
            let store = open_store(&volume)?;
            futures_executor::block_on(async {
                AsyncFactCapable::submit_fact(&store, &fact)
                    .await
                    .map_err(|e| format!("submit: {e:?}"))?;
                store.flush_pending().await
            })?;
        }
        let store = open_store(&volume)?;
        let (content, hash, origin, creator) = futures_executor::block_on(async {
            store.rebuild_cache().await?;
            Ok::<_, String>(store.get_fact_by_id(&fact.id.to_string()).await)
        })?
        .ok_or_else(|| "record not found after reopen".to_string())?;
        let rebuilt = Fact::new(origin, content, creator);
        if rebuilt.id != fact.id || rebuilt.content_hash != hash {
            return Err("seal check failed after reopen".to_string());
        }
        let app_bytes = fs::read(&app).map_err(|e| format!("read app: {e}"))?;
        if app_bytes != b"existing application state".to_vec() {
            return Err("sidecar changed the app file".to_string());
        }
        println!("self-check: PASS");
        println!("record:     {}", fact.id);
        println!("hash:       {}", fact.content_hash);
        Ok(())
    })();
    let _ = fs::remove_dir_all(&root);
    result
}
