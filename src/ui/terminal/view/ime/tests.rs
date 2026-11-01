use super::{byte_range, utf8_offset};

#[test]
fn utf16_preedit_ranges_never_split_utf8_or_surrogate_pairs() {
    let text = "中😀e\u{301}";
    assert_eq!(&text[byte_range(text, 0..1)], "中");
    assert_eq!(&text[byte_range(text, 1..3)], "😀");
    assert_eq!(utf8_offset(text, 2), "中".len());
    assert_eq!(&text[byte_range(text, 3..99)], "e\u{301}");
    assert_eq!(byte_range(text, 99..1), text.len()..text.len());
}
