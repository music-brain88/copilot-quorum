//! Type conversions between AWS Bedrock SDK and domain types
//!
//! Converts Bedrock Converse API responses to domain `LlmResponse`,
//! and domain tool types to Bedrock request formats.

use aws_sdk_bedrockruntime::types as bedrock;
use aws_smithy_types::Document;
use quorum_application::ports::llm_gateway::{GatewayError, ToolResultMessage};
use quorum_domain::{ContentBlock, LlmResponse, StopReason};
use std::collections::HashMap;

// ─── Bedrock → Domain ────────────────────────────────────────────

/// Convert Bedrock stop reason to domain StopReason.
pub fn convert_stop_reason(reason: &bedrock::StopReason) -> StopReason {
    match reason {
        bedrock::StopReason::EndTurn => StopReason::EndTurn,
        bedrock::StopReason::ToolUse => StopReason::ToolUse,
        bedrock::StopReason::MaxTokens => StopReason::MaxTokens,
        other => StopReason::Other(format!("{:?}", other)),
    }
}

/// Convert a single Bedrock content block to a domain ContentBlock.
///
/// Returns `None` for unsupported block types (Image, GuardContent, etc.).
pub fn convert_content_block(block: &bedrock::ContentBlock) -> Option<ContentBlock> {
    match block {
        bedrock::ContentBlock::Text(text) => Some(ContentBlock::Text(text.clone())),
        bedrock::ContentBlock::ToolUse(tool_use) => {
            let input = document_to_json(tool_use.input());
            let input_map = match input {
                serde_json::Value::Object(map) => map
                    .into_iter()
                    .collect::<HashMap<String, serde_json::Value>>(),
                _ => HashMap::new(),
            };
            Some(ContentBlock::ToolUse {
                id: tool_use.tool_use_id().to_string(),
                name: tool_use.name().to_string(),
                input: input_map,
            })
        }
        // Skip Image, GuardContent, Document, etc.
        _ => None,
    }
}

/// Convert a Bedrock ConverseOutput to a domain LlmResponse.
pub fn convert_converse_output(
    output: &bedrock::ConverseOutput,
    stop_reason: &bedrock::StopReason,
    model_id: &str,
) -> LlmResponse {
    let content = match output {
        bedrock::ConverseOutput::Message(message) => message
            .content()
            .iter()
            .filter_map(convert_content_block)
            .collect(),
        _ => return LlmResponse::from_text(""),
    };

    LlmResponse {
        content,
        stop_reason: Some(convert_stop_reason(stop_reason)),
        model: Some(model_id.to_string()),
    }
}

// ─── Domain → Bedrock ────────────────────────────────────────────

/// Convert a domain ToolResultMessage to a Bedrock ContentBlock::ToolResult.
pub fn convert_tool_result(result: &ToolResultMessage) -> bedrock::ContentBlock {
    let status = if result.is_error {
        bedrock::ToolResultStatus::Error
    } else {
        bedrock::ToolResultStatus::Success
    };

    let content = bedrock::ToolResultContentBlock::Text(result.output.clone());

    bedrock::ContentBlock::ToolResult(
        bedrock::ToolResultBlock::builder()
            .tool_use_id(&result.tool_use_id)
            .status(status)
            .content(content)
            .build()
            .expect("tool_use_id is required"),
    )
}

/// Convert a JSON tool schema (from ToolSchemaPort) to a Bedrock Tool::ToolSpec.
pub fn convert_tool_schema(schema: &serde_json::Value) -> Option<bedrock::Tool> {
    let name = schema.get("name")?.as_str()?;
    let description = schema.get("description").and_then(|d| d.as_str());

    let input_schema_json = schema.get("input_schema").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "type": "object",
            "properties": {},
        })
    });
    let input_schema = json_to_document(&input_schema_json);

    let mut builder = bedrock::ToolSpecification::builder()
        .name(name)
        .input_schema(bedrock::ToolInputSchema::Json(input_schema));
    if let Some(desc) = description {
        builder = builder.description(desc);
    }

    Some(bedrock::Tool::ToolSpec(
        builder.build().expect("name and input_schema are required"),
    ))
}

// ─── JSON ↔ Document helpers ─────────────────────────────────────

/// Convert a serde_json::Value to an aws_smithy_types::Document.
pub fn json_to_document(value: &serde_json::Value) -> Document {
    match value {
        serde_json::Value::Null => Document::Null,
        serde_json::Value::Bool(b) => Document::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Document::Number(aws_smithy_types::Number::NegInt(i))
            } else if let Some(f) = n.as_f64() {
                Document::Number(aws_smithy_types::Number::Float(f))
            } else {
                Document::Null
            }
        }
        serde_json::Value::String(s) => Document::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Document::Array(arr.iter().map(json_to_document).collect())
        }
        serde_json::Value::Object(map) => Document::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_document(v)))
                .collect(),
        ),
    }
}

/// Convert an aws_smithy_types::Document to a serde_json::Value.
pub fn document_to_json(doc: &Document) -> serde_json::Value {
    match doc {
        Document::Null => serde_json::Value::Null,
        Document::Bool(b) => serde_json::Value::Bool(*b),
        Document::Number(n) => match n {
            aws_smithy_types::Number::PosInt(i) => serde_json::json!(*i),
            aws_smithy_types::Number::NegInt(i) => serde_json::json!(*i),
            aws_smithy_types::Number::Float(f) => serde_json::Value::Number(
                serde_json::Number::from_f64(*f).unwrap_or_else(|| serde_json::Number::from(0)),
            ),
        },
        Document::String(s) => serde_json::Value::String(s.clone()),
        Document::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(document_to_json).collect())
        }
        Document::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), document_to_json(v)))
                .collect(),
        ),
    }
}

/// Convert a Bedrock SDK error to a GatewayError.
pub fn convert_converse_error(
    err: &aws_sdk_bedrockruntime::error::SdkError<
        aws_sdk_bedrockruntime::operation::converse::ConverseError,
    >,
) -> GatewayError {
    use aws_sdk_bedrockruntime::operation::converse::ConverseError;

    match err {
        aws_sdk_bedrockruntime::error::SdkError::ServiceError(service_err) => {
            match service_err.err() {
                ConverseError::ThrottlingException(e) => {
                    GatewayError::RequestFailed(format!("Bedrock throttled: {}", e))
                }
                ConverseError::ModelNotReadyException(e) => {
                    GatewayError::ModelNotAvailable(format!("Bedrock model not ready: {}", e))
                }
                ConverseError::ValidationException(e) => {
                    GatewayError::RequestFailed(format!("Bedrock validation error: {}", e))
                }
                ConverseError::ModelTimeoutException(_) => GatewayError::Timeout,
                other => GatewayError::RequestFailed(format!("Bedrock error: {:?}", other)),
            }
        }
        other => GatewayError::ConnectionError(format!("Bedrock SDK error: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_stop_reason_end_turn() {
        assert_eq!(
            convert_stop_reason(&bedrock::StopReason::EndTurn),
            StopReason::EndTurn
        );
    }

    #[test]
    fn test_convert_stop_reason_tool_use() {
        assert_eq!(
            convert_stop_reason(&bedrock::StopReason::ToolUse),
            StopReason::ToolUse
        );
    }

    #[test]
    fn test_convert_stop_reason_max_tokens() {
        assert_eq!(
            convert_stop_reason(&bedrock::StopReason::MaxTokens),
            StopReason::MaxTokens
        );
    }

    #[test]
    fn test_convert_text_content_block() {
        let block = bedrock::ContentBlock::Text("hello".to_string());
        let result = convert_content_block(&block).unwrap();
        assert!(matches!(result, ContentBlock::Text(ref t) if t == "hello"));
    }

    #[test]
    fn test_json_document_roundtrip() {
        let original = serde_json::json!({
            "name": "test",
            "count": 42,
            "nested": { "flag": true },
            "items": [1, 2, 3]
        });
        let doc = json_to_document(&original);
        let back = document_to_json(&doc);
        assert_eq!(original, back);
    }

    #[test]
    fn test_convert_tool_result_success() {
        let result = ToolResultMessage {
            tool_use_id: "tool_123".to_string(),
            tool_name: "read_file".to_string(),
            output: "file contents here".to_string(),
            is_error: false,
            is_rejected: false,
        };
        let block = convert_tool_result(&result);
        assert!(matches!(block, bedrock::ContentBlock::ToolResult(_)));
    }

    #[test]
    fn test_convert_tool_result_error() {
        let result = ToolResultMessage {
            tool_use_id: "tool_456".to_string(),
            tool_name: "run_command".to_string(),
            output: "command failed".to_string(),
            is_error: true,
            is_rejected: false,
        };
        let block = convert_tool_result(&result);
        assert!(matches!(block, bedrock::ContentBlock::ToolResult(_)));
    }

    #[test]
    fn test_convert_tool_schema() {
        let schema = serde_json::json!({
            "name": "read_file",
            "description": "Read a file",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path" }
                },
                "required": ["path"]
            }
        });
        let tool = convert_tool_schema(&schema);
        assert!(tool.is_some());
    }

    #[test]
    fn test_convert_tool_schema_missing_name() {
        let schema = serde_json::json!({ "description": "No name" });
        assert!(convert_tool_schema(&schema).is_none());
    }
}
