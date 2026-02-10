//! Plan parsing from LLM responses.
//!
//! Extracts structured [`Plan`] entities from LLM responses — both from
//! Native Tool Use API (`create_plan` tool call) and from text-based
//! responses (` ```plan` blocks or raw JSON).
//!
//! All types referenced ([`Plan`], [`Task`], [`LlmResponse`], [`ContentBlock`])
//! are domain types, making this pure domain logic.

use crate::agent::entities::{Plan, Task};
use crate::session::response::{ContentBlock, LlmResponse};

/// Extract a plan from a structured [`LlmResponse`].
///
/// 1. First looks for a `create_plan` ToolUse block (Native Tool Use path).
/// 2. Falls back to text-based [`parse_plan()`] (for providers that don't support tool use).
pub fn extract_plan_from_response(response: &LlmResponse) -> Option<Plan> {
    // 1. Look for create_plan tool call
    for block in &response.content {
        if let ContentBlock::ToolUse { name, input, .. } = block
            && name == "create_plan"
        {
            let json = serde_json::Value::Object(
                input.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            );
            return parse_plan_json(&json);
        }
    }

    // 2. Fallback: parse from text content (for providers without Native Tool Use)
    let text = response.text_content();
    if !text.is_empty() {
        return parse_plan(&text);
    }

    None
}

/// Parse a plan from model response text.
///
/// Supports two formats:
/// 1. ` ```plan` fenced code blocks containing JSON
/// 2. Raw JSON (the entire response is valid JSON)
///
/// Returns `None` if no valid plan is found, or if the plan has no tasks.
pub fn parse_plan(response: &str) -> Option<Plan> {
    // Look for ```plan ... ``` blocks
    let mut in_plan_block = false;
    let mut current_block = String::new();

    for line in response.lines() {
        if line.trim() == "```plan" {
            in_plan_block = true;
            current_block.clear();
        } else if in_plan_block && line.trim() == "```" {
            in_plan_block = false;
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_block) {
                return parse_plan_json(&parsed);
            }
        } else if in_plan_block {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    // Try parsing the entire response as JSON
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(response) {
        return parse_plan_json(&parsed);
    }

    // No valid plan found — caller should handle the error
    None
}

/// JSON 値を文字列に変換（数値・bool も文字列化、null・空文字は None）
fn json_value_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse a plan from a JSON value.
///
/// Expected schema:
/// ```json
/// {
///   "objective": "string",
///   "reasoning": "string (optional)",
///   "tasks": [
///     {
///       "id": "string",
///       "description": "string",
///       "tool": "string (optional)",
///       "args": { ... },
///       "depends_on": ["task_id", ...]
///     }
///   ]
/// }
/// ```
///
/// Returns `None` if required fields are missing or tasks array is empty.
pub fn parse_plan_json(json: &serde_json::Value) -> Option<Plan> {
    let objective = json.get("objective")?.as_str()?;
    let reasoning = json.get("reasoning").and_then(|v| v.as_str()).unwrap_or("");

    let mut plan = Plan::new(objective, reasoning);

    let tasks = json.get("tasks").and_then(|v| v.as_array())?;

    // Empty tasks array is not a valid plan
    if tasks.is_empty() {
        return None;
    }

    for (index, task_json) in tasks.iter().enumerate() {
        let id = task_json
            .get("id")
            .and_then(json_value_to_string)
            .unwrap_or_else(|| format!("{}", index + 1));
        let description = task_json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description");

        let mut task = Task::new(id, description);

        if let Some(tool) = task_json.get("tool").and_then(|v| v.as_str())
            && tool != "null"
            && !tool.is_empty()
        {
            task = task.with_tool(tool);
        }

        if let Some(args) = task_json.get("args").and_then(|v| v.as_object()) {
            for (key, value) in args {
                task = task.with_arg(key, value.clone());
            }
        }

        if let Some(deps) = task_json.get("depends_on").and_then(|v| v.as_array()) {
            for dep in deps {
                if let Some(dep_id) = json_value_to_string(dep) {
                    task = task.with_dependency(dep_id);
                }
            }
        }

        plan.add_task(task);
    }

    Some(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::value_objects::TaskId;
    use crate::session::response::{LlmResponse, StopReason};
    use std::collections::HashMap;

    #[test]
    fn test_parse_plan() {
        let response = r#"
Here's my plan:

```plan
{
  "objective": "Update the README",
  "reasoning": "The README needs updating",
  "tasks": [
    {
      "id": "1",
      "description": "Read current README",
      "tool": "read_file",
      "args": {"path": "README.md"},
      "depends_on": []
    },
    {
      "id": "2",
      "description": "Write updated README",
      "tool": "write_file",
      "args": {"path": "README.md", "content": "..."},
      "depends_on": ["1"]
    }
  ]
}
```
"#;

        let plan = parse_plan(response).unwrap();
        assert_eq!(plan.objective, "Update the README");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].tool_name, Some("read_file".to_string()));
        assert_eq!(plan.tasks[1].depends_on, vec![TaskId::new("1")]);
    }

    #[test]
    fn test_parse_plan_plain_text_returns_none() {
        let response = "I'll organize the steps for you! Let me check the current state and figure out the best approach.";
        assert!(
            parse_plan(response).is_none(),
            "Plain text should not produce a plan"
        );
    }

    #[test]
    fn test_parse_plan_empty_tasks_returns_none() {
        let response = r#"```plan
{
  "objective": "Do something",
  "reasoning": "because",
  "tasks": []
}
```"#;
        assert!(
            parse_plan(response).is_none(),
            "Plan with empty tasks should return None"
        );
    }

    #[test]
    fn test_parse_plan_json_no_tasks_returns_none() {
        let response = r#"```plan
{
  "objective": "Do something",
  "reasoning": "because"
}
```"#;
        assert!(
            parse_plan(response).is_none(),
            "Plan JSON without tasks field should return None"
        );
    }

    #[test]
    fn test_parse_plan_raw_json_without_plan_block() {
        let response = r#"{"objective": "Test", "reasoning": "test", "tasks": [{"id": "1", "description": "Do it"}]}"#;
        let plan = parse_plan(response);
        assert!(plan.is_some(), "Raw JSON with tasks should parse");
        assert_eq!(plan.unwrap().tasks.len(), 1);
    }

    #[test]
    fn test_parse_plan_raw_json_empty_tasks_returns_none() {
        let response = r#"{"objective": "Test", "reasoning": "test", "tasks": []}"#;
        assert!(
            parse_plan(response).is_none(),
            "Raw JSON with empty tasks should return None"
        );
    }

    #[test]
    fn test_extract_plan_from_tool_call() {
        let mut input = HashMap::new();
        input.insert("objective".to_string(), serde_json::json!("Fix the bug"));
        input.insert(
            "reasoning".to_string(),
            serde_json::json!("Bug is critical"),
        );
        input.insert(
            "tasks".to_string(),
            serde_json::json!([
                {
                    "id": "1",
                    "description": "Read the buggy file",
                    "tool": "read_file",
                    "args": {"path": "src/main.rs"}
                },
                {
                    "id": "2",
                    "description": "Fix the bug",
                    "tool": "write_file",
                    "args": {"path": "src/main.rs", "content": "fixed"},
                    "depends_on": ["1"]
                }
            ]),
        );

        let response = LlmResponse {
            content: vec![
                ContentBlock::Text("Let me create a plan.".to_string()),
                ContentBlock::ToolUse {
                    id: "toolu_abc123".to_string(),
                    name: "create_plan".to_string(),
                    input,
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
            model: None,
        };

        let plan =
            extract_plan_from_response(&response).expect("should extract plan from tool call");
        assert_eq!(plan.objective, "Fix the bug");
        assert_eq!(plan.reasoning, "Bug is critical");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].tool_name, Some("read_file".to_string()));
        assert_eq!(plan.tasks[1].depends_on, vec![TaskId::new("1")]);
    }

    #[test]
    fn test_extract_plan_from_text_fallback() {
        let text_plan = r#"```plan
{
  "objective": "Update README",
  "reasoning": "Needs updating",
  "tasks": [
    {"id": "1", "description": "Read README", "tool": "read_file"}
  ]
}
```"#;
        let response = LlmResponse::from_text(text_plan);

        let plan =
            extract_plan_from_response(&response).expect("should extract plan from text fallback");
        assert_eq!(plan.objective, "Update README");
        assert_eq!(plan.tasks.len(), 1);
    }

    #[test]
    fn test_extract_plan_from_response_no_plan() {
        let response = LlmResponse::from_text("I'll think about this.");
        assert!(extract_plan_from_response(&response).is_none());
    }

    #[test]
    fn test_parse_plan_numeric_task_ids() {
        let json = serde_json::json!({
            "objective": "Fix bug",
            "reasoning": "It's broken",
            "tasks": [
                {"id": 1, "description": "Read file", "tool": "read_file"},
                {"id": 2, "description": "Write fix", "tool": "write_file", "depends_on": [1]}
            ]
        });
        let plan = parse_plan_json(&json).unwrap();
        assert_eq!(plan.tasks[0].id, TaskId::new("1"));
        assert_eq!(plan.tasks[1].id, TaskId::new("2"));
        assert_eq!(plan.tasks[1].depends_on, vec![TaskId::new("1")]);
    }

    #[test]
    fn test_parse_plan_missing_ids_get_sequential() {
        let json = serde_json::json!({
            "objective": "Do stuff",
            "reasoning": "reasons",
            "tasks": [
                {"description": "First task"},
                {"description": "Second task"},
                {"description": "Third task"}
            ]
        });
        let plan = parse_plan_json(&json).unwrap();
        assert_eq!(plan.tasks[0].id, TaskId::new("1"));
        assert_eq!(plan.tasks[1].id, TaskId::new("2"));
        assert_eq!(plan.tasks[2].id, TaskId::new("3"));
    }

    #[test]
    fn test_parse_plan_null_id_gets_sequential() {
        let json = serde_json::json!({
            "objective": "Do stuff",
            "reasoning": "reasons",
            "tasks": [
                {"id": null, "description": "Null ID task"},
                {"id": "", "description": "Empty string ID task"}
            ]
        });
        let plan = parse_plan_json(&json).unwrap();
        assert_eq!(plan.tasks[0].id, TaskId::new("1"));
        assert_eq!(plan.tasks[1].id, TaskId::new("2"));
    }

    #[test]
    fn test_parse_plan_mixed_id_types() {
        let json = serde_json::json!({
            "objective": "Mixed IDs",
            "reasoning": "testing",
            "tasks": [
                {"id": "alpha", "description": "String ID"},
                {"id": 42, "description": "Numeric ID"},
                {"description": "Missing ID"}
            ]
        });
        let plan = parse_plan_json(&json).unwrap();
        assert_eq!(plan.tasks[0].id, TaskId::new("alpha"));
        assert_eq!(plan.tasks[1].id, TaskId::new("42"));
        assert_eq!(plan.tasks[2].id, TaskId::new("3"));
    }
}
