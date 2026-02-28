use super::{ProviderAdapter, ProviderKind};
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
use quorum_domain::{Model, ProviderConfig};
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
    pub fn new(providers: Vec<Arc<dyn ProviderAdapter>>, config: &ProviderConfig) -> Self {
        let mut explicit_model_routing = HashMap::new();

        for (model_name, provider_name) in &config.routing {
            // provider_name
            // からProviderKindを特定するロジックprovidersの中からprovider_nameに一致するものを探す
            let target_kind = match provider_name.as_str() {
                "copilot" => ProviderKind::Copilot,
                "anthropic" => ProviderKind::Anthropic,
                "openai" => ProviderKind::OpenAi,
                "bedrock" => ProviderKind::Bedrock,
                "azure" => ProviderKind::Azure,
                _ => continue, // Skip unknown provider names
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
                "anthropic" => ProviderKind::Anthropic,
                "openai" => ProviderKind::OpenAi,
                "bedrock" => ProviderKind::Bedrock,
                "azure" => ProviderKind::Azure,
                _ => ProviderKind::Copilot,
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
        // 1. Explicit routing table (from config [providers.routing])
        if let Some(&idx) = self.explicit_model_routing.get(model.as_str()) {
            return Ok(self.providers[idx].as_ref());
        }

        // 2. Model family auto-inference
        let inferred_kind = if model.is_claude() {
            Some(ProviderKind::Anthropic)
        } else if model.is_gpt() {
            Some(ProviderKind::OpenAi)
        } else {
            None
        };
        if let Some(ref kind) = inferred_kind
            && let Some(p) = self.providers.iter().find(|p| p.kind() == *kind)
        {
            return Ok(p.as_ref());
        }

        // 3. Default provider kind
        if let Some(p) = self
            .providers
            .iter()
            .find(|p| p.kind() == self.default_kind)
        {
            return Ok(p.as_ref());
        }

        // 4. First provider fallback (Copilot)
        self.providers
            .first()
            .map(|p| p.as_ref())
            .ok_or(GatewayError::ModelNotAvailable(
                "No providers available".to_string(),
            ))
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

#[cfg(test)]
mod tests {
    use super::*;

    // -- Mock ProviderAdapter --------------------------------------------------

    struct MockProvider {
        kind: ProviderKind,
        models: Vec<Model>,
    }

    impl MockProvider {
        fn new(kind: ProviderKind) -> Arc<dyn ProviderAdapter> {
            Arc::new(Self {
                kind,
                models: vec![],
            })
        }

        fn with_models(kind: ProviderKind, models: Vec<Model>) -> Arc<dyn ProviderAdapter> {
            Arc::new(Self { kind, models })
        }
    }

    #[async_trait]
    impl ProviderAdapter for MockProvider {
        fn kind(&self) -> ProviderKind {
            self.kind.clone()
        }

        fn supports_model(&self, _model: &Model) -> bool {
            true
        }

        async fn create_session(
            &self,
            _model: &Model,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            Err(GatewayError::RequestFailed(format!("{:?}", self.kind)))
        }

        async fn create_session_with_system_prompt(
            &self,
            _model: &Model,
            _system_prompt: &str,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            Err(GatewayError::RequestFailed(format!("{:?}", self.kind)))
        }

        async fn create_text_only_session(
            &self,
            _model: &Model,
            _system_prompt: &str,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            Err(GatewayError::RequestFailed(format!("{:?}", self.kind)))
        }

        async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
            Ok(self.models.clone())
        }
    }

    // -- Helpers ---------------------------------------------------------------

    fn default_config() -> ProviderConfig {
        ProviderConfig::default()
    }

    fn config_with_default(default: &str) -> ProviderConfig {
        ProviderConfig {
            default: Some(default.to_string()),
            ..Default::default()
        }
    }

    // -- resolve_provider routing priority tests -------------------------------

    #[test]
    fn explicit_routing_takes_highest_priority() {
        // Even though claude-sonnet-4.5 would auto-infer to Anthropic,
        // explicit routing to Copilot should win.
        let providers = vec![
            MockProvider::new(ProviderKind::Copilot),
            MockProvider::new(ProviderKind::Anthropic),
        ];
        let mut routing = HashMap::new();
        routing.insert("claude-sonnet-4.5".to_string(), "copilot".to_string());
        let config = ProviderConfig {
            routing,
            ..Default::default()
        };
        let gw = RoutingGateway::new(providers, &config);

        let provider = gw.resolve_provider(&Model::ClaudeSonnet45).unwrap();
        assert_eq!(provider.kind(), ProviderKind::Copilot);
    }

    #[test]
    fn claude_model_auto_infers_to_anthropic() {
        let providers = vec![
            MockProvider::new(ProviderKind::Copilot),
            MockProvider::new(ProviderKind::Anthropic),
        ];
        let gw = RoutingGateway::new(providers, &default_config());

        let provider = gw.resolve_provider(&Model::ClaudeOpus45).unwrap();
        assert_eq!(provider.kind(), ProviderKind::Anthropic);
    }

    #[test]
    fn gpt_model_auto_infers_to_openai() {
        let providers = vec![
            MockProvider::new(ProviderKind::Copilot),
            MockProvider::new(ProviderKind::OpenAi),
        ];
        let gw = RoutingGateway::new(providers, &default_config());

        let provider = gw.resolve_provider(&Model::Gpt52Codex).unwrap();
        assert_eq!(provider.kind(), ProviderKind::OpenAi);
    }

    #[test]
    fn falls_back_to_default_kind_when_no_family_match() {
        // Gemini is neither Claude nor GPT, so auto-inference gives None.
        // Should fall back to configured default (Anthropic).
        let providers = vec![
            MockProvider::new(ProviderKind::Copilot),
            MockProvider::new(ProviderKind::Anthropic),
        ];
        let gw = RoutingGateway::new(providers, &config_with_default("anthropic"));

        let provider = gw.resolve_provider(&Model::Gemini3Pro).unwrap();
        assert_eq!(provider.kind(), ProviderKind::Anthropic);
    }

    #[test]
    fn falls_back_to_first_provider_when_default_kind_unavailable() {
        // Default is Anthropic but only Copilot is registered.
        let providers = vec![MockProvider::new(ProviderKind::Copilot)];
        let gw = RoutingGateway::new(providers, &config_with_default("anthropic"));

        let provider = gw.resolve_provider(&Model::Gemini3Pro).unwrap();
        assert_eq!(provider.kind(), ProviderKind::Copilot);
    }

    #[test]
    fn claude_falls_back_through_default_when_no_anthropic_provider() {
        // Claude → tries Anthropic (not found) → default (Copilot) → found
        let providers = vec![MockProvider::new(ProviderKind::Copilot)];
        let gw = RoutingGateway::new(providers, &default_config());

        let provider = gw.resolve_provider(&Model::ClaudeSonnet45).unwrap();
        assert_eq!(provider.kind(), ProviderKind::Copilot);
    }

    #[test]
    fn empty_providers_returns_model_not_available() {
        let gw = RoutingGateway::new(vec![], &default_config());

        let result = gw.resolve_provider(&Model::ClaudeSonnet45);
        assert!(matches!(result, Err(GatewayError::ModelNotAvailable(_))));
    }

    #[test]
    fn unknown_routing_provider_name_is_ignored() {
        let providers = vec![MockProvider::new(ProviderKind::Copilot)];
        let mut routing = HashMap::new();
        routing.insert(
            "claude-sonnet-4.5".to_string(),
            "nonexistent-provider".to_string(),
        );
        let config = ProviderConfig {
            routing,
            ..Default::default()
        };
        let gw = RoutingGateway::new(providers, &config);

        // The unknown entry should be skipped during construction
        assert!(gw.explicit_model_routing.is_empty());
    }

    // -- LlmGateway trait integration tests ------------------------------------

    #[tokio::test]
    async fn available_models_aggregates_from_all_providers() {
        let providers = vec![
            MockProvider::with_models(ProviderKind::Copilot, vec![Model::Gpt52Codex, Model::Gpt41]),
            MockProvider::with_models(ProviderKind::Anthropic, vec![Model::ClaudeSonnet45]),
        ];
        let gw = RoutingGateway::new(providers, &default_config());

        let models = gw.available_models().await.unwrap();
        assert_eq!(models.len(), 3);
        assert!(models.contains(&Model::Gpt52Codex));
        assert!(models.contains(&Model::Gpt41));
        assert!(models.contains(&Model::ClaudeSonnet45));
    }
}
