// VECompositeStorage — Virtual Emulation of CF Workers KV/R2/DO.
//
// A CI test harness that exposes MetaStore, BlobStore, ObjectStore
// semantics over HTTP, simulating the Cloudflare Workers async I/O boundary.
//
// Not for WASM deployment. For local and CI testing only.
//
// Architecture:
//
//   HTTP Client (integration test)
//     ↓ async HTTP
//   VECompositeStorage Server
//     ├── /meta/{key}      GET/PUT  (meta store — cursor, snapshot pointers)
//     ├── /r2/{project}/{key}   PUT/GET/DELETE  (R2)
//     └── /do/{project}/{key}/cas POST          (DO CAS)
//
// Every request goes through IoBufferSession.

mod routes;

use std::sync::Arc;

use axum::Router;
use nexus_storage_composite::IoBufferSession;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

pub struct AppState {
    pub session: IoBufferSession,
}

/// Start VE server on random port, return (base_url, task_handle).
pub async fn start_ve() -> (String, tokio::task::JoinHandle<()>) {
    let session = IoBufferSession::new("ve-test");
    start_ve_with(session).await
}

pub async fn start_ve_with(session: IoBufferSession) -> (String, tokio::task::JoinHandle<()>) {
    let state = Arc::new(AppState { session });

    let app = Router::new()
        .route("/meta/{key}",
            axum::routing::get(routes::meta_get).put(routes::meta_set))
        .route("/r2/{project}/{key}",
            axum::routing::get(routes::r2_get).put(routes::r2_put).delete(routes::r2_delete))
        .route("/do/{project}/{key}/cas", axum::routing::post(routes::do_cas))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind VE");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (url, handle)
}
