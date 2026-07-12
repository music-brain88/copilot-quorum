//! Aggregates coarse execution status across concurrent interactions and
//! publishes [`AppEvent::AgentStatusChanged`] only when the aggregate
//! changes (Issue #309 / RFC Discussion #313).
//!
//! Priority when multiple interactions are in flight: **Blocked** (any HiL
//! pending) beats **Working** (any interaction executing) beats **Idle**
//! (nothing in flight). Callers get a scope guard back from
//! [`StatusTracker::enter_working`] / [`StatusTracker::enter_blocked`] —
//! dropping it (including via panic or task cancellation) always exits that
//! slot and republishes if the aggregate changed as a result, so a crashed
//! or cancelled interaction can never leave the tracker stuck reporting
//! Working/Blocked forever.

use std::sync::{Arc, Mutex};

use quorum_domain::AgentStatus;

use crate::ports::event_publisher::{AppEvent, EventPublisher};

#[derive(Default)]
struct TrackerState {
    working: u32,
    blocked: u32,
    /// Detail of the most recently entered still-active Blocked slot.
    /// Best-effort display sugar only — not tracked per-slot.
    blocked_detail: Option<String>,
    last_published: Option<AgentStatus>,
}

impl TrackerState {
    fn aggregate(&self) -> AgentStatus {
        if self.blocked > 0 {
            AgentStatus::Blocked(self.blocked_detail.clone())
        } else if self.working > 0 {
            AgentStatus::Working(None)
        } else {
            AgentStatus::Idle
        }
    }
}

/// Shared aggregation point for this quorum instance's coarse status.
///
/// Owned by [`AgentController`](crate::use_cases::agent_controller::AgentController)
/// and shared (via `Arc`) into every `RunAgentUseCase` clone and `SpawnContext`
/// so both the "an interaction is running" transition (spawn/finalize) and
/// the "HiL is pending" transition (deep inside `RunAgentUseCase`) feed the
/// same aggregate.
pub struct StatusTracker {
    state: Mutex<TrackerState>,
}

impl StatusTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TrackerState::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TrackerState> {
        self.state.lock().expect("status tracker lock poisoned")
    }

    /// Publish the current aggregate if it differs from the last published one.
    fn publish_if_changed(&self, publisher: &Arc<dyn EventPublisher>) {
        let current = {
            let mut state = self.lock();
            let current = state.aggregate();
            if state.last_published.as_ref() == Some(&current) {
                return;
            }
            state.last_published = Some(current.clone());
            current
        };
        publisher.publish(AppEvent::AgentStatusChanged(current));
    }

    /// Mark one interaction as executing. Returns a guard — dropping it
    /// (normal return, early return, panic, or cancellation) exits the slot.
    pub fn enter_working(self: &Arc<Self>, publisher: Arc<dyn EventPublisher>) -> WorkingGuard {
        self.lock().working += 1;
        self.publish_if_changed(&publisher);
        WorkingGuard {
            tracker: self.clone(),
            publisher,
        }
    }

    /// Mark one interaction as blocked on a human decision (HiL). Returns a
    /// guard — dropping it exits the slot.
    pub fn enter_blocked(
        self: &Arc<Self>,
        detail: impl Into<String>,
        publisher: Arc<dyn EventPublisher>,
    ) -> BlockedGuard {
        {
            let mut state = self.lock();
            state.blocked += 1;
            state.blocked_detail = Some(detail.into());
        }
        self.publish_if_changed(&publisher);
        BlockedGuard {
            tracker: self.clone(),
            publisher,
        }
    }
}

/// RAII guard for [`StatusTracker::enter_working`].
pub struct WorkingGuard {
    tracker: Arc<StatusTracker>,
    publisher: Arc<dyn EventPublisher>,
}

impl Drop for WorkingGuard {
    fn drop(&mut self) {
        {
            let mut state = self.tracker.lock();
            state.working = state.working.saturating_sub(1);
        }
        self.tracker.publish_if_changed(&self.publisher);
    }
}

/// RAII guard for [`StatusTracker::enter_blocked`].
pub struct BlockedGuard {
    tracker: Arc<StatusTracker>,
    publisher: Arc<dyn EventPublisher>,
}

impl Drop for BlockedGuard {
    fn drop(&mut self) {
        {
            let mut state = self.tracker.lock();
            state.blocked = state.blocked.saturating_sub(1);
            if state.blocked == 0 {
                state.blocked_detail = None;
            }
        }
        self.tracker.publish_if_changed(&self.publisher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    struct RecordingPublisher {
        events: StdMutex<Vec<AppEvent>>,
    }

    impl RecordingPublisher {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: StdMutex::new(Vec::new()),
            })
        }

        fn statuses(&self) -> Vec<AgentStatus> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| match e {
                    AppEvent::AgentStatusChanged(s) => s.clone(),
                    other => panic!("unexpected event: {:?}", other),
                })
                .collect()
        }
    }

    impl EventPublisher for RecordingPublisher {
        fn publish(&self, event: AppEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn test_working_then_idle() {
        let tracker = StatusTracker::new();
        let recording = RecordingPublisher::new();
        let publisher: Arc<dyn EventPublisher> = recording.clone();

        let guard = tracker.enter_working(publisher.clone());
        drop(guard);

        assert_eq!(
            recording.statuses(),
            vec![AgentStatus::Working(None), AgentStatus::Idle]
        );
    }

    #[test]
    fn test_blocked_beats_working() {
        let tracker = StatusTracker::new();
        let recording = RecordingPublisher::new();
        let publisher: Arc<dyn EventPublisher> = recording.clone();

        let working = tracker.enter_working(publisher.clone());
        let blocked = tracker.enter_blocked("HiL: プラン承認待ち", publisher.clone());
        drop(blocked);
        drop(working);

        assert_eq!(
            recording.statuses(),
            vec![
                AgentStatus::Working(None),
                AgentStatus::blocked("HiL: プラン承認待ち"),
                AgentStatus::Working(None),
                AgentStatus::Idle,
            ]
        );
    }

    #[test]
    fn test_concurrent_interactions_aggregate_to_working_until_all_finish() {
        let tracker = StatusTracker::new();
        let recording = RecordingPublisher::new();
        let publisher: Arc<dyn EventPublisher> = recording.clone();

        let a = tracker.enter_working(publisher.clone());
        let b = tracker.enter_working(publisher.clone());
        // Second concurrent Working must not re-publish (no change).
        assert_eq!(recording.statuses(), vec![AgentStatus::Working(None)]);

        drop(a);
        // One of two still running — still Working, no new publish.
        assert_eq!(recording.statuses(), vec![AgentStatus::Working(None)]);

        drop(b);
        assert_eq!(
            recording.statuses(),
            vec![AgentStatus::Working(None), AgentStatus::Idle]
        );
    }

    #[test]
    fn test_no_publish_when_status_unchanged() {
        let tracker = StatusTracker::new();
        let recording = RecordingPublisher::new();
        let publisher: Arc<dyn EventPublisher> = recording.clone();

        let a = tracker.enter_working(publisher.clone());
        let b = tracker.enter_working(publisher.clone());
        let c = tracker.enter_working(publisher.clone());
        drop(a);
        drop(b);
        drop(c);

        // Only the first Working and the final Idle are real transitions.
        assert_eq!(
            recording.statuses(),
            vec![AgentStatus::Working(None), AgentStatus::Idle]
        );
    }

    #[test]
    fn test_blocked_detail_changes_republish_while_still_blocked() {
        let tracker = StatusTracker::new();
        let recording = RecordingPublisher::new();
        let publisher: Arc<dyn EventPublisher> = recording.clone();

        let first = tracker.enter_blocked("HiL: プラン承認待ち", publisher.clone());
        drop(first);
        let second = tracker.enter_blocked("HiL: 実行確認待ち", publisher.clone());
        drop(second);

        assert_eq!(
            recording.statuses(),
            vec![
                AgentStatus::blocked("HiL: プラン承認待ち"),
                AgentStatus::Idle,
                AgentStatus::blocked("HiL: 実行確認待ち"),
                AgentStatus::Idle,
            ]
        );
    }
}
