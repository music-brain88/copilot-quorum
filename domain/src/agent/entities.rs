//! Agent domain entities.
//!
//! This module contains the core entities for the autonomous agent system:
//!
//! - [`AgentState`] - Tracks the complete state of an agent execution
//! - [`AgentConfig`] - Configuration for agent behavior including HiL settings
//! - [`Plan`] - A plan consisting of tasks to execute
//! - [`Task`] - A single unit of work within a plan
//! - [`HilMode`] - Human-in-the-loop mode for handling plan revision limits
//! - [`HumanDecision`] - User's decision when intervention is required
//!
//! # Human-in-the-Loop (HiL)
//!
//! When quorum cannot reach consensus after `max_plan_revisions` attempts,
//! the agent behavior is determined by [`HilMode`]:
//!
//! - `Interactive` - Prompt user via `HumanInterventionPort` (application layer)
//! - `AutoReject` - Automatically abort the agent
//! - `AutoApprove` - Automatically approve (use with caution!)

use super::tool_execution::ToolExecution;
use super::value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought};
use crate::core::model::Model;
use crate::orchestration::mode::ConsensusLevel;
use crate::orchestration::scope::PhaseScope;
use crate::orchestration::strategy::OrchestrationStrategy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Human-in-the-loop mode for handling plan revision limits.
///
/// When quorum review repeatedly rejects a plan (exceeding `max_plan_revisions`),
/// this mode determines what action to take.
///
/// # Configuration
///
/// Set via `quorum.toml`:
/// ```toml
/// [agent]
/// hil_mode = "interactive"  # or "auto_reject", "auto_approve"
/// ```
///
/// # Examples
///
/// ```
/// use quorum_domain::HilMode;
///
/// let mode: HilMode = "interactive".parse().unwrap();
/// assert_eq!(mode, HilMode::Interactive);
/// assert_eq!(mode.as_str(), "interactive");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HilMode {
    /// Prompt user for decision (default).
    ///
    /// When plan revision limit is exceeded, the agent will call
    /// `HumanInterventionPort::request_intervention()` to get user input.
    /// The user can choose to approve, reject, or edit the plan.
    #[default]
    Interactive,

    /// Automatically reject/abort if revision limit exceeded.
    ///
    /// This is the safest non-interactive mode. The agent will return
    /// `RunAgentError::HumanRejected` when the limit is reached.
    AutoReject,

    /// Automatically approve last plan (risky - use with caution!).
    ///
    /// **Warning**: This bypasses quorum consensus and may execute
    /// a plan that multiple models rejected. Only use in controlled
    /// environments or when you're confident the rejections are false positives.
    AutoApprove,
}

impl HilMode {
    /// Returns the string representation of this mode.
    pub fn as_str(&self) -> &str {
        match self {
            HilMode::Interactive => "interactive",
            HilMode::AutoReject => "auto_reject",
            HilMode::AutoApprove => "auto_approve",
        }
    }
}

impl std::str::FromStr for HilMode {
    type Err = String;

    /// Parses a string into a HilMode.
    ///
    /// Accepts lowercase variants with or without underscores:
    /// - "interactive"
    /// - "auto_reject" or "autoreject"
    /// - "auto_approve" or "autoapprove"
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "interactive" => Ok(HilMode::Interactive),
            "auto_reject" | "autoreject" => Ok(HilMode::AutoReject),
            "auto_approve" | "autoapprove" => Ok(HilMode::AutoApprove),
            _ => Err(format!("Invalid HilMode: {}", s)),
        }
    }
}

impl std::fmt::Display for HilMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Human decision when plan revision limit is exceeded.
///
/// This enum represents the possible actions a user can take when
/// the agent requires human intervention due to repeated plan rejections.
///
/// # See Also
///
/// - `HumanInterventionPort` (in application layer)
/// - [`HilMode::Interactive`]
#[derive(Debug, Clone)]
pub enum HumanDecision {
    /// Approve the current plan and execute.
    ///
    /// The agent will proceed with executing the plan despite
    /// quorum rejection. Use when you believe the rejections
    /// are false positives.
    Approve,

    /// Reject and abort the agent.
    ///
    /// The agent will return `RunAgentError::HumanRejected`.
    /// Use when the plan is fundamentally flawed.
    Reject,

    /// Edit the plan manually (provides new plan).
    ///
    /// **Note**: This variant is reserved for future implementation.
    /// Currently, the UI will prompt users to use `/approve` or `/reject`.
    Edit(Plan),
}

/// Phase of agent execution
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentPhase {
    /// Gathering context about the project/codebase
    ContextGathering,
    /// Planning the approach to solve the task
    Planning,
    /// Reviewing the plan (with quorum)
    PlanReview,
    /// Executing tasks
    Executing,
    /// Reviewing an action (with quorum)
    ActionReview,
    /// Final review of results (optional, with quorum)
    FinalReview,
    /// Agent has completed
    Completed,
    /// Agent has failed
    Failed,
}

impl AgentPhase {
    pub fn as_str(&self) -> &str {
        match self {
            AgentPhase::ContextGathering => "context_gathering",
            AgentPhase::Planning => "planning",
            AgentPhase::PlanReview => "plan_review",
            AgentPhase::Executing => "executing",
            AgentPhase::ActionReview => "action_review",
            AgentPhase::FinalReview => "final_review",
            AgentPhase::Completed => "completed",
            AgentPhase::Failed => "failed",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            AgentPhase::ContextGathering => "Context Gathering",
            AgentPhase::Planning => "Planning",
            AgentPhase::PlanReview => "Plan Review",
            AgentPhase::Executing => "Executing",
            AgentPhase::ActionReview => "Action Review",
            AgentPhase::FinalReview => "Final Review",
            AgentPhase::Completed => "Completed",
            AgentPhase::Failed => "Failed",
        }
    }

    /// Check if this phase involves quorum voting
    pub fn requires_quorum(&self) -> bool {
        matches!(
            self,
            AgentPhase::PlanReview | AgentPhase::ActionReview | AgentPhase::FinalReview
        )
    }
}

impl std::fmt::Display for AgentPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Status of a task
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TaskStatus {
    /// Task is waiting to be executed
    #[default]
    Pending,
    /// Task is currently being executed
    InProgress,
    /// Task completed successfully
    Completed,
    /// Task failed
    Failed,
    /// Task was skipped
    Skipped,
    /// Task is blocked by quorum review
    AwaitingReview,
}

impl TaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Skipped => "skipped",
            TaskStatus::AwaitingReview => "awaiting_review",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Skipped
        )
    }
}

/// A single task within a plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier for this task
    pub id: TaskId,
    /// Human-readable description of what this task does
    pub description: String,
    /// The tool to use (if any)
    pub tool_name: Option<String>,
    /// Arguments for the tool
    pub tool_args: HashMap<String, serde_json::Value>,
    /// Current status
    pub status: TaskStatus,
    /// Result of execution (if completed)
    pub result: Option<TaskResult>,
    /// Whether this task requires quorum review before execution
    pub requires_review: bool,
    /// Tasks that must complete before this one (task IDs)
    pub depends_on: Vec<TaskId>,
    /// Timestamp (ms since epoch) when this task started executing
    #[serde(default)]
    pub started_at: Option<u64>,
    /// Timestamp (ms since epoch) when this task finished executing
    #[serde(default)]
    pub completed_at: Option<u64>,
    /// Tool executions performed during this task (Native Tool Use loop)
    #[serde(default)]
    pub tool_executions: Vec<ToolExecution>,
}

impl Task {
    pub fn new(id: impl Into<TaskId>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            tool_name: None,
            tool_args: HashMap::new(),
            status: TaskStatus::Pending,
            result: None,
            requires_review: false,
            depends_on: Vec::new(),
            started_at: None,
            completed_at: None,
            tool_executions: Vec::new(),
        }
    }

    pub fn with_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_name = Some(tool_name.into());
        self
    }

    pub fn with_arg(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.tool_args.insert(key.into(), value.into());
        self
    }

    pub fn with_review(mut self) -> Self {
        self.requires_review = true;
        self
    }

    pub fn with_dependency(mut self, task_id: impl Into<TaskId>) -> Self {
        self.depends_on.push(task_id.into());
        self
    }

    pub fn is_ready(&self, resolved_tasks: &[TaskId]) -> bool {
        self.status == TaskStatus::Pending
            && self
                .depends_on
                .iter()
                .all(|dep| resolved_tasks.contains(dep))
    }

    pub fn mark_in_progress(&mut self) {
        self.status = TaskStatus::InProgress;
        self.started_at = Some(current_timestamp());
    }

    pub fn mark_completed(&mut self, result: TaskResult) {
        self.status = TaskStatus::Completed;
        self.result = Some(result);
        self.completed_at = Some(current_timestamp());
    }

    pub fn mark_failed(&mut self, result: TaskResult) {
        self.status = TaskStatus::Failed;
        self.result = Some(result);
        self.completed_at = Some(current_timestamp());
    }

    pub fn mark_skipped(&mut self) {
        self.status = TaskStatus::Skipped;
        self.completed_at = Some(current_timestamp());
    }

    /// Duration in milliseconds from start to completion.
    pub fn duration_ms(&self) -> Option<u64> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end.saturating_sub(start)),
            _ => None,
        }
    }

    /// Add a tool execution to this task.
    pub fn add_tool_execution(&mut self, execution: ToolExecution) {
        self.tool_executions.push(execution);
    }

    /// Get the most recent tool execution.
    pub fn latest_tool_execution(&self) -> Option<&ToolExecution> {
        self.tool_executions.last()
    }

    /// Get a mutable reference to a tool execution by ID.
    pub fn get_tool_execution_mut(
        &mut self,
        id: &super::tool_execution::ToolExecutionId,
    ) -> Option<&mut ToolExecution> {
        self.tool_executions.iter_mut().find(|e| &e.id == id)
    }
}

/// A single model's vote in a quorum review
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVote {
    /// Model identifier
    pub model: String,
    /// Whether this model approved
    pub approved: bool,
    /// Reasoning/feedback from this model
    pub reasoning: String,
}

impl ModelVote {
    pub fn new(model: impl Into<String>, approved: bool, reasoning: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            approved,
            reasoning: reasoning.into(),
        }
    }

    /// Create an approval vote
    pub fn approve(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, true, reasoning)
    }

    /// Create a rejection vote
    pub fn reject(model: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(model, false, reasoning)
    }
}

/// A record of a single review round in quorum voting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRound {
    /// Round number (1-indexed)
    pub round: usize,
    /// Whether this round resulted in approval
    pub approved: bool,
    /// Individual model votes
    pub votes: Vec<ModelVote>,
    /// Timestamp of this review
    pub timestamp: u64,
}

impl ReviewRound {
    pub fn new(round: usize, approved: bool, votes: Vec<ModelVote>) -> Self {
        Self {
            round,
            approved,
            votes,
            timestamp: current_timestamp(),
        }
    }

    /// Count of approving votes
    pub fn approve_count(&self) -> usize {
        self.votes.iter().filter(|v| v.approved).count()
    }

    /// Count of rejecting votes
    pub fn reject_count(&self) -> usize {
        self.votes.iter().filter(|v| !v.approved).count()
    }

    /// Whether this was a unanimous decision
    pub fn is_unanimous(&self) -> bool {
        let approve_count = self.approve_count();
        approve_count == self.votes.len() || approve_count == 0
    }

    /// Generate a visual vote summary (e.g., "[●●○]")
    pub fn vote_summary(&self) -> String {
        let mut summary = String::from("[");
        for vote in &self.votes {
            summary.push(if vote.approved { '●' } else { '○' });
        }
        summary.push(']');
        summary
    }
}

/// A plan consisting of multiple tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Overall goal/objective of the plan
    pub objective: String,
    /// Reasoning for this approach
    pub reasoning: String,
    /// Ordered list of tasks to execute
    pub tasks: Vec<Task>,
    /// Whether this plan has been approved (via quorum)
    pub approved: bool,
    /// Feedback from plan review (if any)
    pub review_feedback: Option<String>,
    /// History of review rounds
    pub review_history: Vec<ReviewRound>,
}

impl Plan {
    pub fn new(objective: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self {
            objective: objective.into(),
            reasoning: reasoning.into(),
            tasks: Vec::new(),
            approved: false,
            review_feedback: None,
            review_history: Vec::new(),
        }
    }

    /// Add a review round to the history
    pub fn add_review_round(&mut self, round: ReviewRound) {
        self.review_history.push(round);
    }

    /// Get the number of revision attempts (rejected rounds)
    pub fn revision_count(&self) -> usize {
        self.review_history.iter().filter(|r| !r.approved).count()
    }

    pub fn with_task(mut self, task: Task) -> Self {
        self.add_task(task);
        self
    }

    pub fn add_task(&mut self, task: Task) {
        if self.tasks.iter().any(|t| t.id == task.id) {
            let new_id = TaskId::new(format!("{}-{}", task.id, self.tasks.len() + 1));
            let mut renamed = task;
            renamed.id = new_id;
            self.tasks.push(renamed);
        } else {
            self.tasks.push(task);
        }
    }

    pub fn approve(&mut self) {
        self.approved = true;
    }

    pub fn reject(&mut self, feedback: impl Into<String>) {
        self.approved = false;
        self.review_feedback = Some(feedback.into());
    }

    /// Get the next task that is ready to execute.
    ///
    /// A task is ready when all its dependencies have reached a terminal state
    /// (Completed, Failed, or Skipped). This ensures that tasks are not blocked
    /// indefinitely when a dependency fails.
    pub fn next_task(&self) -> Option<&Task> {
        let resolved: Vec<TaskId> = self
            .tasks
            .iter()
            .filter(|t| t.status.is_terminal())
            .map(|t| t.id.clone())
            .collect();

        self.tasks.iter().find(|t| t.is_ready(&resolved))
    }

    /// Get a mutable reference to a task by ID
    pub fn get_task_mut(&mut self, id: &TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| &t.id == id)
    }

    /// Check if all tasks are complete
    pub fn is_complete(&self) -> bool {
        self.tasks.iter().all(|t| t.status.is_terminal())
    }

    /// Get completion progress (completed / total)
    pub fn progress(&self) -> (usize, usize) {
        let completed = self.tasks.iter().filter(|t| t.status.is_terminal()).count();
        (completed, self.tasks.len())
    }
}

// ==================== Ensemble Planning Types ====================

/// A plan candidate from ensemble planning
///
/// Represents a plan generated by one model during ensemble planning,
/// along with votes received from other models. Each candidate tracks
/// its originating model, the plan itself, and scores from voting.
///
/// # Voting Process
///
/// During ensemble planning:
/// 1. Each model generates a plan independently
/// 2. Each model votes on other models' plans (1-10 score)
/// 3. Votes are aggregated to select the best plan
///
/// # Example
///
/// ```
/// use quorum_domain::agent::{PlanCandidate, Plan};
/// use quorum_domain::Model;
///
/// // Create a candidate from Claude's plan
/// let plan = Plan::new("Update README", "Edit the file");
/// let mut candidate = PlanCandidate::new(Model::ClaudeSonnet45, plan);
///
/// // Other models vote on this plan
/// candidate.add_vote("GPT-5.2", 8.0);
/// candidate.add_vote("Gemini-3", 7.0);
///
/// // Calculate average score
/// assert_eq!(candidate.average_score(), 7.5);
/// assert_eq!(candidate.vote_count(), 2);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCandidate {
    /// Model that generated this plan
    pub model: Model,
    /// The generated plan
    pub plan: Plan,
    /// Votes received from other models (model name -> score 1-10)
    pub votes: HashMap<String, f64>,
}

impl PlanCandidate {
    /// Create a new plan candidate
    pub fn new(model: Model, plan: Plan) -> Self {
        Self {
            model,
            plan,
            votes: HashMap::new(),
        }
    }

    /// Add a vote from another model
    pub fn add_vote(&mut self, model: impl Into<String>, score: f64) {
        self.votes.insert(model.into(), score);
    }

    /// Calculate the average score from all votes
    pub fn average_score(&self) -> f64 {
        if self.votes.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.votes.values().sum();
        sum / self.votes.len() as f64
    }

    /// Get the number of votes received
    pub fn vote_count(&self) -> usize {
        self.votes.len()
    }

    /// Get a formatted summary of votes (e.g., "GPT:8/10, Gemini:7/10")
    pub fn vote_summary(&self) -> String {
        self.votes
            .iter()
            .map(|(model, score)| format!("{}:{}/10", model, *score as i32))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Result of ensemble planning
///
/// Contains all plan candidates with their votes and the selected winner.
/// Use [`EnsemblePlanResult::select_best`] to automatically select the
/// plan with the highest average score.
///
/// # Ensemble Planning Flow
///
/// ```text
/// Step 1: Independent Generation
///   Model A → Plan A
///   Model B → Plan B
///   Model C → Plan C
///
/// Step 2: Voting
///   B votes on A: 8/10    A votes on B: 6/10
///   C votes on A: 7/10    C votes on B: 7/10
///   A votes on C: 6/10    B votes on C: 5/10
///
/// Step 3: Selection
///   Plan A: avg 7.5/10 → SELECTED
///   Plan B: avg 6.5/10
///   Plan C: avg 5.5/10
/// ```
///
/// # Example
///
/// ```
/// use quorum_domain::agent::{PlanCandidate, Plan, EnsemblePlanResult};
/// use quorum_domain::Model;
///
/// // Create candidates with votes
/// let mut candidate1 = PlanCandidate::new(
///     Model::ClaudeSonnet45,
///     Plan::new("Plan A", "Reasoning A")
/// );
/// candidate1.add_vote("GPT", 8.0);
///
/// let mut candidate2 = PlanCandidate::new(
///     Model::Gpt52Codex,
///     Plan::new("Plan B", "Reasoning B")
/// );
/// candidate2.add_vote("Claude", 6.0);
///
/// // Select the best plan
/// let result = EnsemblePlanResult::select_best(vec![candidate1, candidate2]);
///
/// // Plan A has higher score, so it's selected
/// assert_eq!(result.selected_index, 0);
/// let selected = result.selected().unwrap();
/// assert_eq!(selected.model, Model::ClaudeSonnet45);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsemblePlanResult {
    /// All plan candidates with their votes
    pub candidates: Vec<PlanCandidate>,
    /// Index of the selected plan in candidates
    pub selected_index: usize,
}

impl EnsemblePlanResult {
    /// Create a new ensemble plan result
    pub fn new(candidates: Vec<PlanCandidate>, selected_index: usize) -> Self {
        Self {
            candidates,
            selected_index,
        }
    }

    /// Get the selected plan
    pub fn selected(&self) -> Option<&PlanCandidate> {
        self.candidates.get(self.selected_index)
    }

    /// Get the selected plan (owned)
    pub fn into_selected(mut self) -> Option<PlanCandidate> {
        if self.selected_index < self.candidates.len() {
            Some(self.candidates.swap_remove(self.selected_index))
        } else {
            None
        }
    }

    /// Select the best plan based on average scores
    pub fn select_best(candidates: Vec<PlanCandidate>) -> Self {
        let selected_index = candidates
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.average_score()
                    .partial_cmp(&b.average_score())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0);

        Self {
            candidates,
            selected_index,
        }
    }

    /// Get a summary of all candidates and their scores
    pub fn summary(&self) -> String {
        let mut summary = String::new();
        for (i, candidate) in self.candidates.iter().enumerate() {
            let marker = if i == self.selected_index { "→" } else { " " };
            summary.push_str(&format!(
                "{} Plan {} ({}): avg {:.1}/10\n",
                marker,
                i + 1,
                candidate.model,
                candidate.average_score()
            ));
        }
        summary
    }
}

/// Configuration for agent behavior with role-based model selection
///
/// # Role-based Model Configuration
///
/// Different phases of agent execution have different requirements:
///
/// - **Exploration**: Uses `exploration_model` (default: Haiku - info collection + low-risk tools)
/// - **Decision**: Uses `decision_model` (default: Sonnet - planning + high-risk tool decisions)
/// - **Reviews**: Uses `review_models` (default: [Sonnet, GPT-5.2] - quality judgments)
///
/// # Example
///
/// ```toml
/// [agent]
/// exploration_model = "claude-haiku-4.5"  # Context gathering + low-risk tools
/// decision_model = "claude-sonnet-4.5"    # Planning + high-risk tool decisions
/// review_models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    // ==================== Role-based Model Configuration ====================
    /// Model for exploration: context gathering + low-risk tool execution
    /// (default: Haiku - info collection and read-only ops are cheap)
    pub exploration_model: Model,
    /// Model for decisions: planning + high-risk tool execution
    /// (default: Sonnet - needs strong reasoning for plans and write operations)
    pub decision_model: Model,
    /// Models for review phases: plan review, action review, final review
    /// (default: [Sonnet, GPT-5.2] - quality judgments require high performance)
    pub review_models: Vec<Model>,

    // ==================== Consensus & Orchestration Configuration ====================
    /// Consensus level: Solo or Ensemble (the single user-facing mode axis)
    /// (default: Solo - single model driven)
    pub consensus_level: ConsensusLevel,
    /// Phase scope: Full, Fast, or PlanOnly (orthogonal to consensus level)
    /// (default: Full - all phases included)
    pub phase_scope: PhaseScope,
    /// Orchestration strategy: Quorum or Debate (how multi-model discussion is conducted)
    /// (default: Quorum - equal discussion → review → synthesis)
    pub orchestration_strategy: OrchestrationStrategy,

    // ==================== Behavior Configuration ====================
    /// Whether to require plan review (always true by design)
    pub require_plan_review: bool,
    /// Whether to require final review
    pub require_final_review: bool,
    /// Maximum number of execution iterations
    pub max_iterations: usize,
    /// Working directory for tool execution
    pub working_dir: Option<String>,
    /// Maximum number of retries for tool validation errors
    pub max_tool_retries: usize,
    /// Maximum number of plan revisions before human intervention
    pub max_plan_revisions: usize,
    /// Human-in-the-loop mode for handling revision limits
    pub hil_mode: HilMode,
    /// Maximum number of tool use turns in a single Native Tool Use loop.
    ///
    /// Each turn consists of: LLM response with tool calls → execute tools → send results.
    /// The loop stops when the LLM finishes (stop_reason != ToolUse) or this limit is reached.
    /// Only used in the Native Tool Use path; ignored for prompt-based execution.
    pub max_tool_turns: usize,
    /// Timeout for each ensemble session's plan generation.
    ///
    /// When a model exceeds this timeout during ensemble planning, it is recorded
    /// as timed out and retried once after a backoff period. This works around
    /// Copilot CLI's internal serialization of concurrent `session.send` requests.
    ///
    /// Default: 180 seconds. Set to `None` to disable the timeout.
    pub ensemble_session_timeout: Option<Duration>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            // Role-based defaults (cost-optimized)
            exploration_model: Model::ClaudeHaiku45, // Cheap: info collection + low-risk tools
            decision_model: Model::ClaudeSonnet45, // High performance: planning + high-risk decisions
            review_models: vec![Model::ClaudeSonnet45, Model::Gpt52Codex], // High performance: safety

            // Consensus & orchestration defaults
            consensus_level: ConsensusLevel::Solo,
            phase_scope: PhaseScope::Full,
            orchestration_strategy: OrchestrationStrategy::default(),

            // Behavior defaults
            require_plan_review: true, // Always required
            require_final_review: false,
            max_iterations: 50,
            working_dir: None,
            max_tool_retries: 2,
            max_plan_revisions: 3,
            hil_mode: HilMode::Interactive,
            max_tool_turns: 10,
            ensemble_session_timeout: Some(Duration::from_secs(180)),
        }
    }
}

impl AgentConfig {
    /// Create a new AgentConfig with a specific decision model
    ///
    /// Other role models will use defaults. Use builder methods to customize.
    pub fn new(decision_model: Model) -> Self {
        Self {
            decision_model,
            ..Default::default()
        }
    }

    // ==================== Role-based Model Builders ====================

    /// Set the model for exploration phase (context gathering + low-risk tools)
    pub fn with_exploration_model(mut self, model: Model) -> Self {
        self.exploration_model = model;
        self
    }

    /// Set the model for decision phase (planning + high-risk tool decisions)
    pub fn with_decision_model(mut self, model: Model) -> Self {
        self.decision_model = model;
        self
    }

    /// Set the models for review phases (plan review, action review, final review)
    pub fn with_review_models(mut self, models: Vec<Model>) -> Self {
        self.review_models = models;
        self
    }

    // ==================== Consensus & Orchestration Builders ====================

    /// Set the consensus level (Solo or Ensemble)
    pub fn with_consensus_level(mut self, level: ConsensusLevel) -> Self {
        self.consensus_level = level;
        self
    }

    /// Set the phase scope (Full, Fast, or PlanOnly)
    pub fn with_phase_scope(mut self, scope: PhaseScope) -> Self {
        self.phase_scope = scope;
        self
    }

    /// Set the orchestration strategy (Quorum or Debate)
    pub fn with_orchestration_strategy(mut self, strategy: OrchestrationStrategy) -> Self {
        self.orchestration_strategy = strategy;
        self
    }

    /// Enable ensemble mode
    ///
    /// Shorthand for `with_consensus_level(ConsensusLevel::Ensemble)`
    pub fn with_ensemble(self) -> Self {
        self.with_consensus_level(ConsensusLevel::Ensemble)
    }

    /// Get the planning approach derived from the consensus level
    pub fn planning_approach(&self) -> crate::orchestration::mode::PlanningApproach {
        self.consensus_level.planning_approach()
    }

    // ==================== Legacy Compatibility ====================

    /// Get the primary model (for backward compatibility)
    ///
    /// Returns the decision model, which was previously used as the primary model.
    #[deprecated(since = "0.6.0", note = "Use decision_model directly")]
    pub fn primary_model(&self) -> &Model {
        &self.decision_model
    }

    /// Get the quorum models (for backward compatibility)
    ///
    /// Returns the review models, which are used for quorum voting.
    #[deprecated(since = "0.6.0", note = "Use review_models directly")]
    pub fn quorum_models(&self) -> &[Model] {
        &self.review_models
    }

    // ==================== Behavior Builders ====================

    pub fn with_final_review(mut self) -> Self {
        self.require_final_review = true;
        self
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Skip plan review (for CI/scripting use cases)
    pub fn with_skip_plan_review(mut self) -> Self {
        self.require_plan_review = false;
        self
    }

    /// Set maximum tool retries for validation errors
    pub fn with_max_tool_retries(mut self, max: usize) -> Self {
        self.max_tool_retries = max;
        self
    }

    /// Set maximum plan revisions before human intervention
    pub fn with_max_plan_revisions(mut self, max: usize) -> Self {
        self.max_plan_revisions = max;
        self
    }

    /// Set human-in-the-loop mode
    pub fn with_hil_mode(mut self, mode: HilMode) -> Self {
        self.hil_mode = mode;
        self
    }

    /// Set maximum tool use turns for Native Tool Use loop
    pub fn with_max_tool_turns(mut self, max: usize) -> Self {
        self.max_tool_turns = max;
        self
    }

    /// Set the timeout for ensemble session plan generation
    pub fn with_ensemble_session_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.ensemble_session_timeout = timeout;
        self
    }
}

/// State of an agent execution (Entity).
///
/// Tracks the complete state of an autonomous agent run, including:
/// - Current execution phase
/// - Gathered project context
/// - The execution plan and its approval status
/// - Agent's reasoning history (thoughts)
/// - Iteration count for loop limits
/// - Plan revision count for HiL (Human-in-the-Loop) triggering
#[derive(Debug, Clone)]
pub struct AgentState {
    /// Unique identifier for this agent run
    pub id: AgentId,
    /// The user's original request
    pub request: String,
    /// Configuration for this agent
    pub config: AgentConfig,
    /// Current phase of execution
    pub phase: AgentPhase,
    /// Gathered context about the project
    pub context: AgentContext,
    /// The current plan (if created)
    pub plan: Option<Plan>,
    /// History of thoughts/reasoning
    pub thoughts: Vec<Thought>,
    /// Number of iterations executed
    pub iteration_count: usize,
    /// Number of plan revisions (rejections) - tracked separately from Plan
    /// because Plan is recreated on each revision attempt
    pub plan_revision_count: usize,
    /// Error message if failed
    pub error: Option<String>,
}

impl AgentState {
    /// Creates a new agent state starting in the ContextGathering phase.
    pub fn new(id: impl Into<AgentId>, request: impl Into<String>, config: AgentConfig) -> Self {
        Self {
            id: id.into(),
            request: request.into(),
            config,
            phase: AgentPhase::ContextGathering,
            context: AgentContext::default(),
            plan: None,
            thoughts: Vec::new(),
            iteration_count: 0,
            plan_revision_count: 0,
            error: None,
        }
    }

    /// Records a reasoning step in the agent's thought history.
    pub fn add_thought(&mut self, thought: Thought) {
        self.thoughts.push(thought);
    }

    /// Sets the execution plan and transitions to PlanReview phase.
    pub fn set_plan(&mut self, plan: Plan) {
        self.plan = Some(plan);
        self.phase = AgentPhase::PlanReview;
    }

    /// Marks the plan as approved and transitions to Executing phase.
    pub fn approve_plan(&mut self) {
        if let Some(plan) = &mut self.plan {
            plan.approve();
            self.phase = AgentPhase::Executing;
        }
    }

    /// Marks the plan as rejected and returns to Planning phase for revision.
    ///
    /// Increments `plan_revision_count` which is used to determine when
    /// Human-in-the-Loop intervention is required.
    pub fn reject_plan(&mut self, feedback: impl Into<String>) {
        if let Some(plan) = &mut self.plan {
            plan.reject(feedback);
            // Go back to planning to revise
            self.phase = AgentPhase::Planning;
        }
        // Track revision count at state level (Plan is recreated each revision)
        self.plan_revision_count += 1;
    }

    /// Manually sets the execution phase.
    pub fn set_phase(&mut self, phase: AgentPhase) {
        self.phase = phase;
    }

    /// Marks the agent as failed with an error message.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.phase = AgentPhase::Failed;
    }

    /// Marks the agent as successfully completed.
    pub fn complete(&mut self) {
        self.phase = AgentPhase::Completed;
    }

    /// Increments iteration count and returns `true` if within limits.
    ///
    /// Used to prevent infinite loops during planning and execution.
    pub fn increment_iteration(&mut self) -> bool {
        self.iteration_count += 1;
        self.iteration_count <= self.config.max_iterations
    }

    /// Returns `true` if the agent has finished (completed or failed).
    pub fn is_finished(&self) -> bool {
        matches!(self.phase, AgentPhase::Completed | AgentPhase::Failed)
    }
}

/// Get current timestamp in milliseconds
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_phase_quorum() {
        assert!(!AgentPhase::Planning.requires_quorum());
        assert!(AgentPhase::PlanReview.requires_quorum());
        assert!(AgentPhase::ActionReview.requires_quorum());
        assert!(!AgentPhase::Executing.requires_quorum());
    }

    #[test]
    fn test_task_status() {
        assert!(!TaskStatus::Pending.is_terminal());
        assert!(!TaskStatus::InProgress.is_terminal());
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
    }

    #[test]
    fn test_task_creation() {
        let task = Task::new("task-1", "Read the config file")
            .with_tool("read_file")
            .with_arg("path", "/config.toml")
            .with_review();

        assert_eq!(task.id.as_str(), "task-1");
        assert_eq!(task.tool_name, Some("read_file".to_string()));
        assert!(task.requires_review);
    }

    #[test]
    fn test_task_dependencies() {
        let task1 = Task::new("task-1", "First task");
        let task2 = Task::new("task-2", "Second task").with_dependency("task-1");

        assert!(task1.is_ready(&[]));
        assert!(!task2.is_ready(&[]));
        assert!(task2.is_ready(&["task-1".into()]));
    }

    #[test]
    fn test_plan_progress() {
        let mut plan = Plan::new("Test objective", "Test reasoning")
            .with_task(Task::new("1", "Task 1"))
            .with_task(Task::new("2", "Task 2"))
            .with_task(Task::new("3", "Task 3"));

        assert_eq!(plan.progress(), (0, 3));
        assert!(!plan.is_complete());

        plan.tasks[0].status = TaskStatus::Completed;
        plan.tasks[1].status = TaskStatus::Failed;
        assert_eq!(plan.progress(), (2, 3));
        assert!(!plan.is_complete());

        plan.tasks[2].status = TaskStatus::Skipped;
        assert!(plan.is_complete());
    }

    #[test]
    fn test_agent_state_lifecycle() {
        let config = AgentConfig::default();
        let mut state = AgentState::new("agent-1", "Update the README", config);

        assert_eq!(state.phase, AgentPhase::ContextGathering);
        assert!(!state.is_finished());

        state.set_phase(AgentPhase::Planning);
        let plan = Plan::new("Update README", "Edit the file");
        state.set_plan(plan);

        assert_eq!(state.phase, AgentPhase::PlanReview);

        state.approve_plan();
        assert_eq!(state.phase, AgentPhase::Executing);

        state.complete();
        assert!(state.is_finished());
    }

    #[test]
    fn test_agent_iteration_limit() {
        let config = AgentConfig::default().with_max_iterations(3);
        let mut state = AgentState::new("agent-1", "Test", config);

        assert!(state.increment_iteration()); // 1
        assert!(state.increment_iteration()); // 2
        assert!(state.increment_iteration()); // 3
        assert!(!state.increment_iteration()); // 4 - exceeds limit
    }

    #[test]
    fn test_hil_mode() {
        assert_eq!(HilMode::Interactive.as_str(), "interactive");
        assert_eq!(HilMode::AutoReject.as_str(), "auto_reject");
        assert_eq!(HilMode::AutoApprove.as_str(), "auto_approve");

        assert_eq!(
            "interactive".parse::<HilMode>().ok(),
            Some(HilMode::Interactive)
        );
        assert_eq!(
            "auto_reject".parse::<HilMode>().ok(),
            Some(HilMode::AutoReject)
        );
        assert_eq!(
            "autoapprove".parse::<HilMode>().ok(),
            Some(HilMode::AutoApprove)
        );
        assert!("invalid".parse::<HilMode>().is_err());
    }

    #[test]
    fn test_agent_config_hil_defaults() {
        let config = AgentConfig::default();
        assert_eq!(config.max_plan_revisions, 3);
        assert_eq!(config.hil_mode, HilMode::Interactive);
    }

    #[test]
    fn test_agent_config_hil_builders() {
        let config = AgentConfig::default()
            .with_max_plan_revisions(5)
            .with_hil_mode(HilMode::AutoReject);

        assert_eq!(config.max_plan_revisions, 5);
        assert_eq!(config.hil_mode, HilMode::AutoReject);
    }

    #[test]
    fn test_plan_revision_count() {
        let mut plan = Plan::new("Test", "Reasoning");
        assert_eq!(plan.revision_count(), 0);

        // Add rejected round
        plan.add_review_round(ReviewRound::new(1, false, vec![]));
        assert_eq!(plan.revision_count(), 1);

        // Add approved round
        plan.add_review_round(ReviewRound::new(2, true, vec![]));
        assert_eq!(plan.revision_count(), 1); // Still 1, approved doesn't count

        // Add another rejected round
        plan.add_review_round(ReviewRound::new(3, false, vec![]));
        assert_eq!(plan.revision_count(), 2);
    }

    #[test]
    fn test_agent_state_plan_revision_count() {
        let config = AgentConfig::default();
        let mut state = AgentState::new("agent-1", "Test request", config);

        // Initially zero
        assert_eq!(state.plan_revision_count, 0);

        // Set a plan
        let plan = Plan::new("Test objective", "Test reasoning");
        state.set_plan(plan);
        assert_eq!(state.phase, AgentPhase::PlanReview);

        // Reject the plan - should increment plan_revision_count
        state.reject_plan("First rejection feedback");
        assert_eq!(state.plan_revision_count, 1);
        assert_eq!(state.phase, AgentPhase::Planning);

        // Create and set a new plan (simulating revision)
        let plan2 = Plan::new("Revised objective", "Revised reasoning");
        state.set_plan(plan2);

        // Reject again
        state.reject_plan("Second rejection");
        assert_eq!(state.plan_revision_count, 2);

        // Third rejection
        let plan3 = Plan::new("Third attempt", "Third reasoning");
        state.set_plan(plan3);
        state.reject_plan("Third rejection");
        assert_eq!(state.plan_revision_count, 3);

        // This would trigger HiL with default max_plan_revisions = 3
        assert!(state.plan_revision_count >= state.config.max_plan_revisions);
    }

    #[test]
    fn test_reject_plan_without_plan() {
        let config = AgentConfig::default();
        let mut state = AgentState::new("agent-1", "Test", config);

        // Rejecting without a plan should still increment counter
        // (defensive behavior - counter tracks attempts)
        state.reject_plan("No plan feedback");
        assert_eq!(state.plan_revision_count, 1);
    }

    #[test]
    fn test_agent_config_role_based_defaults() {
        let config = AgentConfig::default();

        // Exploration uses cheap model (context gathering + low-risk tools)
        assert_eq!(config.exploration_model, Model::ClaudeHaiku45);
        // Decision uses high-performance model (planning + high-risk decisions)
        assert_eq!(config.decision_model, Model::ClaudeSonnet45);
        // Reviews use multiple high-performance models
        assert_eq!(config.review_models.len(), 2);
        assert!(config.review_models.contains(&Model::ClaudeSonnet45));
        assert!(config.review_models.contains(&Model::Gpt52Codex));
    }

    #[test]
    fn test_agent_config_role_based_builders() {
        let config = AgentConfig::default()
            .with_exploration_model(Model::ClaudeSonnet45)
            .with_decision_model(Model::ClaudeOpus45)
            .with_review_models(vec![Model::ClaudeOpus45, Model::Gemini3Pro]);

        assert_eq!(config.exploration_model, Model::ClaudeSonnet45);
        assert_eq!(config.decision_model, Model::ClaudeOpus45);
        assert_eq!(config.review_models.len(), 2);
        assert!(config.review_models.contains(&Model::ClaudeOpus45));
        assert!(config.review_models.contains(&Model::Gemini3Pro));
    }

    #[test]
    fn test_agent_config_new_sets_decision_model() {
        let config = AgentConfig::new(Model::ClaudeOpus45);

        // new() sets the decision model, others use defaults
        assert_eq!(config.decision_model, Model::ClaudeOpus45);
        assert_eq!(config.exploration_model, Model::ClaudeHaiku45);
    }

    #[test]
    #[allow(deprecated)]
    fn test_agent_config_legacy_compatibility() {
        let config = AgentConfig::default()
            .with_decision_model(Model::ClaudeOpus45)
            .with_review_models(vec![Model::Gpt52Codex]);

        // Legacy accessors should work
        assert_eq!(config.primary_model(), &Model::ClaudeOpus45);
        assert_eq!(config.quorum_models(), &[Model::Gpt52Codex]);
    }

    // ==================== Ensemble Planning Tests ====================

    #[test]
    fn test_consensus_level_default() {
        let config = AgentConfig::default();
        assert_eq!(config.consensus_level, ConsensusLevel::Solo);
        assert_eq!(config.phase_scope, PhaseScope::Full);
    }

    #[test]
    fn test_consensus_level_ensemble() {
        let config = AgentConfig::default().with_ensemble();
        assert_eq!(config.consensus_level, ConsensusLevel::Ensemble);
        assert!(config.planning_approach().is_ensemble());
    }

    #[test]
    fn test_agent_config_phase_scope() {
        let config = AgentConfig::default().with_phase_scope(PhaseScope::Fast);
        assert_eq!(config.phase_scope, PhaseScope::Fast);

        let config = AgentConfig::default().with_phase_scope(PhaseScope::PlanOnly);
        assert_eq!(config.phase_scope, PhaseScope::PlanOnly);
    }

    #[test]
    fn test_agent_config_planning_approach() {
        let solo_config = AgentConfig::default();
        assert!(!solo_config.planning_approach().is_ensemble());

        let ensemble_config = AgentConfig::default().with_consensus_level(ConsensusLevel::Ensemble);
        assert!(ensemble_config.planning_approach().is_ensemble());
    }

    #[test]
    fn test_plan_candidate() {
        let plan = Plan::new("Test objective", "Test reasoning");
        let mut candidate = PlanCandidate::new(Model::ClaudeSonnet45, plan);

        assert_eq!(candidate.vote_count(), 0);
        assert_eq!(candidate.average_score(), 0.0);

        candidate.add_vote("GPT-5.2", 8.0);
        candidate.add_vote("Gemini-3", 7.0);

        assert_eq!(candidate.vote_count(), 2);
        assert_eq!(candidate.average_score(), 7.5);
        assert!(candidate.vote_summary().contains("GPT-5.2:8/10"));
        assert!(candidate.vote_summary().contains("Gemini-3:7/10"));
    }

    #[test]
    fn test_ensemble_plan_result() {
        let plan1 = Plan::new("Plan 1", "Reasoning 1");
        let plan2 = Plan::new("Plan 2", "Reasoning 2");

        let mut candidate1 = PlanCandidate::new(Model::ClaudeSonnet45, plan1);
        candidate1.add_vote("GPT", 6.0);

        let mut candidate2 = PlanCandidate::new(Model::Gpt52Codex, plan2);
        candidate2.add_vote("Claude", 8.0);

        let result = EnsemblePlanResult::select_best(vec![candidate1, candidate2]);

        // candidate2 has higher score (8.0 vs 6.0)
        assert_eq!(result.selected_index, 1);
        assert!(result.selected().is_some());
        assert_eq!(result.selected().unwrap().model, Model::Gpt52Codex);
    }

    #[test]
    fn test_ensemble_plan_result_summary() {
        let plan = Plan::new("Test", "Reasoning");
        let mut candidate = PlanCandidate::new(Model::ClaudeSonnet45, plan);
        candidate.add_vote("GPT", 7.0);

        let result = EnsemblePlanResult::new(vec![candidate], 0);
        let summary = result.summary();

        assert!(summary.contains("Plan 1"));
        assert!(summary.contains("avg 7.0/10"));
    }

    #[test]
    fn test_next_task_with_dependency() {
        let mut plan = Plan::new("Test", "Reasoning")
            .with_task(Task::new("task-1", "First task"))
            .with_task(Task::new("task-2", "Second task").with_dependency("task-1"));

        // task-1 is ready (no deps), task-2 is blocked
        let next = plan.next_task().unwrap();
        assert_eq!(next.id.as_str(), "task-1");

        // After task-1 completes, task-2 becomes ready
        plan.tasks[0].status = TaskStatus::Completed;
        let next = plan.next_task().unwrap();
        assert_eq!(next.id.as_str(), "task-2");
    }

    #[test]
    fn test_next_task_with_failed_dependency() {
        let mut plan = Plan::new("Test", "Reasoning")
            .with_task(Task::new("task-1", "First task"))
            .with_task(Task::new("task-2", "Second task").with_dependency("task-1"));

        // task-1 fails — task-2 should still become ready (dependency resolved)
        plan.tasks[0].mark_failed(TaskResult::failure("error occurred"));
        let next = plan.next_task().unwrap();
        assert_eq!(next.id.as_str(), "task-2");
    }

    #[test]
    fn test_next_task_with_skipped_dependency() {
        let mut plan = Plan::new("Test", "Reasoning")
            .with_task(Task::new("task-1", "First task"))
            .with_task(Task::new("task-2", "Second task").with_dependency("task-1"));

        // task-1 skipped — task-2 should still become ready
        plan.tasks[0].mark_skipped();
        let next = plan.next_task().unwrap();
        assert_eq!(next.id.as_str(), "task-2");
    }

    #[test]
    fn test_next_task_blocked_by_in_progress() {
        let mut plan = Plan::new("Test", "Reasoning")
            .with_task(Task::new("task-1", "First task"))
            .with_task(Task::new("task-2", "Second task").with_dependency("task-1"));

        // task-1 in progress — task-2 should NOT be ready yet
        plan.tasks[0].mark_in_progress();
        // next_task should return None (task-1 is InProgress, task-2 is blocked)
        assert!(plan.next_task().is_none());
    }

    #[test]
    fn test_plan_add_task_deduplicates_ids() {
        let mut plan = Plan::new("Test", "Reasoning");
        plan.add_task(Task::new("1", "First task"));
        plan.add_task(Task::new("1", "Duplicate ID task"));

        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].id, TaskId::new("1"));
        // Duplicate gets renamed to "1-2"
        assert_eq!(plan.tasks[1].id, TaskId::new("1-2"));
    }
}
