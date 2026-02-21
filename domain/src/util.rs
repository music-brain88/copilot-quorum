//! Shared utility functions.

/// Truncate a string to approximately `max_bytes` without splitting a UTF-8
/// character boundary.
///
/// Returns a sub-slice of the original string. If the string is shorter than
/// `max_bytes`, the entire string is returned unchanged.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Find the nearest valid UTF-8 character boundary at or after `pos`.
///
/// If `pos` is already a boundary, returns `pos`.
/// If `pos` is past the end, returns `s.len()`.
fn find_char_boundary_forward(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    let mut boundary = pos;
    while boundary < s.len() && !s.is_char_boundary(boundary) {
        boundary += 1;
    }
    boundary
}

/// Head+Tail truncation strategy.
///
/// Keeps the first 60% and last 40% of the budget, inserting a truncation
/// marker in between. The 60/40 split favors tails because test output
/// typically has pass/fail summaries at the end.
///
/// If `s` fits within `max_bytes`, returns it unchanged.
pub fn truncate_head_tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    const MARKER: &str = "\n\n... [truncated middle] ...\n\n";
    let marker_len = MARKER.len();

    // If budget is too small for even the marker + some content, fall back to simple head truncation
    if max_bytes <= marker_len + 20 {
        return truncate_str(s, max_bytes).to_string();
    }

    let budget = max_bytes - marker_len;
    let head_budget = budget * 60 / 100;
    let tail_budget = budget - head_budget;

    // Find safe UTF-8 boundaries
    let head_end = {
        let mut end = head_budget;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        end
    };

    let tail_start = find_char_boundary_forward(s, s.len().saturating_sub(tail_budget));

    // If head and tail overlap (budget is generous), just return the whole thing
    if head_end >= tail_start {
        return s.to_string();
    }

    format!("{}{}{}", &s[..head_end], MARKER, &s[tail_start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn truncate_no_op_when_short() {
        assert_eq!(truncate_str("hi", 10), "hi");
    }

    #[test]
    fn truncate_multibyte_boundary() {
        // 'の' is 3 bytes (U+306E): bytes 0xe3 0x81 0xae
        let s = "あのね"; // 9 bytes: 3+3+3
        // Cutting at byte 4 would land inside 'の', should back up to 3
        assert_eq!(truncate_str(s, 4), "あ");
        assert_eq!(truncate_str(s, 6), "あの");
    }

    #[test]
    fn truncate_exact_boundary() {
        let s = "あのね";
        assert_eq!(truncate_str(s, 9), "あのね");
        assert_eq!(truncate_str(s, 3), "あ");
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate_str("", 10), "");
    }

    // ==================== truncate_head_tail tests ====================

    #[test]
    fn head_tail_no_op_when_short() {
        let s = "hello world";
        assert_eq!(truncate_head_tail(s, 100), s);
    }

    #[test]
    fn head_tail_basic_ascii() {
        // Create a string that exceeds the budget
        let s = "A".repeat(200);
        let result = truncate_head_tail(&s, 100);
        assert!(result.len() <= 100);
        assert!(result.contains("... [truncated middle] ..."));
        // Head should start with A's
        assert!(result.starts_with('A'));
        // Tail should end with A's
        assert!(result.ends_with('A'));
    }

    #[test]
    fn head_tail_preserves_utf8_boundaries() {
        // 'あ' = 3 bytes each, 100 chars = 300 bytes
        let s = "あ".repeat(100);
        let result = truncate_head_tail(&s, 150);
        assert!(result.len() <= 150);
        // Should be valid UTF-8 (this would panic on bad boundaries)
        assert!(result.contains("あ"));
        assert!(result.contains("... [truncated middle] ..."));
    }

    #[test]
    fn head_tail_small_budget_falls_back() {
        let s = "hello world, this is a test";
        // Budget too small for marker, falls back to head truncation
        let result = truncate_head_tail(&s, 40);
        assert!(result.len() <= 40);
    }

    #[test]
    fn head_tail_exact_fit() {
        let s = "hello";
        assert_eq!(truncate_head_tail(s, 5), "hello");
    }

    #[test]
    fn head_tail_empty() {
        assert_eq!(truncate_head_tail("", 100), "");
    }

    #[test]
    fn head_tail_ratio_check() {
        // Verify that roughly 60% head and 40% tail are preserved
        let head = "H".repeat(100);
        let tail = "T".repeat(100);
        let s = format!("{}{}", head, tail);
        let result = truncate_head_tail(&s, 150);
        // Should have more H's than T's (60/40 split)
        let h_count = result.chars().filter(|&c| c == 'H').count();
        let t_count = result.chars().filter(|&c| c == 'T').count();
        assert!(
            h_count > t_count,
            "Head ({}) should be larger than tail ({})",
            h_count,
            t_count
        );
    }

    // ==================== find_char_boundary_forward tests ====================

    #[test]
    fn boundary_forward_at_boundary() {
        let s = "あのね"; // 9 bytes
        assert_eq!(find_char_boundary_forward(s, 0), 0);
        assert_eq!(find_char_boundary_forward(s, 3), 3);
        assert_eq!(find_char_boundary_forward(s, 6), 6);
    }

    #[test]
    fn boundary_forward_mid_char() {
        let s = "あのね";
        assert_eq!(find_char_boundary_forward(s, 1), 3); // inside 'あ', advance to 'の'
        assert_eq!(find_char_boundary_forward(s, 4), 6); // inside 'の', advance to 'ね'
    }

    #[test]
    fn boundary_forward_past_end() {
        let s = "abc";
        assert_eq!(find_char_boundary_forward(s, 100), 3);
    }
}
