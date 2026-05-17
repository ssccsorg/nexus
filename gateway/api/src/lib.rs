// gateway-api library crate.
//
// Exports the router builder and shared state so integration tests,
// alternative frontends, and future transports can reuse them.

mod routes;
pub mod state;

use axum::{Router, routing::{get, post}};
use state::AppState;
use tower_http::cors::CorsLayer;

/// Build the versioned axum Router with all FIH endpoints.
///
/// Exported so the same router can be embedded in other binaries
/// (e.g., test harnesses, alternative frontends, WASM workers).
pub fn build_router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/facts", post(routes::submit_fact))
        .route("/state", get(routes::read_state))
        .route("/intents", post(routes::submit_intent))
        .route("/intents/{id}/claim", post(routes::claim_intent))
        .route("/intents/{id}/heartbeat", post(routes::heartbeat_intent))
        .route("/intents/{id}/release", post(routes::release_intent))
        .route("/intents/{id}/conclude", post(routes::conclude_intent))
        .route("/hints", post(routes::submit_hint));

    Router::new()
        .nest("/api/v1/fih", v1)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
