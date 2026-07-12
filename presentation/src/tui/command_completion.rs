//! Vim-like `wildmode=longest:full` completion for Command mode (`:`, #326).
//!
//! Pure logic only — no rendering, no `TuiState` mutation. `advance()` is the
//! single entry point `app_action_handler` calls from `KeyAction::CommandComplete`;
//! everything else here is a private helper covered directly by unit tests.
//!
//! Step semantics (mirrors vim's `wildmode=longest:full`, simplified by one
//! detail noted below):
//! - 1st Tab: extend the word to the longest common prefix of all matches.
//! - If that doesn't change anything (already at the longest prefix, or a
//!   single exact match), the *same* keypress starts full cycling instead of
//!   requiring a second, no-op Tab press first — a deliberate small
//!   deviation from vim to avoid a dead keystroke.
//! - Further Tab/Shift+Tab presses cycle forward/backward through the full
//!   match list.

use super::command_registry;
use super::mode::CompletionDirection;

/// Active completion state, carried in `TuiState` across Tab presses within
/// the same completion session (discarded on text edits or Esc — see
/// `app_action_handler`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCompletion {
    /// Full `command_input` text as typed before the first Tab press.
    /// Restored verbatim when Esc cancels the completion.
    pub original_input: String,
    /// Portion of `command_input` before the completed word (e.g.
    /// `"strategy "` for `:strategy q<Tab>`). Unchanged across cycling.
    pub prefix: String,
    /// Sorted, deduped candidate pool matching the word at the time Tab was
    /// first pressed.
    pub matches: Vec<String>,
    /// `None` while still in the "longest common prefix" step. `Some(i)`
    /// once cycling has started — `matches[i]` is the currently shown entry.
    pub cycle_index: Option<usize>,
}

/// Advance completion by one Tab (`Forward`) / Shift+Tab (`Backward`) press.
///
/// `command_input` is the current Command-mode buffer; `prior` is the
/// completion state from the previous press in this session (`None` on the
/// first press). `lua_command_names` supplies user-defined command names
/// (`quorum.command.register`) so they complete alongside builtins.
///
/// Returns `None` when there is nothing to complete (Tab is then a no-op —
/// the caller decides how to surface that, e.g. a flash message). Otherwise
/// returns the replacement `command_input` text and the completion state to
/// keep for the next press.
pub fn advance(
    command_input: &str,
    prior: Option<&CommandCompletion>,
    lua_command_names: &[String],
    direction: CompletionDirection,
) -> Option<(String, CommandCompletion)> {
    if let Some(state) = prior.filter(|s| !s.matches.is_empty()) {
        let len = state.matches.len();
        let next_index = match (state.cycle_index, direction) {
            (None, CompletionDirection::Forward) => 0,
            (None, CompletionDirection::Backward) => len - 1,
            (Some(i), CompletionDirection::Forward) => (i + 1) % len,
            (Some(i), CompletionDirection::Backward) => (i + len - 1) % len,
        };
        let new_input = format!("{}{}", state.prefix, state.matches[next_index]);
        return Some((
            new_input,
            CommandCompletion {
                original_input: state.original_input.clone(),
                prefix: state.prefix.clone(),
                matches: state.matches.clone(),
                cycle_index: Some(next_index),
            },
        ));
    }

    let scope = completion_scope(command_input, lua_command_names);
    let matches = matches_for_prefix(&scope.candidates, &scope.word);
    if matches.is_empty() {
        return None;
    }
    let original_input = command_input.to_string();
    let lcp = longest_common_prefix(&matches);
    if lcp.len() > scope.word.len() {
        let new_input = format!("{}{}", scope.prefix, lcp);
        Some((
            new_input,
            CommandCompletion {
                original_input,
                prefix: scope.prefix,
                matches,
                cycle_index: None,
            },
        ))
    } else {
        let idx = match direction {
            CompletionDirection::Forward => 0,
            CompletionDirection::Backward => matches.len() - 1,
        };
        let new_input = format!("{}{}", scope.prefix, matches[idx]);
        Some((
            new_input,
            CommandCompletion {
                original_input,
                prefix: scope.prefix,
                matches,
                cycle_index: Some(idx),
            },
        ))
    }
}

/// What `command_input` is asking to complete: the text to keep as-is
/// (`prefix`), the token being completed (`word`), and the full candidate
/// pool for that word (unfiltered by `word` — `advance` filters by prefix).
struct CompletionScope {
    prefix: String,
    word: String,
    candidates: Vec<String>,
}

/// Resolve completion scope from the raw Command-mode buffer.
///
/// - No space yet → completing the command name itself (builtin + alias +
///   Lua-registered names).
/// - Exactly one space, nothing after it but the first argument word →
///   completing that command's first-argument candidates (`:strategy`,
///   `:config` — #326 stretch scope; unknown commands have no candidates).
/// - Anything past the first argument → no completion (future scope).
fn completion_scope(input: &str, lua_command_names: &[String]) -> CompletionScope {
    match input.split_once(' ') {
        None => CompletionScope {
            prefix: String::new(),
            word: input.to_string(),
            candidates: command_name_candidates(lua_command_names),
        },
        Some((cmd, rest)) if !rest.contains(' ') => CompletionScope {
            prefix: format!("{} ", cmd),
            word: rest.to_string(),
            candidates: first_arg_candidates(cmd),
        },
        Some(_) => CompletionScope {
            prefix: input.to_string(),
            word: String::new(),
            candidates: Vec::new(),
        },
    }
}

/// Command-name candidate pool: builtin canonical names + aliases (#302's
/// `command_registry`, the single source of truth) plus Lua-registered
/// command names.
fn command_name_candidates(lua_command_names: &[String]) -> Vec<String> {
    let mut names: Vec<String> = command_registry::builtin_commands()
        .iter()
        .flat_map(|c| {
            std::iter::once(c.name.to_string()).chain(c.aliases.iter().map(|a| a.to_string()))
        })
        .collect();
    names.extend(lua_command_names.iter().cloned());
    names.sort();
    names.dedup();
    names
}

/// First-argument candidate pool for commands with a fixed value set.
/// Everything else (including Lua-registered commands, which have no
/// argument metadata) has no candidates — Tab is then a no-op there.
fn first_arg_candidates(cmd: &str) -> Vec<String> {
    match cmd {
        "strategy" => vec!["quorum".to_string(), "debate".to_string()],
        "config" => quorum_domain::known_keys()
            .iter()
            .map(|k| k.key.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Candidates whose name starts with `word`. `candidates` is already sorted
/// (see `command_name_candidates`), so filtering preserves order.
fn matches_for_prefix(candidates: &[String], word: &str) -> Vec<String> {
    candidates
        .iter()
        .filter(|c| c.starts_with(word))
        .cloned()
        .collect()
}

/// Longest common byte prefix of `items` (command/config names are ASCII,
/// so byte-wise comparison is safe and avoids UTF-8 boundary bookkeeping).
fn longest_common_prefix(items: &[String]) -> String {
    match items.split_first() {
        None => String::new(),
        Some((first, rest)) => {
            let mut len = first.len();
            for s in rest {
                let common = first
                    .bytes()
                    .zip(s.bytes())
                    .take_while(|(a, b)| a == b)
                    .count();
                len = len.min(common);
            }
            first[..len].to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // -- longest_common_prefix --

    #[test]
    fn lcp_of_empty_is_empty() {
        assert_eq!(longest_common_prefix(&[]), "");
    }

    #[test]
    fn lcp_of_single_is_itself() {
        assert_eq!(longest_common_prefix(&names(&["solo"])), "solo");
    }

    #[test]
    fn lcp_stops_at_divergence() {
        assert_eq!(longest_common_prefix(&names(&["scope", "solo"])), "s");
        assert_eq!(
            longest_common_prefix(&names(&["strategy", "scope", "solo"])),
            "s"
        );
    }

    #[test]
    fn lcp_of_shared_prefix_word() {
        assert_eq!(longest_common_prefix(&names(&["q", "qa"])), "q");
    }

    // -- matches_for_prefix --

    #[test]
    fn filters_by_prefix_preserving_order() {
        let pool = names(&["ask", "agent", "config"]);
        assert_eq!(matches_for_prefix(&pool, "a"), names(&["ask", "agent"]));
    }

    #[test]
    fn empty_prefix_matches_everything() {
        let pool = names(&["a", "b"]);
        assert_eq!(matches_for_prefix(&pool, ""), pool);
    }

    // -- command_name_candidates --

    #[test]
    fn command_name_candidates_include_aliases_and_lua() {
        let cands = command_name_candidates(&names(&["mytool"]));
        assert!(cands.contains(&"qa".to_string()));
        assert!(cands.contains(&"qall".to_string())); // alias of qa
        assert!(cands.contains(&"mytool".to_string()));
    }

    #[test]
    fn command_name_candidates_dedup_and_sorted() {
        // "help" alias "h" and "?" shouldn't collide with anything, but the
        // list overall must be sorted + deduped even with repeats from Lua.
        let cands = command_name_candidates(&names(&["q", "q"]));
        let mut sorted = cands.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(cands, sorted);
    }

    // -- advance: command-name completion --

    #[test]
    fn advance_extends_to_longest_common_prefix() {
        // "s" matches scope/solo/strategy among builtins. Their LCP is just
        // "s" again (no shared second character), so this press can't
        // extend the text — it falls straight into cycling (see module doc
        // deviation from vim).
        let (new_input, state) = advance("s", None, &[], CompletionDirection::Forward).unwrap();
        assert_eq!(state.matches, names(&["scope", "solo", "strategy"]));
        assert_eq!(new_input, "scope");
        assert_eq!(state.cycle_index, Some(0));
    }

    #[test]
    fn advance_extends_partial_word_to_lcp() {
        // "st" narrows to strategy only among builtins — LCP extends fully.
        let (new_input, state) = advance("st", None, &[], CompletionDirection::Forward).unwrap();
        assert_eq!(new_input, "strategy");
        assert_eq!(state.matches, names(&["strategy"]));
        assert_eq!(state.cycle_index, None);
    }

    #[test]
    fn advance_cycles_forward_through_matches() {
        // "q" matches q, qa (+ aliases quit/qall/quitall/exit) — LCP is "q"
        // itself, so the very first Tab starts cycling.
        let (first, state1) = advance("q", None, &[], CompletionDirection::Forward).unwrap();
        let (second, state2) =
            advance(&first, Some(&state1), &[], CompletionDirection::Forward).unwrap();
        assert_ne!(first, second);
        assert_eq!(state2.matches, state1.matches);
        assert_eq!(state2.cycle_index, Some(1));
    }

    #[test]
    fn advance_cycle_wraps_around() {
        let (_, state1) = advance("q", None, &[], CompletionDirection::Forward).unwrap();
        let total = state1.matches.len();
        let mut state = state1;
        for _ in 0..total - 1 {
            let (_, next) = advance("", Some(&state), &[], CompletionDirection::Forward).unwrap();
            state = next;
        }
        // We've now cycled through every match once; one more Forward wraps to 0.
        let (_, wrapped) = advance("", Some(&state), &[], CompletionDirection::Forward).unwrap();
        assert_eq!(wrapped.cycle_index, Some(0));
    }

    #[test]
    fn advance_backward_cycles_in_reverse() {
        let (_, state1) = advance("q", None, &[], CompletionDirection::Backward).unwrap();
        // Backward on first press with an already-maximal LCP jumps straight
        // to the last match.
        assert_eq!(state1.cycle_index, Some(state1.matches.len() - 1));
    }

    #[test]
    fn advance_no_match_returns_none() {
        assert_eq!(
            advance("zzz", None, &[], CompletionDirection::Forward),
            None
        );
    }

    #[test]
    fn advance_single_match_completes_fully() {
        let (new_input, state) = advance("hel", None, &[], CompletionDirection::Forward).unwrap();
        assert_eq!(new_input, "help");
        assert_eq!(state.matches, names(&["help"]));
    }

    #[test]
    fn advance_preserves_original_input_across_cycles() {
        let (_, state1) = advance("q", None, &[], CompletionDirection::Forward).unwrap();
        assert_eq!(state1.original_input, "q");
        let (_, state2) = advance("qa", Some(&state1), &[], CompletionDirection::Forward).unwrap();
        assert_eq!(state2.original_input, "q");
    }

    // -- advance: argument completion (stretch scope) --

    #[test]
    fn advance_completes_strategy_argument() {
        let (new_input, state) =
            advance("strategy ", None, &[], CompletionDirection::Forward).unwrap();
        // LCP of quorum/debate is empty — falls straight into cycling.
        // Declared order (quorum first) is kept rather than sorted
        // alphabetically, since quorum is the default strategy.
        assert_eq!(state.matches, names(&["quorum", "debate"]));
        assert_eq!(new_input, "strategy quorum");
    }

    #[test]
    fn advance_completes_strategy_argument_with_partial_word() {
        let (new_input, _state) =
            advance("strategy q", None, &[], CompletionDirection::Forward).unwrap();
        assert_eq!(new_input, "strategy quorum");
    }

    #[test]
    fn advance_completes_config_key_argument() {
        let (new_input, _state) =
            advance("config agent.cons", None, &[], CompletionDirection::Forward).unwrap();
        assert_eq!(new_input, "config agent.consensus_level");
    }

    #[test]
    fn advance_unknown_command_argument_has_no_candidates() {
        assert_eq!(
            advance("solo x", None, &[], CompletionDirection::Forward),
            None
        );
    }

    #[test]
    fn advance_beyond_first_argument_has_no_candidates() {
        assert_eq!(
            advance(
                "strategy quorum extra",
                None,
                &[],
                CompletionDirection::Forward
            ),
            None
        );
    }
}
