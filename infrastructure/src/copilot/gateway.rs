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
/// Copilot CLI has no model-listing endpoint, so this list is maintained by
/// hand and verified against the CLI version we target. Verified against
/// Copilot CLI 1.0.65 (#262/#263): `gpt-5.2-codex` and `gemini-3-pro-preview`
/// were removed upstream (`session.create` fails with "Model not available"),
/// and `gpt-5.4` / `gpt-5.3-codex` were added.
///
/// Must remain a superset of [`Model::default_models`] so the default
/// configuration never references a dropped model (enforced by a unit test).
fn known_available_models() -> Vec<Model> {
    vec![
        Model::ClaudeSonnet46,
        Model::ClaudeOpus46,
        Model::ClaudeSonnet45,
        Model::ClaudeHaiku45,
        Model::ClaudeOpus45,
        Model::ClaudeSonnet4,
        Model::Gpt54,
        Model::Gpt53Codex,
        Model::Gpt51CodexMax,
        Model::Gpt51Codex,
        Model::Gpt52,
        Model::Gpt51,
        Model::Gpt5,
        Model::Gpt51CodexMini,
        Model::Gpt5Mini,
        Model::Gpt41,
        Model::Gemini31Pro,
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
    fn available_models_exclude_dropped_models() {
        // gpt-5.2-codex / gemini-3-pro-preview were removed in Copilot CLI 1.0.65.
        let available = known_available_models();
        assert!(!available.contains(&Model::Gpt52Codex));
        assert!(!available.contains(&Model::Gemini3Pro));
    }
}
