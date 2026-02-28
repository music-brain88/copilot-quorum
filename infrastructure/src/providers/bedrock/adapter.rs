//! Bedrock provider adapter
//!
//! Implements `ProviderAdapter` to plug into the `RoutingGateway`.
//! Handles AWS credential initialization and session creation.

use super::model_map;
use super::session::BedrockSession;
use crate::providers::{ProviderAdapter, ProviderKind};
use async_trait::async_trait;
use aws_sdk_bedrockruntime::Client as BedrockClient;
use quorum_application::ports::llm_gateway::{GatewayError, LlmSession};
use quorum_domain::BedrockProviderConfig;
use quorum_domain::Model;
use std::sync::Arc;
use tracing::{info, warn};

pub struct BedrockProviderAdapter {
    client: Arc<BedrockClient>,
    region: String,
    max_tokens: i32,
    cross_region: bool,
}

impl BedrockProviderAdapter {
    /// Create a new Bedrock provider adapter.
    ///
    /// Initializes AWS credentials and creates a Bedrock Runtime client.
    pub async fn new(config: &BedrockProviderConfig) -> Result<Self, GatewayError> {
        let mut aws_config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(config.region.clone()));

        if let Some(ref profile) = config.profile {
            aws_config_loader = aws_config_loader.profile_name(profile);
        }

        let aws_config = aws_config_loader.load().await;
        let client = BedrockClient::new(&aws_config);

        Ok(Self {
            client: Arc::new(client),
            region: config.region.clone(),
            max_tokens: config.max_tokens as i32,
            cross_region: config.cross_region.unwrap_or(false),
        })
    }

    /// Try to create a new Bedrock provider adapter.
    ///
    /// Returns `None` if AWS credential initialization fails.
    /// Used for auto-detection during DI assembly.
    pub async fn try_new(config: &BedrockProviderConfig) -> Option<Self> {
        match Self::new(config).await {
            Ok(adapter) => {
                info!(region = %adapter.region, "Bedrock provider initialized");
                Some(adapter)
            }
            Err(e) => {
                warn!("Bedrock provider not available: {}", e);
                None
            }
        }
    }

    fn create_bedrock_session(
        &self,
        model: &Model,
        system_prompt: Option<String>,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        let bedrock_model_id = model_map::to_bedrock_model_id(
            model,
            self.cross_region,
            &self.region,
        )
        .ok_or_else(|| {
            GatewayError::ModelNotAvailable(format!("Model {} is not supported by Bedrock", model))
        })?;

        Ok(Box::new(BedrockSession::new(
            self.client.clone(),
            model.clone(),
            bedrock_model_id,
            system_prompt,
            self.max_tokens,
        )))
    }
}

#[async_trait]
impl ProviderAdapter for BedrockProviderAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Bedrock
    }

    fn supports_model(&self, model: &Model) -> bool {
        model_map::is_bedrock_supported(model)
    }

    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.create_bedrock_session(model, None)
    }

    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.create_bedrock_session(model, Some(system_prompt.to_string()))
    }

    async fn create_text_only_session(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        // Bedrock doesn't have a special "text-only" mode,
        // so we just create a normal session without tool config.
        self.create_bedrock_session(model, Some(system_prompt.to_string()))
    }

    async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
        Ok(vec![
            Model::ClaudeSonnet46,
            Model::ClaudeOpus46,
            Model::ClaudeSonnet45,
            Model::ClaudeHaiku45,
            Model::ClaudeOpus45,
            Model::ClaudeSonnet4,
        ])
    }
}
