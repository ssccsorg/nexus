// ── nex-hub: the hub as a process ──────────────────────────────────────
//
// Usage:
//   NEX_HUB_SOCKET=/tmp/nex-hub.sock nex-hub

use std::path::PathBuf;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nex_hub=info")),
        )
        .try_init();

    let socket = std::env::var("NEX_HUB_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/nex-hub.sock"));

    nex_hub::serve(&socket, nex_hub::state()).await
}
