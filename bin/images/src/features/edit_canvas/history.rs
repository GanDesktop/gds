//! Bounded undo/redo command stacks with explicit coalescing session boundaries.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::entities::document::DocumentState;

use super::command::EditCommand;

pub const MAX_UNDO_DEPTH: usize = 100;
pub const COALESCE_WINDOW: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleField {
    Color,
    StrokeWidth,
    MarkerDiameter,
    FontSize,
    FontFamily,
    MosaicBlockSize,
    BlurRadius,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoalesceKey {
    KeyboardNudge {
        selection_hash: u64,
    },
    StyleChange {
        selection_hash: u64,
        field: StyleField,
    },
}

#[derive(Clone, Copy, Debug)]
struct CoalesceSession {
    key: CoalesceKey,
    last_at: Instant,
}

#[derive(Default)]
pub struct EditHistory {
    undo_stack: VecDeque<EditCommand>,
    redo_stack: Vec<EditCommand>,
    active_coalesce: Option<CoalesceSession>,
}

impl EditHistory {
    pub fn break_coalesce(&mut self) {
        self.active_coalesce = None;
    }

    pub fn apply(&mut self, command: EditCommand, doc: &mut DocumentState) {
        self.break_coalesce();
        command.apply(doc);
        self.push_undo(command);
        self.redo_stack.clear();
    }

    pub fn apply_coalesced(
        &mut self,
        key: CoalesceKey,
        command: EditCommand,
        doc: &mut DocumentState,
    ) {
        self.apply_coalesced_at(key, command, doc, Instant::now());
    }

    pub fn apply_coalesced_at(
        &mut self,
        key: CoalesceKey,
        command: EditCommand,
        doc: &mut DocumentState,
        now: Instant,
    ) {
        let within_window = self.active_coalesce.is_some_and(|session| {
            session.key == key && now.saturating_duration_since(session.last_at) <= COALESCE_WINDOW
        });
        if self.redo_stack.is_empty()
            && within_window
            && let EditCommand::UpdateAnnotations {
                after: new_after, ..
            } = &command
            && let Some(EditCommand::UpdateAnnotations {
                after: prev_after, ..
            }) = self.undo_stack.back_mut()
        {
            command.apply(doc);
            *prev_after = std::sync::Arc::clone(new_after);
            self.active_coalesce = Some(CoalesceSession { key, last_at: now });
            return;
        }

        command.apply(doc);
        self.push_undo(command);
        self.redo_stack.clear();
        self.active_coalesce = Some(CoalesceSession { key, last_at: now });
    }

    pub fn undo(&mut self, doc: &mut DocumentState) -> bool {
        self.break_coalesce();
        let Some(command) = self.undo_stack.pop_back() else {
            return false;
        };
        command.undo(doc);
        self.redo_stack.push(command);
        true
    }

    pub fn redo(&mut self, doc: &mut DocumentState) -> bool {
        self.break_coalesce();
        let Some(command) = self.redo_stack.pop() else {
            return false;
        };
        command.apply(doc);
        self.push_undo(command);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.break_coalesce();
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    fn push_undo(&mut self, command: EditCommand) {
        if self.undo_stack.len() >= MAX_UNDO_DEPTH {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(command);
    }
}
