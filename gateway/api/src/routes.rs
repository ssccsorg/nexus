// HTTP route handlers for the FIH Blackboard API.
//
// Each handler maps one HTTP endpoint to a Blackboard trait method.
// Errors from the Blackboard are mapped to appropriate HTTP status codes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use nexus_graph::{BlackboardError, Fact, FihHash, Hint, Intent};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

// ── Error handling ───────────────────────────────────────────────────────

/// API error response body.
#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
    pub detail: String,
}

/// Map a BlackboardError to HTTP status code + JSON error body.
fn err_response(e: BlackboardError) -> (StatusCode, Json<ApiError>) {
    let (code, label) = match &e {
        BlackboardError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        BlackboardError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
        BlackboardError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
        BlackboardError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    (
        code,
        Json(ApiError {
            error: label.into(),
            detail: e.to_string(),
        }),
    )
}

// ── Request / Response types ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SubmitFactRequest {
    pub id: Option<String>,
    pub origin: String,
    pub content: serde_json::Value,
    pub creator: String,
}

#[derive(Serialize)]
pub struct SubmitFactResponse {
    pub id: String,
}

#[derive(Deserialize)]
pub struct SubmitIntentRequest {
    pub id: Option<String>,
    pub from_facts: Vec<String>,
    pub description: String,
    pub creator: String,
}

#[derive(Serialize)]
pub struct SubmitIntentResponse {
    pub id: String,
}

#[derive(Deserialize)]
pub struct ClaimRequest {
    pub agent: String,
}

#[derive(Deserialize)]
pub struct HeartbeatRequest {
    pub agent: String,
}

#[derive(Deserialize)]
pub struct ReleaseRequest {
    pub agent: String,
}

#[derive(Deserialize)]
pub struct ConcludeRequest {
    pub result: serde_json::Value,
}

#[derive(Serialize)]
pub struct ConcludeResponse {
    pub fact: Fact,
}

#[derive(Deserialize)]
pub struct SubmitHintRequest {
    pub id: Option<String>,
    pub content: String,
    pub creator: String,
}

// ── Handlers ─────────────────────────────────────────────────────────────

/// POST /fih/facts
pub async fn submit_fact(
    State(state): State<AppState>,
    Json(req): Json<SubmitFactRequest>,
) -> Result<Json<SubmitFactResponse>, (StatusCode, Json<ApiError>)> {
    let id = req.id.unwrap_or_else(|| format!("fact_{}", uuid_v4()));
    let fact = Fact {
        id: FihHash(id.clone()),
        origin: req.origin,
        content: req.content.into(),
        creator: req.creator,
    };
    let hash = {
        let mut bb = state.blackboard.lock().unwrap();
        bb.submit_fact(&fact).map_err(err_response)?
    };
    Ok(Json(SubmitFactResponse { id: hash.0 }))
}

/// GET /fih/state
pub async fn read_state(
    State(state): State<AppState>,
) -> Json<nexus_graph::BoardState> {
    let bb = state.blackboard.lock().unwrap();
    let board_state = bb.read_state();
    Json(board_state)
}

/// POST /fih/intents
pub async fn submit_intent(
    State(state): State<AppState>,
    Json(req): Json<SubmitIntentRequest>,
) -> Result<Json<SubmitIntentResponse>, (StatusCode, Json<ApiError>)> {
    let id = req.id.unwrap_or_else(|| format!("intent_{}", uuid_v4()));
    if req.from_facts.is_empty() {
        return Err(err_response(BlackboardError::Forbidden(
            "intent must be grounded in at least one fact".into(),
        )));
    }
    let intent = Intent {
        id: FihHash(id.clone()),
        from_facts: req.from_facts,
        description: req.description,
        creator: req.creator,
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    };
    let hash = {
        let mut bb = state.blackboard.lock().unwrap();
        bb.submit_intent(&intent).map_err(err_response)?
    };
    Ok(Json(SubmitIntentResponse { id: hash.0 }))
}

/// POST /fih/intents/{id}/claim
pub async fn claim_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(req): Json<ClaimRequest>,
) -> Result<Json<()>, (StatusCode, Json<ApiError>)> {
    let mut bb = state.blackboard.lock().unwrap();
    bb.claim_intent(&intent_id, &req.agent)
        .map_err(err_response)?;
    Ok(Json(()))
}

/// POST /fih/intents/{id}/heartbeat
pub async fn heartbeat_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(req): Json<HeartbeatRequest>,
) -> Result<Json<()>, (StatusCode, Json<ApiError>)> {
    let mut bb = state.blackboard.lock().unwrap();
    bb.heartbeat(&intent_id, &req.agent)
        .map_err(err_response)?;
    Ok(Json(()))
}

/// POST /fih/intents/{id}/release
pub async fn release_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(req): Json<ReleaseRequest>,
) -> Result<Json<()>, (StatusCode, Json<ApiError>)> {
    let mut bb = state.blackboard.lock().unwrap();
    bb.release_intent(&intent_id, &req.agent)
        .map_err(err_response)?;
    Ok(Json(()))
}

/// POST /fih/intents/{id}/conclude
pub async fn conclude_intent(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(req): Json<ConcludeRequest>,
) -> Result<Json<ConcludeResponse>, (StatusCode, Json<ApiError>)> {
    let mut bb = state.blackboard.lock().unwrap();
    let fact = bb
        .conclude_intent(&intent_id, &req.result)
        .map_err(err_response)?;
    Ok(Json(ConcludeResponse {
        fact,
    }))
}

/// POST /fih/hints
pub async fn submit_hint(
    State(state): State<AppState>,
    Json(req): Json<SubmitHintRequest>,
) -> Result<Json<()>, (StatusCode, Json<ApiError>)> {
    let id = req.id.unwrap_or_else(|| format!("hint_{}", uuid_v4()));
    let hint = Hint {
        id: FihHash(id),
        content: req.content,
        creator: req.creator,
    };
    let mut bb = state.blackboard.lock().unwrap();
    bb.submit_hint(&hint)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: "hint_error".into(), detail: e.to_string() })))?;
    Ok(Json(()))
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Generate a v4 UUID string.
fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
