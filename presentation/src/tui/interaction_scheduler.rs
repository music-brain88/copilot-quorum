//! InteractionScheduler — pure "Cancel & Replace" scheduling logic (#212)
//!
//! Decides, for a given [`InteractionId`], whether a new request should be
//! spawned immediately or deferred until the currently running task for that
//! same interaction finishes cancelling.
//!
//! This module is intentionally free of any `tokio` (or other async runtime)
//! dependency: it only tracks generation counters and pending restarts as
//! plain data, so it can be unit tested without spawning tasks.
//!
//! ## Generation protocol
//!
//! - Each interaction that currently has a task "in flight" has an entry in
//!   [`InteractionScheduler::generation`] holding that task's generation
//!   number.
//! - [`InteractionScheduler::request`] either starts a fresh generation
//!   (interaction was idle) or defers the request (interaction is busy),
//!   overwriting any previously deferred request for the same interaction.
//! - [`InteractionScheduler::complete`] is called once the in-flight task for
//!   a given generation actually finishes (naturally or after graceful
//!   cancellation). If the generation matches the tracked one, either the
//!   deferred request is promoted to a new generation (Cancel & Replace) or
//!   the interaction is marked idle. If the generation is stale (does not
//!   match, e.g. a late completion signal for an already-superseded task),
//!   the call is a no-op.

use quorum_domain::interaction::{InteractionForm, InteractionId};
use std::collections::HashMap;

/// A request that arrived while its interaction was still busy, to be
/// re-spawned once the currently running task finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRestart {
    pub form: InteractionForm,
    pub request: String,
}

/// Outcome of [`InteractionScheduler::request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestAction {
    /// The interaction was idle: spawn the task now with the given
    /// generation number.
    SpawnNow(u64),
    /// The interaction was busy: the request has been stored as a
    /// [`PendingRestart`] and will be returned by a later
    /// [`InteractionScheduler::complete`] call.
    Deferred,
}

/// Pure "Cancel & Replace" scheduling logic, keyed by [`InteractionId`].
#[derive(Debug, Default)]
pub struct InteractionScheduler {
    /// Generation number of the task currently in flight for an interaction.
    /// Absence of an entry means the interaction is idle.
    generation: HashMap<InteractionId, u64>,
    /// Deferred request waiting for the in-flight task to finish.
    pending: HashMap<InteractionId, PendingRestart>,
}

impl InteractionScheduler {
    /// Creates an empty scheduler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new request for `id`.
    ///
    /// If the interaction is idle, starts generation `1` and returns
    /// [`RequestAction::SpawnNow`]. If the interaction is busy, stores (and
    /// replaces any previous) pending restart and returns
    /// [`RequestAction::Deferred`].
    pub fn request(
        &mut self,
        id: InteractionId,
        form: InteractionForm,
        request: String,
    ) -> RequestAction {
        use std::collections::hash_map::Entry;

        match self.generation.entry(id) {
            Entry::Vacant(entry) => {
                let generation_num = 1;
                entry.insert(generation_num);
                RequestAction::SpawnNow(generation_num)
            }
            Entry::Occupied(_) => {
                self.pending.insert(id, PendingRestart { form, request });
                RequestAction::Deferred
            }
        }
    }

    /// Reports that the in-flight task for `id` at the given generation has
    /// finished.
    ///
    /// Returns `Some((new_gen, pending))` if a request was deferred while
    /// the task was running, so the caller can immediately spawn it as the
    /// next generation. Returns `None` if there was nothing pending (the
    /// interaction becomes idle) or if `generation_num` is stale (does not
    /// match the currently tracked generation for `id`), in which case the
    /// call is ignored entirely.
    pub fn complete(
        &mut self,
        id: InteractionId,
        generation_num: u64,
    ) -> Option<(u64, PendingRestart)> {
        match self.generation.get(&id) {
            Some(&current) if current == generation_num => {
                if let Some(pending) = self.pending.remove(&id) {
                    let new_gen = current + 1;
                    self.generation.insert(id, new_gen);
                    Some((new_gen, pending))
                } else {
                    self.generation.remove(&id);
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: usize) -> InteractionId {
        InteractionId(n)
    }

    #[test]
    fn same_id_multiple_inputs_defers_and_replaces_pending() {
        let mut scheduler = InteractionScheduler::new();
        let a = id(1);

        // First request: interaction is idle, spawn immediately at generation 1.
        let action = scheduler.request(a, InteractionForm::Agent, "first".to_string());
        assert_eq!(action, RequestAction::SpawnNow(1));

        // Second request while busy: deferred.
        let action = scheduler.request(a, InteractionForm::Agent, "second".to_string());
        assert_eq!(action, RequestAction::Deferred);

        // Third request while still busy: replaces the second pending request.
        let action = scheduler.request(a, InteractionForm::Agent, "third".to_string());
        assert_eq!(action, RequestAction::Deferred);

        // Completion of generation 1 promotes only the latest pending request.
        let result = scheduler.complete(a, 1);
        assert_eq!(
            result,
            Some((
                2,
                PendingRestart {
                    form: InteractionForm::Agent,
                    request: "third".to_string(),
                }
            ))
        );

        // No more pending after promotion: completing generation 2 with nothing
        // pending marks the interaction idle.
        assert_eq!(scheduler.complete(a, 2), None);

        // Interaction is idle again: a new request spawns immediately,
        // restarting the generation counter.
        let action = scheduler.request(a, InteractionForm::Ask, "fourth".to_string());
        assert_eq!(action, RequestAction::SpawnNow(1));
    }

    #[test]
    fn different_ids_are_independent() {
        let mut scheduler = InteractionScheduler::new();
        let a = id(1);
        let b = id(2);

        let action_a = scheduler.request(a, InteractionForm::Agent, "a1".to_string());
        assert_eq!(action_a, RequestAction::SpawnNow(1));

        // b is unrelated to a: also spawns immediately even though a is busy.
        let action_b = scheduler.request(b, InteractionForm::Discuss, "b1".to_string());
        assert_eq!(action_b, RequestAction::SpawnNow(1));

        // A second request for `a` defers, but must not affect `b` at all.
        let action_a2 = scheduler.request(a, InteractionForm::Agent, "a2".to_string());
        assert_eq!(action_a2, RequestAction::Deferred);

        // Completing `b` has no pending restart and does not touch `a`'s
        // pending request.
        assert_eq!(scheduler.complete(b, 1), None);

        let result_a = scheduler.complete(a, 1);
        assert_eq!(
            result_a,
            Some((
                2,
                PendingRestart {
                    form: InteractionForm::Agent,
                    request: "a2".to_string(),
                }
            ))
        );
    }

    #[test]
    fn stale_generation_is_ignored() {
        let mut scheduler = InteractionScheduler::new();
        let a = id(42);

        let action = scheduler.request(a, InteractionForm::Agent, "first".to_string());
        assert_eq!(action, RequestAction::SpawnNow(1));

        // Defer a replacement while generation 1 is still running.
        let action = scheduler.request(a, InteractionForm::Agent, "second".to_string());
        assert_eq!(action, RequestAction::Deferred);

        // Promote to generation 2.
        let result = scheduler.complete(a, 1);
        assert_eq!(
            result,
            Some((
                2,
                PendingRestart {
                    form: InteractionForm::Agent,
                    request: "second".to_string(),
                }
            ))
        );

        // A late/duplicate completion signal for the now-superseded generation 1
        // must be ignored: it must not clear generation 2's tracking nor fabricate
        // a spurious pending restart.
        assert_eq!(scheduler.complete(a, 1), None);

        // generation 2 is still tracked as in-flight (unaffected by the stale
        // signal above): a new request while it's active still defers.
        let action = scheduler.request(a, InteractionForm::Discuss, "third".to_string());
        assert_eq!(action, RequestAction::Deferred);

        // Completing the real, current generation (2) now correctly
        // promotes the pending request.
        let result = scheduler.complete(a, 2);
        assert_eq!(
            result,
            Some((
                3,
                PendingRestart {
                    form: InteractionForm::Discuss,
                    request: "third".to_string(),
                }
            ))
        );

        // An unknown id is also safely ignored.
        assert_eq!(scheduler.complete(id(999), 1), None);
    }

    /// Regression for issue #318 (finding ②): `TuiCommand::SpawnInteraction`/
    /// `SpawnRootInteraction` used to skip `request()` entirely and tag their
    /// task with a fixed placeholder generation, so the scheduler had no
    /// entry for a freshly spawned (now tab-bound) interaction. An input
    /// arriving at that tab while the spawn task was still running then saw
    /// the interaction as idle and raced a second concurrent task for it —
    /// exactly the double-execution `#212` was meant to prevent.
    ///
    /// This reproduces the fixed flow: the spawn path now calls `request()`
    /// too (always `SpawnNow(1)`, since the id is freshly allocated), so a
    /// concurrent input for the same id correctly defers instead of racing,
    /// and is promoted once the spawn task completes.
    #[test]
    fn spawn_registration_then_concurrent_request_defers_and_promotes() {
        let mut scheduler = InteractionScheduler::new();
        let spawned = id(99);

        // The spawn path registers its freshly allocated id — always idle,
        // so always SpawnNow(1).
        let action = scheduler.request(spawned, InteractionForm::Agent, "spawn query".to_string());
        assert_eq!(action, RequestAction::SpawnNow(1));

        // While the spawn task is still running, input arrives at the now
        // bound tab (e.g. the user types into it): must defer, not spawn a
        // second concurrent task for the same interaction.
        let action = scheduler.request(
            spawned,
            InteractionForm::Agent,
            "concurrent input".to_string(),
        );
        assert_eq!(action, RequestAction::Deferred);

        // Completing the spawn task's generation promotes the deferred
        // request (Cancel & Replace).
        let result = scheduler.complete(spawned, 1);
        assert_eq!(
            result,
            Some((
                2,
                PendingRestart {
                    form: InteractionForm::Agent,
                    request: "concurrent input".to_string(),
                }
            ))
        );
    }
}
