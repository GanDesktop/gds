//! Address editing and the active tab's name filter.

mod search_input;

use super::*;

pub(super) struct State {
    pub path: PathInput,
    pub filter: FilterInput,
}

pub(super) struct PathInput {
    pub input: Entity<InputState>,
    pub editing: bool,
}

pub(super) struct FilterInput {
    pub input: Entity<InputState>,
    pub sync_pending: bool,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            path: PathInput {
                input: cx.new(|cx| {
                    InputState::new(window, cx).placeholder(i18n::text("path-placeholder", cx))
                }),
                editing: false,
            },
            filter: FilterInput {
                input: cx.new(|cx| {
                    InputState::new(window, cx).placeholder(i18n::text("filter-placeholder", cx))
                }),
                sync_pending: false,
            },
        }
    }
}

impl Browser {
    pub(super) fn on_path_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Blur) {
            if self.toolbar.path.editing && !notifications::is_focused(window, cx) {
                self.toolbar.path.editing = false;
                file_clipboard::refresh(cx);
                cx.notify();
            }
            return;
        }
        if !matches!(event, InputEvent::PressEnter { .. }) {
            return;
        }
        let value = input.read(cx).value();
        if crate::entities::scheme::recognizes(&value) {
            match Location::from_uri(&value) {
                Ok(Location::Directory(path)) => {
                    self.navigate(PathBuf::from(path), cx);
                    Self::restore_file_focus(cx.entity().downgrade(), self.tabs.active_id, window);
                }
                Ok(location) => self.show_location(location, window, cx),
                Err(error) => notifications::error(error.to_string(), cx),
            }
            return;
        }
        let current = self
            .current_path()
            .unwrap_or_else(|| self.locations.home.clone());
        let target = if value.as_ref() == current.to_string_lossy() {
            Ok(current.clone())
        } else {
            navigation::resolve_input(value.as_ref(), &current, Some(&self.locations.home))
        };
        match target {
            Ok(path) => {
                self.navigate(path, cx);
                Self::restore_file_focus(cx.entity().downgrade(), self.tabs.active_id, window);
                file_clipboard::refresh(cx);
            }
            Err(error) => {
                notifications::Report::new(
                    notifications::Level::Error,
                    i18n::text("load-failed", cx),
                )
                .detail(error.to_string())
                .replacing::<LoadNotice>()
                .show(cx);
                cx.notify();
            }
        }
    }

    pub(super) fn on_filter_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Blur) {
            file_clipboard::refresh(cx);
            return;
        }
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = input.read(cx).value().to_string();
        if let Some(slot) = self.current_tab_mut() {
            slot.filter = value;
            slot.refresh_visible();
        }
        cx.notify();
    }

    pub(super) fn sync_filter_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        let filter = self
            .current_tab()
            .map(|tab| tab.filter.clone())
            .unwrap_or_default();
        if self.toolbar.filter.input.read(cx).value().as_ref() != filter {
            self.toolbar
                .filter
                .input
                .update(cx, |input, cx| input.set_value(filter, window, cx));
        }
    }
    /// A path or filter edit owns its keys; workspace shortcuts stay out of it.
    pub(super) fn editing_input_focused(&self, window: &Window, cx: &App) -> bool {
        self.editing_path(window, cx)
            || self
                .toolbar
                .filter
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }

    /// Whether the path editor is open or holds focus.
    ///
    /// Its text lives only in the widget and losing focus discards the
    /// uncommitted path, so commands that would move focus stay out of it. The
    /// filter box commits on every change and keeps its word instead.
    pub(super) fn editing_path(&self, window: &Window, cx: &App) -> bool {
        self.toolbar.path.editing
            || self
                .toolbar
                .path
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }
}
