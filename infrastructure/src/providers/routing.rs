use super::{ProviderAdapter, ProviderKind};
use crate::config::FileProvidersConfig;
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
use quorum_domain::Model;
use std::collections::HashMap;
use std::sync::Arc;

pub struct RoutingGateway {
    providers: Vec<Arc<dyn ProviderAdapter>>,
    /// Model name to provider name mapping, used for routing requests to the correct provider
    /// based on the model specified in the request.
    explicit_model_routing: HashMap<String, usize>,
    default_kind: ProviderKind,
}

impl RoutingGateway {
    pub fn new(providers: Vec<Arc<dyn ProviderAdapter>>, config: &FileProvidersConfig) -> Self {
        let mut explicit_model_routing = HashMap::new();

        for (model_name, provider_name) in &config.routing {
            // provider_name
            // からProviderKindを特定するロジックprovidersの中からprovider_nameに一致するものを探す
            let target_kind = match provider_name.as_str() {
                "copilot" => ProviderKind::Copilot,
                // 後ほどAzureやAnthopic, OpenAiのプロバイダも追加する予定
                // "azure" => ProviderKind::Azure,
                "bedrock" => ProviderKind::Bedrock,
                _ => continue, // Skip invalid provider names
            };

            if let Some(idx) = providers.iter().position(|p| p.kind() == target_kind) {
                explicit_model_routing.insert(model_name.clone(), idx);
            }
        }

        Self {
            providers,
            explicit_model_routing,
            default_kind: match config.default.as_deref().unwrap_or("copilot") {
                "copilot" => ProviderKind::Copilot,
                "azure" => ProviderKind::Azure,
                "bedrock" => ProviderKind::Bedrock,
                _ => ProviderKind::Copilot, // Fallback to Copilot if default is invalid
            },
        }
    }
    /// モデルに基づいて適切なプロバイダーを解決するロジック
    /// 設計意図としては、モデルごとに明示的なルーティングがあればそれを優先し、なければデフォルトのプロバイダーを使用する形にしたい
    /// zero configでも動くように、明示的なルーティングがなくても providersの最初の要素をデフォルトとして使用する
    /// ルーティング優先順位は:
    ///  1. explicit_model_routing に model の文字列表現があればその index のプロバイダー
    ///  2. なければ default_kind に一致するプロバイダーを providers から探す
    ///  3. それもなければ providers の最初の要素（Copilot fallback）
    ///  4. providers が空なら GatewayError::ModelNotAvailable
    fn resolve_provider(&self, model: &Model) -> Result<&dyn ProviderAdapter, GatewayError> {
        if let Some(&idx) = self.explicit_model_routing.get(model.as_str()) {
            Ok(self.providers[idx].as_ref())
        } else {
            let provider = self
                .providers
                .iter()
                .find(|p| p.kind() == self.default_kind);
            if let Some(p) = provider {
                Ok(p.as_ref())
            } else {
                self.providers
                    .first()
                    .map(|p| p.as_ref())
                    .ok_or(GatewayError::ModelNotAvailable(
                        "No providers available".to_string(),
                    ))
            }
        }
    }
}

#[async_trait]
impl LlmGateway for RoutingGateway {
    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.resolve_provider(model)?.create_session(model).await
    }

    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.resolve_provider(model)?
            .create_session_with_system_prompt(model, system_prompt)
            .await
    }

    async fn create_text_only_session(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.resolve_provider(model)?
            .create_text_only_session(model, system_prompt)
            .await
    }

    async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
        let mut all_models = Vec::new();
        for provider in &self.providers {
            if let Ok(models) = provider.available_models().await {
                all_models.extend(models);
            }
        }
        Ok(all_models)
    }
}
