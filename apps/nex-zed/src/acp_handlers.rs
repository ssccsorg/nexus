// ── ACP Request/Notification Handlers ──────────────────────────────────
//
// All handlers Zed may send via the Agent Client Protocol.
// Phase 1: echo mode (no neXus dependency).
// Phase 2+: replaced with FIH blackboard-backed implementations.

use agent_client_protocol::schema as acp;
use agent_client_protocol::{ConnectionTo, Responder};

use crate::session::SessionManager;

// ── Shared application state ──────────────────────────────────────────

pub struct AppState {
    pub session_manager: std::sync::Mutex<SessionManager>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            session_manager: std::sync::Mutex::new(SessionManager::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Request Handlers ──────────────────────────────────────────────────

/// Handle InitializeRequest: respond with agent metadata and capabilities.
pub async fn handle_initialize_request(
    _req: acp::InitializeRequest,
    responder: Responder<acp::InitializeResponse>,
    _connection: ConnectionTo<acp::Agent>,
) {
    log::info!("Received InitializeRequest");

    let response = acp::InitializeResponse {
        protocol_version: acp::ProtocolVersion::V1,
        agent_info: acp::AgentInfo {
            name: "nexus-zed".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        agent_capabilities: acp::AgentCapabilities {
            load_session: false,
            session_capabilities: None,
            prompt_capabilities: Some(acp::PromptCapabilities {
                tools: None, // Phase 1: no tools; Phase 3+: tool definitions
                stream: false,
                chat: Some(acp::ChatCapabilities {
                    max_message_count: 100,
                    max_message_size: 64000,
                }),
            }),
        },
        auth_methods: vec![],
        env_capabilities: None,
        display_capabilities: None,
        env: vec![],
        prompt_overrides: None,
    };

    let _ = responder.send(Ok(response));
}

/// Handle NewSessionRequest: create a new session scope.
pub async fn handle_new_session_request(
    req: acp::NewSessionRequest,
    responder: Responder<acp::NewSessionResponse>,
    _connection: ConnectionTo<acp::Agent>,
    state: &AppState,
) {
    log::info!("Received NewSessionRequest: session_id={}", req.session_id);

    state
        .session_manager
        .lock()
        .expect("session manager lock")
        .create_session(req.session_id.clone());

    let response = acp::NewSessionResponse {
        session_id: req.session_id,
        modes: None,
        model: None,
        config_options: None,
        additional_directories: None,
    };

    let _ = responder.send(Ok(response));
}

/// Handle LoadSessionRequest: not supported.
pub async fn handle_load_session_request(
    _req: acp::LoadSessionRequest,
    responder: Responder<acp::LoadSessionResponse>,
    _connection: ConnectionTo<acp::Agent>,
) {
    log::warn!("LoadSessionRequest received but not supported");
    let _ = responder.send(Err(acp::Error {
        code: acp::ErrorCode::MethodNotFound,
        message: "session loading is not supported by nexus-zed".into(),
        data: None,
    }));
}

/// Handle ResumeSessionRequest: not supported.
pub async fn handle_resume_session_request(
    _req: acp::ResumeSessionRequest,
    responder: Responder<acp::ResumeSessionResponse>,
    _connection: ConnectionTo<acp::Agent>,
) {
    log::warn!("ResumeSessionRequest received but not supported");
    let _ = responder.send(Err(acp::Error {
        code: acp::ErrorCode::MethodNotFound,
        message: "session resume is not supported by nexus-zed".into(),
        data: None,
    }));
}

/// Handle SetSessionModeRequest: store mode in session state.
pub async fn handle_set_session_mode_request(
    req: acp::SetSessionModeRequest,
    responder: Responder<acp::SetSessionModeResponse>,
    _connection: ConnectionTo<acp::Agent>,
    state: &AppState,
) {
    log::info!("SetSessionMode: session={}, mode={}", req.session_id, req.mode);

    if let Some(session) = state
        .session_manager
        .lock()
        .expect("session manager lock")
        .get_mut(&req.session_id)
    {
        session.mode = Some(req.mode);
        // Phase 2+: also write as Hint to neXus blackboard
    }

    let _ = responder.send(Ok(acp::SetSessionModeResponse {
        session_id: req.session_id,
    }));
}

/// Handle SetSessionModelRequest: store model preference.
pub async fn handle_set_session_model_request(
    req: acp::SetSessionModelRequest,
    responder: Responder<acp::SetSessionModelResponse>,
    _connection: ConnectionTo<acp::Agent>,
    state: &AppState,
) {
    log::info!("SetSessionModel: session={}, model={:?}", req.session_id, req.model);

    if let Some(session) = state
        .session_manager
        .lock()
        .expect("session manager lock")
        .get_mut(&req.session_id)
    {
        session.model = req.model.clone();
        // Phase 2+: also write as Hint to neXus blackboard
    }

    let _ = responder.send(Ok(acp::SetSessionModelResponse {
        session_id: req.session_id,
    }));
}

/// Handle SetSessionConfigOptionRequest: store config option.
pub async fn handle_set_session_config_option(
    req: acp::SetSessionConfigOptionRequest,
    responder: Responder<acp::SetSessionConfigOptionResponse>,
    _connection: ConnectionTo<acp::Agent>,
    state: &AppState,
) {
    log::info!(
        "SetSessionConfigOption: session={}, key={}, value={:?}",
        req.session_id,
        req.key,
        req.value
    );

    if let Some(session) = state
        .session_manager
        .lock()
        .expect("session manager lock")
        .get_mut(&req.session_id)
    {
        if let Some(value) = req.value {
            session.config_options.insert(req.key.clone(), value);
        } else {
            session.config_options.remove(&req.key);
        }
        // Phase 2+: also write as Hint to neXus blackboard
    }

    let _ = responder.send(Ok(acp::SetSessionConfigOptionResponse {
        session_id: req.session_id,
    }));
}

/// Handle PromptRequest: core handler — phase 1 echo, phase 2+ FIH.
pub async fn handle_prompt_request(
    req: acp::PromptRequest,
    responder: Responder<acp::PromptResponse>,
    _connection: ConnectionTo<acp::Agent>,
) {
    log::info!("Received PromptRequest: session_id={}", req.session_id);

    // Phase 1: echo mode — stream user text back as partial chunks.
    let user_text: String = req
        .message
        .content
        .iter()
        .filter_map(|block| match block {
            acp::ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<&str>>()
        .join("\n");

    // Send partial chunks (simulating streaming).
    let partial = acp::PromptResponse {
        message: acp::Message {
            role: acp::MessageRole::Assistant,
            content: vec![acp::ContentBlock::Text(acp::TextContent {
                text: format!("[nex-zed echo] {}", user_text),
            })],
            stop_reason: None,
        },
        conversation_id: None,
        tool_call_ids: vec![],
        output: None,
    };
    let _ = responder.send(Ok(partial));

    // Send final response with stop reason.
    let final_resp = acp::PromptResponse {
        message: acp::Message {
            role: acp::MessageRole::Assistant,
            content: vec![],
            stop_reason: Some(acp::StopReason::EndTurn),
        },
        conversation_id: None,
        tool_call_ids: vec![],
        output: None,
    };
    let _ = responder.send(Ok(final_resp));
}

/// Handle DeleteSession request.
pub async fn handle_delete_session(
    req: acp::DeleteSessionRequest,
    responder: Responder<acp::DeleteSessionResponse>,
    _connection: ConnectionTo<acp::Agent>,
    state: &AppState,
) {
    log::info!("DeleteSession: session_id={}", req.session_id);

    state
        .session_manager
        .lock()
        .expect("session manager lock")
        .remove(&req.session_id);

    // Phase 2+: release neXus scope

    let _ = responder.send(Ok(acp::DeleteSessionResponse {
        session_id: req.session_id,
    }));
}

/// Handle LogoutRequest: not supported.
pub async fn handle_logout_request(
    _req: acp::LogoutRequest,
    responder: Responder<acp::LogoutResponse>,
    _connection: ConnectionTo<acp::Agent>,
) {
    log::warn!("LogoutRequest received but not supported");
    let _ = responder.send(Err(acp::Error {
        code: acp::ErrorCode::MethodNotFound,
        message: "logout is not supported by nexus-zed".into(),
        data: None,
    }));
}

// ── Notification Handlers ─────────────────────────────────────────────

/// Handle CancelNotification: cancel an in-flight prompt or tool call.
pub async fn handle_cancel_notification(
    notif: acp::CancelNotification,
    _connection: ConnectionTo<acp::Agent>,
    _state: &AppState,
) {
    log::info!(
        "CancelNotification: session={}, tool_ids={:?}",
        notif.session_id,
        notif.tool_call_ids
    );
    // Phase 2+: update Intent status to cancelled on neXus
}

/// Handle DeleteSessionNotification: release session resources.
pub async fn handle_delete_session_notification(
    notif: acp::DeleteSessionNotification,
    _connection: ConnectionTo<acp::Agent>,
    state: &AppState,
) {
    log::info!("DeleteSessionNotification: session_id={}", notif.session_id);

    state
        .session_manager
        .lock()
        .expect("session manager lock")
        .remove(&notif.session_id);

    // Phase 2+: release neXus scope
}
