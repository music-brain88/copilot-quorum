//! Interaction domain module — peer forms of user-system dialogue.
//!
//! An **Interaction** is a single unit of dialogue between the user and the
//! system. Unlike the legacy model where "agent mode" was the primary path and
//! other modes were bolted on, all three forms are treated as equal peers:
//!
//! | Form | Description | Context Default |
//! |------|-------------|-----------------|
//! | [`Agent`](InteractionForm::Agent) | Autonomous task execution with planning | `Full` |
//! | [`Ask`](InteractionForm::Ask) | Single question → answer (no tool use) | `Projected` |
//! | [`Discuss`](InteractionForm::Discuss) | Multi-model discussion / council | `Full` |
//!
//! # Nesting
//!
//! Interactions can spawn child interactions up to [`DEFAULT_MAX_NESTING_DEPTH`].
//! For example, an Agent task might spawn an Ask sub-interaction to clarify
//! requirements, or a Discuss to get multi-model input on a design decision.
//!
//! # Examples
//!
//! ```
//! use quorum_domain::interaction::{InteractionForm, InteractionId, Interaction};
//! use quorum_domain::context::ContextMode;
//!
//! let form = InteractionForm::Ask;
//! assert_eq!(form.default_context_mode(), ContextMode::Projected);
//! assert!(!form.uses_agent_policy());
//!
//! let interaction = Interaction::root(InteractionId(1), InteractionForm::Agent);
//! assert!(interaction.can_spawn());
//! assert_eq!(interaction.depth, 0);
//! ```

use crate::context::ContextMode;
use serde::{Deserialize, Serialize};

/// Maximum nesting depth for interactions.
///
/// Prevents unbounded recursion when interactions spawn children.
/// Depth 0 is the root interaction, so max depth of 3 allows 4 levels total.
pub const DEFAULT_MAX_NESTING_DEPTH: usize = 3;

/// The form of an interaction — determines behavior, context defaults, and
/// which config types are relevant.
///
/// All forms are equal peers; none is "primary" or "default".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionForm {
    /// Autonomous task execution with planning, tool use, and review.
    ///
    /// Uses: `SessionMode`, `AgentPolicy`, `ExecutionParams`
    Agent,
    /// Single question → answer. No tool use, no planning.
    ///
    /// Uses: `SessionMode` (for model selection only)
    Ask,
    /// Multi-model discussion / Quorum council.
    ///
    /// Uses: `SessionMode` (consensus + strategy)
    Discuss,
}

impl InteractionForm {
    /// The default [`ContextMode`] for this interaction form.
    ///
    /// - `Agent` → `Full` (needs full project awareness for planning)
    /// - `Ask` → `Projected` (focused question, focused context)
    /// - `Discuss` → `Full` (council needs full picture)
    pub fn default_context_mode(&self) -> ContextMode {
        match self {
            InteractionForm::Agent => ContextMode::Full,
            InteractionForm::Ask => ContextMode::Projected,
            InteractionForm::Discuss => ContextMode::Full,
        }
    }

    /// Whether this form uses `SessionMode` for orchestration decisions.
    ///
    /// All forms use `SessionMode` (at minimum for model selection), but
    /// Agent and Discuss use it for consensus level and strategy too.
    pub fn uses_session_mode(&self) -> bool {
        true
    }

    /// Whether this form uses `AgentPolicy` (HiL, plan review, etc.).
    ///
    /// Only `Agent` needs policy — Ask and Discuss don't do planning or
    /// autonomous execution.
    pub fn uses_agent_policy(&self) -> bool {
        matches!(self, InteractionForm::Agent)
    }

    /// Whether this form uses `ExecutionParams` (iteration limits, tool turns, etc.).
    ///
    /// Only `Agent` has execution loops that need limiting.
    pub fn uses_execution_params(&self) -> bool {
        matches!(self, InteractionForm::Agent)
    }

    /// Returns the canonical string representation.
    pub fn as_str(&self) -> &str {
        match self {
            InteractionForm::Agent => "agent",
            InteractionForm::Ask => "ask",
            InteractionForm::Discuss => "discuss",
        }
    }
}

impl std::str::FromStr for InteractionForm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "agent" => Ok(InteractionForm::Agent),
            "ask" => Ok(InteractionForm::Ask),
            "discuss" | "council" => Ok(InteractionForm::Discuss),
            _ => Err(format!("Invalid InteractionForm: {}", s)),
        }
    }
}

impl std::fmt::Display for InteractionForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// InteractionId
// =============================================================================

/// Unique identifier for an interaction instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InteractionId(pub usize);

impl std::fmt::Display for InteractionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "interaction-{}", self.0)
    }
}

// =============================================================================
// Interaction
// =============================================================================

/// A single interaction instance — a unit of dialogue with form, context mode,
/// and optional parent for nesting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interaction {
    /// Unique identifier.
    pub id: InteractionId,
    /// The form of this interaction (Agent, Ask, Discuss).
    pub form: InteractionForm,
    /// How much context this interaction inherits.
    pub context_mode: ContextMode,
    /// Parent interaction (if this is a nested/child interaction).
    pub parent: Option<InteractionId>,
    /// Nesting depth (0 = root).
    pub depth: usize,
}

impl Interaction {
    /// Create a root-level interaction (depth 0, no parent).
    ///
    /// Context mode is set to the form's default.
    pub fn root(id: InteractionId, form: InteractionForm) -> Self {
        Self {
            id,
            context_mode: form.default_context_mode(),
            form,
            parent: None,
            depth: 0,
        }
    }

    /// Create a child interaction spawned from a parent.
    ///
    /// Context mode is set to the form's default, and depth is parent + 1.
    pub fn child(id: InteractionId, form: InteractionForm, parent: &Interaction) -> Self {
        Self {
            id,
            context_mode: form.default_context_mode(),
            form,
            parent: Some(parent.id),
            depth: parent.depth + 1,
        }
    }

    /// Create an interaction with an explicit context mode override.
    pub fn with_context_mode(mut self, mode: ContextMode) -> Self {
        self.context_mode = mode;
        self
    }

    /// Whether this interaction can spawn children (depth check).
    pub fn can_spawn(&self) -> bool {
        self.depth < DEFAULT_MAX_NESTING_DEPTH
    }
}

// =============================================================================
// InteractionResult
// =============================================================================

/// The result of a completed interaction, carrying form-specific output.
#[derive(Debug, Clone)]
pub enum InteractionResult {
    /// Result from an Ask interaction — a direct answer.
    AskResult {
        /// The answer text.
        answer: String,
    },
    /// Result from a Discuss interaction — synthesized discussion output.
    DiscussResult {
        /// The synthesized output from multi-model discussion.
        synthesis: String,
        /// Number of models that participated.
        participant_count: usize,
    },
    /// Result from an Agent interaction — task execution summary.
    AgentResult {
        /// Summary of what was accomplished.
        summary: String,
        /// Whether the agent completed successfully.
        success: bool,
    },
}

impl InteractionResult {
    /// Convert to a text representation suitable for injecting into a parent
    /// interaction's context.
    ///
    /// When a child interaction completes, its result can be fed back to the
    /// parent as additional context.
    pub fn to_context_injection(&self) -> String {
        match self {
            InteractionResult::AskResult { answer } => {
                format!("[Ask Result]: {}", answer)
            }
            InteractionResult::DiscussResult {
                synthesis,
                participant_count,
            } => {
                format!(
                    "[Discuss Result ({} models)]: {}",
                    participant_count, synthesis
                )
            }
            InteractionResult::AgentResult { summary, success } => {
                let status = if *success { "completed" } else { "failed" };
                format!("[Agent Result ({})]: {}", status, summary)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // InteractionForm tests
    // =========================================================================

    #[test]
    fn test_interaction_form_as_str() {
        assert_eq!(InteractionForm::Agent.as_str(), "agent");
        assert_eq!(InteractionForm::Ask.as_str(), "ask");
        assert_eq!(InteractionForm::Discuss.as_str(), "discuss");
    }

    #[test]
    fn test_interaction_form_display() {
        assert_eq!(format!("{}", InteractionForm::Agent), "agent");
        assert_eq!(format!("{}", InteractionForm::Ask), "ask");
        assert_eq!(format!("{}", InteractionForm::Discuss), "discuss");
    }

    #[test]
    fn test_interaction_form_from_str() {
        assert_eq!(
            "agent".parse::<InteractionForm>().unwrap(),
            InteractionForm::Agent
        );
        assert_eq!(
            "ask".parse::<InteractionForm>().unwrap(),
            InteractionForm::Ask
        );
        assert_eq!(
            "discuss".parse::<InteractionForm>().unwrap(),
            InteractionForm::Discuss
        );
        // "council" alias
        assert_eq!(
            "council".parse::<InteractionForm>().unwrap(),
            InteractionForm::Discuss
        );
        // Case insensitive
        assert_eq!(
            "AGENT".parse::<InteractionForm>().unwrap(),
            InteractionForm::Agent
        );
        // Invalid
        assert!("invalid".parse::<InteractionForm>().is_err());
    }

    #[test]
    fn test_interaction_form_default_context_mode() {
        assert_eq!(
            InteractionForm::Agent.default_context_mode(),
            ContextMode::Full
        );
        assert_eq!(
            InteractionForm::Ask.default_context_mode(),
            ContextMode::Projected
        );
        assert_eq!(
            InteractionForm::Discuss.default_context_mode(),
            ContextMode::Full
        );
    }

    #[test]
    fn test_interaction_form_uses_session_mode() {
        // All forms use SessionMode
        assert!(InteractionForm::Agent.uses_session_mode());
        assert!(InteractionForm::Ask.uses_session_mode());
        assert!(InteractionForm::Discuss.uses_session_mode());
    }

    #[test]
    fn test_interaction_form_uses_agent_policy() {
        assert!(InteractionForm::Agent.uses_agent_policy());
        assert!(!InteractionForm::Ask.uses_agent_policy());
        assert!(!InteractionForm::Discuss.uses_agent_policy());
    }

    #[test]
    fn test_interaction_form_uses_execution_params() {
        assert!(InteractionForm::Agent.uses_execution_params());
        assert!(!InteractionForm::Ask.uses_execution_params());
        assert!(!InteractionForm::Discuss.uses_execution_params());
    }

    #[test]
    fn test_interaction_form_serde_roundtrip() {
        for form in [
            InteractionForm::Agent,
            InteractionForm::Ask,
            InteractionForm::Discuss,
        ] {
            let json = serde_json::to_string(&form).unwrap();
            let deserialized: InteractionForm = serde_json::from_str(&json).unwrap();
            assert_eq!(form, deserialized);
        }
    }

    // =========================================================================
    // InteractionId tests
    // =========================================================================

    #[test]
    fn test_interaction_id_display() {
        assert_eq!(format!("{}", InteractionId(42)), "interaction-42");
    }

    #[test]
    fn test_interaction_id_equality() {
        assert_eq!(InteractionId(1), InteractionId(1));
        assert_ne!(InteractionId(1), InteractionId(2));
    }

    // =========================================================================
    // Interaction tests
    // =========================================================================

    #[test]
    fn test_interaction_root() {
        let interaction = Interaction::root(InteractionId(1), InteractionForm::Agent);
        assert_eq!(interaction.id, InteractionId(1));
        assert_eq!(interaction.form, InteractionForm::Agent);
        assert_eq!(interaction.context_mode, ContextMode::Full);
        assert_eq!(interaction.parent, None);
        assert_eq!(interaction.depth, 0);
    }

    #[test]
    fn test_interaction_child() {
        let parent = Interaction::root(InteractionId(1), InteractionForm::Agent);
        let child = Interaction::child(InteractionId(2), InteractionForm::Ask, &parent);

        assert_eq!(child.id, InteractionId(2));
        assert_eq!(child.form, InteractionForm::Ask);
        assert_eq!(child.context_mode, ContextMode::Projected); // Ask default
        assert_eq!(child.parent, Some(InteractionId(1)));
        assert_eq!(child.depth, 1);
    }

    #[test]
    fn test_interaction_with_context_mode() {
        let interaction = Interaction::root(InteractionId(1), InteractionForm::Ask)
            .with_context_mode(ContextMode::Full);
        assert_eq!(interaction.context_mode, ContextMode::Full);
    }

    #[test]
    fn test_interaction_can_spawn() {
        let root = Interaction::root(InteractionId(1), InteractionForm::Agent);
        assert!(root.can_spawn());

        // Build a chain up to max depth
        let mut current = root;
        for i in 1..=DEFAULT_MAX_NESTING_DEPTH {
            let child = Interaction::child(InteractionId(i + 1), InteractionForm::Ask, &current);
            if i < DEFAULT_MAX_NESTING_DEPTH {
                assert!(child.can_spawn(), "depth {} should allow spawning", i);
            } else {
                assert!(!child.can_spawn(), "depth {} should NOT allow spawning", i);
            }
            current = child;
        }
    }

    #[test]
    fn test_interaction_nesting_depth_limit() {
        // Build chain: root(0) → child(1) → child(2) → child(3)
        let root = Interaction::root(InteractionId(0), InteractionForm::Agent);
        let d1 = Interaction::child(InteractionId(1), InteractionForm::Ask, &root);
        let d2 = Interaction::child(InteractionId(2), InteractionForm::Discuss, &d1);
        let d3 = Interaction::child(InteractionId(3), InteractionForm::Agent, &d2);

        assert_eq!(d3.depth, 3);
        assert!(!d3.can_spawn()); // At DEFAULT_MAX_NESTING_DEPTH
    }

    // =========================================================================
    // InteractionResult tests
    // =========================================================================

    #[test]
    fn test_ask_result_to_context_injection() {
        let result = InteractionResult::AskResult {
            answer: "The answer is 42.".to_string(),
        };
        let injection = result.to_context_injection();
        assert_eq!(injection, "[Ask Result]: The answer is 42.");
    }

    #[test]
    fn test_discuss_result_to_context_injection() {
        let result = InteractionResult::DiscussResult {
            synthesis: "Consensus reached on approach A.".to_string(),
            participant_count: 3,
        };
        let injection = result.to_context_injection();
        assert_eq!(
            injection,
            "[Discuss Result (3 models)]: Consensus reached on approach A."
        );
    }

    #[test]
    fn test_agent_result_to_context_injection() {
        let success = InteractionResult::AgentResult {
            summary: "README updated successfully.".to_string(),
            success: true,
        };
        assert_eq!(
            success.to_context_injection(),
            "[Agent Result (completed)]: README updated successfully."
        );

        let failure = InteractionResult::AgentResult {
            summary: "Build failed.".to_string(),
            success: false,
        };
        assert_eq!(
            failure.to_context_injection(),
            "[Agent Result (failed)]: Build failed."
        );
    }
}
