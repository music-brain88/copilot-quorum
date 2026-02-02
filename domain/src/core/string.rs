//! String utilities for the domain layer.

/// Truncate a string to a maximum length with ellipsis (UTF-8 safe)
///
/// Uses byte length for max_len but ensures truncation occurs at valid
/// UTF-8 character boundaries.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let target = max_len.saturating_sub(3);
        let mut end = target.min(s.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_ascii() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_multibyte() {
        assert_eq!(truncate("日本語テスト", 30), "日本語テスト");
        assert_eq!(truncate("日本語テスト文字列", 15), "日本語テ...");
        // Mixed ASCII and Japanese
        assert_eq!(truncate("Hello、世界！", 20), "Hello、世界！");
    }

    #[test]
    fn test_truncate_emoji() {
        assert_eq!(truncate("Hello 👋 World", 20), "Hello 👋 World");
        // Emojis are 4 bytes each: 👋(4) + 🌍(4) + 🎉(4) = 12 bytes
        // max_len=10 -> target=7 -> back to char boundary at 4 -> "👋..."
        assert_eq!(truncate("👋🌍🎉", 10), "👋...");
        // max_len=11 -> target=8 -> char boundary at 8 -> "👋🌍..."
        assert_eq!(truncate("👋🌍🎉", 11), "👋🌍...");
    }
}
