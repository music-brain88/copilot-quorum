//! Bedrock LLM session implementation
//!
//! Wraps the AWS Bedrock Converse API to implement the `LlmSession` trait.
//! Manages conversation history locally since the Converse API is stateless.

use super::types;
use async_trait::async_trait;
use aws_sdk_bedrockruntime::Client as BedrockClient;
use aws_sdk_bedrockruntime::types as bedrock;
use quorum_application::ports::llm_gateway::{GatewayError, LlmSession, ToolResultMessage};
use quorum_domain::LlmResponse;
use quorum_domain::Model;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

pub struct BedrockSession {
    client: Arc<BedrockClient>,
    model: Model,
    bedrock_model_id: String,
    system_prompt: Option<String>,
    max_tokens: i32,
    /// Conversation history (stateless API requires full history each call)
    messages: Mutex<Vec<bedrock::Message>>,
    /// Tool configuration (set when send_with_tools is first called)
    tool_config: Mutex<Option<bedrock::ToolConfiguration>>,
}

impl BedrockSession {
    pub fn new(
        client: Arc<BedrockClient>,
        model: Model,
        bedrock_model_id: String,
        system_prompt: Option<String>,
        max_tokens: i32,
    ) -> Self {
        Self {
            client,
            model,
            bedrock_model_id,
            system_prompt,
            max_tokens,
            messages: Mutex::new(Vec::new()),
            tool_config: Mutex::new(None),
        }
    }

    /// Build the system prompt as a SystemContentBlock list.
    fn system_blocks(&self) -> Vec<bedrock::SystemContentBlock> {
        match &self.system_prompt {
            Some(prompt) if !prompt.is_empty() => {
                vec![bedrock::SystemContentBlock::Text(prompt.clone())]
            }
            _ => vec![],
        }
    }

    /// Execute a Converse API call with the current message history.
    async fn converse(&self, messages: &[bedrock::Message]) -> Result<LlmResponse, GatewayError> {
        let tool_config = self.tool_config.lock().await;

        let mut request = self
            .client
            .converse()
            .model_id(&self.bedrock_model_id)
            .set_system(Some(self.system_blocks()))
            .set_messages(Some(messages.to_vec()))
            .inference_config(
                bedrock::InferenceConfiguration::builder()
                    .max_tokens(self.max_tokens)
                    .build(),
            );

        if let Some(ref tc) = *tool_config {
            request = request.tool_config(tc.clone());
        }

        debug!(
            model = %self.bedrock_model_id,
            messages = messages.len(),
            "Calling Bedrock Converse API"
        );

        let response = request
            .send()
            .await
            .map_err(|e| types::convert_converse_error(&e))?;

        let stop_reason = response.stop_reason();
        let output = response.output().ok_or_else(|| {
            GatewayError::RequestFailed("No output in Bedrock response".to_string())
        })?;

        Ok(types::convert_converse_output(
            output,
            stop_reason,
            &self.bedrock_model_id,
        ))
    }

    /// Append a user message and call the Converse API.
    async fn send_user_message(
        &self,
        content: Vec<bedrock::ContentBlock>,
    ) -> Result<LlmResponse, GatewayError> {
        let user_msg = bedrock::Message::builder()
            .role(bedrock::ConversationRole::User)
            .set_content(Some(content))
            .build()
            .map_err(|e| GatewayError::RequestFailed(format!("Failed to build message: {}", e)))?;

        let mut messages = self.messages.lock().await;
        messages.push(user_msg);

        let response = self.converse(&messages).await?;

        // Append assistant response to history
        if let Some(assistant_content) = self.response_to_content_blocks(&response) {
            let assistant_msg = bedrock::Message::builder()
                .role(bedrock::ConversationRole::Assistant)
                .set_content(Some(assistant_content))
                .build()
                .map_err(|e| {
                    GatewayError::RequestFailed(format!("Failed to build assistant message: {}", e))
                })?;
            messages.push(assistant_msg);
        }

        Ok(response)
    }

    /// Convert an LlmResponse back to Bedrock ContentBlocks for history tracking.
    fn response_to_content_blocks(
        &self,
        response: &LlmResponse,
    ) -> Option<Vec<bedrock::ContentBlock>> {
        let blocks: Vec<bedrock::ContentBlock> = response
            .content
            .iter()
            .map(|block| match block {
                quorum_domain::ContentBlock::Text(text) => {
                    bedrock::ContentBlock::Text(text.clone())
                }
                quorum_domain::ContentBlock::ToolUse { id, name, input } => {
                    let input_doc = types::json_to_document(&serde_json::json!(input));
                    bedrock::ContentBlock::ToolUse(
                        bedrock::ToolUseBlock::builder()
                            .tool_use_id(id)
                            .name(name)
                            .input(input_doc)
                            .build()
                            .expect("tool_use_id, name, input are required"),
                    )
                }
            })
            .collect();

        if blocks.is_empty() {
            None
        } else {
            Some(blocks)
        }
    }
}

#[async_trait]
impl LlmSession for BedrockSession {
    async fn send(&self, message: &str) -> Result<String, GatewayError> {
        let content = vec![bedrock::ContentBlock::Text(message.to_string())];
        let response: LlmResponse = self.send_user_message(content).await?;
        Ok(response.text_content())
    }

    async fn send_with_tools(
        &self,
        message: &str,
        tools: &[serde_json::Value],
    ) -> Result<LlmResponse, GatewayError> {
        // Convert tool schemas to Bedrock ToolConfiguration
        let bedrock_tools: Vec<bedrock::Tool> = tools
            .iter()
            .filter_map(types::convert_tool_schema)
            .collect();

        if !bedrock_tools.is_empty() {
            let tool_config = bedrock::ToolConfiguration::builder()
                .set_tools(Some(bedrock_tools))
                .build()
                .map_err(|e| {
                    GatewayError::RequestFailed(format!("Failed to build tool config: {}", e))
                })?;
            *self.tool_config.lock().await = Some(tool_config);
        }

        let content = vec![bedrock::ContentBlock::Text(message.to_string())];
        self.send_user_message(content).await
    }

    async fn send_tool_results(
        &self,
        results: &[ToolResultMessage],
    ) -> Result<LlmResponse, GatewayError> {
        let content: Vec<bedrock::ContentBlock> =
            results.iter().map(types::convert_tool_result).collect();

        self.send_user_message(content).await
    }

    fn model(&self) -> &Model {
        &self.model
    }
}
