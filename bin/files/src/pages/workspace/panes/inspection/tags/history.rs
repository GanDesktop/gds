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
    current: Snapshot,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    typing: Option<Instant>,
}

impl History {
    #[cfg(test)]
    pub fn new(initial: Snapshot) -> Self {
        Self {
            current: initial,
            undo: Vec::new(),
            redo: Vec::new(),
            typing: None,
        }
    }

    pub fn with_tags(tags: Vec<String>) -> Self {
        Self {
            current: Snapshot {
                tags,
                draft: String::new(),
                selection: 0..0,
                reversed: false,
            },
            undo: Vec::new(),
            redo: Vec::new(),
            typing: None,
        }
    }

    #[cfg(test)]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

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

    pub fn step(&mut self, redo: bool) -> Option<&Snapshot> {
        self.break_typing();
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let state = from.pop()?;
        to.push(std::mem::replace(&mut self.current, state));
        Some(&self.current)
    }
}

#[cfg(test)]
mod tests;
