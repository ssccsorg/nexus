// gateway-api binary entry point.
//
// Thin wrapper around the library crate. Starts an axum HTTP server
// with the FIH Blackboard endpoints.
//
// Usage:
//   cargo run                    # in-memory
//   cargo run -- --db data.db    # SQLite persistence

use nexus_gateway_api::build_router;
use nexus_gateway_api::state::AppState;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("nexus_gateway_api=info")),
        )
        .init();

    let state = match std::env::args().nth(1).as_deref() {
        Some("--db") => {
            let path = std::env::args()
                .nth(2)
                .expect("--db requires a path argument");
            AppState::with_sqlite(&path).expect("failed to open SQLite database")
        }
        _ => AppState::in_memory(),
    };

    let app = build_router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("gateway-api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app)
        .await
        .expect("server error");
}
