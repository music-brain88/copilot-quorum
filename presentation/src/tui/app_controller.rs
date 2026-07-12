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
use quorum_application::use_cases::agent_controller::TaskCompletion;
use quorum_application::{AgentController, CommandAction, build_partial_context_prefix};
use quorum_domain::AgentState;
use quorum_domain::interaction::{InteractionForm, InteractionId};
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
                    Ok((iid, generation, completion)) => {
                        let cancelled_state = completion.cancelled_state.clone();
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
                        // Task panic or cancellation
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
                                    let progress = TuiProgressBridge::for_interaction(tx, child_id);
                                    let completion = context.execute(Some(child_id), form, clean_query, full_query, &progress).await;
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
                            let progress = TuiProgressBridge::for_interaction(tx, root_id);
                            let completion = context
                                .execute(Some(root_id), form, label, material, &progress)
                                .await;
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
        let progress = TuiProgressBridge::for_interaction(tx, iid);
        let completion = context
            .execute(None, form, clean_query, full_query, &progress)
            .await;
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
        let progress = TuiProgressBridge::for_interaction(tx, iid);
        let completion = context
            .execute(None, form, clean_query, full_query, &progress)
            .await;
        (iid, generation, completion)
    });
}
