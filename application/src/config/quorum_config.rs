//! Quorum configuration container.
//!
//! [`QuorumConfig`] groups the split configuration types into a single
//! container that buffer controllers hold for child buffer spawning.
//!
//! # Design
//!
//! Use Cases receive only the config slices they need (honest type signatures).
//! Buffer Controllers hold the full `QuorumConfig` for propagation to child buffers.
//!
//! All configuration types are runtime-mutable via [`ConfigAccessorPort`]
//! (Lua `quorum.config` API and TUI `:config set` commands).
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
use crate::ports::config_accessor::{ConfigAccessError, ConfigAccessorPort, ConfigValue};
use crate::use_cases::run_agent::RunAgentInput;
use crate::use_cases::run_ask::RunAskInput;
use crate::use_cases::run_quorum::RunQuorumInput;
use quorum_domain::agent::validation::{ConfigIssue, Severity};
use quorum_domain::config::config_key::lookup_key;
use quorum_domain::{
    AgentPolicy, ConsensusLevel, HilMode, Model, ModelConfig, OrchestrationStrategy, OutputFormat,
    PhaseScope, SessionMode,
};

/// Configuration container for buffer controllers.
///
/// Groups configuration types and provides:
/// - Mutable accessors for all config via [`ConfigAccessorPort`]
/// - Factory methods to build Use Case inputs (`to_agent_input()`, `to_quorum_input()`)
#[derive(Debug, Clone)]
pub struct QuorumConfig {
    mode: SessionMode,
    models: ModelConfig,
    policy: AgentPolicy,
    execution: ExecutionParams,
    output_format: OutputFormat,
    color: bool,
    show_progress: bool,
    history_file: Option<String>,
}

impl Default for QuorumConfig {
    fn default() -> Self {
        Self {
            mode: SessionMode::default(),
            models: ModelConfig::default(),
            policy: AgentPolicy::default(),
            execution: ExecutionParams::default(),
            output_format: OutputFormat::default(),
            color: true,
            show_progress: true,
            history_file: None,
        }
    }
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
            output_format: OutputFormat::default(),
            color: true,
            show_progress: true,
            history_file: None,
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

    /// Mutable access to model configuration.
    pub fn models_mut(&mut self) -> &mut ModelConfig {
        &mut self.models
    }

    /// Agent behavioral policy.
    pub fn policy(&self) -> &AgentPolicy {
        &self.policy
    }

    /// Mutable access to agent policy.
    pub fn policy_mut(&mut self) -> &mut AgentPolicy {
        &mut self.policy
    }

    /// Execution loop control parameters.
    pub fn execution(&self) -> &ExecutionParams {
        &self.execution
    }

    /// Mutable access to execution parameters.
    pub fn execution_mut(&mut self) -> &mut ExecutionParams {
        &mut self.execution
    }

    /// Output format.
    pub fn output_format(&self) -> OutputFormat {
        self.output_format
    }

    /// Whether colored output is enabled.
    pub fn color(&self) -> bool {
        self.color
    }

    /// Whether progress indicators are shown.
    pub fn show_progress(&self) -> bool {
        self.show_progress
    }

    /// Path to REPL history file.
    pub fn history_file(&self) -> Option<&str> {
        self.history_file.as_deref()
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

    /// Set the output format.
    pub fn with_output_format(mut self, format: OutputFormat) -> Self {
        self.output_format = format;
        self
    }

    /// Set color output.
    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// Set progress display.
    pub fn with_show_progress(mut self, show: bool) -> Self {
        self.show_progress = show;
        self
    }

    /// Set history file path.
    pub fn with_history_file(mut self, path: Option<String>) -> Self {
        self.history_file = path;
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

impl ConfigAccessorPort for QuorumConfig {
    fn config_get(&self, key: &str) -> Result<ConfigValue, ConfigAccessError> {
        match key {
            // ---- agent.* ----
            "agent.consensus_level" => {
                Ok(ConfigValue::String(self.mode.consensus_level.to_string()))
            }
            "agent.phase_scope" => Ok(ConfigValue::String(self.mode.phase_scope.to_string())),
            "agent.strategy" => {
                let name = match &self.mode.strategy {
                    OrchestrationStrategy::Quorum(_) => "quorum",
                    OrchestrationStrategy::Debate(_) => "debate",
                };
                Ok(ConfigValue::String(name.to_string()))
            }
            "agent.hil_mode" => Ok(ConfigValue::String(self.policy.hil_mode.to_string())),
            "agent.max_plan_revisions" => {
                Ok(ConfigValue::Integer(self.policy.max_plan_revisions as i64))
            }
            // ---- models.* ----
            "models.exploration" => Ok(ConfigValue::String(self.models.exploration.to_string())),
            "models.decision" => Ok(ConfigValue::String(self.models.decision.to_string())),
            "models.review" => Ok(ConfigValue::StringList(
                self.models.review.iter().map(|m| m.to_string()).collect(),
            )),
            "models.participants" => Ok(ConfigValue::StringList(
                self.models
                    .participants
                    .iter()
                    .map(|m| m.to_string())
                    .collect(),
            )),
            "models.moderator" => Ok(ConfigValue::String(self.models.moderator.to_string())),
            "models.ask" => Ok(ConfigValue::String(self.models.ask.to_string())),
            // ---- execution.* ----
            "execution.max_iterations" => {
                Ok(ConfigValue::Integer(self.execution.max_iterations as i64))
            }
            "execution.max_tool_turns" => {
                Ok(ConfigValue::Integer(self.execution.max_tool_turns as i64))
            }
            // ---- output.* ----
            "output.format" => Ok(ConfigValue::String(self.output_format.to_string())),
            "output.color" => Ok(ConfigValue::Boolean(self.color)),
            // ---- repl.* ----
            "repl.show_progress" => Ok(ConfigValue::Boolean(self.show_progress)),
            "repl.history_file" => Ok(ConfigValue::String(
                self.history_file.clone().unwrap_or_default(),
            )),
            // ---- context_budget.* ----
            "context_budget.max_entry_bytes" => Ok(ConfigValue::Integer(
                self.execution.context_budget.max_entry_bytes() as i64,
            )),
            "context_budget.max_total_bytes" => Ok(ConfigValue::Integer(
                self.execution.context_budget.max_total_bytes() as i64,
            )),
            "context_budget.recent_full_count" => Ok(ConfigValue::Integer(
                self.execution.context_budget.recent_full_count() as i64,
            )),
            _ => Err(ConfigAccessError::UnknownKey {
                key: key.to_string(),
            }),
        }
    }

    fn config_set(
        &mut self,
        key: &str,
        value: ConfigValue,
    ) -> Result<Vec<ConfigIssue>, ConfigAccessError> {
        // Check key exists (all keys are Mutable in Phase 1.5)
        let _info = lookup_key(key).ok_or_else(|| ConfigAccessError::UnknownKey {
            key: key.to_string(),
        })?;

        match key {
            // ---- agent.* (SessionMode + AgentPolicy) ----
            "agent.consensus_level" => {
                let s = extract_string(key, value)?;
                let level = s.parse::<ConsensusLevel>().map_err(|e| {
                    ConfigAccessError::InvalidValue {
                        key: key.to_string(),
                        message: e,
                    }
                })?;
                self.mode.consensus_level = level;
                Ok(self.mode.validate_combination())
            }
            "agent.phase_scope" => {
                let s = extract_string(key, value)?;
                let scope =
                    s.parse::<PhaseScope>()
                        .map_err(|e| ConfigAccessError::InvalidValue {
                            key: key.to_string(),
                            message: e,
                        })?;
                self.mode.phase_scope = scope;
                Ok(self.mode.validate_combination())
            }
            "agent.strategy" => {
                let s = extract_string(key, value)?;
                match s.to_lowercase().as_str() {
                    "quorum" => {
                        self.mode.strategy = OrchestrationStrategy::default();
                    }
                    "debate" => {
                        self.mode.strategy = OrchestrationStrategy::Debate(
                            quorum_domain::DebateConfig::default(),
                        );
                    }
                    _ => {
                        return Err(ConfigAccessError::InvalidValue {
                            key: key.to_string(),
                            message: format!(
                                "unknown strategy '{}', valid: quorum, debate",
                                s
                            ),
                        });
                    }
                }
                Ok(self.mode.validate_combination())
            }
            "agent.hil_mode" => {
                let s = extract_string(key, value)?;
                let mode =
                    s.parse::<HilMode>()
                        .map_err(|e| ConfigAccessError::InvalidValue {
                            key: key.to_string(),
                            message: e,
                        })?;
                self.policy.hil_mode = mode;
                Ok(vec![])
            }
            "agent.max_plan_revisions" => {
                let n = extract_positive_int(key, value)?;
                self.policy.max_plan_revisions = n;
                Ok(vec![])
            }
            // ---- models.* (ModelConfig) ----
            "models.exploration" => {
                let s = extract_string(key, value)?;
                self.models.exploration = s.parse::<Model>().unwrap();
                Ok(vec![])
            }
            "models.decision" => {
                let s = extract_string(key, value)?;
                self.models.decision = s.parse::<Model>().unwrap();
                Ok(vec![])
            }
            "models.review" => {
                let list = extract_string_list(key, value)?;
                self.models.review = list.into_iter().map(|s| s.parse::<Model>().unwrap()).collect();
                Ok(vec![])
            }
            "models.participants" => {
                let list = extract_string_list(key, value)?;
                self.models.participants =
                    list.into_iter().map(|s| s.parse::<Model>().unwrap()).collect();
                Ok(vec![])
            }
            "models.moderator" => {
                let s = extract_string(key, value)?;
                self.models.moderator = s.parse::<Model>().unwrap();
                Ok(vec![])
            }
            "models.ask" => {
                let s = extract_string(key, value)?;
                self.models.ask = s.parse::<Model>().unwrap();
                Ok(vec![])
            }
            // ---- execution.* ----
            "execution.max_iterations" => {
                let n = extract_positive_int(key, value)?;
                self.execution.max_iterations = n;
                Ok(vec![])
            }
            "execution.max_tool_turns" => {
                let n = extract_positive_int(key, value)?;
                self.execution.max_tool_turns = n;
                Ok(vec![])
            }
            // ---- output.* ----
            "output.format" => {
                let s = extract_string(key, value)?;
                let format = s.parse::<OutputFormat>().map_err(|e| {
                    ConfigAccessError::InvalidValue {
                        key: key.to_string(),
                        message: e,
                    }
                })?;
                self.output_format = format;
                Ok(vec![])
            }
            "output.color" => {
                let b = extract_bool(key, value)?;
                self.color = b;
                Ok(vec![])
            }
            // ---- repl.* ----
            "repl.show_progress" => {
                let b = extract_bool(key, value)?;
                self.show_progress = b;
                Ok(vec![])
            }
            "repl.history_file" => {
                let s = extract_string(key, value)?;
                self.history_file = if s.is_empty() { None } else { Some(s) };
                Ok(vec![])
            }
            // ---- context_budget.* ----
            "context_budget.max_entry_bytes" => {
                let n = extract_positive_int(key, value)?;
                let budget = quorum_domain::ContextBudget::try_new(
                    n,
                    self.execution.context_budget.max_total_bytes(),
                    self.execution.context_budget.recent_full_count(),
                )
                .map_err(|errors| ConfigAccessError::InvalidValue {
                    key: key.to_string(),
                    message: errors.join("; "),
                })?;
                self.execution.context_budget = budget;
                Ok(vec![])
            }
            "context_budget.max_total_bytes" => {
                let n = extract_positive_int(key, value)?;
                let budget = quorum_domain::ContextBudget::try_new(
                    self.execution.context_budget.max_entry_bytes(),
                    n,
                    self.execution.context_budget.recent_full_count(),
                )
                .map_err(|errors| ConfigAccessError::InvalidValue {
                    key: key.to_string(),
                    message: errors.join("; "),
                })?;
                self.execution.context_budget = budget;
                Ok(vec![])
            }
            "context_budget.recent_full_count" => {
                let n = extract_positive_int(key, value)?;
                let budget = quorum_domain::ContextBudget::try_new(
                    self.execution.context_budget.max_entry_bytes(),
                    self.execution.context_budget.max_total_bytes(),
                    n,
                )
                .map_err(|errors| ConfigAccessError::InvalidValue {
                    key: key.to_string(),
                    message: errors.join("; "),
                })?;
                self.execution.context_budget = budget;
                Ok(vec![])
            }
            _ => Err(ConfigAccessError::UnknownKey {
                key: key.to_string(),
            }),
        }
    }

    fn config_keys(&self) -> Vec<String> {
        quorum_domain::known_keys()
            .iter()
            .map(|k| k.key.to_string())
            .collect()
    }
}

// ==================== Value Extraction Helpers ====================

fn extract_string(key: &str, value: ConfigValue) -> Result<String, ConfigAccessError> {
    match value {
        ConfigValue::String(s) => Ok(s),
        _ => Err(ConfigAccessError::InvalidValue {
            key: key.to_string(),
            message: "expected a string value".to_string(),
        }),
    }
}

fn extract_bool(key: &str, value: ConfigValue) -> Result<bool, ConfigAccessError> {
    match value {
        ConfigValue::Boolean(b) => Ok(b),
        _ => Err(ConfigAccessError::InvalidValue {
            key: key.to_string(),
            message: "expected a boolean value".to_string(),
        }),
    }
}

fn extract_positive_int(key: &str, value: ConfigValue) -> Result<usize, ConfigAccessError> {
    match value {
        ConfigValue::Integer(n) if n >= 0 => Ok(n as usize),
        ConfigValue::Integer(_) => Err(ConfigAccessError::InvalidValue {
            key: key.to_string(),
            message: "value must be non-negative".to_string(),
        }),
        _ => Err(ConfigAccessError::InvalidValue {
            key: key.to_string(),
            message: "expected an integer value".to_string(),
        }),
    }
}

fn extract_string_list(key: &str, value: ConfigValue) -> Result<Vec<String>, ConfigAccessError> {
    match value {
        ConfigValue::StringList(list) => Ok(list),
        _ => Err(ConfigAccessError::InvalidValue {
            key: key.to_string(),
            message: "expected a string list value".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::agent::validation::ConfigIssueCode;
    use quorum_domain::{ConsensusLevel, HilMode, Model, OutputFormat, PhaseScope};

    #[test]
    fn test_default() {
        let config = QuorumConfig::default();
        assert_eq!(config.mode().consensus_level, ConsensusLevel::Solo);
        assert_eq!(config.mode().phase_scope, PhaseScope::Full);
        assert_eq!(config.models().decision, Model::ClaudeSonnet45);
        assert!(!config.policy().require_final_review);
        assert_eq!(config.execution().max_iterations, 50);
        // New fields
        assert_eq!(config.output_format(), OutputFormat::Synthesis);
        assert!(config.color());
        assert!(config.show_progress());
        assert_eq!(config.history_file(), None);
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

    // ==================== ConfigAccessorPort Tests ====================

    #[test]
    fn test_config_get_consensus_level() {
        let config = QuorumConfig::default();
        let val = config.config_get("agent.consensus_level").unwrap();
        assert_eq!(val, ConfigValue::String("solo".to_string()));
    }

    #[test]
    fn test_config_get_unknown_key() {
        let config = QuorumConfig::default();
        let err = config.config_get("nonexistent").unwrap_err();
        assert!(matches!(err, ConfigAccessError::UnknownKey { .. }));
    }

    #[test]
    fn test_config_set_consensus_level() {
        let mut config = QuorumConfig::default();
        let issues = config
            .config_set(
                "agent.consensus_level",
                ConfigValue::String("ensemble".to_string()),
            )
            .unwrap();
        assert_eq!(config.mode().consensus_level, ConsensusLevel::Ensemble);
        // Solo→Ensemble with Full+Quorum is valid, no issues
        assert!(issues.is_empty());
    }

    #[test]
    fn test_config_set_phase_scope() {
        let mut config = QuorumConfig::default();
        config
            .config_set("agent.phase_scope", ConfigValue::String("fast".to_string()))
            .unwrap();
        assert_eq!(config.mode().phase_scope, PhaseScope::Fast);
    }

    #[test]
    fn test_config_set_strategy() {
        let mut config = QuorumConfig::default();
        config
            .config_set("agent.strategy", ConfigValue::String("debate".to_string()))
            .unwrap();
        assert!(matches!(
            config.mode().strategy,
            OrchestrationStrategy::Debate(_)
        ));
    }

    #[test]
    fn test_config_set_hil_mode() {
        let mut config = QuorumConfig::default();
        config
            .config_set(
                "agent.hil_mode",
                ConfigValue::String("auto_approve".to_string()),
            )
            .unwrap();
        assert_eq!(config.policy().hil_mode, HilMode::AutoApprove);
    }

    #[test]
    fn test_config_set_max_plan_revisions() {
        let mut config = QuorumConfig::default();
        config
            .config_set("agent.max_plan_revisions", ConfigValue::Integer(5))
            .unwrap();
        assert_eq!(config.policy().max_plan_revisions, 5);
    }

    #[test]
    fn test_config_set_model_exploration() {
        let mut config = QuorumConfig::default();
        config
            .config_set(
                "models.exploration",
                ConfigValue::String("claude-opus-4.5".to_string()),
            )
            .unwrap();
        assert_eq!(config.models().exploration, Model::ClaudeOpus45);
    }

    #[test]
    fn test_config_set_model_participants() {
        let mut config = QuorumConfig::default();
        config
            .config_set(
                "models.participants",
                ConfigValue::StringList(vec![
                    "claude-opus-4.5".to_string(),
                    "gpt-5.2-codex".to_string(),
                ]),
            )
            .unwrap();
        assert_eq!(
            config.models().participants,
            vec![Model::ClaudeOpus45, Model::Gpt52Codex]
        );
    }

    #[test]
    fn test_config_set_model_moderator() {
        let mut config = QuorumConfig::default();
        config
            .config_set(
                "models.moderator",
                ConfigValue::String("claude-opus-4.5".to_string()),
            )
            .unwrap();
        assert_eq!(config.models().moderator, Model::ClaudeOpus45);
    }

    #[test]
    fn test_config_set_model_ask() {
        let mut config = QuorumConfig::default();
        config
            .config_set(
                "models.ask",
                ConfigValue::String("gpt-5.2-codex".to_string()),
            )
            .unwrap();
        assert_eq!(config.models().ask, Model::Gpt52Codex);
    }

    #[test]
    fn test_config_set_execution_params() {
        let mut config = QuorumConfig::default();
        config
            .config_set("execution.max_iterations", ConfigValue::Integer(100))
            .unwrap();
        config
            .config_set("execution.max_tool_turns", ConfigValue::Integer(20))
            .unwrap();
        assert_eq!(config.execution().max_iterations, 100);
        assert_eq!(config.execution().max_tool_turns, 20);
    }

    #[test]
    fn test_config_set_output_format() {
        let mut config = QuorumConfig::default();
        config
            .config_set("output.format", ConfigValue::String("json".to_string()))
            .unwrap();
        assert_eq!(config.output_format(), OutputFormat::Json);
    }

    #[test]
    fn test_config_set_output_color() {
        let mut config = QuorumConfig::default();
        config
            .config_set("output.color", ConfigValue::Boolean(false))
            .unwrap();
        assert!(!config.color());
    }

    #[test]
    fn test_config_set_repl_show_progress() {
        let mut config = QuorumConfig::default();
        config
            .config_set("repl.show_progress", ConfigValue::Boolean(false))
            .unwrap();
        assert!(!config.show_progress());
    }

    #[test]
    fn test_config_set_repl_history_file() {
        let mut config = QuorumConfig::default();
        config
            .config_set(
                "repl.history_file",
                ConfigValue::String("/tmp/history.txt".to_string()),
            )
            .unwrap();
        assert_eq!(config.history_file(), Some("/tmp/history.txt"));
    }

    #[test]
    fn test_config_set_context_budget() {
        let mut config = QuorumConfig::default();
        config
            .config_set("context_budget.max_entry_bytes", ConfigValue::Integer(10_000))
            .unwrap();
        config
            .config_set("context_budget.max_total_bytes", ConfigValue::Integer(50_000))
            .unwrap();
        config
            .config_set("context_budget.recent_full_count", ConfigValue::Integer(5))
            .unwrap();
        assert_eq!(config.execution().context_budget.max_entry_bytes(), 10_000);
        assert_eq!(config.execution().context_budget.max_total_bytes(), 50_000);
        assert_eq!(config.execution().context_budget.recent_full_count(), 5);
    }

    #[test]
    fn test_config_set_context_budget_validation() {
        let mut config = QuorumConfig::default();
        // max_entry_bytes > max_total_bytes should fail
        let err = config
            .config_set("context_budget.max_entry_bytes", ConfigValue::Integer(100_000))
            .unwrap_err();
        assert!(matches!(err, ConfigAccessError::InvalidValue { .. }));
    }

    #[test]
    fn test_config_set_negative_int_rejected() {
        let mut config = QuorumConfig::default();
        let err = config
            .config_set("execution.max_iterations", ConfigValue::Integer(-1))
            .unwrap_err();
        assert!(matches!(err, ConfigAccessError::InvalidValue { .. }));
    }

    #[test]
    fn test_config_set_invalid_value() {
        let mut config = QuorumConfig::default();
        let err = config
            .config_set(
                "agent.consensus_level",
                ConfigValue::String("typo".to_string()),
            )
            .unwrap_err();
        assert!(matches!(err, ConfigAccessError::InvalidValue { .. }));
    }

    #[test]
    fn test_config_set_wrong_type() {
        let mut config = QuorumConfig::default();
        let err = config
            .config_set("agent.consensus_level", ConfigValue::Integer(42))
            .unwrap_err();
        assert!(matches!(err, ConfigAccessError::InvalidValue { .. }));
    }

    #[test]
    fn test_config_set_returns_validation_warnings() {
        let mut config = QuorumConfig::default();
        // Set Ensemble + Fast → should produce EnsembleWithFast warning
        config
            .config_set(
                "agent.consensus_level",
                ConfigValue::String("ensemble".to_string()),
            )
            .unwrap();
        let issues = config
            .config_set("agent.phase_scope", ConfigValue::String("fast".to_string()))
            .unwrap();
        assert!(!issues.is_empty());
        assert!(
            issues
                .iter()
                .any(|i| i.code == ConfigIssueCode::EnsembleWithFast)
        );
    }

    #[test]
    fn test_config_keys_returns_all_20() {
        let config = QuorumConfig::default();
        let keys = config.config_keys();
        assert_eq!(keys.len(), 20);
        // Spot-check new keys
        assert!(keys.contains(&"models.participants".to_string()));
        assert!(keys.contains(&"output.format".to_string()));
        assert!(keys.contains(&"repl.show_progress".to_string()));
        assert!(keys.contains(&"context_budget.max_entry_bytes".to_string()));
    }

    #[test]
    fn test_config_get_new_keys() {
        let config = QuorumConfig::default();
        // models.*
        assert!(matches!(
            config.config_get("models.participants").unwrap(),
            ConfigValue::StringList(_)
        ));
        assert!(matches!(
            config.config_get("models.moderator").unwrap(),
            ConfigValue::String(_)
        ));
        assert!(matches!(
            config.config_get("models.ask").unwrap(),
            ConfigValue::String(_)
        ));
        // output.*
        assert_eq!(
            config.config_get("output.format").unwrap(),
            ConfigValue::String("synthesis".to_string())
        );
        assert_eq!(
            config.config_get("output.color").unwrap(),
            ConfigValue::Boolean(true)
        );
        // repl.*
        assert_eq!(
            config.config_get("repl.show_progress").unwrap(),
            ConfigValue::Boolean(true)
        );
        // context_budget.*
        assert_eq!(
            config.config_get("context_budget.max_entry_bytes").unwrap(),
            ConfigValue::Integer(20_000)
        );
    }
}
