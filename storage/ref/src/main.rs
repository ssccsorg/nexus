// nexus-storage-ref: the smallest consumer of the neXus storage key space.
//
// A volume is read through the storage contract alone: a listing hands over the
// keys, and a record's address is the key its bytes live at. This crate writes
// no record, opens no database, keeps no index, and decodes no record.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nex::io::{FileIo, fs_io::FsIo};
use nex_fih::FactRecord;

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
    /// Add a sample volume beside an existing application file.
    Sidecar(SidecarArgs),
    /// Run the read path in a temporary volume.
    SelfCheck,
}

#[derive(clap::Args)]
struct KeysArgs {
    #[arg(long)]
    volume: PathBuf,
    /// The key prefix to list. An empty prefix lists the whole key space.
    #[arg(long, default_value = "facts/")]
    prefix: String,
}

#[derive(clap::Args)]
struct ReadArgs {
    #[arg(long)]
    volume: PathBuf,
    /// The record address.
    #[arg(long)]
    coordinate: String,
}

#[derive(clap::Args)]
struct SidecarArgs {
    #[arg(long)]
    app: PathBuf,
    #[arg(long)]
    volume: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Keys(args) => keys(&args.volume, &args.prefix),
        Command::Read(args) => read(&args.volume, &args.coordinate),
        Command::Sidecar(args) => sidecar(&args),
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

fn sample_volume() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples/volume")
}

fn open_io(volume: &Path) -> Result<FsIo, String> {
    FsIo::new(volume).map_err(|e| format!("open volume {}: {e}", volume.display()))
}

fn keys(volume: &Path, prefix: &str) -> Result<(), String> {
    let io = open_io(volume)?;
    let listing = futures_executor::block_on(io.list(prefix))?;
    for key in &listing {
        println!("{key}");
    }
    println!("{} key(s) under {:?}", listing.len(), prefix);
    Ok(())
}

fn read(volume: &Path, coordinate: &str) -> Result<(), String> {
    let io = open_io(volume)?;
    let key = FactRecord::fact_key(coordinate);
    let bytes = futures_executor::block_on(io.read(&key))?;
    println!("coordinate: {coordinate}");
    println!("key:        {key}");
    match bytes {
        Some(bytes) => {
            println!("bytes:      {}", bytes.len());
            println!("present:    yes");
            Ok(())
        }
        None => {
            println!("bytes:      0");
            println!("present:    no");
            Err(format!("no bytes at {key}"))
        }
    }
}

fn sidecar(args: &SidecarArgs) -> Result<(), String> {
    let metadata =
        fs::metadata(&args.app).map_err(|e| format!("app {}: {e}", args.app.display()))?;
    copy_volume(&sample_volume(), &args.volume)?;
    println!(
        "app:       {} (untouched, {} bytes)",
        args.app.display(),
        metadata.len()
    );
    println!("volume:    {} (added)", args.volume.display());
    Ok(())
}

fn copy_volume(from: &Path, to: &Path) -> Result<(), String> {
    for entry in fs::read_dir(from).map_err(|e| format!("read {}: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("read {}: {e}", from.display()))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("create {}: {e}", target.display()))?;
            copy_volume(&source, &target)?;
        } else {
            fs::copy(&source, &target).map_err(|e| format!("copy {}: {e}", source.display()))?;
        }
    }
    Ok(())
}

fn self_check() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("nexus-storage-ref-{}", std::process::id()));
    fs::create_dir_all(&root).map_err(|e| format!("create {}: {e}", root.display()))?;
    let app = root.join("app.db");
    let volume = root.join("records.vol");
    let result = (|| -> Result<(), String> {
        fs::write(&app, b"existing application state").map_err(|e| format!("write app: {e}"))?;
        copy_volume(&sample_volume(), &volume)?;

        let coordinate = "가뛈튋엀춣덞긗폟뮶윙첌췠뭻궖뺨감춑꼊쐝끒";
        let io = open_io(&volume)?;
        let listing = futures_executor::block_on(io.list("facts/"))?;
        if !listing.contains(&FactRecord::fact_key(coordinate)) {
            return Err("the listing does not hold the record key".to_string());
        }

        let bytes = futures_executor::block_on(io.read(&FactRecord::fact_key(coordinate)))?;
        if bytes.is_none() {
            return Err("the key the address implies holds no bytes".to_string());
        }

        let app_bytes = fs::read(&app).map_err(|e| format!("read app: {e}"))?;
        if app_bytes != b"existing application state" {
            return Err("the volume changed the app file".to_string());
        }

        println!("self-check: PASS");
        Ok(())
    })();
    let _ = fs::remove_dir_all(&root);
    result
}
