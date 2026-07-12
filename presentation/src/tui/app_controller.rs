//! Background controller task (Actor pattern)
//!
//! Owns the AgentController and processes commands from the TUI event loop.

// Pure "Cancel & Replace" scheduling logic (#212). File lives alongside this
// one at `tui/interaction_scheduler.rs`, not under `tui/app_controller/`.
#[path = "interaction_scheduler.rs"]
mod interaction_scheduler;

use self::interaction_scheduler::{InteractionScheduler, PendingRestart, RequestAction};
use super::event::{RoutedTuiEvent, TuiCommand, TuiEvent};
use super::progress::TuiProgressBridge;
use futures::FutureExt;
use quorum_application::use_cases::agent_controller::TaskCompletion;
use quorum_application::{AgentController, CommandAction, build_partial_context_prefix};
use quorum_domain::AgentState;
use quorum_domain::interaction::{InteractionForm, InteractionId};
use std::panic::AssertUnwindSafe;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

/// Tasks in flight, each tagged with the [`InteractionId`] and generation
/// number the [`InteractionScheduler`] assigned it (or a fixed `0` for
/// `SpawnInteraction`/`SpawnRootInteraction` tasks, which always use a brand
/// new id and so never participate in Cancel & Replace), so `join_next` can
/// route completions back through the scheduler.
type ControllerJoinSet = JoinSet<(InteractionId, u64, TaskCompletion)>;

/// Background controller task (Actor)
///
/// Owns the AgentController and processes commands from the TUI event loop.
pub(super) async fn controller_task(
    mut controller: AgentController,
    mut cmd_rx: mpsc::UnboundedReceiver<TuiCommand>,
    progress_tx: mpsc::UnboundedSender<RoutedTuiEvent>,
) {
    // Send welcome on startup
    controller.send_welcome();

    let mut tasks: ControllerJoinSet = JoinSet::new();
    let mut scheduler = InteractionScheduler::new();

    loop {
        tokio::select! {
            biased;

            // Handle completed tasks (spawns + inline executions)
            Some(res) = tasks.join_next() => {
                match res {
                    Ok((iid, generation, mut completion)) => {
                        let cancelled_state = completion.cancelled_state.take();
                        controller.finalize(completion);
                        if let Some((new_generation, pending)) = scheduler.complete(iid, generation) {
                            spawn_pending(
                                &mut controller,
                                &mut tasks,
                                &progress_tx,
                                iid,
                                new_generation,
                                pending,
                                cancelled_state,
                            );
                        }
                    }
                    Err(e) => {
                        // Genuine `JoinError` (task aborted, or — in
                        // principle — a panic from outside the
                        // `catch_unwind` guard every task body is now
                        // wrapped in below, which shouldn't happen). Unlike
                        // a guarded panic, this carries no `(InteractionId,
                        // generation)`, so the scheduler entry for whichever
                        // interaction owned this task can't be cleared here
                        // — it stays "busy" until the tab closes.
                        if e.is_cancelled() {
                            // ignore
                        } else {
                            let _ = progress_tx.send(RoutedTuiEvent::global(TuiEvent::Flash(
                                format!("Task panicked: {}", e)
                            )));
                        }
                    }
                }
            }

            // Handle commands
            cmd_opt = cmd_rx.recv() => {
                let cmd = match cmd_opt {
                    Some(c) => c,
                    None => break, // Channel closed
                };

                match cmd {
                    TuiCommand::ProcessRequest { interaction_id, request } => {
                        let iid = interaction_id.unwrap_or_else(|| controller.active_interaction_id());
                        match scheduler.request(iid, InteractionForm::Agent, request.clone()) {
                            RequestAction::SpawnNow(generation) => {
                                spawn_inline(&mut controller, &mut tasks, &progress_tx, iid, generation, InteractionForm::Agent, request);
                            }
                            RequestAction::Deferred => {
                                // A task for this interaction is already running:
                                // cancel it now. Its completion drives `scheduler.complete`
                                // above, which promotes this deferred request (Cancel & Replace).
                                controller.cancel_interaction(iid);
                            }
                        }
                    }
                    TuiCommand::HandleCommand { interaction_id, command } => {
                        if command == "__welcome" {
                             continue;
                        }
                        if command.starts_with("__") {
                            continue;
                        }

                        let cmd_str = format!("/{}", command);

                        let iid = interaction_id.unwrap_or_else(|| controller.active_interaction_id());
                        let progress = TuiProgressBridge::for_interaction(progress_tx.clone(), iid);

                        match controller.handle_command(&cmd_str, &progress).await {
                            CommandAction::Exit => {
                                break;
                            }
                            CommandAction::Continue => {}
                            CommandAction::Execute { form, query } => {
                                match scheduler.request(iid, form, query.clone()) {
                                    RequestAction::SpawnNow(generation) => {
                                        spawn_inline(&mut controller, &mut tasks, &progress_tx, iid, generation, form, query);
                                    }
                                    RequestAction::Deferred => {
                                        controller.cancel_interaction(iid);
                                    }
                                }
                            }
                        }
                    }
                    TuiCommand::SetVerbose(verbose) => {
                        controller.set_verbose(verbose);
                    }
                    TuiCommand::SetCancellation(token) => {
                        controller.set_cancellation(token);
                    }
                    TuiCommand::SetReferenceResolver(resolver) => {
                        controller.set_reference_resolver(resolver);
                    }
                    TuiCommand::SetScriptingEngine(engine) => {
                        controller.set_scripting_engine(engine);
                    }
                    TuiCommand::SpawnInteraction {
                        form,
                        query,
                        context_mode_override,
                    } => {
                        match controller.prepare_spawn(form, &query, context_mode_override) {
                            Ok((child_id, clean_query, full_query)) => {
                                let context = controller.build_spawn_context_for(child_id);
                                let tx = progress_tx.clone();

                                tasks.spawn(async move {
                                    let progress = TuiProgressBridge::for_interaction(tx.clone(), child_id);
                                    let query_for_panic = clean_query.clone();
                                    let fut = context.execute(Some(child_id), form, clean_query, full_query, &progress);
                                    let (completion, panic_message) =
                                        catch_panic(Some(child_id), form, query_for_panic, fut).await;
                                    if let Some(message) = panic_message {
                                        let _ = tx.send(RoutedTuiEvent::for_interaction(
                                            child_id,
                                            TuiEvent::Flash(format!("タスクが異常終了しました: {message}")),
                                        ));
                                    }
                                    // `child_id` is always freshly allocated, so this task never
                                    // participates in Cancel & Replace; generation `0` is a fixed
                                    // placeholder the scheduler safely ignores on completion.
                                    (child_id, 0, completion)
                                });
                            }
                            Err(e) => {
                                let _ = progress_tx.send(RoutedTuiEvent::global(TuiEvent::Flash(
                                    format!("Failed to prepare spawn: {}", e)
                                )));
                            }
                        }
                    }
                    TuiCommand::SpawnRootInteraction {
                        form,
                        label,
                        material,
                        respond_to,
                    } => {
                        let (root_id, label, material) =
                            controller.prepare_root_spawn(form, label, material);
                        let _ = respond_to.send(root_id);

                        let context = controller.build_spawn_context_for(root_id);
                        let tx = progress_tx.clone();
                        tasks.spawn(async move {
                            let progress = TuiProgressBridge::for_interaction(tx.clone(), root_id);
                            let query_for_panic = label.clone();
                            let fut = context.execute(Some(root_id), form, label, material, &progress);
                            let (completion, panic_message) =
                                catch_panic(Some(root_id), form, query_for_panic, fut).await;
                            if let Some(message) = panic_message {
                                let _ = tx.send(RoutedTuiEvent::for_interaction(
                                    root_id,
                                    TuiEvent::Flash(format!("タスクが異常終了しました: {message}")),
                                ));
                            }
                            // Root spawns get a brand new `root_id` too — not subject to
                            // Cancel & Replace, hence the same fixed `0` generation.
                            (root_id, 0, completion)
                        });
                    }
                    TuiCommand::ActivateInteraction(id) => {
                        controller.set_active_interaction(id);
                    }
                    TuiCommand::CancelInteraction(id) => {
                        controller.cancel_interaction(id);
                    }
                    TuiCommand::Quit => {
                        break;
                    }
                }
            }
        }
    }
}

/// Spawn an inline (no tree node) or command-execute task for `iid`, tagging
/// its completion with `generation` so `join_next` can route it back through
/// the [`InteractionScheduler`] via [`InteractionScheduler::complete`].
fn spawn_inline(
    controller: &mut AgentController,
    tasks: &mut ControllerJoinSet,
    progress_tx: &mpsc::UnboundedSender<RoutedTuiEvent>,
    iid: InteractionId,
    generation: u64,
    form: InteractionForm,
    request: String,
) {
    let (clean_query, full_query) = controller.prepare_inline(&request);
    let context = controller.build_spawn_context_for(iid);
    let tx = progress_tx.clone();
    tasks.spawn(async move {
        let progress = TuiProgressBridge::for_interaction(tx.clone(), iid);
        let query_for_panic = clean_query.clone();
        let fut = context.execute(None, form, clean_query, full_query, &progress);
        let (completion, panic_message) = catch_panic(None, form, query_for_panic, fut).await;
        if let Some(message) = panic_message {
            let _ = tx.send(RoutedTuiEvent::for_interaction(
                iid,
                TuiEvent::Flash(format!("タスクが異常終了しました: {message}")),
            ));
        }
        (iid, generation, completion)
    });
}

/// Spawn the request that was deferred while the previous generation for
/// `iid` was still running (Cancel & Replace promotion), injecting a summary
/// of the cancelled task's partial results as a prefix to the replacement
/// request when available (Agent form only; issue #212).
fn spawn_pending(
    controller: &mut AgentController,
    tasks: &mut ControllerJoinSet,
    progress_tx: &mpsc::UnboundedSender<RoutedTuiEvent>,
    iid: InteractionId,
    generation: u64,
    pending: PendingRestart,
    cancelled_state: Option<Box<AgentState>>,
) {
    spawn_inline_with_partial_context(
        controller,
        tasks,
        progress_tx,
        iid,
        generation,
        pending.form,
        pending.request,
        cancelled_state,
    );
}

/// Like [`spawn_inline`], but for Agent-form requests promoted via Cancel &
/// Replace: prefixes `clean_query` with a summary of `cancelled_state` (the
/// in-flight `AgentState` snapshot from the task it replaced), when present
/// and non-empty, so the new run can pick up where the cancelled one left off.
///
/// For non-Agent forms `cancelled_state` is always `None` (only Agent
/// executions produce a snapshot), so this is a no-op wrapper around
/// [`spawn_inline`] in that case.
#[allow(clippy::too_many_arguments)]
fn spawn_inline_with_partial_context(
    controller: &mut AgentController,
    tasks: &mut ControllerJoinSet,
    progress_tx: &mpsc::UnboundedSender<RoutedTuiEvent>,
    iid: InteractionId,
    generation: u64,
    form: InteractionForm,
    request: String,
    cancelled_state: Option<Box<AgentState>>,
) {
    let (mut clean_query, full_query) = controller.prepare_inline(&request);

    if form == InteractionForm::Agent
        && let Some(state) = cancelled_state.as_deref()
    {
        let prefix = build_partial_context_prefix(state);
        if !prefix.is_empty() {
            clean_query = format!("{prefix}\n\n{clean_query}");
        }
    }

    let context = controller.build_spawn_context_for(iid);
    let tx = progress_tx.clone();
    tasks.spawn(async move {
        let progress = TuiProgressBridge::for_interaction(tx.clone(), iid);
        let query_for_panic = clean_query.clone();
        let fut = context.execute(None, form, clean_query, full_query, &progress);
        let (completion, panic_message) = catch_panic(None, form, query_for_panic, fut).await;
        if let Some(message) = panic_message {
            let _ = tx.send(RoutedTuiEvent::for_interaction(
                iid,
                TuiEvent::Flash(format!("タスクが異常終了しました: {message}")),
            ));
        }
        (iid, generation, completion)
    });
}

/// Runs `fut` to completion, catching a panic and converting it into a
/// synthetic [`TaskCompletion`] (`result: None`, `cancelled_state: None`)
/// instead of letting it unwind out of the spawned task.
///
/// Without this, a panicking task only surfaced as a bare `JoinError` at
/// `tasks.join_next()`, which carries no `(InteractionId, generation)` — so
/// `scheduler.complete` was never called for it: the scheduler entry for the
/// interaction never cleared, and every future input to it was deferred
/// forever (issue #318). Returns the panic's message alongside the
/// completion so the caller can surface it to the user.
async fn catch_panic(
    interaction_id: Option<InteractionId>,
    form: InteractionForm,
    query: String,
    fut: impl std::future::Future<Output = TaskCompletion> + Send,
) -> (TaskCompletion, Option<String>) {
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(completion) => (completion, None),
        Err(payload) => (
            TaskCompletion {
                interaction_id,
                form,
                query,
                result: None,
                cancelled_state: None,
            },
            // `&*payload`, not `&payload`: `Box<dyn Any + Send>` is itself
            // `Any + Send` (blanket impl), so `&payload` would coerce to
            // `&(dyn Any + Send)` by unsizing the *outer* Box rather than
            // dereferencing to the panic value it holds — every downcast_ref
            // in `panic_payload_message` would then silently miss.
            Some(panic_payload_message(&*payload)),
        ),
    }
}

/// Best-effort extraction of a human-readable message from a panic payload
/// (`std::panic::catch_unwind`'s error type carries no guaranteed structure —
/// only the two conventional payload types produced by `panic!` are handled).
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: usize) -> InteractionId {
        InteractionId(n)
    }

    #[tokio::test]
    async fn catch_panic_passes_through_successful_completion() {
        let completion = TaskCompletion {
            interaction_id: None,
            form: InteractionForm::Agent,
            query: "hello".to_string(),
            result: None,
            cancelled_state: None,
        };
        let fut = async { completion };

        let (completion, message) =
            catch_panic(None, InteractionForm::Agent, "hello".to_string(), fut).await;

        assert_eq!(completion.query, "hello");
        assert!(message.is_none());
    }

    #[tokio::test]
    async fn catch_panic_recovers_from_panic_with_message() {
        let fut = async {
            panic!("boom");
            #[allow(unreachable_code)]
            TaskCompletion {
                interaction_id: None,
                form: InteractionForm::Agent,
                query: String::new(),
                result: None,
                cancelled_state: None,
            }
        };

        let (completion, message) = catch_panic(
            Some(id(1)),
            InteractionForm::Agent,
            "original request".to_string(),
            fut,
        )
        .await;

        assert_eq!(completion.interaction_id, Some(id(1)));
        assert_eq!(completion.form, InteractionForm::Agent);
        assert_eq!(completion.query, "original request");
        assert!(completion.result.is_none());
        assert!(completion.cancelled_state.is_none());
        assert!(message.unwrap().contains("boom"));
    }

    /// Regression for issue #318 (finding ①): before this fix, a panicking
    /// task never called `scheduler.complete`, so the scheduler's generation
    /// entry for the interaction never cleared and every later request for
    /// it was deferred forever. This reproduces the exact sequence
    /// `controller_task`'s `join_next` Ok arm now drives — panic recovery via
    /// `catch_panic`, then feeding the recovered completion's generation into
    /// `scheduler.complete` — and checks the interaction goes back to idle
    /// (no pending request) rather than staying stuck.
    #[tokio::test]
    async fn panicking_task_still_lets_scheduler_return_to_idle() {
        let mut scheduler = InteractionScheduler::new();
        let iid = id(1);

        assert_eq!(
            scheduler.request(iid, InteractionForm::Agent, "first".to_string()),
            RequestAction::SpawnNow(1)
        );

        let panicking = async {
            panic!("boom");
            #[allow(unreachable_code)]
            TaskCompletion {
                interaction_id: None,
                form: InteractionForm::Agent,
                query: String::new(),
                result: None,
                cancelled_state: None,
            }
        };
        let (completion, message) =
            catch_panic(None, InteractionForm::Agent, "first".to_string(), panicking).await;
        assert!(completion.result.is_none());
        assert!(message.is_some());

        // No request was deferred while generation 1 was running, so
        // completing it marks the interaction idle again.
        assert_eq!(scheduler.complete(iid, 1), None);

        // Idle: the SAME interaction id must be able to spawn immediately —
        // before the fix, the scheduler's entry for `iid` was never cleared,
        // so this would have incorrectly deferred.
        assert_eq!(
            scheduler.request(iid, InteractionForm::Agent, "second".to_string()),
            RequestAction::SpawnNow(1)
        );
    }

    /// Same as above, but a request arrived (and was deferred) while the
    /// now-panicking task was running: the panic recovery must still drive
    /// Cancel & Replace's promotion, not just leave the interaction idle.
    #[tokio::test]
    async fn panicking_task_still_promotes_pending_request() {
        let mut scheduler = InteractionScheduler::new();
        let iid = id(7);

        assert_eq!(
            scheduler.request(iid, InteractionForm::Agent, "first".to_string()),
            RequestAction::SpawnNow(1)
        );
        assert_eq!(
            scheduler.request(iid, InteractionForm::Agent, "second".to_string()),
            RequestAction::Deferred
        );

        let panicking = async {
            panic!("boom");
            #[allow(unreachable_code)]
            TaskCompletion {
                interaction_id: None,
                form: InteractionForm::Agent,
                query: String::new(),
                result: None,
                cancelled_state: None,
            }
        };
        let (completion, _message) =
            catch_panic(None, InteractionForm::Agent, "first".to_string(), panicking).await;
        assert!(completion.result.is_none());

        let promoted = scheduler.complete(iid, 1);
        assert_eq!(
            promoted,
            Some((
                2,
                PendingRestart {
                    form: InteractionForm::Agent,
                    request: "second".to_string(),
                }
            ))
        );
    }
}
