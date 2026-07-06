// ── nex-client: thin wire wrapper for nex-server IPC ──────────────────────
//
// Pure protocol layer. No knowledge of FihStorage, FIH, or nex internals.
// Only knows JSON-RPC 2.0 over Unix domain socket.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

// ── Protocol types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RpcRequest {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }

    pub fn invalid_request(id: Value) -> Self {
        Self::error(id, -32600, "Invalid Request")
    }

    pub fn method_not_found(id: Value, method: &str) -> Self {
        Self::error(id, -32601, format!("Method not found: {method}"))
    }

    pub fn invalid_params(id: Value, msg: String) -> Self {
        Self::error(id, -32602, msg)
    }
}

// ── Client ─────────────────────────────────────────────────────────────

pub struct NexClient {
    writer: tokio::io::WriteHalf<tokio::net::UnixStream>,
    reader: BufReader<tokio::io::ReadHalf<tokio::net::UnixStream>>,
}

impl NexClient {
    /// Connect to a nex-server Unix socket.
    pub async fn connect(path: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(path)
            .await
            .map_err(|e| format!("connect failed: {e}"))?;
        let (reader, writer) = tokio::io::split(stream);
        Ok(Self {
            writer,
            reader: BufReader::new(reader),
        })
    }

    /// Send a JSON-RPC request and receive the response.
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = serde_json::json!(1);
        let req = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let mut buf = serde_json::to_string(&req).map_err(|e| format!("serialize failed: {e}"))?;
        buf.push('\n');

        self.writer
            .write_all(buf.as_bytes())
            .await
            .map_err(|e| format!("write failed: {e}"))?;

        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("read failed: {e}"))?;

        if line.trim().is_empty() {
            return Err("empty response".into());
        }

        let resp: RpcResponse =
            serde_json::from_str(&line).map_err(|e| format!("deserialize failed: {e}"))?;

        if let Some(err) = resp.error {
            return Err(format!("RPC error [{}]: {}", err.code, err.message));
        }

        resp.result.ok_or_else(|| "no result".into())
    }

    /// Send a JSON-RPC request and receive the full RpcResponse.
    /// Unlike `call()`, this preserves the raw error code/message.
    pub async fn call_raw(
        &mut self,
        method: &str,
        params: Value,
    ) -> RpcResponse {
        let id = serde_json::json!(1);
        let req = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let mut buf = serde_json::to_string(&req).unwrap_or_default();
        buf.push('\n');

        let _ = self.writer.write_all(buf.as_bytes()).await;

        let mut line = String::new();
        let _ = self.reader.read_line(&mut line).await;

        serde_json::from_str(line.trim()).unwrap_or_else(|_| {
            RpcResponse::error(id, -32000, "invalid response")
        })
    }

    /// Convenience: write a Fact.
    pub async fn write_fact(
        &mut self,
        origin: &str,
        content: &str,
        creator: &str,
    ) -> Result<String, String> {
        let result = self
            .call(
                "write_fact",
                serde_json::json!({
                    "origin": origin,
                    "content": content,
                    "creator": creator,
                }),
            )
            .await?;
        Ok(result["id"].as_str().unwrap_or("???").to_string())
    }

    /// Convenience: read the full board state.
    pub async fn read_state(&mut self) -> Result<Value, String> {
        self.call("read_state", serde_json::json!({})).await
    }

    /// Convenience: submit an Intent.
    pub async fn write_intent(
        &mut self,
        from_facts: Vec<&str>,
        description: &str,
        creator: &str,
    ) -> Result<String, String> {
        let result = self
            .call(
                "write_intent",
                serde_json::json!({
                    "from_facts": from_facts,
                    "description": description,
                    "creator": creator,
                }),
            )
            .await?;
        Ok(result["id"].as_str().unwrap_or("???").to_string())
    }

    /// Convenience: claim an Intent.
    pub async fn claim_intent(&mut self, id: &str, agent: &str) -> Result<(), String> {
        self.call(
            "claim_intent",
            serde_json::json!({ "id": id, "agent": agent }),
        )
        .await?;
        Ok(())
    }

    /// Convenience: heartbeat an Intent.
    pub async fn heartbeat_intent(&mut self, id: &str, agent: &str) -> Result<(), String> {
        self.call(
            "heartbeat_intent",
            serde_json::json!({ "id": id, "agent": agent }),
        )
        .await?;
        Ok(())
    }

    /// Convenience: conclude an Intent.
    pub async fn conclude_intent(&mut self, id: &str, result: &str) -> Result<Value, String> {
        self.call(
            "conclude_intent",
            serde_json::json!({ "id": id, "result": result }),
        )
        .await
    }

    /// Convenience: write a Hint.
    pub async fn write_hint(
        &mut self,
        id: &str,
        content: &str,
        creator: &str,
    ) -> Result<(), String> {
        self.call(
            "write_hint",
            serde_json::json!({ "id": id, "content": content, "creator": creator }),
        )
        .await?;
        Ok(())
    }
}
