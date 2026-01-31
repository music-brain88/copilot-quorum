//! Copilot LLM Gateway implementation

use crate::copilot::session::CopilotSession;
use crate::copilot::transport::StdioTransport;
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
use quorum_domain::Model;
use std::sync::Arc;
use tracing::info;

/// LLM Gateway implementation for GitHub Copilot CLI
pub struct CopilotLlmGateway {
    transport: Arc<StdioTransport>,
}

impl CopilotLlmGateway {
    /// Create a new gateway by spawning the Copilot CLI
    pub async fn new() -> Result<Self, GatewayError> {
        let transport = StdioTransport::spawn()
            .await
            .map_err(|e| GatewayError::ConnectionError(e.to_string()))?;

        info!("CopilotLlmGateway initialized");

        Ok(Self {
            transport: Arc::new(transport),
        })
    }

    /// Create a gateway with a custom command (for testing)
    pub async fn with_command(cmd: &str) -> Result<Self, GatewayError> {
        let transport = StdioTransport::spawn_with_command(cmd)
            .await
            .map_err(|e| GatewayError::ConnectionError(e.to_string()))?;

        Ok(Self {
            transport: Arc::new(transport),
        })
    }

    /// Create a gateway with an existing transport
    pub fn with_transport(transport: Arc<StdioTransport>) -> Self {
        Self { transport }
    }

    /// Get a reference to the underlying transport
    pub fn transport(&self) -> &Arc<StdioTransport> {
        &self.transport
    }
}

#[async_trait]
impl LlmGateway for CopilotLlmGateway {
    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
        let session = CopilotSession::new(Arc::clone(&self.transport), model.clone())
            .await
            .map_err(|e| GatewayError::SessionError(e.to_string()))?;

        Ok(Box::new(session))
    }

    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        let session = CopilotSession::new_with_system_prompt(
            Arc::clone(&self.transport),
            model.clone(),
            Some(system_prompt.to_string()),
        )
        .await
        .map_err(|e| GatewayError::SessionError(e.to_string()))?;

        Ok(Box::new(session))
    }

    async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
        // Copilot CLI doesn't have a model listing endpoint,
        // so we return the known available models
        Ok(vec![
            Model::ClaudeSonnet45,
            Model::ClaudeHaiku45,
            Model::ClaudeOpus45,
            Model::ClaudeSonnet4,
            Model::Gpt52Codex,
            Model::Gpt51CodexMax,
            Model::Gpt51Codex,
            Model::Gpt52,
            Model::Gpt51,
            Model::Gpt5,
            Model::Gpt51CodexMini,
            Model::Gpt5Mini,
            Model::Gpt41,
            Model::Gemini3Pro,
        ])
    }
}
