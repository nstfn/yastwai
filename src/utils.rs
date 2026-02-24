/// Truncate a string to at most `max_bytes` bytes without splitting a UTF-8 character.
///
/// Returns the longest prefix of `text` whose byte length is ≤ `max_bytes`
/// and that ends on a character boundary.
pub fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_within_limit() {
        assert_eq!(truncate_utf8("hello", 10), "hello");
    }

    #[test]
    fn ascii_at_limit() {
        assert_eq!(truncate_utf8("hello", 5), "hello");
    }

    #[test]
    fn ascii_over_limit() {
        assert_eq!(truncate_utf8("hello world", 5), "hello");
    }

    #[test]
    fn multibyte_char_not_split() {
        // 'é' is 2 bytes in UTF-8
        let text = "café";
        // "caf" = 3 bytes, "café" = 5 bytes
        assert_eq!(truncate_utf8(text, 4), "caf");
        assert_eq!(truncate_utf8(text, 5), "café");
    }

    #[test]
    fn cjk_char_not_split() {
        // '日' is 3 bytes
        let text = "日本語";
        assert_eq!(truncate_utf8(text, 3), "日");
        assert_eq!(truncate_utf8(text, 5), "日");
        assert_eq!(truncate_utf8(text, 6), "日本");
    }

    #[test]
    fn empty_string() {
        assert_eq!(truncate_utf8("", 5), "");
    }

    #[test]
    fn zero_max() {
        assert_eq!(truncate_utf8("hello", 0), "");
    }
}
