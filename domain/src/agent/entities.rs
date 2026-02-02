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

use super::value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought};
use crate::core::model::Model;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    pub fn is_ready(&self, completed_tasks: &[TaskId]) -> bool {
        self.status == TaskStatus::Pending
            && self
                .depends_on
                .iter()
                .all(|dep| completed_tasks.contains(dep))
    }

    pub fn mark_in_progress(&mut self) {
        self.status = TaskStatus::InProgress;
    }

    pub fn mark_completed(&mut self, result: TaskResult) {
        self.status = TaskStatus::Completed;
        self.result = Some(result);
    }

    pub fn mark_failed(&mut self, result: TaskResult) {
        self.status = TaskStatus::Failed;
        self.result = Some(result);
    }

    pub fn mark_skipped(&mut self) {
        self.status = TaskStatus::Skipped;
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
        self.tasks.push(task);
        self
    }

    pub fn add_task(&mut self, task: Task) {
        self.tasks.push(task);
    }

    pub fn approve(&mut self) {
        self.approved = true;
    }

    pub fn reject(&mut self, feedback: impl Into<String>) {
        self.approved = false;
        self.review_feedback = Some(feedback.into());
    }

    /// Get the next task that is ready to execute
    pub fn next_task(&self) -> Option<&Task> {
        let completed: Vec<TaskId> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .map(|t| t.id.clone())
            .collect();

        self.tasks.iter().find(|t| t.is_ready(&completed))
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

/// Configuration for agent behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Primary model for agent execution
    pub primary_model: Model,
    /// Models for quorum voting (plan review, action review)
    pub quorum_models: Vec<Model>,
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
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            primary_model: Model::ClaudeSonnet45,
            quorum_models: Model::default_models(),
            require_plan_review: true, // Always required
            require_final_review: false,
            max_iterations: 50,
            working_dir: None,
            max_tool_retries: 2,
            max_plan_revisions: 3,
            hil_mode: HilMode::Interactive,
        }
    }
}

impl AgentConfig {
    pub fn new(primary_model: Model) -> Self {
        Self {
            primary_model,
            ..Default::default()
        }
    }

    pub fn with_quorum_models(mut self, models: Vec<Model>) -> Self {
        self.quorum_models = models;
        self
    }

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
}

/// State of an agent execution (Entity).
///
/// Tracks the complete state of an autonomous agent run, including:
/// - Current execution phase
/// - Gathered project context
/// - The execution plan and its approval status
/// - Agent's reasoning history (thoughts)
/// - Iteration count for loop limits
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
    pub fn reject_plan(&mut self, feedback: impl Into<String>) {
        if let Some(plan) = &mut self.plan {
            plan.reject(feedback);
            // Go back to planning to revise
            self.phase = AgentPhase::Planning;
        }
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
}
