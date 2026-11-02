//! Pages cover a workspace without destroying it; each entry remembers its focus.
use gpui_kit::{App, FocusHandle, Subscription, Window};

pub struct Entry<T> {
    pub content: T,
    pub return_focus: Option<FocusHandle>,
    pub subscription: Option<Subscription>,
}

impl<T> Entry<T> {
    pub fn restore_focus(&self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = &self.return_focus {
            focus.focus(window, cx);
        }
    }
}

pub struct Stack<T>(Vec<Entry<T>>);

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> std::ops::Deref for Stack<T> {
    type Target = [Entry<T>];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Stack<T> {
    pub fn push(&mut self, page: Entry<T>) {
        self.0.push(page);
    }
    pub fn pop(&mut self) -> Option<Entry<T>> {
        self.0.pop()
    }
    /// Leaving the entire page stack is followed by focus in the workspace.
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn current(&self) -> Option<&T> {
        self.last().map(|page| &page.content)
    }
}
