// gateway-api binary entry point.
//
// Thin wrapper around the library crate. Starts an axum HTTP server
// with the FIH Blackboard endpoints.
//
// Usage:
//   cargo run                    # in-memory

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

    let state = AppState::in_memory();

    let app = build_router(state);

    let port: u16 = std::env::var("GATEWAY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(30922);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("gateway-api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app).await.expect("server error");
}
