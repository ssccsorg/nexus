// Zed manager — WebSocket connection, session management, and settings bootstrap

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Manages a single Zed WebSocket connection and message dispatch.
#[allow(dead_code)]
pub struct ZedManager {
    pub session_id: String,
    pub ws_host: String,
    pub zed_connected: bool,
    pub agent_ready: bool,
    /// Channel to send WebSocket messages to Zed
    pub ws_tx: Option<mpsc::UnboundedSender<String>>,
    /// Threads managed by this Zed instance
    pub threads: HashMap<String, ThreadSession>,
    /// Mapping from request_id to acp_thread_id (for correlating responses)
    pub pending_requests: HashMap<String, String>,
    /// Mapping from zed_thread_id to local_thread_id (for reverse lookup)
    pub thread_id_map: HashMap<String, String>,
}

/// A single conversation thread.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadSession {
    pub id: String,
    pub messages: Vec<ThreadMessage>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// True when the last assistant response is complete (message_completed received)
    pub completed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub role: String,
    pub content: String,
    pub message_id: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ZedManager {
    pub fn new(session_id: String, ws_host: String) -> Self {
        Self {
            session_id,
            ws_host,
            zed_connected: false,
            agent_ready: false,
            ws_tx: None,
            threads: HashMap::new(),
            pending_requests: HashMap::new(),
            thread_id_map: HashMap::new(),
        }
    }

    pub fn set_ws_tx(&mut self, tx: mpsc::UnboundedSender<String>) {
        self.ws_tx = Some(tx);
    }

    pub fn get_or_create_thread(&mut self, thread_id: Option<&str>) -> String {
        let id = thread_id
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        if !self.threads.contains_key(&id) {
            self.threads.insert(
                id.clone(),
                ThreadSession {
                    id: id.clone(),
                    messages: vec![],
                    created_at: chrono::Utc::now(),
                    completed: false,
                },
            );
        }
        id
    }

    pub fn add_message(&mut self, thread_id: &str, role: &str, content: &str, message_id: Option<String>) {
        if let Some(thread) = self.threads.get_mut(thread_id) {
            if let Some(ref mid) = message_id {
                if let Some(last) = thread.messages.last_mut() {
                    if last.message_id.as_deref() == Some(mid) {
                        last.content = content.to_string();
                        return;
                    }
                }
            }
            thread.messages.push(ThreadMessage {
                role: role.to_string(),
                content: content.to_string(),
                message_id,
                timestamp: chrono::Utc::now(),
            });
        }
    }

    /// Send a JSON command to Zed via WebSocket. Returns error if not connected.
    pub fn send_command(&self, cmd: &str) -> Result<(), String> {
        match &self.ws_tx {
            Some(tx) => tx.send(cmd.to_string()).map_err(|e| e.to_string()),
            None => Err("WebSocket not connected".to_string()),
        }
    }
}

// ── WebSocket server (Zed connects to us) ──────────────────────────────

pub async fn run_ws_server(ws_host: &str, zed_manager: Arc<RwLock<ZedManager>>) -> anyhow::Result<()> {
    let port = ws_host.split(':').nth(1).unwrap_or("8080");
    let listener = TcpListener::bind(&format!("127.0.0.1:{}", port)).await?;
    tracing::info!("WebSocket server listening on ws://127.0.0.1:{}", listener.local_addr()?.port());

    let (stream, peer) = listener.accept().await?;
    tracing::info!("Zed connecting from {}", peer);

    let ws_stream = accept_async(stream).await?;
    let (mut write, mut read) = ws_stream.split();

    // Create channel for sending commands to Zed
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    {
        let mut mgr = zed_manager.write().await;
        mgr.zed_connected = true;
        mgr.set_ws_tx(tx);
    }
    tracing::info!("Zed WebSocket connected");

    // Spawn write task: forward messages from channel to WebSocket
    let write_handle = tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            if let Err(e) = write.send(Message::Text(cmd.into())).await {
                tracing::error!("WebSocket write error: {}", e);
                break;
            }
        }
    });

    // Read loop
    let read_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            handle_zed_event(&zed_manager, &text).await;
                        }
                        Some(Ok(Message::Ping(_))) => {}
                        Some(Ok(Message::Close(_))) => {
                            tracing::info!("Zed WebSocket closed");
                            let mut mgr = zed_manager.write().await;
                            mgr.zed_connected = false;
                            break;
                        }
                        Some(Err(e)) => {
                            tracing::error!("WebSocket read error: {}", e);
                            let mut mgr = zed_manager.write().await;
                            mgr.zed_connected = false;
                            break;
                        }
                        None => {
                            tracing::info!("WebSocket stream ended");
                            let mut mgr = zed_manager.write().await;
                            mgr.zed_connected = false;
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    read_handle.await?;
    write_handle.abort();
    Ok(())
}

async fn handle_zed_event(zed_manager: &Arc<RwLock<ZedManager>>, text: &str) {
    let msg: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    let event_type = msg.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
    let data = msg.get("data").and_then(|v| v.as_object()).cloned().unwrap_or_default();

    match event_type {
        "ping" => {}
        "agent_ready" => {
            let mut mgr = zed_manager.write().await;
            mgr.agent_ready = true;
            tracing::info!("Agent ready ({})", data.get("agent_name").and_then(|v| v.as_str()).unwrap_or("?"));
        }
        "thread_created" => {
            let acp_id = data.get("acp_thread_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let rid = data.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut mgr = zed_manager.write().await;
            tracing::info!("Thread created: {}", acp_id);
            // Look up the original local thread_id from pending_requests
            if let Some(local_id) = mgr.pending_requests.get(&rid).cloned() {
                mgr.thread_id_map.insert(acp_id.clone(), local_id);
            }
            mgr.get_or_create_thread(Some(&acp_id));
            mgr.pending_requests.insert(rid, acp_id);
        }
        "message_added" => {
            let acp_id = data.get("acp_thread_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let role = data.get("role").and_then(|v| v.as_str()).unwrap_or("assistant").to_string();
            let msg_id = data.get("message_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            let mut mgr = zed_manager.write().await;
            mgr.add_message(&acp_id, &role, &content, msg_id.clone());
            // Mirror to the original local thread
            if let Some(local_id) = mgr.thread_id_map.get(&acp_id).cloned() {
                mgr.add_message(&local_id, &role, &content, msg_id);
            }
        }
        "message_completed" => {
            let acp_id = data.get("acp_thread_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut mgr = zed_manager.write().await;
            if let Some(thread) = mgr.threads.get_mut(&acp_id) {
                thread.completed = true;
            }
            if let Some(local_id) = mgr.thread_id_map.get(&acp_id).cloned() {
                if let Some(thread) = mgr.threads.get_mut(&local_id) {
                    thread.completed = true;
                }
            }
            tracing::info!("Message complete for thread {}", &acp_id[..acp_id.len().min(12)]);
        }
        "chat_response_error" => {
            let error = data.get("error").and_then(|v| v.as_str()).unwrap_or("?");
            tracing::error!("Chat response error: {}", error);
        }
        _ => {
            tracing::debug!("Unhandled event: {}", event_type);
        }
    }
}

// ── Launch Zed headless ────────────────────────────────────────────────

pub async fn launch_zed(
    bin_path: &Path,
    workdir: &Path,
    user_data_dir: &Path,
    session_id: &str,
    ws_host: &str,
) -> anyhow::Result<tokio::process::Child> {
    tracing::info!("Launching Zed headless...");

    let stderr_log = std::fs::File::create("/tmp/nex-zed-headless.log")?;
    let child = Command::new(bin_path)
        .args(["--headless", "--allow-multiple-instances"])
        .arg("--user-data-dir")
        .arg(user_data_dir)
        .arg(workdir)
        .env("ZED_EXTERNAL_SYNC_ENABLED", "true")
        .env("ZED_WEBSOCKET_SYNC_ENABLED", "true")
        .env("ZED_HELIX_URL", ws_host)
        .env("ZED_HELIX_TOKEN", "test-token")
        .env("HELIX_SESSION_ID", session_id)
        .env("ZED_STATELESS", "1")
        .env("RUST_LOG", "info")
        .stdout(std::process::Stdio::null())
        .stderr(stderr_log)
        .spawn()?;

    tracing::info!("Zed started (PID: {:?})", child.id());
    Ok(child)
}

// ── Zed settings bootstrap ─────────────────────────────────────────────

pub fn ensure_zed_settings(data_dir: &Path, api_key: &str) -> anyhow::Result<()> {
    use std::fs;
    use std::io::Write;

    let settings_dir = data_dir.join("config");
    fs::create_dir_all(&settings_dir)?;
    let settings_file = settings_dir.join("settings.json");

    let mut settings: serde_json::Value = if settings_file.exists() {
        serde_json::from_str(&fs::read_to_string(&settings_file)?)?
    } else {
        serde_json::json!({})
    };

    if settings
        .get("language_models")
        .and_then(|lm| lm.get("openai_compatible"))
        .and_then(|oc| oc.get("deepseek"))
        .is_none()
    {
        settings["language_models"]["openai_compatible"]["deepseek"] = serde_json::json!({
            "api_url": "https://api.deepseek.com/v1",
            "available_models": [{
                "name": "deepseek-chat",
                "display_name": "DeepSeek V3",
                "max_tokens": 65536,
                "max_output_tokens": 8192,
                "tool_use": true,
            }],
        });
    }

    let mut f = fs::File::create(&settings_file)?;
    f.write_all(serde_json::to_string_pretty(&settings)?.as_bytes())?;

    let creds_dir = data_dir.join("credentials");
    fs::create_dir_all(&creds_dir)?;
    let creds_file = creds_dir.join("credentials.json");

    let creds = serde_json::json!({
        "provider/deepseek": { "api_key": api_key }
    });

    let mut f = fs::File::create(&creds_file)?;
    f.write_all(serde_json::to_string_pretty(&creds)?.as_bytes())?;

    tracing::info!("Zed settings written to {}", settings_file.display());
    Ok(())
}
