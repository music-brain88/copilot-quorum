use super::{ProviderAdapter, ProviderKind};
use crate::CopilotLlmGateway;
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
use quorum_domain::Model;

pub(crate) struct CopilotProviderAdapter {
    inner: CopilotLlmGateway,
}

impl CopilotProviderAdapter {
    pub fn new(inner: CopilotLlmGateway) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ProviderAdapter for CopilotProviderAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Copilot
    }

    /// TODO: Copilot-Cli起動時にモデルをチェックして、
    /// Copilot CLI がサポートしているモデルを出力するようにする。
    /// 現状は、CopilotLlmGatewayがサポートしているモデルを全てサポートしていると仮定する。
    fn supports_model(&self, _model: &Model) -> bool {
        true
    }

    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.inner.create_session(model).await
    }

    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.inner
            .create_session_with_system_prompt(model, system_prompt)
            .await
    }

    async fn create_text_only_session(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        // Copilot adapter overrides this to send `availableTools: []`,
        // disabling CLI built-in tools.
        self.inner
            .create_text_only_session(model, system_prompt)
            .await
    }

    async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
        self.inner.available_models().await
    }
}
