pub mod copilot_adapter;
pub mod routing;

use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmSession};
use quorum_domain::Model;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProviderKind {
    #[default]
    Copilot,
    Bedrock,
    Azure,
}

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn supports_model(&self, model: &Model) -> bool;
    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError>;
    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError>;
    async fn create_text_only_session(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError>;
    async fn available_models(&self) -> Result<Vec<Model>, GatewayError>;
}
