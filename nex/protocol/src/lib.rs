// ── nex-protocol: the wire the hub speaks ──────────────────────────────
//
// A request names a method and carries parameters. A response carries a result or an error.
// Both are JSON values, so the wire says nothing about where a record lives or which
// implementation answers.
//
// This crate is the hub core's portability floor: it builds with alloc and nothing else, so
// a node with no operating system can speak it. A transport supplies the bytes.

#![no_std]
extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request from a client to a node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// The answer to a request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// Why a request was not answered.
#[derive(Clone, Debug, Serialize, Deserialize)]
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

/// The bytes a transport carries for a request.
pub fn encode_request(request: &RpcRequest) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(request)
}

/// The bytes a transport carries for a response.
pub fn encode_response(response: &RpcResponse) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(response)
}

/// Read a request out of the bytes a transport delivered.
pub fn decode_request(bytes: &[u8]) -> Result<RpcRequest, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Read a response out of the bytes a transport delivered.
pub fn decode_response(bytes: &[u8]) -> Result<RpcResponse, serde_json::Error> {
    serde_json::from_slice(bytes)
}
