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
}
