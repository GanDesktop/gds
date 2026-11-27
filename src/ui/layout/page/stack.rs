//! Pages cover a workspace without destroying it; each entry remembers its return focus.

use zgui::view::NodeRef;

pub struct Entry<T> {
    pub content: T,
    pub return_focus: Option<NodeRef>,
}

impl<T> Entry<T> {
    #[must_use]
    pub fn new(content: T, return_focus: Option<NodeRef>) -> Self {
        Self {
            content,
            return_focus,
        }
    }

    pub fn restore_focus(&self) {
        if let Some(focus) = self.return_focus {
            focus.focus();
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
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn push(&mut self, page: Entry<T>) {
        self.0.push(page);
    }

    pub fn pop(&mut self) -> Option<Entry<T>> {
        self.0.pop()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn current(&self) -> Option<&T> {
        self.last().map(|page| &page.content)
    }
}
