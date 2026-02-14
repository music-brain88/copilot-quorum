//! JSON-RPC protocol types for Copilot CLI communication.
//!
//! Defines the message structures used in the JSON-RPC 2.0 protocol for
//! communicating with the Copilot CLI process over TCP.
//!
//! # Protocol Overview
//!
//! - **Requests** (Client → CLI): `session.create`, `session.send`
//! - **Responses** (CLI → Client): result or error, correlated by request `id`
//! - **Notifications** (CLI → Client): `session.event` carrying streaming deltas,
//!   `session.idle`, `session.start`
//! - **Incoming Requests** (CLI → Client): `tool.call` — the LLM wants a tool
//!   executed (**Native Tool Use**)
//!
//! # Feature relevance
//!
//! | Type | Feature |
//! |------|---------|
//! | [`CreateSessionParams`] | All features (session creation) |
//! | [`CopilotToolDefinition`] | Native Tool Use / Agent System |
//! | [`ToolCallParams`] | Native Tool Use (incoming `tool.call` requests) |
//! | [`ToolCallResult`] / [`JsonRpcResponseOut`] | Native Tool Use (returning tool results) |

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

/// Parameters for `session.create` — sent to the Copilot CLI to open a new
/// conversation session.
///
/// When `tools` is `Some`, the CLI-side LLM is allowed to issue `tool.call`
/// requests (**Native Tool Use**). This is used by
/// [`CopilotSession::create_tool_session_and_send`](super::session::CopilotSession).
///
/// The official Copilot SDK uses `systemMessage` (object with `mode` + `content`)
/// rather than `systemPrompt` (plain string). Both are sent for maximum
/// compatibility — the CLI should recognise at least one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Legacy field — kept for backwards compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Official SDK format: `{"mode": "append"|"replace", "content": "..."}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<SystemMessageConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CopilotToolDefinition>>,
}

/// System message configuration matching the official Copilot SDK format.
///
/// - `"append"`: adds to the CLI's default system prompt
/// - `"replace"`: completely replaces the default system prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMessageConfig {
    pub mode: String,
    pub content: String,
}

/// Tool definition for the Copilot CLI session (**Native Tool Use**).
///
/// Converted from the domain's [`ToolSpec::to_api_tools()`](quorum_domain::tool::ToolSpec::to_api_tools)
/// JSON Schema format via [`from_api_tool`](Self::from_api_tool).
///
/// The official Copilot SDK uses `"parameters"` for the tool schema field,
/// not `"inputSchema"` or `"input_schema"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotToolDefinition {
    pub name: String,
    pub description: String,
    /// Tool parameter schema. Serialized as `"parameters"` to match the
    /// official Copilot SDK wire format (Go: `Tool.Parameters`,
    /// Node.js: `tool.parameters`).
    pub parameters: serde_json::Value,
}

impl CopilotToolDefinition {
    /// Convert from the domain's `to_api_tools()` JSON format.
    ///
    /// Reads `"input_schema"` from the domain-layer JSON and maps it to
    /// `parameters` for the Copilot CLI wire format.
    pub fn from_api_tool(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            name: value.get("name")?.as_str()?.to_string(),
            description: value.get("description")?.as_str()?.to_string(),
            parameters: value.get("input_schema")?.clone(),
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

/// JSON-RPC response sent from SDK → CLI (e.g., `tool.call` result).
///
/// Used by **Native Tool Use** — after executing a tool,
/// [`CopilotSession::send_tool_results`](super::session::CopilotSession)
/// wraps the result in this type and sends it via
/// [`MessageRouter::send_response`](super::router::MessageRouter::send_response).
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

/// Result payload for a `tool.call` response (**Native Tool Use**).
///
/// Serialized inside [`JsonRpcResponseOut::result`] and sent back to the
/// CLI-side LLM so it can see the tool output and decide what to do next
/// (generate more text or call another tool).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    /// The text result that the LLM should see.
    pub text_result_for_llm: String,
    /// Result type per official Copilot SDK `ToolResultType`:
    /// `"success"`, `"failure"`, `"rejected"`, or `"denied"`.
    pub result_type: String,
}

impl ToolCallResult {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text_result_for_llm: text.into(),
            result_type: "success".to_string(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text_result_for_llm: text.into(),
            result_type: "failure".to_string(),
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
            system_message: None,
            tools: Some(vec![CopilotToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({
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
        assert!(json.get("systemMessage").is_none());
        // tools must use "parameters" to match official Copilot SDK wire format
        let tools = json["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "read_file");
        assert!(tools[0]["parameters"]["properties"]["path"].is_object());
        // Must NOT use "inputSchema" — that field name is not recognized by the Copilot CLI
        assert!(tools[0].get("inputSchema").is_none());
    }

    #[test]
    fn create_session_params_without_tools_omits_field() {
        let params = CreateSessionParams {
            model: Some("gpt-4".to_string()),
            system_prompt: None,
            system_message: None,
            tools: None,
        };

        let json = serde_json::to_value(&params).unwrap();
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn create_session_params_system_message_format() {
        let params = CreateSessionParams {
            model: Some("claude-sonnet-4.5".to_string()),
            system_prompt: Some("test prompt".to_string()),
            system_message: Some(SystemMessageConfig {
                mode: "append".to_string(),
                content: "test prompt".to_string(),
            }),
            tools: None,
        };

        let json = serde_json::to_value(&params).unwrap();
        // Both formats should be present
        assert_eq!(json["systemPrompt"], "test prompt");
        assert_eq!(json["systemMessage"]["mode"], "append");
        assert_eq!(json["systemMessage"]["content"], "test prompt");
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
        assert_eq!(tool.parameters["type"], "object");
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
        assert_eq!(json["result"]["resultType"], "success");
    }

    #[test]
    fn tool_call_result_error() {
        let result = ToolCallResult::error("File not found");
        assert_eq!(result.result_type, "failure");
        assert_eq!(result.text_result_for_llm, "File not found");
    }
}
