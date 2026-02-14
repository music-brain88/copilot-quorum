//! Orchestration strategy definitions and execution traits.
//!
//! [`OrchestrationStrategy`] is an enum representing different discussion strategies,
//! each carrying its own configuration. [`StrategyExecutor`] is the trait for executing
//! a strategy against LLM models.

use crate::core::error::DomainError;
use crate::core::model::Model;
use crate::core::question::Question;
use crate::orchestration::entities::{Phase, QuorumConfig};
use crate::orchestration::value_objects::QuorumResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Callback for progress updates during orchestration
pub trait ProgressNotifier: Send + Sync {
    /// Called when a phase starts
    fn on_phase_start(&self, phase: &Phase, total_tasks: usize);

    /// Called when a task completes within a phase
    fn on_task_complete(&self, phase: &Phase, model: &Model, success: bool);

    /// Called when a phase completes
    fn on_phase_complete(&self, phase: &Phase);
}

/// No-op progress notifier
pub struct NoProgress;

impl ProgressNotifier for NoProgress {
    fn on_phase_start(&self, _phase: &Phase, _total_tasks: usize) {}
    fn on_task_complete(&self, _phase: &Phase, _model: &Model, _success: bool) {}
    fn on_phase_complete(&self, _phase: &Phase) {}
}

/// Gateway trait for LLM communication
///
/// This is defined in the domain layer but implemented in infrastructure.
/// It provides the interface for orchestration strategies to communicate with LLMs.
#[async_trait]
pub trait LlmGateway: Send + Sync {
    /// Error type for gateway operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Send a query to a model and get a response
    async fn query(
        &self,
        model: &Model,
        system_prompt: Option<&str>,
        user_prompt: &str,
    ) -> Result<String, Self::Error>;
}

/// Trait for executing orchestration strategies.
///
/// Different executors implement different flows for the Quorum discussion.
/// Examples:
/// - ThreePhaseStrategy: Initial → Review → Synthesis
/// - FastStrategy: Initial → Synthesis (no review)
/// - DebateStrategy: Models discuss and debate with each other
#[async_trait]
pub trait StrategyExecutor: Send + Sync {
    /// Get the name of this executor
    fn name(&self) -> &'static str;

    /// Get the phases this executor will run
    fn phases(&self) -> Vec<Phase>;

    /// Execute the orchestration strategy
    async fn execute<G: LlmGateway>(
        &self,
        question: &Question,
        models: &[Model],
        moderator: &Model,
        gateway: &G,
        notifier: &dyn ProgressNotifier,
    ) -> Result<QuorumResult, DomainError>;
}

/// Orchestration strategy — determines how multi-model discussion is conducted.
///
/// Each variant carries its own configuration. This is orthogonal to
/// [`ConsensusLevel`](super::mode::ConsensusLevel): strategies define *how*
/// models discuss, while consensus level defines *whether* multiple models
/// participate at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrchestrationStrategy {
    /// Quorum strategy: equal discussion → review → synthesis.
    /// Uses the existing [`QuorumConfig`] for configuration.
    Quorum(QuorumConfig),
    /// Debate strategy: adversarial discussion → consensus building.
    Debate(DebateConfig),
}

impl Default for OrchestrationStrategy {
    fn default() -> Self {
        OrchestrationStrategy::Quorum(QuorumConfig::default())
    }
}

impl OrchestrationStrategy {
    /// Get the models participating in this strategy
    pub fn models(&self) -> &[Model] {
        match self {
            OrchestrationStrategy::Quorum(config) => &config.models,
            OrchestrationStrategy::Debate(config) => &config.models,
        }
    }

    /// Get the moderator model for this strategy
    pub fn moderator(&self) -> Option<&Model> {
        match self {
            OrchestrationStrategy::Quorum(config) => config.get_moderator(),
            OrchestrationStrategy::Debate(config) => config.moderator.as_ref(),
        }
    }

    /// Get the phases this strategy will execute
    pub fn phases(&self) -> Vec<Phase> {
        match self {
            OrchestrationStrategy::Quorum(config) => {
                if config.enable_review {
                    vec![Phase::Initial, Phase::Review, Phase::Synthesis]
                } else {
                    vec![Phase::Initial, Phase::Synthesis]
                }
            }
            OrchestrationStrategy::Debate(_) => {
                vec![Phase::Initial, Phase::Review, Phase::Synthesis]
            }
        }
    }

    /// Get the strategy name as a string
    pub fn name(&self) -> &'static str {
        match self {
            OrchestrationStrategy::Quorum(_) => "quorum",
            OrchestrationStrategy::Debate(_) => "debate",
        }
    }
}

impl std::fmt::Display for OrchestrationStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Configuration for the Debate strategy.
///
/// Debate is an adversarial discussion format where models argue
/// opposing positions, moderated by an optional moderator model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebateConfig {
    /// Models participating in the debate
    pub models: Vec<Model>,
    /// Optional moderator to guide the debate
    pub moderator: Option<Model>,
    /// Intensity of the debate
    pub intensity: DebateIntensity,
    /// Whether third-party models can interject during the debate
    pub allow_interjection: bool,
    /// Maximum number of debate rounds
    pub max_rounds: usize,
}

impl Default for DebateConfig {
    fn default() -> Self {
        Self {
            models: Model::default_models(),
            moderator: None,
            intensity: DebateIntensity::default(),
            allow_interjection: false,
            max_rounds: 3,
        }
    }
}

/// Intensity level for debate strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DebateIntensity {
    /// Mild: collaborative discussion with gentle pushback
    #[default]
    Mild,
    /// Strong: aggressive counterarguments and challenge
    Strong,
}

impl std::fmt::Display for DebateIntensity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DebateIntensity::Mild => write!(f, "mild"),
            DebateIntensity::Strong => write!(f, "strong"),
        }
    }
}

impl std::str::FromStr for DebateIntensity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mild" => Ok(DebateIntensity::Mild),
            "strong" => Ok(DebateIntensity::Strong),
            _ => Err(format!("Invalid DebateIntensity: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_default() {
        let strategy = OrchestrationStrategy::default();
        assert_eq!(strategy.name(), "quorum");
    }

    #[test]
    fn test_strategy_quorum_phases() {
        let strategy = OrchestrationStrategy::Quorum(QuorumConfig::default());
        let phases = strategy.phases();
        assert_eq!(phases.len(), 3); // Initial, Review, Synthesis

        let strategy_no_review =
            OrchestrationStrategy::Quorum(QuorumConfig::default().without_review());
        let phases = strategy_no_review.phases();
        assert_eq!(phases.len(), 2); // Initial, Synthesis
    }

    #[test]
    fn test_strategy_debate_phases() {
        let strategy = OrchestrationStrategy::Debate(DebateConfig::default());
        let phases = strategy.phases();
        assert_eq!(phases.len(), 3);
    }

    #[test]
    fn test_debate_intensity() {
        assert_eq!(DebateIntensity::default(), DebateIntensity::Mild);
        assert_eq!(format!("{}", DebateIntensity::Mild), "mild");
        assert_eq!(format!("{}", DebateIntensity::Strong), "strong");
        assert_eq!(
            "mild".parse::<DebateIntensity>().ok(),
            Some(DebateIntensity::Mild)
        );
        assert_eq!(
            "strong".parse::<DebateIntensity>().ok(),
            Some(DebateIntensity::Strong)
        );
    }
}
