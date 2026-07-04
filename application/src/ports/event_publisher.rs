//! EventPublisher port — the seam for typed application events.
//!
//! This is deliberately a *seam*, not a bus: use cases publish typed
//! [`AppEvent`]s through a single point, and today's subscribers (the JSONL
//! conversation log, the Lua scripting engine) fan out behind it. A future
//! Application / Interaction Event Bus is introduced by swapping the
//! implementation and adding `AppEvent` variants — callers stay unchanged.
//! See RFC Discussion #304.

use std::sync::Arc;

use quorum_domain::quorum::{QUORUM_RESULT_EVENT_TYPE, QuorumResultPayload};
use quorum_domain::scripting::{ScriptEventData, ScriptEventType, ScriptValue};
use tracing::warn;

use super::conversation_logger::{ConversationEvent, ConversationLogger};
use super::scripting_engine::ScriptingEnginePort;

/// A typed application event.
///
/// Variants grow as more events adopt the seam (e.g. `InteractionCompleted`
/// for #303, supervisor reporting for Track B).
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// A quorum review concluded (plan / action / final review)
    QuorumResult(QuorumResultPayload),
}

/// Port for publishing typed application events.
///
/// Same contract as [`ConversationLogger`]: synchronous, non-fallible,
/// fire-and-forget — publishing must never disrupt the main execution flow.
pub trait EventPublisher: Send + Sync {
    /// Publish an event to all subscribers.
    fn publish(&self, event: AppEvent);
}

/// No-op implementation for tests and defaults.
pub struct NoEventPublisher;

impl EventPublisher for NoEventPublisher {
    fn publish(&self, _event: AppEvent) {}
}

/// Fans an event out to multiple subscribers.
pub struct CompositeEventPublisher {
    subscribers: Vec<Arc<dyn EventPublisher>>,
}

impl CompositeEventPublisher {
    pub fn new(subscribers: Vec<Arc<dyn EventPublisher>>) -> Self {
        Self { subscribers }
    }
}

impl EventPublisher for CompositeEventPublisher {
    fn publish(&self, event: AppEvent) {
        for subscriber in &self.subscribers {
            subscriber.publish(event.clone());
        }
    }
}

/// Subscriber: records events to the structured conversation log (JSONL).
pub struct ConversationLogEventPublisher {
    logger: Arc<dyn ConversationLogger>,
}

impl ConversationLogEventPublisher {
    pub fn new(logger: Arc<dyn ConversationLogger>) -> Self {
        Self { logger }
    }
}

impl EventPublisher for ConversationLogEventPublisher {
    fn publish(&self, event: AppEvent) {
        match event {
            AppEvent::QuorumResult(payload) => match serde_json::to_value(&payload) {
                Ok(json) => {
                    self.logger
                        .log(ConversationEvent::new(QUORUM_RESULT_EVENT_TYPE, json));
                }
                Err(e) => warn!("Failed to serialize quorum_result payload: {}", e),
            },
        }
    }
}

/// Subscriber: forwards events to the Lua scripting engine.
///
/// `ScriptEventData` is a flat map, so the votes array is passed as a JSON
/// string (same precedent as `ToolCallBefore`'s `args`).
pub struct ScriptEventPublisher {
    engine: Arc<dyn ScriptingEnginePort>,
}

impl ScriptEventPublisher {
    pub fn new(engine: Arc<dyn ScriptingEnginePort>) -> Self {
        Self { engine }
    }
}

impl EventPublisher for ScriptEventPublisher {
    fn publish(&self, event: AppEvent) {
        if !self.engine.is_available() {
            return;
        }
        match event {
            AppEvent::QuorumResult(payload) => {
                let approve_count = payload.votes.iter().filter(|v| v.is_approve()).count();
                let reject_count = payload.votes.iter().filter(|v| v.is_reject()).count();
                let votes_json = serde_json::to_string(&payload.votes).unwrap_or_default();
                // Serde form ("majority"), matching the JSONL rule field
                let rule = match serde_json::to_value(payload.rule) {
                    Ok(serde_json::Value::String(s)) => s,
                    Ok(other) => other.to_string(),
                    Err(_) => String::new(),
                };
                let opt_string = |value: Option<String>| {
                    value.map(ScriptValue::String).unwrap_or(ScriptValue::Nil)
                };
                let (task_id, tool) = payload
                    .target
                    .map(|t| (t.task_id, t.tool))
                    .unwrap_or((None, None));
                let data = ScriptEventData::new()
                    .with_field(
                        "topic",
                        ScriptValue::String(payload.topic.as_str().to_string()),
                    )
                    .with_field("approved", ScriptValue::Boolean(payload.approved))
                    .with_field("approve_count", ScriptValue::Integer(approve_count as i64))
                    .with_field("reject_count", ScriptValue::Integer(reject_count as i64))
                    .with_field(
                        "api_version",
                        ScriptValue::Integer(payload.api_version as i64),
                    )
                    .with_field("rule", ScriptValue::String(rule))
                    .with_field("task_id", opt_string(task_id))
                    .with_field("tool", opt_string(tool))
                    .with_field("feedback", opt_string(payload.feedback))
                    .with_field("votes_json", ScriptValue::String(votes_json));
                if let Err(e) = self.engine.emit_event(ScriptEventType::QuorumResult, data) {
                    warn!("QuorumResult script event failed: {}", e);
                }
            }
        }
    }
}

/// Test double that records every published event.
#[cfg(test)]
pub(crate) struct RecordingEventPublisher {
    pub events: std::sync::Mutex<Vec<AppEvent>>,
}

#[cfg(test)]
impl RecordingEventPublisher {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl EventPublisher for RecordingEventPublisher {
    fn publish(&self, event: AppEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::quorum::{QuorumResultPayload, QuorumTarget, QuorumTopic, Vote, VoteResult};
    use std::sync::Mutex;

    fn sample_payload() -> QuorumResultPayload {
        let result = VoteResult::from_votes(vec![
            Vote::approve("model-a", "OK"),
            Vote::reject("model-b", "Risky"),
            Vote::model_error("model-c", "timeout"),
        ]);
        QuorumResultPayload::new(
            QuorumTopic::ActionReview,
            Some(QuorumTarget::action("task-1", Some("run_command".into()))),
            &result,
        )
    }

    struct RecordingLogger {
        events: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl ConversationLogger for RecordingLogger {
        fn log(&self, event: ConversationEvent) {
            self.events
                .lock()
                .unwrap()
                .push((event.event_type.to_string(), event.payload));
        }
    }

    #[test]
    fn test_conversation_log_publisher_emits_quorum_result() {
        let logger = Arc::new(RecordingLogger {
            events: Mutex::new(Vec::new()),
        });
        let publisher = ConversationLogEventPublisher::new(logger.clone());

        publisher.publish(AppEvent::QuorumResult(sample_payload()));

        let events = logger.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        let (event_type, payload) = &events[0];
        assert_eq!(event_type, "quorum_result");
        assert_eq!(payload["api_version"], 1);
        assert_eq!(payload["topic"], "action_review");
        assert_eq!(payload["target"]["task_id"], "task-1");
        assert_eq!(payload["votes"][0]["verdict"], "approve");
        assert_eq!(payload["votes"][2]["verdict"], "model_error");
    }

    struct RecordingScriptEngine {
        events: Mutex<Vec<(ScriptEventType, ScriptEventData)>>,
        available: bool,
    }

    impl ScriptingEnginePort for RecordingScriptEngine {
        fn emit_event(
            &self,
            event: ScriptEventType,
            data: ScriptEventData,
        ) -> Result<
            super::super::scripting_engine::EventOutcome,
            super::super::scripting_engine::ScriptError,
        > {
            self.events.lock().unwrap().push((event, data));
            Ok(super::super::scripting_engine::EventOutcome::Continue)
        }

        fn load_script(
            &self,
            _path: &std::path::Path,
        ) -> Result<(), super::super::scripting_engine::ScriptError> {
            Ok(())
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn registered_keymaps(
            &self,
        ) -> Vec<(String, String, super::super::scripting_engine::KeymapAction)> {
            Vec::new()
        }

        fn execute_callback(
            &self,
            _callback_id: u64,
        ) -> Result<(), super::super::scripting_engine::ScriptError> {
            Ok(())
        }
    }

    #[test]
    fn test_script_publisher_emits_quorum_result() {
        let engine = Arc::new(RecordingScriptEngine {
            events: Mutex::new(Vec::new()),
            available: true,
        });
        let publisher = ScriptEventPublisher::new(engine.clone());

        publisher.publish(AppEvent::QuorumResult(sample_payload()));

        let events = engine.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        let (event_type, data) = &events[0];
        assert_eq!(*event_type, ScriptEventType::QuorumResult);
        assert_eq!(
            data.fields().get("topic"),
            Some(&ScriptValue::String("action_review".into()))
        );
        assert_eq!(
            data.fields().get("approved"),
            Some(&ScriptValue::Boolean(false))
        );
        assert_eq!(
            data.fields().get("approve_count"),
            Some(&ScriptValue::Integer(1))
        );
        // Correlation fields: a plugin must be able to tell which
        // task / tool this verdict belongs to
        assert_eq!(
            data.fields().get("api_version"),
            Some(&ScriptValue::Integer(1))
        );
        assert_eq!(
            data.fields().get("rule"),
            Some(&ScriptValue::String("majority".into()))
        );
        assert_eq!(
            data.fields().get("task_id"),
            Some(&ScriptValue::String("task-1".into()))
        );
        assert_eq!(
            data.fields().get("tool"),
            Some(&ScriptValue::String("run_command".into()))
        );
        assert_eq!(
            data.fields().get("feedback"),
            Some(&ScriptValue::String("model-b: Risky".into()))
        );
        let votes_json = match data.fields().get("votes_json") {
            Some(ScriptValue::String(s)) => s,
            other => panic!("votes_json missing: {:?}", other),
        };
        let votes: serde_json::Value = serde_json::from_str(votes_json).unwrap();
        assert_eq!(votes[2]["verdict"], "model_error");
    }

    #[test]
    fn test_script_publisher_nil_target_fields_without_target() {
        let engine = Arc::new(RecordingScriptEngine {
            events: Mutex::new(Vec::new()),
            available: true,
        });
        let publisher = ScriptEventPublisher::new(engine.clone());

        let result = VoteResult::from_votes(vec![Vote::approve("m", "ok")]);
        let payload = QuorumResultPayload::new(QuorumTopic::PlanReview, None, &result);
        publisher.publish(AppEvent::QuorumResult(payload));

        let events = engine.events.lock().unwrap();
        let (_, data) = &events[0];
        assert_eq!(data.fields().get("task_id"), Some(&ScriptValue::Nil));
        assert_eq!(data.fields().get("tool"), Some(&ScriptValue::Nil));
        assert_eq!(data.fields().get("feedback"), Some(&ScriptValue::Nil));
    }

    #[test]
    fn test_script_publisher_skips_unavailable_engine() {
        let engine = Arc::new(RecordingScriptEngine {
            events: Mutex::new(Vec::new()),
            available: false,
        });
        let publisher = ScriptEventPublisher::new(engine.clone());

        publisher.publish(AppEvent::QuorumResult(sample_payload()));

        assert!(engine.events.lock().unwrap().is_empty());
    }

    #[test]
    fn test_composite_fans_out() {
        let a = Arc::new(RecordingEventPublisher::new());
        let b = Arc::new(RecordingEventPublisher::new());
        let composite =
            CompositeEventPublisher::new(vec![a.clone() as Arc<dyn EventPublisher>, b.clone()]);

        composite.publish(AppEvent::QuorumResult(sample_payload()));

        assert_eq!(a.events.lock().unwrap().len(), 1);
        assert_eq!(b.events.lock().unwrap().len(), 1);
    }
}
