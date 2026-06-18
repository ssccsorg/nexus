]8;;file:///Users/blackgene/Documents/ssccs-nexus/apps/nex-zed/src/main.rs]8;;
// ── nex-zed: neXus instance with an ACP surface for the Zed editor ──────
//
// A standalone binary that embeds ACP (Agent Client Protocol) as one of its
// communication surfaces. To the Zed editor, it appears as a custom agent
// server spawned as a child process with piped stdin/stdout/stderr.
//
// Architecture:
//   Zed Editor
//     └── spawns child process (ACP stdio)
//          └── nex-zed (agentic neXus instance)
//               ├── ACP surface (inbound from Zed)
//               ├── ACP surface (outbound to Zed)
//               └── FIH surface (neXus blackboard)
//
// References:
//   - https://docs.ssccs.org/projects/nexus/apps/zed.llms.md
//   - https://github.com/ssccsorg/nexus/issues/72

use agent_client_protocol::schema as acp;
use agent_client_protocol::{ConnectionTo, Lines as LinesTransport};
use clap::Parser;

mod acp_handlers;
mod config;
pub mod session;

use acp_handlers::AppState;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = config::Args::parse();

    // Initialize logging
    if args.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(&args.log_level),
    )
    .init();

    log::info!("Starting nex-zed v{}", env!("CARGO_PKG_VERSION"));
    log::info!("neXus socket: {}", args.nexus_socket);

    // Shared application state (Phase 1: session manager only).
    // Phase 2+: add NexusTransport, FihStorage, etc.
    let state = std::sync::Arc::new(AppState::new());

    // ACP transport over stdin/stdout (Content-Length-delimited JSON-RPC 2.0).
    // Same framing as MCP: "Content-Length: N\r\n\r\n{...}".
    // Zed captures and logs stderr but does not interpret it as protocol.
    let transport = LinesTransport::stdio();

    log::info!("ACP transport initialized, entering main loop");

    // Ensure all critical ACP handlers are registered. Zed's
    // connect_client_future() (crates/agent_servers/src/acp.rs:706)
    // expects the full set.
    let _connection = acp::Client::builder()
        .name("nexus-zed")
        // ── Requests (inbound from Zed) ────────────────────────────
        .on_receive_request(
            |req, responder, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_initialize_request(req, responder, connection).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_new_session_request(req, responder, connection, &state).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                async move { acp_handlers::handle_load_session_request(req, responder, connection).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                async move { acp_handlers::handle_resume_session_request(req, responder, connection).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_set_session_mode_request(req, responder, connection, &state).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_set_session_model_request(req, responder, connection, &state).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_set_session_config_option(req, responder, connection, &state).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                async move { acp_handlers::handle_prompt_request(req, responder, connection).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_delete_session(req, responder, connection, &state).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req, responder, connection| {
                async move { acp_handlers::handle_logout_request(req, responder, connection).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // ── Notifications (inbound from Zed) ───────────────────────
        .on_receive_notification(
            |notif, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_cancel_notification(notif, connection, &state).await }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_notification(
            |notif, connection| {
                let state = state.clone();
                async move { acp_handlers::handle_delete_session_notification(notif, connection, &state).await }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        // ── Connect ────────────────────────────────────────────────
        .connect_with(transport, |_connection: ConnectionTo<acp::Agent>| async move {
            // Connection established; keep alive until transport closes.
            futures::future::pending::<Result<(), acp::Error>>().await
        })
        .await?;

    log::info!("nex-zed connection closed, exiting");
    Ok(())
}
