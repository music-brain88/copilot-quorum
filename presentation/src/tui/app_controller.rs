//! Background controller task (Actor pattern)
//!
//! Owns the AgentController and processes commands from the TUI event loop.

use super::event::{RoutedTuiEvent, TuiCommand, TuiEvent};
use super::progress::TuiProgressBridge;
use quorum_application::{AgentController, CommandAction};
use quorum_domain::interaction::InteractionForm;
use tokio::sync::mpsc;

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

    let mut tasks = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            biased;

            // Handle completed tasks (spawns + inline executions)
            Some(res) = tasks.join_next() => {
                match res {
                    Ok(completion) => controller.finalize(completion),
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
                        let (clean_query, full_query) = controller.prepare_inline(&request);
                        let context = controller.build_spawn_context();
                        let tx = progress_tx.clone();
                        tasks.spawn(async move {
                            let progress = TuiProgressBridge::for_interaction(tx, iid);
                            context.execute(None, InteractionForm::Agent, clean_query, full_query, &progress).await
                        });
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
                                let (clean_query, full_query) = controller.prepare_inline(&query);
                                let context = controller.build_spawn_context();
                                let tx = progress_tx.clone();
                                tasks.spawn(async move {
                                    let progress = TuiProgressBridge::for_interaction(tx, iid);
                                    context.execute(None, form, clean_query, full_query, &progress).await
                                });
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
                                let context = controller.build_spawn_context();
                                let tx = progress_tx.clone();

                                tasks.spawn(async move {
                                    let progress = TuiProgressBridge::for_interaction(tx, child_id);
                                    context.execute(Some(child_id), form, clean_query, full_query, &progress).await
                                });
                            }
                            Err(e) => {
                                let _ = progress_tx.send(RoutedTuiEvent::global(TuiEvent::Flash(
                                    format!("Failed to prepare spawn: {}", e)
                                )));
                            }
                        }
                    }
                    TuiCommand::ActivateInteraction(id) => {
                        controller.set_active_interaction(id);
                    }
                    TuiCommand::Quit => {
                        break;
                    }
                }
            }
        }
    }
}
