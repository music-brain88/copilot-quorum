//! JSON-RPC protocol types for Copilot CLI communication.
//!
//! This module defines the message structures used in the JSON-RPC 2.0 protocol
//! for communicating with the Copilot CLI process.
//!
//! # Protocol Overview
//!
//! - **Requests**: Client → Copilot CLI (e.g., `session.create`, `session.send`)
//! - **Responses**: Copilot CLI → Client (result or error)
//! - **Notifications**: Copilot CLI → Client (e.g., `assistant.message`, `session.idle`)

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global request ID counter for JSON-RPC requests.
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Generates a unique request ID.
fn next_id() -> u64 {
    REQUEST_ID.fetch_add(1, Ordering::SeqCst)
}

/// JSON-RPC request
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Creates a new JSON-RPC request with an auto-generated ID.
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: next_id(),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC response
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<RpcError>,
}

/// JSON-RPC error object
#[derive(Debug, Clone, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// Chat message role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    /// Creates a system message (instructions for the model).
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    /// Creates a user message (human input).
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// Creates an assistant message (model response).
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Session creation parameters
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CopilotToolDefinition>>,
}

/// Tool definition for the Copilot CLI session.
///
/// Converted from the domain's `to_api_tools()` JSON Schema format via [`from_api_tool()`](Self::from_api_tool).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl CopilotToolDefinition {
    /// Convert from the domain's `to_api_tools()` JSON format.
    ///
    /// Expects `{"name": "...", "description": "...", "input_schema": {...}}`.
    pub fn from_api_tool(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            name: value.get("name")?.as_str()?.to_string(),
            description: value.get("description")?.as_str()?.to_string(),
            input_schema: value.get("input_schema")?.clone(),
        })
    }
}

/// Session creation result
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionResult {
    pub session_id: String,
}

/// Session event params (from session.event notification)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEventParams {
    pub session_id: String,
    pub event: SessionEvent,
}

/// Session event
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    #[serde(rename = "type")]
    pub event_type: String,
}

/// Send parameters (for session.send)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendParams {
    pub session_id: String,
    pub prompt: String,
}

/// Send result
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub message_id: String,
}

/// Notification from server (assistant.message, session.idle, etc.)
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

/// Assistant message event params
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageParams {
    pub session_id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub done: bool,
}

/// Parameters for a `tool.call` request from the Copilot CLI.
///
/// When the CLI-side LLM wants to call a tool, it sends a JSON-RPC **request**
/// (with `id`) rather than a notification. The SDK must execute the tool and
/// respond with [`JsonRpcResponseOut`] containing [`ToolCallResult`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallParams {
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// An incoming JSON-RPC request from the Copilot CLI (has `id` + `method`).
///
/// Distinguished from a response (has `id` but no `method`) and a notification
/// (has `method` but no `id`).
#[derive(Debug, Clone, Deserialize)]
pub struct IncomingJsonRpcRequest {
    pub id: u64,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC response sent from SDK → CLI (e.g., tool.call result).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponseOut {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub result: serde_json::Value,
}

impl JsonRpcResponseOut {
    pub fn new(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result,
        }
    }
}

/// Result payload for a `tool.call` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    /// The text result that the LLM should see.
    pub text_result_for_llm: String,
    /// Result type: "text" for normal results, "error" for errors.
    pub result_type: String,
}

impl ToolCallResult {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text_result_for_llm: text.into(),
            result_type: "text".to_string(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text_result_for_llm: text.into(),
            result_type: "error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_session_params_with_tools_serializes_correctly() {
        let params = CreateSessionParams {
            model: Some("gpt-4".to_string()),
            system_prompt: None,
            tools: Some(vec![CopilotToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "required": ["path"]
                }),
            }]),
        };

        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["model"], "gpt-4");
        assert!(json.get("systemPrompt").is_none());
        // tools should be present with inputSchema (camelCase)
        let tools = json["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "read_file");
        assert!(tools[0]["inputSchema"]["properties"]["path"].is_object());
    }

    #[test]
    fn create_session_params_without_tools_omits_field() {
        let params = CreateSessionParams {
            model: Some("gpt-4".to_string()),
            system_prompt: None,
            tools: None,
        };

        let json = serde_json::to_value(&params).unwrap();
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn copilot_tool_definition_from_api_tool() {
        let api_tool = serde_json::json!({
            "name": "read_file",
            "description": "Read file contents",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"}
                },
                "required": ["path"]
            }
        });

        let tool = CopilotToolDefinition::from_api_tool(&api_tool).unwrap();
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.description, "Read file contents");
        assert_eq!(tool.input_schema["type"], "object");
    }

    #[test]
    fn copilot_tool_definition_from_api_tool_missing_field() {
        let bad = serde_json::json!({"name": "foo"});
        assert!(CopilotToolDefinition::from_api_tool(&bad).is_none());
    }

    #[test]
    fn tool_call_params_deserialize() {
        let json = serde_json::json!({
            "sessionId": "sess-123",
            "toolCallId": "tc-456",
            "toolName": "read_file",
            "arguments": {"path": "/src/main.rs"}
        });

        let params: ToolCallParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.session_id, "sess-123");
        assert_eq!(params.tool_call_id, "tc-456");
        assert_eq!(params.tool_name, "read_file");
        assert_eq!(params.arguments["path"], "/src/main.rs");
    }

    #[test]
    fn json_rpc_response_out_serialize() {
        let resp = JsonRpcResponseOut::new(
            42,
            serde_json::to_value(ToolCallResult::success("file contents")).unwrap(),
        );

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 42);
        assert_eq!(json["result"]["textResultForLlm"], "file contents");
        assert_eq!(json["result"]["resultType"], "text");
    }

    #[test]
    fn tool_call_result_error() {
        let result = ToolCallResult::error("File not found");
        assert_eq!(result.result_type, "error");
        assert_eq!(result.text_result_for_llm, "File not found");
    }
}
