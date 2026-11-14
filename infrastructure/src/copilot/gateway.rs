//! Copilot LLM Gateway — entry point for all LLM communication.
//!
//! [`CopilotLlmGateway`] implements the
//! [`LlmGateway`] port
//! and is the **single factory** for creating [`CopilotSession`]s.
//!
//! Every user-facing feature goes through this gateway:
//!
//! - **Solo mode** — `create_session()` once
//! - **Quorum Discussion** — `create_session()` × N models × phases
//! - **Ensemble Planning** — `create_session()` × N² for plans + voting
//! - **Agent System** — sessions with tool definitions via `CopilotSession::send_with_tools`
//!
//! Internally, the gateway owns an [`Arc<MessageRouter>`](super::router::MessageRouter)
//! which is shared with all sessions. The router handles the actual TCP
//! communication and demultiplexing.

use crate::copilot::error::CopilotError;
use crate::copilot::router::MessageRouter;
use crate::copilot::session::CopilotSession;
use async_trait::async_trait;
use quorum_application::ConversationLogger;
use quorum_application::ports::llm_gateway::{
    GatewayError, LlmGateway, LlmSession, StreamObserver,
};
use quorum_domain::Model;
use std::sync::Arc;
use tracing::info;

/// Map a session-creation [`CopilotError`] to the port-level [`GatewayError`].
///
/// The router classifies Copilot CLI "Model X is not available" responses as
/// [`CopilotError::InvalidModel`] (see [`MessageRouter::create_session`]). We
/// preserve that distinction as [`GatewayError::ModelNotAvailable`] so callers
/// (`/init`, Quorum Discussion, Plan Review) can surface a clear "利用不可モデル"
/// hint instead of a generic session error that silently drops the model.
fn map_session_error(err: CopilotError) -> GatewayError {
    match err {
        CopilotError::InvalidModel(msg) => GatewayError::ModelNotAvailable(msg),
        other => GatewayError::SessionError(other.to_string()),
    }
}

/// Models known to be available on GitHub Copilot CLI.
///
/// This list is maintained by hand and verified against the CLI version we
/// target. Verified against Copilot CLI 1.0.91: it is every model returned by
/// the CLI's `models.list` JSON-RPC method (except the `auto` pseudo-model),
/// and `session.create` succeeded for each of them. The Claude 4.x (other than
/// Haiku 4.5), GPT-5.4 and earlier, and Gemini 3.x models that earlier CLI
/// versions offered now fail `session.create` with "Model not available".
///
/// The list is still static: this gateway does not call `models.list` at
/// runtime, so it goes stale whenever Copilot CLI rotates its lineup (#262 was
/// the same failure against 1.0.65).
///
/// Must remain a superset of [`Model::default_models`] and of every role in
/// `ModelConfig::default()` so the default configuration never references a
/// dropped model (enforced by unit tests).
fn known_available_models() -> Vec<Model> {
    vec![
        Model::ClaudeSonnet55,
        Model::ClaudeSonnet5,
        Model::ClaudeOpus55,
        Model::ClaudeOpus5,
        Model::ClaudeOpus48,
        Model::ClaudeHaiku55,
        Model::ClaudeHaiku45,
        Model::Gpt61Sol,
        Model::Gpt61Terra,
        Model::Gpt6Sol,
        Model::Gpt6Luna,
        Model::Gpt56Sol,
        Model::Gpt56Terra,
        Model::Gpt56Luna,
        Model::Gpt53Codex,
        Model::Gemini4Carbon,
        Model::Gemini4Hydrogen,
        Model::Grok47,
        Model::Grok46,
        Model::MaiCode11Flash,
    ]
}

/// LLM Gateway implementation for GitHub Copilot CLI.
///
/// Owns the [`MessageRouter`] and creates [`CopilotSession`]s on demand.
/// A single gateway instance is shared across the entire application lifetime.
pub struct CopilotLlmGateway {
    router: Arc<MessageRouter>,
}

impl CopilotLlmGateway {
    /// Create a new gateway by spawning the Copilot CLI.
    ///
    /// This is the standard production path, called during application startup
    /// in `cli/src/main.rs`.
    pub async fn new() -> Result<Self, GatewayError> {
        let router = MessageRouter::spawn()
            .await
            .map_err(|e| GatewayError::ConnectionError(e.to_string()))?;

        info!("CopilotLlmGateway initialized");

        Ok(Self { router })
    }

    /// Create a new gateway with a conversation logger for recording internal tool executions.
    ///
    /// Like [`new`](Self::new), but passes the logger through to the
    /// [`MessageRouter`] so that Copilot CLI internal tool events
    /// (e.g. `apply_patch`) are recorded in the conversation JSONL.
    ///
    /// `working_dir` sets the Copilot CLI process's working directory so its
    /// built-in tools resolve relative paths against the project instead of
    /// the CLI's session-state directory (#240).
    pub async fn new_with_logger(
        logger: Arc<dyn ConversationLogger>,
        working_dir: Option<&str>,
    ) -> Result<Self, GatewayError> {
        let router = MessageRouter::spawn_with_logger(logger, working_dir)
            .await
            .map_err(|e| GatewayError::ConnectionError(e.to_string()))?;

        info!("CopilotLlmGateway initialized (with conversation logger)");

        Ok(Self { router })
    }

    /// Create a gateway with a custom command (for testing)
    pub async fn with_command(cmd: &str) -> Result<Self, GatewayError> {
        let router = MessageRouter::spawn_with_command(cmd)
            .await
            .map_err(|e| GatewayError::ConnectionError(e.to_string()))?;

        Ok(Self { router })
    }

    /// Create a gateway with a pre-built router (useful for shared test setups).
    pub fn with_router(router: Arc<MessageRouter>) -> Self {
        Self { router }
    }

    /// Get a reference to the underlying router
    pub fn router(&self) -> &Arc<MessageRouter> {
        &self.router
    }
}

#[async_trait]
impl LlmGateway for CopilotLlmGateway {
    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
        let session = CopilotSession::new(Arc::clone(&self.router), model.clone())
            .await
            .map_err(map_session_error)?;

        Ok(Box::new(session))
    }

    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        let session = CopilotSession::new_with_system_prompt(
            Arc::clone(&self.router),
            model.clone(),
            Some(system_prompt.to_string()),
        )
        .await
        .map_err(map_session_error)?;

        Ok(Box::new(session))
    }

    async fn create_streaming_session(
        &self,
        model: &Model,
        system_prompt: &str,
        observer: StreamObserver,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        let session = CopilotSession::new_with_observer(
            Arc::clone(&self.router),
            model.clone(),
            Some(system_prompt.to_string()),
            observer,
        )
        .await
        .map_err(map_session_error)?;

        Ok(Box::new(session))
    }

    async fn create_text_only_session(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        let session = CopilotSession::new_text_only(
            Arc::clone(&self.router),
            model.clone(),
            Some(system_prompt.to_string()),
        )
        .await
        .map_err(map_session_error)?;

        Ok(Box::new(session))
    }

    async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
        Ok(known_available_models())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_invalid_model_to_model_not_available() {
        let err = map_session_error(CopilotError::InvalidModel(
            "Model \"gpt-5.2-codex\" is not available.".to_string(),
        ));
        match err {
            GatewayError::ModelNotAvailable(msg) => assert!(msg.contains("gpt-5.2-codex")),
            other => panic!("expected ModelNotAvailable, got {other:?}"),
        }
    }

    #[test]
    fn maps_other_errors_to_session_error() {
        let err = map_session_error(CopilotError::SessionNotInitialized);
        assert!(matches!(err, GatewayError::SessionError(_)));
    }

    #[test]
    fn available_models_superset_of_defaults() {
        // Regression guard for #262: every default model must be in the
        // known-available list, so the default config never silently drops a
        // model that Copilot CLI rejects.
        let available = known_available_models();
        for model in Model::default_models() {
            assert!(
                available.contains(&model),
                "default model {model} is missing from known_available_models()"
            );
        }
    }

    #[test]
    fn available_models_superset_of_default_model_config() {
        // Same guard for the role-based defaults: `:ask` broke on Copilot CLI
        // 1.0.91 because `ModelConfig::default().ask` was claude-sonnet-4.5,
        // which the CLI had stopped offering.
        let available = known_available_models();
        let defaults = quorum_domain::ModelConfig::default();
        let roles = [
            &defaults.exploration,
            &defaults.decision,
            &defaults.moderator,
            &defaults.ask,
        ]
        .into_iter()
        .chain(defaults.review.iter())
        .chain(defaults.participants.iter());
        for model in roles {
            assert!(
                available.contains(model),
                "default role model {model} is missing from known_available_models()"
            );
        }
    }

    #[test]
    fn available_models_exclude_dropped_models() {
        // The Claude 4.x models other than Haiku 4.5 were gone by Copilot CLI
        // 1.0.91. They stay in the `Model` enum only for the Bedrock provider,
        // so they must not be advertised as available on Copilot.
        let available = known_available_models();
        for dropped in [
            Model::ClaudeSonnet46,
            Model::ClaudeOpus46,
            Model::ClaudeSonnet45,
            Model::ClaudeOpus45,
            Model::ClaudeSonnet4,
        ] {
            assert!(
                !available.contains(&dropped),
                "{dropped} is no longer offered by Copilot CLI"
            );
        }
        // Identifiers of models dropped from the enum parse as `Custom`, which
        // is never in the list either.
        assert!(!available.contains(&"gpt-5.2-codex".parse::<Model>().unwrap()));
        assert!(!available.contains(&"gemini-3.1-pro-preview".parse::<Model>().unwrap()));
    }
}
