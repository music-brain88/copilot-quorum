//! Prompt templates for the Agent system

use crate::agent::{AgentContext, Plan, Task};
use serde_json::json;

/// Templates for generating agent prompts
pub struct AgentPromptTemplate;

impl AgentPromptTemplate {
    /// System prompt for the agent.
    ///
    /// Tool definitions are passed via the Native Tool Use API, so the system
    /// prompt only contains general agent guidelines — no tool descriptions.
    pub fn agent_system() -> String {
        r#"You are an autonomous coding agent that helps users with software engineering tasks.

## Your Capabilities

You can analyze codebases, write code, execute commands, and help with various development tasks.
You work by creating and executing plans, using tools to interact with the file system.

## Guidelines

1. **Plan First**: Always analyze the request and create a clear plan before acting
2. **Be Cautious**: High-risk operations (file writes, commands) should be well-justified
3. **Verify**: After making changes, verify they work as expected
4. **Explain**: Keep the user informed about what you're doing and why
5. **Ask When Unsure**: If requirements are unclear, ask for clarification

## Output Format

Structure your responses with clear sections:
- **Thinking**: Your analysis and reasoning
- **Plan**: What you intend to do (when planning)
- **Action**: The tool call (when executing)
- **Result**: What happened (after tool execution)

When you need to use a tool, simply call it using the available tool functions.
Do not wrap tool calls in code blocks."#
            .to_string()
    }

    /// JSON Schema for the `create_plan` virtual tool.
    ///
    /// This is passed to `send_with_tools()` so the LLM returns a structured
    /// plan via Native Tool Use rather than free-text. The schema includes
    /// nested `tasks` array with objects — which `ToolDefinition`/`ToolParameter`
    /// cannot express, so we build raw JSON Schema directly.
    ///
    /// This tool is NOT registered in `ToolSpec` or executed by `LocalToolExecutor`.
    /// It is a "virtual" tool used only during the planning phase to extract
    /// structured data from the LLM.
    pub fn plan_tool_schema() -> serde_json::Value {
        json!({
            "name": "create_plan",
            "description": "Create an execution plan. You MUST call this tool to submit your plan.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "objective": {
                        "type": "string",
                        "description": "What we're trying to accomplish"
                    },
                    "reasoning": {
                        "type": "string",
                        "description": "Why this approach makes sense"
                    },
                    "tasks": {
                        "type": "array",
                        "description": "Ordered list of tasks to execute",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "Unique task identifier"
                                },
                                "description": {
                                    "type": "string",
                                    "description": "What this task does"
                                },
                                "tool": {
                                    "type": "string",
                                    "description": "Tool name to use (or null if no tool needed)"
                                },
                                "args": {
                                    "type": "object",
                                    "description": "Arguments for the tool"
                                },
                                "depends_on": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "IDs of tasks this depends on"
                                }
                            },
                            "required": ["id", "description"]
                        }
                    }
                },
                "required": ["objective", "reasoning", "tasks"]
            }
        })
    }

    /// Prompt for context gathering phase
    pub fn context_gathering(request: &str, project_root: Option<&str>) -> String {
        let root_info = project_root
            .map(|r| format!("Project root: {}\n\n", r))
            .unwrap_or_default();

        format!(
            r#"## Task

Gather context about the project to understand its structure before planning.

{root_info}## User Request

{request}

## Instructions

1. Use `glob_search` to find important files (package.json, Cargo.toml, etc.)
2. Use `read_file` to examine key configuration files
3. Use `grep_search` if you need to find specific code patterns

After gathering context, summarize:
- Project type and structure
- Key files relevant to the request
- Any important patterns or conventions observed

Output your findings in a structured format."#,
            root_info = root_info,
            request = request
        )
    }

    /// Prompt for planning phase
    pub fn planning(request: &str, context: &AgentContext) -> String {
        Self::planning_with_feedback(request, context, None)
    }

    /// Prompt for planning phase with optional feedback from previous rejection
    pub fn planning_with_feedback(
        request: &str,
        context: &AgentContext,
        previous_feedback: Option<&str>,
    ) -> String {
        let context_info = context.to_prompt_context();

        let feedback_section = previous_feedback
            .map(|fb| {
                format!(
                    r#"

## Previous Plan Feedback

Your previous plan was rejected by the review committee. Please address the following concerns:

{fb}

---

"#
                )
            })
            .unwrap_or_default();

        format!(
            r#"## Task

Create a detailed plan to accomplish the user's request.

## Project Context

{context_info}

## User Request

{request}{feedback_section}

## Instructions

Create a step-by-step plan with:
1. Clear objective
2. Reasoning for your approach
3. Ordered list of tasks with:
   - Description of what to do
   - Which tool to use (if any)
   - Expected outcome
   - Any dependencies on other tasks

## IMPORTANT: Correct Tool Names

You MUST use the exact tool names listed below. Common mistakes are shown for reference.

| Correct Name    | Common Mistakes (DO NOT USE)              |
|-----------------|-------------------------------------------|
| `read_file`     | `view`, `cat`, `open`                     |
| `write_file`    | `edit`, `save`, `create_file`             |
| `run_command`   | `bash`, `shell`, `execute`, `terminal`    |
| `glob_search`   | `glob`, `find`, `find_files`, `list`      |
| `grep_search`   | `grep`, `rg`, `search`, `ripgrep`, `find_in_files` |

## Submitting Your Plan

Use the `create_plan` tool to submit your plan. Be thorough but focused. Don't include unnecessary steps."#,
            context_info = context_info,
            request = request,
            feedback_section = feedback_section
        )
    }

    /// Prompt for plan review (used in quorum)
    pub fn plan_review(request: &str, plan: &Plan, context: &AgentContext) -> String {
        let context_info = context.to_prompt_context();
        let tasks_description = plan
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let tool_info = t
                    .tool_name
                    .as_ref()
                    .map(|n| format!(" (using {})", n))
                    .unwrap_or_default();
                format!("{}. {}{}", i + 1, t.description, tool_info)
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"## Task

Review the following plan and provide feedback.

## User Request

{request}

## Project Context

{context_info}

## Proposed Plan

**Objective**: {objective}

**Reasoning**: {reasoning}

**Tasks**:
{tasks}

## Review Instructions

Evaluate the plan for:
1. **Correctness**: Will this plan achieve the stated objective?
2. **Safety**: Are there any risky operations that need more consideration?
3. **Completeness**: Are all necessary steps included?
4. **Efficiency**: Is there a simpler approach?
5. **Potential Issues**: What could go wrong?

Provide your assessment with:
- Overall recommendation: APPROVE or REVISE
- Specific feedback and suggestions
- Any concerns about safety or correctness"#,
            request = request,
            context_info = context_info,
            objective = plan.objective,
            reasoning = plan.reasoning,
            tasks = tasks_description
        )
    }

    /// Prompt for ensemble plan voting
    ///
    /// Used during ensemble planning when one model evaluates another model's plan.
    /// The evaluating model scores the plan on a 1-10 scale based on:
    ///
    /// - **Completeness**: Does the plan cover all requirements?
    /// - **Safety**: Are risky operations handled appropriately?
    /// - **Efficiency**: Is there unnecessary complexity?
    /// - **Feasibility**: Can each step be executed?
    ///
    /// # Response Format
    ///
    /// The prompt requests a JSON response:
    /// ```json
    /// {
    ///   "score": 8,
    ///   "reasoning": "Good plan with minor improvements possible"
    /// }
    /// ```
    ///
    /// The score is parsed by `parse_vote_score` in the application layer.
    ///
    /// # See Also
    ///
    /// - [`EnsemblePlanResult`](crate::agent::EnsemblePlanResult) - Aggregates votes
    /// - `docs/features/ensemble-mode.md` - Research background
    pub fn plan_voting(plan: &Plan) -> String {
        let tasks_description = plan
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let tool_info = t
                    .tool_name
                    .as_ref()
                    .map(|n| format!(" (using {})", n))
                    .unwrap_or_default();
                format!("{}. {}{}", i + 1, t.description, tool_info)
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"## Task

You are evaluating a plan generated by another model.
Score this plan from 1 to 10 and provide brief reasoning.

## Plan to Evaluate

**Objective**: {objective}

**Reasoning**: {reasoning}

**Tasks**:
{tasks}

## Scoring Criteria

- **10**: Excellent - Well-structured, safe, efficient, and complete
- **8-9**: Good - Solid plan with minor improvements possible
- **6-7**: Adequate - Achieves goal but has notable issues
- **4-5**: Weak - Missing important steps or has safety concerns
- **1-3**: Poor - Fundamentally flawed or dangerous

## Response Format

Respond with JSON only:
```json
{{
  "score": <1-10>,
  "reasoning": "<brief explanation>"
}}
```"#,
            objective = plan.objective,
            reasoning = plan.reasoning,
            tasks = tasks_description
        )
    }

    /// Prompt for task execution
    pub fn task_execution(task: &Task, context: &AgentContext, previous_results: &str) -> String {
        let tool_info = task
            .tool_name
            .as_ref()
            .map(|t| format!("\n\nTool to use: `{}`", t))
            .unwrap_or_default();

        let args_info = if !task.tool_args.is_empty() {
            format!(
                "\n\nPrepared arguments:\n```json\n{}\n```",
                serde_json::to_string_pretty(&task.tool_args).unwrap_or_default()
            )
        } else {
            String::new()
        };

        let context_summary = if !context.to_prompt_context().is_empty() {
            format!("\n\n## Context\n\n{}", context.to_prompt_context())
        } else {
            String::new()
        };

        let previous = if !previous_results.is_empty() {
            format!("\n\n## Previous Results\n\n{}", previous_results)
        } else {
            String::new()
        };

        format!(
            r#"## Current Task

**Task ID**: {id}
**Description**: {description}{tool_info}{args_info}{context_summary}{previous}

## Instructions

Execute this task. If you need to use a tool, output the tool call in the specified format.
After execution, report the result and any observations."#,
            id = task.id,
            description = task.description,
            tool_info = tool_info,
            args_info = args_info,
            context_summary = context_summary,
            previous = previous
        )
    }

    /// Prompt for action review (used in quorum for high-risk operations)
    pub fn action_review(task: &Task, tool_call: &str, context: &AgentContext) -> String {
        let context_info = context.to_prompt_context();

        format!(
            r#"## Task

Review the following action before it is executed.

## Project Context

{context_info}

## Task Being Executed

**Description**: {description}

## Proposed Action

```
{tool_call}
```

## Review Instructions

This is a high-risk operation. Evaluate:
1. **Necessity**: Is this action necessary for the task?
2. **Safety**: Could this cause unintended damage?
3. **Correctness**: Are the arguments correct?
4. **Alternatives**: Is there a safer approach?

Provide your assessment with:
- Recommendation: APPROVE or REJECT
- Specific concerns (if any)
- Suggested modifications (if applicable)"#,
            context_info = context_info,
            description = task.description,
            tool_call = tool_call
        )
    }

    /// Prompt for retrying a failed tool call due to validation error
    pub fn tool_retry(
        tool_name: &str,
        error_message: &str,
        previous_args: &std::collections::HashMap<String, serde_json::Value>,
    ) -> String {
        let args_json = serde_json::to_string_pretty(previous_args).unwrap_or_default();

        format!(
            r#"## Tool Execution Failed

The tool call failed due to a validation error. Please fix the issue and provide a corrected tool call.

**Tool**: `{tool_name}`

**Error**: {error_message}

**Previous Arguments**:
```json
{args_json}
```

## Instructions

Analyze the error message and fix the arguments. Common issues include:
- Missing required parameters
- Invalid parameter values or types
- Incorrect file paths

Provide the corrected tool call:

```tool
{{
  "tool": "{tool_name}",
  "args": {{
    // Fix the arguments based on the error
  }},
  "reasoning": "Explanation of what was fixed"
}}
```

IMPORTANT: Respond with ONLY the ```tool code block. Do NOT include any text outside the block."#,
            tool_name = tool_name,
            error_message = error_message,
            args_json = args_json
        )
    }

    /// Prompt for analyzing project context (used in /init command)
    pub fn context_analysis(project_files: &str) -> String {
        format!(
            r#"## Task

Analyze the following project files and provide a structured summary of the project.

## Project Files

{project_files}

## Instructions

Based on the provided files, analyze and summarize:

1. **Project Overview**: What is this project about? What problem does it solve?
2. **Tech Stack**: What languages, frameworks, and tools are used?
3. **Architecture**: What is the overall architecture pattern?
4. **Key Directories**: What are the important directories and their purposes?
5. **Build System**: How is the project built and tested?
6. **Key Concepts**: What are the important domain concepts or abstractions?

Provide your analysis in a clear, structured format that would help a developer quickly understand the project."#,
            project_files = project_files
        )
    }

    /// System prompt for context synthesis moderator (used in /init command)
    pub fn context_synthesis_system() -> &'static str {
        r#"You are a technical documentation synthesizer. Your task is to combine multiple analyses of a software project into a single, comprehensive context document.

Guidelines:
- Combine insights from all analyses, keeping the most accurate and complete information
- Resolve any conflicts by choosing the most detailed or accurate description
- Use clear, concise language
- Format the output as a markdown document suitable for developers
- Include all relevant technical details while avoiding redundancy

Output format:
```markdown
# Project Context

## Overview
[Brief description of what the project does]

## Tech Stack
- Language: [primary language]
- Framework: [if applicable]
- Build: [build system]

## Architecture
[Architecture pattern and key design decisions]

## Key Directories
[Important directories and their purposes]

## Key Concepts
[Important domain concepts or abstractions]

---
*Generated by Quorum Council on [date]*
*Models: [list of models]*
```"#
    }

    /// Prompt for synthesizing context from multiple model analyses
    pub fn context_synthesis(analyses: &[(String, String)], date: &str) -> String {
        let analyses_text = analyses
            .iter()
            .map(|(model, analysis)| format!("### Analysis by {}\n\n{}", model, analysis))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        let model_names = analyses
            .iter()
            .map(|(model, _)| model.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            r#"## Task

Synthesize the following project analyses from multiple AI models into a single, comprehensive context document.

## Individual Analyses

{analyses_text}

## Instructions

Create a unified project context document that:
1. Combines the best insights from all analyses
2. Resolves any conflicting information
3. Follows the standard context document format
4. Includes the generation date: {date}
5. Lists the contributing models: {model_names}

Output the final context document in markdown format."#,
            analyses_text = analyses_text,
            date = date,
            model_names = model_names
        )
    }

    /// Prompt for final review
    pub fn final_review(request: &str, plan: &Plan, results_summary: &str) -> String {
        let tasks_summary = plan
            .tasks
            .iter()
            .map(|t| {
                let status = t.status.as_str();
                let result = t
                    .result
                    .as_ref()
                    .map(|r| {
                        if r.success {
                            "Success".to_string()
                        } else {
                            format!("Failed: {}", r.error.as_deref().unwrap_or("Unknown error"))
                        }
                    })
                    .unwrap_or_else(|| "Not executed".to_string());
                format!("- {} [{}]: {}", t.description, status, result)
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"## Task

Review the results of the agent execution.

## Original Request

{request}

## Plan Objective

{objective}

## Execution Summary

{tasks_summary}

## Results

{results_summary}

## Review Instructions

Evaluate the overall execution:
1. **Goal Achievement**: Was the original request fulfilled?
2. **Quality**: Are the changes correct and well-implemented?
3. **Completeness**: Is anything missing?
4. **Issues**: Were there any problems during execution?

Provide:
- Overall assessment: SUCCESS, PARTIAL, or FAILURE
- Summary of what was accomplished
- Any recommendations or follow-up actions"#,
            request = request,
            objective = plan.objective,
            tasks_summary = tasks_summary,
            results_summary = results_summary
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_system_prompt() {
        let prompt = AgentPromptTemplate::agent_system();

        assert!(prompt.contains("autonomous coding agent"));
        // Native mode: no tool descriptions in prompt
        assert!(!prompt.contains("Available Tools"));
    }

    #[test]
    fn test_context_gathering_prompt() {
        let prompt =
            AgentPromptTemplate::context_gathering("Update the README", Some("/project/root"));

        assert!(prompt.contains("Update the README"));
        assert!(prompt.contains("/project/root"));
        assert!(prompt.contains("glob_search"));
    }

    #[test]
    fn test_planning_prompt() {
        let context = AgentContext::new()
            .with_project_root("/project")
            .with_project_type("rust");

        let prompt = AgentPromptTemplate::planning("Add a new feature", &context);

        assert!(prompt.contains("Add a new feature"));
        assert!(prompt.contains("rust"));
        assert!(prompt.contains("objective"));
        assert!(prompt.contains("tasks"));
        // Should reference create_plan tool, not ```plan block
        assert!(prompt.contains("create_plan"));
        assert!(!prompt.contains("```plan"));
    }

    #[test]
    fn test_plan_tool_schema() {
        let schema = AgentPromptTemplate::plan_tool_schema();
        assert_eq!(schema["name"], "create_plan");
        assert_eq!(schema["input_schema"]["type"], "object");
        let required = schema["input_schema"]["required"]
            .as_array()
            .expect("required should be array");
        assert_eq!(required.len(), 3);
        assert!(schema["input_schema"]["properties"]["tasks"]["minItems"] == 1);
    }

    #[test]
    fn test_planning_with_feedback_prompt() {
        let context = AgentContext::new()
            .with_project_root("/project")
            .with_project_type("rust");

        // Without feedback - should be same as planning()
        let prompt_no_feedback =
            AgentPromptTemplate::planning_with_feedback("Add a feature", &context, None);
        assert!(prompt_no_feedback.contains("Add a feature"));
        assert!(!prompt_no_feedback.contains("Previous Plan Feedback"));

        // With feedback - should include feedback section
        let prompt_with_feedback = AgentPromptTemplate::planning_with_feedback(
            "Add a feature",
            &context,
            Some("The plan is too risky. Please add more validation steps."),
        );
        assert!(prompt_with_feedback.contains("Add a feature"));
        assert!(prompt_with_feedback.contains("Previous Plan Feedback"));
        assert!(prompt_with_feedback.contains("rejected by the review committee"));
        assert!(prompt_with_feedback.contains("too risky"));
        assert!(prompt_with_feedback.contains("validation steps"));
    }

    #[test]
    fn test_plan_review_prompt() {
        let context = AgentContext::new();
        let plan = Plan::new("Test objective", "Test reasoning")
            .with_task(Task::new("1", "First task").with_tool("read_file"));

        let prompt = AgentPromptTemplate::plan_review("Original request", &plan, &context);

        assert!(prompt.contains("Test objective"));
        assert!(prompt.contains("First task"));
        assert!(prompt.contains("APPROVE or REVISE"));
    }

    #[test]
    fn test_task_execution_prompt() {
        let context = AgentContext::new();
        let task = Task::new("1", "Read the config")
            .with_tool("read_file")
            .with_arg("path", "/config.toml");

        let prompt = AgentPromptTemplate::task_execution(&task, &context, "");

        assert!(prompt.contains("Read the config"));
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("config.toml"));
    }

    #[test]
    fn test_action_review_prompt() {
        let context = AgentContext::new();
        let task = Task::new("1", "Write to important file");

        let prompt =
            AgentPromptTemplate::action_review(&task, r#"{"tool": "write_file"}"#, &context);

        assert!(prompt.contains("Write to important file"));
        assert!(prompt.contains("high-risk operation"));
        assert!(prompt.contains("APPROVE or REJECT"));
    }

    #[test]
    fn test_final_review_prompt() {
        let mut plan = Plan::new("Complete the task", "Reasoning");
        let mut task = Task::new("1", "Do something");
        task.status = crate::agent::TaskStatus::Completed;
        task.result = Some(crate::agent::TaskResult::success("Done"));
        plan.tasks.push(task);

        let prompt =
            AgentPromptTemplate::final_review("Original request", &plan, "Everything worked");

        assert!(prompt.contains("Original request"));
        assert!(prompt.contains("Do something"));
        assert!(prompt.contains("SUCCESS, PARTIAL, or FAILURE"));
    }

    #[test]
    fn test_tool_retry_prompt() {
        let mut args = std::collections::HashMap::new();
        args.insert("path".to_string(), serde_json::json!("README.md"));

        let prompt = AgentPromptTemplate::tool_retry(
            "read_file",
            "Missing required argument: encoding",
            &args,
        );

        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("Missing required argument: encoding"));
        assert!(prompt.contains("README.md"));
        assert!(prompt.contains("Tool Execution Failed"));
        assert!(prompt.contains("validation error"));
    }

    #[test]
    fn test_tool_retry_format_instruction() {
        let args = std::collections::HashMap::new();
        let prompt = AgentPromptTemplate::tool_retry("read_file", "error", &args);
        assert!(prompt.contains("ONLY"));
        assert!(prompt.contains("tool code block"));
    }
}
