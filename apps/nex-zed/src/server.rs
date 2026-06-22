// HTTP API server — axum-based REST endpoints

use std::sync::Arc;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::zed::ZedManager;

// ── App State ──────────────────────────────────────────────────────────

pub struct AppState {
    pub zed_manager: Arc<RwLock<ZedManager>>,
}

impl AppState {
    pub fn new(zed_manager: Arc<RwLock<ZedManager>>) -> Self {
        Self { zed_manager }
    }
}

type SharedState = Arc<AppState>;

// ── Models ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub zed_connected: bool,
    pub agent_ready: bool,
    pub active_threads: usize,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub thread_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub require_approval: bool,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub task_id: String,
    pub status: String,
    pub thread_id: String,
}

#[derive(Serialize)]
#[allow(dead_code)]
pub struct TaskStatus {
    pub id: String,
    pub status: String,
    pub thread_id: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ThreadListResponse {
    pub threads: Vec<ThreadSummary>,
}

#[derive(Serialize)]
pub struct ThreadSummary {
    pub id: String,
    pub message_count: usize,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ThreadDetailResponse {
    pub id: String,
    pub messages: Vec<serde_json::Value>,
    pub created_at: String,
}

// ── Handlers ───────────────────────────────────────────────────────────

async fn health(State(state): State<SharedState>) -> Json<HealthResponse> {
    let mgr = state.zed_manager.read().await;
    Json(HealthResponse {
        status: "ok".to_string(),
        zed_connected: mgr.zed_connected,
        agent_ready: mgr.agent_ready,
        active_threads: mgr.threads.len(),
    })
}

async fn chat_async(
    State(state): State<SharedState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let mut mgr = state.zed_manager.write().await;

    if !mgr.zed_connected || !mgr.agent_ready {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    let thread_id = mgr.get_or_create_thread(req.thread_id.as_deref());
    mgr.add_message(&thread_id, "user", &req.message, None);

    let request_id = uuid::Uuid::new_v4().to_string();

    // Build WebSocket command
    let cmd = serde_json::json!({
        "type": "chat_message",
        "data": {
            "message": req.message,
            "request_id": request_id,
            "acp_thread_id": null,  // Zed will create/use thread internally
        }
    });

    // Store pending request mapping
    mgr.pending_requests.insert(request_id.clone(), thread_id.clone());

    // Drop the write lock before sending (send_ws_command acquires lock again)
    drop(mgr);

    send_ws_command(&state.zed_manager, &cmd.to_string()).await?;

    Ok(Json(ChatResponse {
        task_id: uuid::Uuid::new_v4().to_string(),
        status: "approved".to_string(),
        thread_id,
    }))
}

async fn list_threads(
    State(state): State<SharedState>,
) -> Json<ThreadListResponse> {
    let mgr = state.zed_manager.read().await;
    let threads = mgr
        .threads
        .values()
        .map(|t| ThreadSummary {
            id: t.id.clone(),
            message_count: t.messages.len(),
            created_at: t.created_at.to_rfc3339(),
        })
        .collect();
    Json(ThreadListResponse { threads })
}

async fn get_thread(
    State(state): State<SharedState>,
    axum::extract::Path(thread_id): axum::extract::Path<String>,
) -> Result<Json<ThreadDetailResponse>, StatusCode> {
    let mgr = state.zed_manager.read().await;
    match mgr.threads.get(&thread_id) {
        Some(thread) => {
            let messages: Vec<serde_json::Value> = thread
                .messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "role": m.role,
                        "content": m.content,
                        "message_id": m.message_id,
                        "timestamp": m.timestamp.to_rfc3339(),
                    })
                })
                .collect();
            Ok(Json(ThreadDetailResponse {
                id: thread.id.clone(),
                messages,
                created_at: thread.created_at.to_rfc3339(),
            }))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

// ── WebSocket command sender ───────────────────────────────────────────

async fn send_ws_command(
    zed_manager: &Arc<RwLock<ZedManager>>,
    cmd: &str,
) -> Result<(), StatusCode> {
    let mgr = zed_manager.read().await;
    mgr.send_command(cmd).map_err(|e| {
        tracing::error!("Failed to send WS command: {}", e);
        StatusCode::SERVICE_UNAVAILABLE
    })
}

// ── Router ─────────────────────────────────────────────────────────────

pub async fn run_http_server(addr: &str, state: SharedState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/chat/async", post(chat_async))
        .route("/v1/threads", get(list_threads))
        .route("/v1/threads/{thread_id}", get(get_thread))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HTTP API server listening on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
