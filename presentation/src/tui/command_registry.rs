//! Single source of truth for built-in `:` commands.
//!
//! Read by both the Help overlay (`app_render::help_lines`) and the Remote
//! Control API's `commands.list` (#302), so the human-facing help text and
//! the machine-facing API can't drift apart — the same structural guarantee
//! Issue #302 asks for ("human が TUI で触れる領域は全て API からも可読").
//!
//! This table only covers *metadata* (name/aliases/usage/description). The
//! actual dispatch logic stays where it already lives — `app_tab_command.rs`
//! (quit/tab family) and `AgentController::handle_command` (mode/config/
//! interaction family) — this registry does not change control flow.

/// Metadata for one built-in `:` command.
#[derive(Debug, Clone, Copy)]
pub struct CommandInfo {
    /// Canonical name, without the leading `:` or `/` (e.g. `"q"`).
    pub name: &'static str,
    /// Alternative names that behave identically (e.g. `["quit"]`).
    pub aliases: &'static [&'static str],
    /// Usage string shown to humans, with the `:` prefix (e.g. `":q"`).
    pub usage: &'static str,
    /// One-line description.
    pub description: &'static str,
}

/// All built-in commands, in the order they should be presented.
pub fn builtin_commands() -> &'static [CommandInfo] {
    COMMANDS
}

static COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        name: "q",
        aliases: &["quit"],
        usage: ":q",
        description: "Close the active tab (quit the app on the last tab)",
    },
    CommandInfo {
        name: "qa",
        aliases: &["qall", "quitall", "exit"],
        usage: ":qa",
        description: "Quit the app (closes all tabs)",
    },
    CommandInfo {
        name: "help",
        aliases: &["h", "?"],
        usage: ":help",
        description: "Show the keyboard/command help overlay",
    },
    CommandInfo {
        name: "solo",
        aliases: &[],
        usage: ":solo",
        description: "Switch to Solo mode (single model, quick execution)",
    },
    CommandInfo {
        name: "ens",
        aliases: &["ensemble"],
        usage: ":ens",
        description: "Switch to Ensemble mode (multi-model driven)",
    },
    CommandInfo {
        name: "fast",
        aliases: &[],
        usage: ":fast",
        description: "Toggle Fast phase scope (skip review phases)",
    },
    CommandInfo {
        name: "mode",
        aliases: &[],
        usage: ":mode <solo|ensemble>",
        description: "Change the consensus level explicitly",
    },
    CommandInfo {
        name: "scope",
        aliases: &[],
        usage: ":scope <full|fast|plan-only>",
        description: "Change the phase scope explicitly",
    },
    CommandInfo {
        name: "strategy",
        aliases: &[],
        usage: ":strategy <quorum|debate>",
        description: "Change the orchestration strategy",
    },
    CommandInfo {
        name: "agent",
        aliases: &[],
        usage: ":agent <task>",
        description: "Open a new Agent tab and run a task",
    },
    CommandInfo {
        name: "ask",
        aliases: &[],
        usage: ":ask <question>",
        description: "Open a new Ask tab (lightweight, read-only Q&A)",
    },
    CommandInfo {
        name: "discuss",
        aliases: &[],
        usage: ":discuss <question>",
        description: "Open a new Discuss tab (multi-model Quorum Discussion)",
    },
    CommandInfo {
        name: "council",
        aliases: &[],
        usage: ":council <question>",
        description: "Run a Quorum Discussion inline in the active tab (no new tab)",
    },
    CommandInfo {
        name: "tabnew",
        aliases: &[],
        usage: ":tabnew [agent|ask|discuss]",
        description: "Open a new tab (defaults to Agent)",
    },
    CommandInfo {
        name: "tabclose",
        aliases: &[],
        usage: ":tabclose",
        description: "Close the active tab",
    },
    CommandInfo {
        name: "tabs",
        aliases: &[],
        usage: ":tabs",
        description: "List open tabs",
    },
    CommandInfo {
        name: "config",
        aliases: &[],
        usage: ":config [section]",
        description: "Show current configuration, optionally filtered by section",
    },
    CommandInfo {
        name: "clear",
        aliases: &[],
        usage: ":clear",
        description: "Clear conversation history",
    },
    CommandInfo {
        name: "init",
        aliases: &[],
        usage: ":init[!]",
        description: "Initialize project context (.quorum/context.md); `!` forces a re-run",
    },
    CommandInfo {
        name: "verbose",
        aliases: &[],
        usage: ":verbose",
        description: "Show the current verbose-mode status",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_commands_not_empty() {
        assert!(!builtin_commands().is_empty());
    }

    #[test]
    fn no_duplicate_names_or_aliases() {
        let mut seen = std::collections::HashSet::new();
        for cmd in builtin_commands() {
            assert!(seen.insert(cmd.name), "duplicate name: {}", cmd.name);
            for alias in cmd.aliases {
                assert!(seen.insert(*alias), "duplicate alias: {}", alias);
            }
        }
    }

    #[test]
    fn every_entry_has_a_description_and_usage() {
        for cmd in builtin_commands() {
            assert!(
                !cmd.description.is_empty(),
                "{} has no description",
                cmd.name
            );
            assert!(!cmd.usage.is_empty(), "{} has no usage", cmd.name);
        }
    }
}
