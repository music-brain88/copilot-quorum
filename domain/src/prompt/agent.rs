//! Prompt templates for the Agent system

use crate::agent::{AgentContext, Plan, Task};
use crate::tool::entities::ToolSpec;

/// Templates for generating agent prompts
pub struct AgentPromptTemplate;

impl AgentPromptTemplate {
    /// System prompt for the agent
    pub fn agent_system(tool_spec: &ToolSpec) -> String {
        let tool_descriptions = tool_spec
            .all()
            .map(|t| {
                let params = t
                    .parameters
                    .iter()
                    .map(|p| {
                        let required = if p.required { " (required)" } else { "" };
                        format!("    - {}: {}{}", p.name, p.description, required)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                format!(
                    "- **{}**: {}\n  Risk: {}\n  Parameters:\n{}",
                    t.name, t.description, t.risk_level, params
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        format!(
            r#"You are an autonomous coding agent that helps users with software engineering tasks.

## Your Capabilities

You can analyze codebases, write code, execute commands, and help with various development tasks.
You work by creating and executing plans, using tools to interact with the file system.

## Available Tools

{tool_descriptions}

## How to Use Tools

When you need to use a tool, output a JSON block in this format:

```tool
{{
  "tool": "tool_name",
  "args": {{
    "arg1": "value1",
    "arg2": "value2"
  }},
  "reasoning": "Brief explanation of why you're using this tool"
}}
```

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
"#,
            tool_descriptions = tool_descriptions
        )
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
        let context_info = context.to_prompt_context();

        format!(
            r#"## Task

Create a detailed plan to accomplish the user's request.

## Project Context

{context_info}

## User Request

{request}

## Instructions

Create a step-by-step plan with:
1. Clear objective
2. Reasoning for your approach
3. Ordered list of tasks with:
   - Description of what to do
   - Which tool to use (if any)
   - Expected outcome
   - Any dependencies on other tasks

Format your plan as:

```plan
{{
  "objective": "What we're trying to accomplish",
  "reasoning": "Why this approach makes sense",
  "tasks": [
    {{
      "id": "1",
      "description": "What this task does",
      "tool": "tool_name or null",
      "args": {{}},
      "depends_on": []
    }}
  ]
}}
```

Be thorough but focused. Don't include unnecessary steps."#,
            context_info = context_info,
            request = request
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
    use crate::tool::entities::{RiskLevel, ToolDefinition, ToolParameter};

    fn create_test_tool_spec() -> ToolSpec {
        ToolSpec::new()
            .register(
                ToolDefinition::new("read_file", "Read file contents", RiskLevel::Low)
                    .with_parameter(ToolParameter::new("path", "File path", true)),
            )
            .register(ToolDefinition::new(
                "write_file",
                "Write to file",
                RiskLevel::High,
            ))
    }

    #[test]
    fn test_agent_system_prompt() {
        let spec = create_test_tool_spec();
        let prompt = AgentPromptTemplate::agent_system(&spec);

        assert!(prompt.contains("autonomous coding agent"));
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("write_file"));
        assert!(prompt.contains("Available Tools"));
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
}
