//! Shared test double for `run_quorum` strategy executor tests.

use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
use async_trait::async_trait;
use quorum_domain::Model;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// A gateway that returns pre-scripted text responses per model, in FIFO order.
///
/// Each `create_session[_with_system_prompt]` call yields a fresh session; each
/// session's `send()` pops the next queued response for that model. Returns a
/// `GatewayError` (not a panic) when a model's queue is exhausted, so tests can
/// also exercise the failure path (e.g. "no response scripted" ~= "model failed").
#[derive(Default)]
pub(super) struct ScriptedGateway {
    responses: Arc<Mutex<HashMap<Model, VecDeque<String>>>>,
}

impl ScriptedGateway {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Queue a response for `model`, returned on its next `send()` call.
    pub(super) fn respond(&self, model: Model, text: impl Into<String>) {
        self.responses
            .lock()
            .unwrap()
            .entry(model)
            .or_default()
            .push_back(text.into());
    }
}

struct ScriptedSession {
    model: Model,
    responses: Arc<Mutex<HashMap<Model, VecDeque<String>>>>,
}

#[async_trait]
impl LlmSession for ScriptedSession {
    fn model(&self) -> &Model {
        &self.model
    }

    async fn send(&self, _content: &str) -> Result<String, GatewayError> {
        self.responses
            .lock()
            .unwrap()
            .get_mut(&self.model)
            .and_then(|q| q.pop_front())
            .ok_or_else(|| {
                GatewayError::Other(format!("no scripted response left for {}", self.model))
            })
    }
}

#[async_trait]
impl LlmGateway for ScriptedGateway {
    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
        Ok(Box::new(ScriptedSession {
            model: model.clone(),
            responses: Arc::clone(&self.responses),
        }))
    }

    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        _system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.create_session(model).await
    }

    async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
        Ok(vec![])
    }
}
