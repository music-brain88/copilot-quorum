//! External $EDITOR delegation
//!
//! Launches the user's preferred editor as a subprocess with a temporary file.
//! The TUI suspends while the editor runs (raw mode disabled, alternate screen left).
//! On save (:wq), the file content is returned; on cancel (:q!), `Cancelled` is returned.

use std::process::Command;

/// RAII guard that removes a temporary file on drop.
///
/// Ensures cleanup even on panic or early return.
struct TempFileGuard(std::path::PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Result of an $EDITOR invocation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorResult {
    /// User saved and content is non-empty (after filtering comments)
    Saved(String),
    /// User cancelled (:q!) or content was empty
    Cancelled,
}

/// Context displayed as a comment header in the temp file
#[derive(Debug, Clone, Default)]
pub struct EditorContext {
    pub consensus_level: String,
    pub phase_scope: String,
    pub strategy: String,
}

/// Launch the user's $EDITOR with optional initial text.
///
/// Creates a temp file with a context header (comment lines starting with `#`),
/// writes `initial_text` below the header, and opens the editor.
/// After the editor exits, reads the file, strips comment lines, and returns the result.
///
/// If `show_header` is false, the context header is omitted (comment filtering still applies).
pub fn launch_editor(initial_text: &str, context: &EditorContext) -> EditorResult {
    launch_editor_with_options(initial_text, context, true)
}

pub fn launch_editor_with_options(
    initial_text: &str,
    context: &EditorContext,
    show_header: bool,
) -> EditorResult {
    // 1. Create temp file
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!(".quorum-prompt-{}.md", std::process::id()));

    // 2. Write context header + initial text
    let content = if show_header {
        let header = format!(
            "# --- Quorum Prompt ---\n\
             # Mode: {} | Scope: {} | Strategy: {}\n\
             # Write your prompt below. Lines starting with # are ignored.\n\
             # Save and quit to send, quit without saving to cancel.\n\
             # ---------------------\n",
            context.consensus_level, context.phase_scope, context.strategy,
        );
        if initial_text.is_empty() {
            format!("{}\n", header)
        } else {
            format!("{}\n{}", header, initial_text)
        }
    } else if initial_text.is_empty() {
        String::new()
    } else {
        initial_text.to_string()
    };

    if let Err(e) = std::fs::write(&temp_path, &content) {
        eprintln!("Failed to create temp file: {}", e);
        return EditorResult::Cancelled;
    }

    // RAII guard ensures cleanup even on panic
    let _guard = TempFileGuard(temp_path.clone());

    // 3. Detect editor: $VISUAL → $EDITOR → vi
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    // 4. Launch editor subprocess
    let status = Command::new(&editor)
        .arg(&temp_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    // 5. Read back file content (guard drops and cleans up on return)
    match status {
        Ok(exit_status) if exit_status.success() => match std::fs::read_to_string(&temp_path) {
            Ok(raw) => {
                let filtered = filter_comments(&raw);
                if filtered.is_empty() {
                    EditorResult::Cancelled
                } else {
                    EditorResult::Saved(filtered)
                }
            }
            Err(_) => EditorResult::Cancelled,
        },
        _ => EditorResult::Cancelled,
    }
}

/// Strip lines starting with `#` (comment lines) and trim the result
pub fn filter_comments(text: &str) -> String {
    let filtered: Vec<&str> = text.lines().filter(|line| !line.starts_with('#')).collect();

    // Join and trim leading/trailing whitespace
    let result = filtered.join("\n");
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_comments_removes_header() {
        let input = "# --- Quorum Prompt ---\n\
                      # Mode: Solo | Scope: Full | Strategy: Quorum\n\
                      # Write your prompt below.\n\
                      # ---------------------\n\
                      \n\
                      Fix the login bug\n\
                      and add tests";
        let result = filter_comments(input);
        assert_eq!(result, "Fix the login bug\nand add tests");
    }

    #[test]
    fn test_filter_comments_empty_content() {
        let input = "# --- Quorum Prompt ---\n\
                      # All comments\n";
        let result = filter_comments(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_filter_comments_preserves_non_comment_hash() {
        let input = "Use Rust's ## operator\n\
                      # This is a comment\n\
                      Implement feature";
        // Lines starting with # are removed, but "Use Rust's ## operator" stays
        // because it doesn't START with #
        let result = filter_comments(input);
        assert_eq!(result, "Use Rust's ## operator\nImplement feature");
    }

    #[test]
    fn test_filter_comments_all_empty_lines() {
        let input = "# comment\n\n\n# another comment\n\n";
        let result = filter_comments(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_filter_comments_multiline_content() {
        let input = "# header\n\
                      Line 1\n\
                      Line 2\n\
                      # mid-comment\n\
                      Line 3";
        let result = filter_comments(input);
        assert_eq!(result, "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_temp_file_guard_removes_on_drop() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(".quorum-test-guard.tmp");
        std::fs::write(&path, "test").unwrap();
        assert!(path.exists());

        {
            let _guard = TempFileGuard(path.clone());
        } // guard drops here

        assert!(!path.exists());
    }

    #[test]
    fn test_temp_file_guard_no_panic_on_missing_file() {
        let path = std::path::PathBuf::from("/tmp/.quorum-nonexistent-guard.tmp");
        let _guard = TempFileGuard(path);
        // Should not panic on drop
    }

    #[test]
    fn test_editor_result_variants() {
        let saved = EditorResult::Saved("hello".into());
        assert_eq!(saved, EditorResult::Saved("hello".into()));

        let cancelled = EditorResult::Cancelled;
        assert_eq!(cancelled, EditorResult::Cancelled);
    }
}
