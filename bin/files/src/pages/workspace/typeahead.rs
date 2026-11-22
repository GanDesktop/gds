//! Incremental keyboard type-ahead matching for file views when auto-focus search is disabled.
//!
//! Follows the dual Win32 `SysListView32` / Windows File Explorer interaction model
//! (<https://devblogs.microsoft.com/oldnewthing/20240408-00/>):
//! - Consecutive distinct characters within [`TIMEOUT`] narrow a case-insensitive prefix match,
//!   keeping the current item when it already matches the refined prefix.
//! - Pressing the same single character repeatedly cycles forward through items starting with
//!   that character while retaining the full buffer, so typing a different character next
//!   (e.g. `l`, `l`, `a`) immediately matches the multi-character prefix (`"lla"`).
//! - Pausing longer than [`TIMEOUT`] starts a fresh query.

use std::time::Duration;

use zgui::vocab::Timestamp;

/// Idle interval after which the next keystroke begins a fresh type-ahead query.
pub(super) const TIMEOUT: Duration = Duration::from_millis(1000);

/// Maximum query length in bytes (above Linux `NAME_MAX = 255`) so sustained key-repeat stays bounded.
const MAX_QUERY_BYTES: usize = 256;

#[derive(Default)]
pub(super) struct TypeAhead {
    query: String,
    last_input: Option<Timestamp>,
    overflowed: bool,
}

impl TypeAhead {
    pub(super) fn is_active(&self, now: Timestamp) -> bool {
        (!self.query.is_empty() || self.overflowed)
            && self
                .last_input
                .is_some_and(|last| now.saturating_since(last) <= TIMEOUT)
    }

    pub(super) fn reset(&mut self) {
        self.query.clear();
        self.last_input = None;
        self.overflowed = false;
    }

    /// Append typed or committed text and return the matched row index, if any.
    ///
    /// Returns `None` without modifying state when `count == 0`, `inserted` contains only
    /// control characters, or a fresh query would start with whitespace-only input.
    pub(super) fn push_and_match<F>(
        &mut self,
        inserted: &str,
        now: Timestamp,
        count: usize,
        current: Option<usize>,
        matches: F,
    ) -> Option<usize>
    where
        F: Fn(usize, &str) -> bool,
    {
        if count == 0 || inserted.is_empty() || inserted.chars().all(char::is_control) {
            return None;
        }

        let was_active = self.is_active(now);
        if !was_active {
            self.reset();
        }

        if self.query.is_empty() && !self.overflowed && inserted.chars().all(char::is_whitespace) {
            return None;
        }

        self.last_input = Some(now);
        if self.overflowed {
            return None;
        }

        let lowered = inserted.to_lowercase();
        let inserted_single = single_char(&lowered);
        let fresh_single_char = !was_active && inserted_single.is_some();

        let repeating_same_char =
            was_active && inserted_single.is_some_and(|ch| all_chars_equal(&self.query, ch));

        if self.query.len().saturating_add(lowered.len()) <= MAX_QUERY_BYTES {
            self.query.push_str(&lowered);
        } else if !repeating_same_char {
            self.overflowed = true;
            return None;
        }

        self.find_match(
            count,
            current,
            fresh_single_char,
            repeating_same_char,
            matches,
        )
    }

    /// Trim the last character from an active type-ahead query and re-match without advancing
    /// past an already-matching cursor item.
    ///
    /// Returns `None` when no type-ahead session is active (so `Backspace` is not consumed),
    /// `Some(None)` when the session stays consumed without moving selection, or
    /// `Some(Some(index))` with the matched row index.
    pub(super) fn pop_and_match<F>(
        &mut self,
        now: Timestamp,
        count: usize,
        current: Option<usize>,
        matches: F,
    ) -> Option<Option<usize>>
    where
        F: Fn(usize, &str) -> bool,
    {
        if !self.is_active(now) {
            self.reset();
            return None;
        }
        if self.overflowed {
            self.reset();
            return Some(None);
        }

        self.query.pop();
        if self.query.is_empty() {
            self.last_input = None;
            return Some(None);
        }

        self.last_input = Some(now);
        let repeated = repeated_single_char(&self.query);
        let found = if let Some(ch) = repeated {
            let mut buf = [0u8; 4];
            let single = ch.encode_utf8(&mut buf);
            let start = current.filter(|&idx| idx < count).unwrap_or(0);
            scan_wrapped(count, start, |idx| matches(idx, single))
        } else {
            self.find_match(count, current, false, false, matches)
        };
        Some(found)
    }

    fn find_match<F>(
        &self,
        count: usize,
        current: Option<usize>,
        fresh_single_char: bool,
        repeating_same_char: bool,
        matches: F,
    ) -> Option<usize>
    where
        F: Fn(usize, &str) -> bool,
    {
        if count == 0 || self.query.is_empty() {
            return None;
        }
        let current = current.filter(|&idx| idx < count);

        if repeating_same_char && let Some(ch) = repeated_single_char(&self.query) {
            let mut buf = [0u8; 4];
            let single = ch.encode_utf8(&mut buf);
            let start = current.map_or(0, |idx| (idx + 1) % count);
            return scan_wrapped(count, start, |idx| matches(idx, single));
        }

        let start = if fresh_single_char {
            current.map_or(0, |idx| (idx + 1) % count)
        } else {
            current.unwrap_or(0)
        };
        scan_wrapped(count, start, |idx| matches(idx, &self.query))
    }
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

fn all_chars_equal(text: &str, expected: char) -> bool {
    !text.is_empty() && text.chars().all(|ch| ch == expected)
}

fn repeated_single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    let second = chars.next()?;
    (second == first && chars.all(|ch| ch == first)).then_some(first)
}

fn scan_wrapped(
    count: usize,
    start: usize,
    mut predicate: impl FnMut(usize) -> bool,
) -> Option<usize> {
    (0..count)
        .map(|offset| (start + offset) % count)
        .find(|&idx| predicate(idx))
}

#[cfg(test)]
mod tests;
