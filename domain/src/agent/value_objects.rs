//! Agent domain value objects - immutable types for the autonomous Agent system.
//!
//! This module contains value objects used throughout the Agent execution flow:
//!
//! # Identifiers
//! - [`AgentId`] - Unique identifier for an agent run
//! - [`TaskId`] - Unique identifier for a task within a plan
//!
//! # Execution Data
//! - [`TaskResult`] - Outcome of a task execution (success/failure)
//! - [`AgentContext`] - Gathered information about the project
//!
//! # Reasoning
//! - [`Thought`] - A recorded reasoning step from the agent
//! - [`ThoughtType`] - Categories of agent thoughts (analysis, planning, etc.)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for an agent run.
///
/// Each agent execution session has a unique ID for tracking and correlation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(String);

impl AgentId {
    /// Creates an AgentId from an existing string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generates a new unique AgentId using a UUID-like format.
    pub fn generate() -> Self {
        Self(uuid_v4())
    }

    /// Returns the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: Into<String>> From<T> for AgentId {
    fn from(s: T) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a task within a plan.
///
/// Tasks are numbered sequentially within a plan (e.g., "1", "2", "3").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    /// Creates a TaskId from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: Into<String>> From<T> for TaskId {
    fn from(s: T) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Result of a task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Whether the task succeeded
    pub success: bool,
    /// Output/result content
    pub output: String,
    /// Error message if failed
    pub error: Option<String>,
    /// Tool result metadata (if a tool was used)
    pub tool_result: Option<crate::tool::value_objects::ToolResult>,
}

impl TaskResult {
    /// Creates a successful task result with the given output.
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
            tool_result: None,
        }
    }

    /// Creates a failed task result with an error message.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error.into()),
            tool_result: None,
        }
    }

    /// Creates a TaskResult from a tool execution result.
    ///
    /// Extracts success status, output, and error from the tool result.
    pub fn from_tool_result(tool_result: crate::tool::value_objects::ToolResult) -> Self {
        Self {
            success: tool_result.success,
            output: tool_result.output.clone().unwrap_or_default(),
            error: tool_result.error.as_ref().map(|e| e.message.clone()),
            tool_result: Some(tool_result),
        }
    }
}

/// Context gathered about the project/codebase
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentContext {
    /// Project root directory
    pub project_root: Option<String>,
    /// Detected project type (e.g., "rust", "python", "nodejs")
    pub project_type: Option<String>,
    /// Important files discovered
    pub key_files: Vec<String>,
    /// Project structure summary
    pub structure_summary: Option<String>,
    /// Additional context gathered
    pub additional: HashMap<String, String>,
}

impl AgentContext {
    /// Creates an empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the project root directory path.
    pub fn with_project_root(mut self, root: impl Into<String>) -> Self {
        self.project_root = Some(root.into());
        self
    }

    /// Sets the detected project type (e.g., "rust", "python", "nodejs").
    pub fn with_project_type(mut self, project_type: impl Into<String>) -> Self {
        self.project_type = Some(project_type.into());
        self
    }

    /// Adds an important file discovered during context gathering.
    pub fn add_key_file(&mut self, file: impl Into<String>) {
        self.key_files.push(file.into());
    }

    /// Sets the project structure summary.
    pub fn set_structure_summary(&mut self, summary: impl Into<String>) {
        self.structure_summary = Some(summary.into());
    }

    /// Adds arbitrary key-value context information.
    pub fn add_context(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.additional.insert(key.into(), value.into());
    }

    /// Formats all gathered context as a string for use in LLM prompts.
    pub fn to_prompt_context(&self) -> String {
        let mut parts = Vec::new();

        if let Some(root) = &self.project_root {
            parts.push(format!("Project Root: {}", root));
        }

        if let Some(project_type) = &self.project_type {
            parts.push(format!("Project Type: {}", project_type));
        }

        if !self.key_files.is_empty() {
            parts.push(format!("Key Files:\n- {}", self.key_files.join("\n- ")));
        }

        if let Some(summary) = &self.structure_summary {
            parts.push(format!("Structure:\n{}", summary));
        }

        for (key, value) in &self.additional {
            parts.push(format!("{}: {}", key, value));
        }

        parts.join("\n\n")
    }
}

/// A thought/reasoning step from the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    /// Type of thought
    pub thought_type: ThoughtType,
    /// The actual thought content
    pub content: String,
    /// Timestamp (for ordering)
    pub timestamp: u64,
}

/// Types of thoughts the agent can have
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThoughtType {
    /// Initial analysis of the request
    Analysis,
    /// Planning how to approach the task
    Planning,
    /// Reasoning about a decision
    Reasoning,
    /// Observing the result of an action
    Observation,
    /// Reflecting on progress or issues
    Reflection,
    /// Final conclusion or summary
    Conclusion,
}

impl ThoughtType {
    /// Returns the type as a lowercase string identifier.
    pub fn as_str(&self) -> &str {
        match self {
            ThoughtType::Analysis => "analysis",
            ThoughtType::Planning => "planning",
            ThoughtType::Reasoning => "reasoning",
            ThoughtType::Observation => "observation",
            ThoughtType::Reflection => "reflection",
            ThoughtType::Conclusion => "conclusion",
        }
    }

    /// Returns an emoji representing this thought type for display.
    pub fn emoji(&self) -> &str {
        match self {
            ThoughtType::Analysis => "🔍",
            ThoughtType::Planning => "📋",
            ThoughtType::Reasoning => "💭",
            ThoughtType::Observation => "👀",
            ThoughtType::Reflection => "🤔",
            ThoughtType::Conclusion => "✅",
        }
    }
}

impl Thought {
    /// Creates a new thought with the specified type and content.
    pub fn new(thought_type: ThoughtType, content: impl Into<String>) -> Self {
        Self {
            thought_type,
            content: content.into(),
            timestamp: current_timestamp(),
        }
    }

    /// Creates an analysis thought (initial examination of the request).
    pub fn analysis(content: impl Into<String>) -> Self {
        Self::new(ThoughtType::Analysis, content)
    }

    /// Creates a planning thought (strategizing approach).
    pub fn planning(content: impl Into<String>) -> Self {
        Self::new(ThoughtType::Planning, content)
    }

    /// Creates a reasoning thought (explaining a decision).
    pub fn reasoning(content: impl Into<String>) -> Self {
        Self::new(ThoughtType::Reasoning, content)
    }

    /// Creates an observation thought (noting action results).
    pub fn observation(content: impl Into<String>) -> Self {
        Self::new(ThoughtType::Observation, content)
    }

    /// Creates a reflection thought (considering progress or issues).
    pub fn reflection(content: impl Into<String>) -> Self {
        Self::new(ThoughtType::Reflection, content)
    }

    /// Creates a conclusion thought (final summary).
    pub fn conclusion(content: impl Into<String>) -> Self {
        Self::new(ThoughtType::Conclusion, content)
    }
}

/// Generate a simple UUID v4 (without external dependency)
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    // Simple pseudo-random based on time
    let nanos = now.as_nanos();
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (nanos >> 96) as u32,
        (nanos >> 80) as u16,
        (nanos >> 64) as u16 & 0x0fff,
        ((nanos >> 48) as u16 & 0x3fff) | 0x8000,
        (nanos & 0xffffffffffff) as u64
    )
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
    fn test_agent_id() {
        let id = AgentId::new("test-agent");
        assert_eq!(id.as_str(), "test-agent");

        let generated = AgentId::generate();
        assert!(!generated.as_str().is_empty());
    }

    #[test]
    fn test_task_id() {
        let id: TaskId = "task-1".into();
        assert_eq!(id.as_str(), "task-1");
    }

    #[test]
    fn test_task_result() {
        let success = TaskResult::success("Output content");
        assert!(success.success);
        assert_eq!(success.output, "Output content");

        let failure = TaskResult::failure("Something went wrong");
        assert!(!failure.success);
        assert_eq!(failure.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_agent_context() {
        let mut ctx = AgentContext::new()
            .with_project_root("/home/user/project")
            .with_project_type("rust");

        ctx.add_key_file("Cargo.toml");
        ctx.add_key_file("src/main.rs");
        ctx.add_context("Git Branch", "main");

        let prompt = ctx.to_prompt_context();
        assert!(prompt.contains("Project Root:"));
        assert!(prompt.contains("Project Type: rust"));
        assert!(prompt.contains("Cargo.toml"));
    }

    #[test]
    fn test_thought() {
        let thought = Thought::analysis("Analyzing the request");
        assert_eq!(thought.thought_type, ThoughtType::Analysis);
        assert_eq!(thought.content, "Analyzing the request");
        assert!(thought.timestamp > 0);
    }

    #[test]
    fn test_thought_types() {
        assert_eq!(ThoughtType::Analysis.emoji(), "🔍");
        assert_eq!(ThoughtType::Planning.emoji(), "📋");
        assert_eq!(ThoughtType::Conclusion.emoji(), "✅");
    }
}
