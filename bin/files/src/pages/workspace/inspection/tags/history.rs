//! One editing history for the chips and the text that has not become a chip yet.
use std::{
    ops::Range,
    time::{Duration, Instant},
};

const TYPING_COALESCE_INTERVAL: Duration = Duration::from_millis(750);

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) struct Snapshot {
    pub tags: Vec<String>,
    pub draft: String,
    pub selection: Range<usize>,
    pub reversed: bool,
}

#[derive(Default)]
pub(in crate::pages::workspace) struct History {
    pub current: Snapshot,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    typing: Option<Instant>,
}

impl History {
    pub fn record(&mut self, next: Snapshot, typing: bool) {
        if self.current.tags == next.tags && self.current.draft == next.draft {
            self.current = next;
            return;
        }
        let now = Instant::now();
        let coalesce = typing
            && self.redo.is_empty()
            && self
                .typing
                .is_some_and(|last| now.duration_since(last) < TYPING_COALESCE_INTERVAL);
        if !coalesce {
            self.undo.push(self.current.clone());
        }
        self.redo.clear();
        self.current = next;
        self.typing = typing.then_some(now);
    }

    pub fn break_typing(&mut self) {
        self.typing = None;
    }

    pub fn step(&mut self, redo: bool) -> bool {
        self.break_typing();
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let Some(state) = from.pop() else {
            return false;
        };
        to.push(std::mem::replace(&mut self.current, state));
        true
    }
}

#[cfg(test)]
mod tests;
