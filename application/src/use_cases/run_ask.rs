//! Run Ask use case.
//!
//! Executes an Ask interaction — lightweight Q&A with read-only tool access.
//!
//! Unlike [`RunAgentUseCase`](super::run_agent::RunAgentUseCase), Ask has no
//! planning phase, no HiL review, and only uses [`RiskLevel::Low`] tools.
//! The `exploration` model handles everything.

use crate::config::ExecutionParams;
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::llm_gateway::{GatewayError, LlmGateway, ToolResultMessage};
use crate::ports::tool_executor::ToolExecutorPort;
use quorum_domain::agent::model_config::ModelConfig;
use quorum_domain::interaction::InteractionResult;
use quorum_domain::util::truncate_str;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during Ask execution.
#[derive(Error, Debug)]
pub enum RunAskError {
    #[error("Gateway error: {0}")]
    GatewayError(#[from] GatewayError),

    #[error("No response from model")]
    EmptyResponse,
}

/// Input for the [`RunAskUseCase`].
///
/// Ask uses only the `exploration` model from [`ModelConfig`] and limits
/// tool access to [`RiskLevel::Low`] operations.
#[derive(Debug, Clone)]
pub struct RunAskInput {
    /// The user's question.
    pub query: String,
    /// Model configuration — only `exploration` is used.
    pub models: ModelConfig,
    /// Execution parameters — `max_tool_turns` limits the tool loop.
    pub execution: ExecutionParams,
}

impl RunAskInput {
    pub fn new(
        query: impl Into<String>,
        models: ModelConfig,
        execution: ExecutionParams,
    ) -> Self {
        Self {
            query: query.into(),
            models,
            execution,
        }
    }
}

/// Use case for running an Ask interaction.
///
/// Executes a lightweight Q&A flow:
/// 1. Create session with the `exploration` model
/// 2. Send query with low-risk tools via [`send_with_tools()`]
/// 3. Multi-turn tool loop (low-risk only, parallel execution)
/// 4. Return [`InteractionResult::AskResult`]
pub struct RunAskUseCase<G: LlmGateway, T: ToolExecutorPort> {
    gateway: Arc<G>,
    tool_executor: Arc<T>,
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static> RunAskUseCase<G, T> {
    pub fn new(gateway: Arc<G>, tool_executor: Arc<T>) -> Self {
        Self {
            gateway,
            tool_executor,
        }
    }

    /// Execute the Ask interaction with progress callbacks.
    pub async fn execute(
        &self,
        input: RunAskInput,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<InteractionResult, RunAskError> {
        info!("Starting Ask interaction: {}", truncate_str(&input.query, 100));

        // Create session with the exploration model
        let session = self
            .gateway
            .create_session(&input.models.exploration)
            .await?;

        // Build low-risk tools only
        let tools = self.tool_executor.tool_spec().to_low_risk_api_tools();

        debug!(
            "Ask: using model {}, {} low-risk tools available",
            input.models.exploration,
            tools.len()
        );

        // Initial request
        progress.on_llm_stream_start("ask");
        let mut response = session
            .send_with_tools(&input.query, &tools)
            .await
            .map_err(RunAskError::GatewayError)?;

        let text = response.text_content();
        if !text.is_empty() {
            progress.on_llm_chunk(&text);
        }
        progress.on_llm_stream_end();

        // Multi-turn tool loop (low-risk only)
        let max_turns = input.execution.max_tool_turns;
        let mut turn_count = 0;
        let mut all_text = Vec::new();

        if !text.is_empty() {
            all_text.push(text);
        }

        loop {
            let tool_calls = response.tool_calls();

            if tool_calls.is_empty() {
                break;
            }

            turn_count += 1;
            if turn_count > max_turns {
                warn!(
                    "Ask tool loop exceeded max_tool_turns ({})",
                    max_turns
                );
                break;
            }

            // All tools are low-risk — execute in parallel
            let mut futures = Vec::new();
            for call in &tool_calls {
                progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));
                futures.push(self.tool_executor.execute(call));
            }

            let results: Vec<_> = futures::future::join_all(futures).await;

            let mut tool_result_messages = Vec::new();
            for (call, result) in tool_calls.iter().zip(results) {
                let is_error = !result.is_success();
                let output = if is_error {
                    result
                        .error()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown error".to_string())
                } else {
                    result.output().unwrap_or("").to_string()
                };

                progress.on_tool_result(&call.tool_name, !is_error);

                if let Some(native_id) = call.native_id.clone() {
                    tool_result_messages.push(ToolResultMessage {
                        tool_use_id: native_id,
                        tool_name: call.tool_name.clone(),
                        output,
                        is_error,
                    });
                } else {
                    warn!(
                        "Missing native_id for tool call '{}'; skipping result.",
                        call.tool_name
                    );
                }
            }

            // Send tool results back to LLM
            debug!(
                "Ask tool turn {}/{}: sending {} tool results",
                turn_count,
                max_turns,
                tool_result_messages.len()
            );

            progress.on_llm_stream_start("ask");
            response = session
                .send_tool_results(&tool_result_messages)
                .await
                .map_err(RunAskError::GatewayError)?;

            let text = response.text_content();
            if !text.is_empty() {
                progress.on_llm_chunk(&text);
                all_text.push(text);
            }
            progress.on_llm_stream_end();
        }

        // Build the answer from collected text
        let answer = all_text.join("\n");
        if answer.is_empty() {
            return Err(RunAskError::EmptyResponse);
        }

        info!("Ask completed in {} tool turns", turn_count);

        Ok(InteractionResult::AskResult { answer })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::agent_progress::NoAgentProgress;
    use crate::ports::llm_gateway::LlmSession;
    use crate::ports::tool_executor::ToolExecutorPort;
    use async_trait::async_trait;
    use quorum_domain::session::response::{ContentBlock, LlmResponse, StopReason};
    use quorum_domain::tool::entities::{ToolCall, ToolDefinition, RiskLevel, ToolSpec};
    use quorum_domain::ToolResult;
    use quorum_domain::Model;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    // ==================== Test Mocks ====================

    struct MockSession {
        model: Model,
        responses: Mutex<VecDeque<LlmResponse>>,
    }

    impl MockSession {
        fn new(responses: Vec<LlmResponse>) -> Self {
            Self {
                model: Model::Gpt5Mini,
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmSession for MockSession {
        fn model(&self) -> &Model {
            &self.model
        }

        async fn send(&self, _content: &str) -> Result<String, GatewayError> {
            let response = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| GatewayError::Other("No more responses".to_string()))?;
            Ok(response.text_content())
        }

        async fn send_with_tools(
            &self,
            _content: &str,
            _tools: &[serde_json::Value],
        ) -> Result<LlmResponse, GatewayError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| GatewayError::Other("No more responses".to_string()))
        }

        async fn send_tool_results(
            &self,
            _results: &[ToolResultMessage],
        ) -> Result<LlmResponse, GatewayError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| GatewayError::Other("No more responses".to_string()))
        }
    }

    struct MockGateway {
        session: Mutex<Option<Box<dyn LlmSession>>>,
    }

    impl MockGateway {
        fn new(session: impl LlmSession + 'static) -> Self {
            Self {
                session: Mutex::new(Some(Box::new(session))),
            }
        }
    }

    #[async_trait]
    impl LlmGateway for MockGateway {
        async fn create_session(
            &self,
            _model: &Model,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            self.session
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| GatewayError::Other("Session already taken".to_string()))
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

    struct MockToolExecutor {
        spec: ToolSpec,
    }

    impl MockToolExecutor {
        fn new() -> Self {
            Self {
                spec: ToolSpec::new()
                    .register(ToolDefinition::new("read_file", "Read a file", RiskLevel::Low))
                    .register(ToolDefinition::new("write_file", "Write a file", RiskLevel::High)),
            }
        }
    }

    #[async_trait]
    impl ToolExecutorPort for MockToolExecutor {
        async fn execute(&self, call: &ToolCall) -> ToolResult {
            ToolResult::success(&call.tool_name, "mock output")
        }

        fn execute_sync(&self, call: &ToolCall) -> ToolResult {
            ToolResult::success(&call.tool_name, "mock output")
        }

        fn tool_spec(&self) -> &ToolSpec {
            &self.spec
        }
    }

    fn text_response(text: &str) -> LlmResponse {
        LlmResponse {
            content: vec![ContentBlock::Text(text.to_string())],
            stop_reason: Some(StopReason::EndTurn),
            model: Some("test-model".to_string()),
        }
    }

    fn tool_use_response(tool_name: &str, native_id: &str) -> LlmResponse {
        LlmResponse {
            content: vec![ContentBlock::ToolUse {
                id: native_id.to_string(),
                name: tool_name.to_string(),
                input: std::collections::HashMap::new(),
            }],
            stop_reason: Some(StopReason::ToolUse),
            model: Some("test-model".to_string()),
        }
    }

    // ==================== Tests ====================

    #[tokio::test]
    async fn test_simple_ask_no_tools() {
        let session = MockSession::new(vec![text_response("The answer is 42.")]);
        let gateway = Arc::new(MockGateway::new(session));
        let executor = Arc::new(MockToolExecutor::new());
        let use_case = RunAskUseCase::new(gateway, executor);

        let input = RunAskInput::new(
            "What is the meaning of life?",
            ModelConfig::default(),
            ExecutionParams::default(),
        );

        let result = use_case.execute(input, &NoAgentProgress).await.unwrap();

        match result {
            InteractionResult::AskResult { answer } => {
                assert_eq!(answer, "The answer is 42.");
            }
            _ => panic!("Expected AskResult"),
        }
    }

    #[tokio::test]
    async fn test_ask_with_tool_use() {
        // LLM calls read_file, then answers
        let session = MockSession::new(vec![
            tool_use_response("read_file", "toolu_1"),
            text_response("Based on the file, the answer is X."),
        ]);
        let gateway = Arc::new(MockGateway::new(session));
        let executor = Arc::new(MockToolExecutor::new());
        let use_case = RunAskUseCase::new(gateway, executor);

        let input = RunAskInput::new(
            "What's in main.rs?",
            ModelConfig::default(),
            ExecutionParams::default(),
        );

        let result = use_case.execute(input, &NoAgentProgress).await.unwrap();

        match result {
            InteractionResult::AskResult { answer } => {
                assert_eq!(answer, "Based on the file, the answer is X.");
            }
            _ => panic!("Expected AskResult"),
        }
    }

    fn text_and_tool_response(text: &str, tool_name: &str, native_id: &str) -> LlmResponse {
        LlmResponse {
            content: vec![
                ContentBlock::Text(text.to_string()),
                ContentBlock::ToolUse {
                    id: native_id.to_string(),
                    name: tool_name.to_string(),
                    input: std::collections::HashMap::new(),
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
            model: Some("test-model".to_string()),
        }
    }

    #[tokio::test]
    async fn test_ask_respects_max_tool_turns() {
        // LLM keeps using tools with partial text each turn
        let mut responses = Vec::new();
        // Initial response has text + tool
        responses.push(text_and_tool_response("Thinking...", "read_file", "toolu_0"));
        // Subsequent tool result responses also have text + tool
        for i in 1..15 {
            responses.push(text_and_tool_response(
                &format!("Still working ({})...", i),
                "read_file",
                &format!("toolu_{}", i),
            ));
        }

        let session = MockSession::new(responses);
        let gateway = Arc::new(MockGateway::new(session));
        let executor = Arc::new(MockToolExecutor::new());
        let use_case = RunAskUseCase::new(gateway, executor);

        let execution = ExecutionParams::default().with_max_tool_turns(3);
        let input = RunAskInput::new("Complex question", ModelConfig::default(), execution);

        let result = use_case.execute(input, &NoAgentProgress).await.unwrap();

        match result {
            InteractionResult::AskResult { answer } => {
                // Should contain text from the turns that ran (initial + 3 tool turns)
                assert!(answer.contains("Thinking..."));
                assert!(answer.contains("Still working (1)..."));
            }
            _ => panic!("Expected AskResult"),
        }
    }

    #[tokio::test]
    async fn test_ask_empty_response_is_error() {
        let session = MockSession::new(vec![LlmResponse {
            content: vec![],
            stop_reason: Some(StopReason::EndTurn),
            model: None,
        }]);
        let gateway = Arc::new(MockGateway::new(session));
        let executor = Arc::new(MockToolExecutor::new());
        let use_case = RunAskUseCase::new(gateway, executor);

        let input = RunAskInput::new(
            "Hello?",
            ModelConfig::default(),
            ExecutionParams::default(),
        );

        let result = use_case.execute(input, &NoAgentProgress).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RunAskError::EmptyResponse));
    }

    #[tokio::test]
    async fn test_ask_only_uses_low_risk_tools() {
        // Verify that to_low_risk_api_tools() filters correctly
        let executor = MockToolExecutor::new();
        let low_risk_tools = executor.tool_spec().to_low_risk_api_tools();

        // Should only have read_file, not write_file
        assert_eq!(low_risk_tools.len(), 1);
        assert_eq!(low_risk_tools[0]["name"], "read_file");
    }
}
