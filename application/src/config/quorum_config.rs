//! Quorum configuration container.
//!
//! [`QuorumConfig`] groups the four split configuration types into a single
//! container that buffer controllers hold for child buffer spawning.
//!
//! # Design
//!
//! Use Cases receive only the config slices they need (honest type signatures).
//! Buffer Controllers hold the full `QuorumConfig` for propagation to child buffers.
//!
//! Only [`SessionMode`] is runtime-mutable (TUI commands like `/solo`, `/ens`,
//! `/fast`, `/strategy`). Other types are set at initialization and remain static.
//!
//! # Buffer Necessity Map
//!
//! | Type | Agent | Ask | Discuss |
//! |------|-------|-----|---------|
//! | `SessionMode` | Yes | No (Solo fixed) | Yes |
//! | `ModelConfig` | Yes | Yes | Yes |
//! | `AgentPolicy` | Yes | No | No |
//! | `ExecutionParams` | Yes | Yes | No |

use crate::config::ExecutionParams;
use crate::use_cases::run_agent::RunAgentInput;
use crate::use_cases::run_ask::RunAskInput;
use crate::use_cases::run_quorum::RunQuorumInput;
use quorum_domain::agent::validation::{ConfigIssue, Severity};
use quorum_domain::{AgentPolicy, ConsensusLevel, ModelConfig, SessionMode};

/// Configuration container for buffer controllers.
///
/// Groups the four split configuration types and provides:
/// - Read-only accessors for static config (models, policy, execution)
/// - Mutable accessor for runtime-mutable mode (`mode_mut()`)
/// - Factory methods to build Use Case inputs (`to_agent_input()`, `to_quorum_input()`)
#[derive(Debug, Clone, Default)]
pub struct QuorumConfig {
    mode: SessionMode,
    models: ModelConfig,
    policy: AgentPolicy,
    execution: ExecutionParams,
}

impl QuorumConfig {
    /// Create a new QuorumConfig from the four split types.
    pub fn new(
        mode: SessionMode,
        models: ModelConfig,
        policy: AgentPolicy,
        execution: ExecutionParams,
    ) -> Self {
        Self {
            mode,
            models,
            policy,
            execution,
        }
    }

    // ==================== Accessors ====================

    /// Runtime-mutable orchestration mode (read-only).
    pub fn mode(&self) -> &SessionMode {
        &self.mode
    }

    /// Runtime-mutable orchestration mode (mutable for TUI commands).
    pub fn mode_mut(&mut self) -> &mut SessionMode {
        &mut self.mode
    }

    /// Role-based model configuration.
    pub fn models(&self) -> &ModelConfig {
        &self.models
    }

    /// Agent behavioral policy.
    pub fn policy(&self) -> &AgentPolicy {
        &self.policy
    }

    /// Execution loop control parameters.
    pub fn execution(&self) -> &ExecutionParams {
        &self.execution
    }

    // ==================== Builder Methods (init-time) ====================

    /// Set the working directory.
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.execution = self.execution.with_working_dir(dir);
        self
    }

    /// Enable final review.
    pub fn with_final_review(mut self) -> Self {
        self.policy = self.policy.with_require_final_review(true);
        self
    }

    /// Set the consensus level.
    pub fn with_consensus_level(mut self, level: ConsensusLevel) -> Self {
        self.mode = self.mode.with_consensus_level(level);
        self
    }

    // ==================== Validation ====================

    /// Validate the configuration combination.
    ///
    /// Delegates to [`SessionMode::validate_combination()`].
    pub fn validate(&self) -> Vec<ConfigIssue> {
        self.mode.validate_combination()
    }

    /// Check whether any issues are errors (i.e. fatal).
    pub fn has_errors(issues: &[ConfigIssue]) -> bool {
        issues.iter().any(|i| i.severity == Severity::Error)
    }

    // ==================== Use Case Input Factories ====================

    /// Build a [`RunAgentInput`] from this config and a user request.
    pub fn to_agent_input(&self, request: impl Into<String>) -> RunAgentInput {
        RunAgentInput::new(
            request,
            self.mode.clone(),
            self.models.clone(),
            self.policy.clone(),
            self.execution.clone(),
        )
    }

    /// Build a [`RunAskInput`] for a lightweight Q&A interaction.
    ///
    /// Uses `ask` model and execution params (for `max_tool_turns`).
    /// Ask is always Solo — no `SessionMode` or `AgentPolicy` needed.
    pub fn to_ask_input(&self, query: impl Into<String>) -> RunAskInput {
        RunAskInput::new(query, self.models.clone(), self.execution.clone())
    }

    /// Build a [`RunQuorumInput`] for an ad-hoc quorum discussion.
    ///
    /// Uses `participants` models for discussion and `moderator` for synthesis.
    pub fn to_quorum_input(&self, question: impl Into<String>) -> RunQuorumInput {
        let question_str: String = question.into();
        RunQuorumInput::new(question_str, self.models.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::{ConsensusLevel, Model, PhaseScope};

    #[test]
    fn test_default() {
        let config = QuorumConfig::default();
        assert_eq!(config.mode().consensus_level, ConsensusLevel::Solo);
        assert_eq!(config.mode().phase_scope, PhaseScope::Full);
        assert_eq!(config.models().decision, Model::ClaudeSonnet45);
        assert!(!config.policy().require_final_review);
        assert_eq!(config.execution().max_iterations, 50);
    }

    #[test]
    fn test_builder_methods() {
        let config = QuorumConfig::default()
            .with_working_dir("/tmp/test")
            .with_final_review()
            .with_consensus_level(ConsensusLevel::Ensemble);

        assert_eq!(
            config.execution().working_dir,
            Some("/tmp/test".to_string())
        );
        assert!(config.policy().require_final_review);
        assert_eq!(config.mode().consensus_level, ConsensusLevel::Ensemble);
    }

    #[test]
    fn test_mode_mut() {
        let mut config = QuorumConfig::default();
        config.mode_mut().consensus_level = ConsensusLevel::Ensemble;
        config.mode_mut().phase_scope = PhaseScope::Fast;

        assert_eq!(config.mode().consensus_level, ConsensusLevel::Ensemble);
        assert_eq!(config.mode().phase_scope, PhaseScope::Fast);
    }

    #[test]
    fn test_to_agent_input() {
        let config = QuorumConfig::default()
            .with_working_dir("/project")
            .with_consensus_level(ConsensusLevel::Ensemble);

        let input = config.to_agent_input("Fix the bug");
        assert_eq!(input.request, "Fix the bug");
        assert_eq!(input.mode.consensus_level, ConsensusLevel::Ensemble);
        assert_eq!(input.execution.working_dir, Some("/project".to_string()));
    }

    #[test]
    fn test_to_ask_input() {
        let config = QuorumConfig::default().with_working_dir("/project");

        let input = config.to_ask_input("What does main.rs do?");
        assert_eq!(input.query, "What does main.rs do?");
        assert_eq!(input.models.exploration, config.models().exploration);
        assert_eq!(input.execution.working_dir, Some("/project".to_string()));
    }

    #[test]
    fn test_to_quorum_input() {
        let config = QuorumConfig::default();
        let input = config.to_quorum_input("Best approach?");
        assert_eq!(input.question.content(), "Best approach?");
        assert_eq!(
            input.models.participants.len(),
            config.models().participants.len()
        );
        assert_eq!(input.models.moderator, config.models().moderator);
    }

    #[test]
    fn test_validate_valid() {
        let config = QuorumConfig::default(); // Solo + Full + Quorum
        assert!(config.validate().is_empty());
    }

    #[test]
    fn test_validate_detects_issues() {
        let mut config = QuorumConfig::default();
        config.mode_mut().consensus_level = ConsensusLevel::Ensemble;
        config.mode_mut().phase_scope = PhaseScope::Fast;

        let issues = config.validate();
        assert!(!issues.is_empty());
        assert!(!QuorumConfig::has_errors(&issues)); // Warning only
    }
}
