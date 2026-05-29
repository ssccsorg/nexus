// HTTP route handlers for VECompositeStorage.
// Uses IoBufferSession exclusively — no WASM or CF bindings.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use nexus_model::SessionExecute;
use nexus_storage_composite::{BlobStore, KeyValueStore, ObjectStore};

use crate::AppState;

macro_rules! stor {
    ($s:expr) => { $s.session.storage() };
}

// ─── KV ─────────────────────────────────────────────────────────────────

pub async fn kv_get(
    State(s): State<Arc<AppState>>,
    Path((project, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let fk = format!("{project}:{key}");
    match stor!(s).kv().get(&fk) {
        Ok(Some(v)) => Json(serde_json::json!({"value": v})).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn kv_put(
    State(s): State<Arc<AppState>>,
    Path((project, key)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let fk = format!("{project}:{key}");
    let val = body["value"].as_str().unwrap_or("");
    match stor!(s).kv().set(&fk, val) {
        Ok(()) => Json(serde_json::json!({"status":"ok"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn kv_delete(
    State(s): State<Arc<AppState>>,
    Path((project, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let fk = format!("{project}:{key}");
    match stor!(s).kv().delete(&fk) {
        Ok(()) => Json(serde_json::json!({"status":"deleted"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn kv_list(
    State(s): State<Arc<AppState>>,
    Path(project): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let prefix = params.get("prefix").cloned().unwrap_or_default();
    let fk = format!("{project}:{prefix}");
    match stor!(s).kv().list(&fk) {
        Ok(keys) => {
            let strip: Vec<String> = keys.iter().map(|k| k.strip_prefix(&format!("{project}:")).unwrap_or(k).to_string()).collect();
            Json(serde_json::json!({"keys": strip})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

// ─── R2 (Blob) ──────────────────────────────────────────────────────────

pub async fn r2_get(
    State(s): State<Arc<AppState>>,
    Path((project, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let fk = format!("{project}/{key}");
    match stor!(s).blob().get(&fk) {
        Ok(Some(data)) => {
            let b64 = base64_encode(&data);
            Json(serde_json::json!({"data_base64": b64})).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn r2_put(
    State(s): State<Arc<AppState>>,
    Path((project, key)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let fk = format!("{project}/{key}");
    let b64 = body["data_base64"].as_str().unwrap_or("");
    let data = base64_decode(b64).unwrap_or_default();
    match stor!(s).blob().put(&fk, &data) {
        Ok(()) => Json(serde_json::json!({"status":"ok"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn r2_delete(
    State(s): State<Arc<AppState>>,
    Path((project, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let fk = format!("{project}/{key}");
    match stor!(s).blob().delete(&fk) {
        Ok(()) => Json(serde_json::json!({"status":"deleted"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

// ─── DO (CAS) ───────────────────────────────────────────────────────────

pub async fn do_cas(
    State(s): State<Arc<AppState>>,
    Path((project, key)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let fk = format!("{project}:{key}");
    let expected = body["expected"].as_str().unwrap_or("");
    let new_val = body["new"].as_str().unwrap_or("");
    match stor!(s).object().put_state(&fk, expected, new_val) {
        Ok(true) => Json(serde_json::json!({"status":"cas_success"})).into_response(),
        Ok(false) => (StatusCode::CONFLICT, Json(serde_json::json!({"status":"cas_conflict"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

// ─── Base64 ─────────────────────────────────────────────────────────────

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut r = String::new();
    for c in data.chunks(3) {
        let b0 = c[0] as u32;
        let b1 = c.get(1).copied().unwrap_or(0) as u32;
        let b2 = c.get(2).copied().unwrap_or(0) as u32;
        let t = (b0 << 16) | (b1 << 8) | b2;
        r.push(CHARS[((t >> 18) & 0x3F) as usize] as char);
        r.push(CHARS[((t >> 12) & 0x3F) as usize] as char);
        if c.len() > 1 { r.push(CHARS[((t >> 6) & 0x3F) as usize] as char); } else { r.push('='); }
        if c.len() > 2 { r.push(CHARS[(t & 0x3F) as usize] as char); } else { r.push('='); }
    }
    r
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut d = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for c in input.chars() {
        let v = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' => 62, '/' => 63, '=' => break, _ => return None,
        } as u32;
        buf = (buf << 6) | v; bits += 6;
        if bits >= 8 { bits -= 8; d.push((buf >> bits) as u8); buf &= (1 << bits) - 1; }
    }
    Some(d)
}
