//! Script progress bridge — maps AgentProgressNotifier callbacks to ScriptEventType emissions.
//!
//! This bridge translates agent lifecycle events into scripting events,
//! allowing Lua scripts to react to agent execution progress.
//!
//! # Event Mapping
//!
//! | AgentProgressNotifier callback | ScriptEventType |
//! |-------------------------------|-----------------|
//! | `on_phase_change(phase)` | `PhaseChanged` { phase } |
//! | `on_plan_created(plan)` | `PlanCreated` { objective, task_count } |
//! | `on_tool_execution_completed(...)` | `ToolCallAfter` { tool_name, success: true, ... } |
//! | `on_tool_execution_failed(...)` | `ToolCallAfter` { tool_name, success: false, ... } |
//!
//! Note: `ToolCallBefore` is NOT emitted through this bridge because it requires
//! a return value (cancellation). It's called directly by `ExecuteTaskUseCase`.

use super::agent_progress::AgentProgressNotifier;
use super::scripting_engine::ScriptingEnginePort;
use quorum_domain::scripting::{ScriptEventData, ScriptEventType, ScriptValue};
use quorum_domain::{AgentPhase, Plan};
use std::sync::Arc;

/// Bridge that translates agent progress callbacks into scripting events.
pub struct ScriptProgressBridge {
    engine: Arc<dyn ScriptingEnginePort>,
}

impl ScriptProgressBridge {
    pub fn new(engine: Arc<dyn ScriptingEnginePort>) -> Self {
        Self { engine }
    }
}

impl AgentProgressNotifier for ScriptProgressBridge {
    fn on_phase_change(&self, phase: &AgentPhase) {
        let data =
            ScriptEventData::new().with_field("phase", ScriptValue::String(phase.to_string()));
        let _ = self.engine.emit_event(ScriptEventType::PhaseChanged, data);
    }

    fn on_plan_created(&self, plan: &Plan) {
        let data = ScriptEventData::new()
            .with_field("objective", ScriptValue::String(plan.objective.clone()))
            .with_field("task_count", ScriptValue::Integer(plan.tasks.len() as i64));
        let _ = self.engine.emit_event(ScriptEventType::PlanCreated, data);
    }

    fn on_tool_execution_completed(
        &self,
        _task_id: &str,
        _execution_id: &str,
        tool_name: &str,
        duration_ms: u64,
        output_preview: &str,
    ) {
        let data = ScriptEventData::new()
            .with_field("tool_name", ScriptValue::String(tool_name.to_string()))
            .with_field("success", ScriptValue::Boolean(true))
            .with_field("duration_ms", ScriptValue::Integer(duration_ms as i64))
            .with_field(
                "output_preview",
                ScriptValue::String(output_preview.to_string()),
            );
        let _ = self.engine.emit_event(ScriptEventType::ToolCallAfter, data);
    }

    fn on_tool_execution_failed(
        &self,
        _task_id: &str,
        _execution_id: &str,
        tool_name: &str,
        error: &str,
    ) {
        let data = ScriptEventData::new()
            .with_field("tool_name", ScriptValue::String(tool_name.to_string()))
            .with_field("success", ScriptValue::Boolean(false))
            .with_field("duration_ms", ScriptValue::Integer(0))
            .with_field("error", ScriptValue::String(error.to_string()));
        let _ = self.engine.emit_event(ScriptEventType::ToolCallAfter, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::scripting_engine::{EventOutcome, NoScriptingEngine, ScriptError};
    use quorum_domain::scripting::ScriptEventData;
    use std::sync::Mutex;

    struct RecordingEngine {
        events: Mutex<Vec<(ScriptEventType, Vec<String>)>>,
    }

    impl RecordingEngine {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn recorded_events(&self) -> Vec<(ScriptEventType, Vec<String>)> {
            self.events.lock().unwrap().clone()
        }
    }

    impl ScriptingEnginePort for RecordingEngine {
        fn emit_event(
            &self,
            event: ScriptEventType,
            data: ScriptEventData,
        ) -> Result<EventOutcome, ScriptError> {
            let field_keys: Vec<String> = {
                let mut keys: Vec<_> = data.fields().keys().cloned().collect();
                keys.sort();
                keys
            };
            self.events.lock().unwrap().push((event, field_keys));
            Ok(EventOutcome::Continue)
        }

        fn load_script(&self, _path: &std::path::Path) -> Result<(), ScriptError> {
            Ok(())
        }

        fn is_available(&self) -> bool {
            true
        }

        fn registered_keymaps(
            &self,
        ) -> Vec<(String, String, crate::ports::scripting_engine::KeymapAction)> {
            Vec::new()
        }

        fn execute_callback(&self, _callback_id: u64) -> Result<(), ScriptError> {
            Ok(())
        }
    }

    #[test]
    fn test_bridge_emits_phase_changed() {
        let engine = Arc::new(RecordingEngine::new());
        let bridge = ScriptProgressBridge::new(engine.clone());

        bridge.on_phase_change(&AgentPhase::Planning);

        let events = engine.recorded_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, ScriptEventType::PhaseChanged);
        assert!(events[0].1.contains(&"phase".to_string()));
    }

    #[test]
    fn test_bridge_emits_plan_created() {
        let engine = Arc::new(RecordingEngine::new());
        let bridge = ScriptProgressBridge::new(engine.clone());

        let plan = Plan::new("Test objective", "reasoning");
        bridge.on_plan_created(&plan);

        let events = engine.recorded_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, ScriptEventType::PlanCreated);
        assert!(events[0].1.contains(&"objective".to_string()));
        assert!(events[0].1.contains(&"task_count".to_string()));
    }

    #[test]
    fn test_bridge_emits_tool_call_after_success() {
        let engine = Arc::new(RecordingEngine::new());
        let bridge = ScriptProgressBridge::new(engine.clone());

        bridge.on_tool_execution_completed("t1", "e1", "read_file", 42, "file contents...");

        let events = engine.recorded_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, ScriptEventType::ToolCallAfter);
        assert!(events[0].1.contains(&"tool_name".to_string()));
        assert!(events[0].1.contains(&"success".to_string()));
        assert!(events[0].1.contains(&"duration_ms".to_string()));
    }

    #[test]
    fn test_bridge_emits_tool_call_after_failure() {
        let engine = Arc::new(RecordingEngine::new());
        let bridge = ScriptProgressBridge::new(engine.clone());

        bridge.on_tool_execution_failed("t1", "e2", "write_file", "permission denied");

        let events = engine.recorded_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, ScriptEventType::ToolCallAfter);
        assert!(events[0].1.contains(&"error".to_string()));
    }

    #[test]
    fn test_bridge_with_no_scripting_engine() {
        let engine: Arc<dyn ScriptingEnginePort> = Arc::new(NoScriptingEngine);
        let bridge = ScriptProgressBridge::new(engine);

        // Should not panic even with NoScriptingEngine
        bridge.on_phase_change(&AgentPhase::Planning);
        bridge.on_plan_created(&Plan::new("test", "reasoning"));
        bridge.on_tool_execution_completed("t1", "e1", "read_file", 0, "");
        bridge.on_tool_execution_failed("t1", "e2", "write_file", "err");
    }
}
