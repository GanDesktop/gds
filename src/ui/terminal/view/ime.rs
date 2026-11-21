//! IME preedit state and UTF-8 / UTF-16 offset helpers.

use std::ops::Range;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PreeditState {
    pub text: String,
    pub cursor: Range<usize>,
}

impl PreeditState {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn set(&mut self, text: &str, cursor: Option<Range<usize>>) {
        self.text.clear();
        self.text.push_str(text);
        self.cursor = match cursor {
            Some(range) => clamp_utf8_range(text, range),
            None => text.len()..text.len(),
        };
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0..0;
    }
}

fn clamp_utf8_offset(text: &str, offset: usize) -> usize {
    let mut clamped = offset.min(text.len());
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

fn clamp_utf8_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = clamp_utf8_offset(text, range.start);
    let end = clamp_utf8_offset(text, range.end);
    if start <= end { start..end } else { end..end }
}

#[cfg(test)]
pub(super) fn utf8_offset(text: &str, utf16: usize) -> usize {
    let mut seen = 0usize;
    for (byte, ch) in text.char_indices() {
        let len = ch.len_utf16();
        if seen + len > utf16 {
            return byte;
        }
        seen += len;
    }
    text.len()
}

#[cfg(test)]
pub(super) fn byte_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf8_offset(text, range.start);
    let end = utf8_offset(text, range.end);
    start..end.max(start)
}
