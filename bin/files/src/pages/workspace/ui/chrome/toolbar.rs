//! Address editing and the active tab's name filter.

mod search_input;

use super::*;

pub(in crate::pages::workspace) struct State {
    pub path: PathInput,
    pub filter: FilterInput,
}

pub(in crate::pages::workspace) struct PathInput {
    pub input: Entity<InputState>,
    pub editing: bool,
}

pub(in crate::pages::workspace) struct FilterInput {
    pub input: Entity<InputState>,
}

impl State {
    pub(in crate::pages::workspace) fn new(window: &mut Window, cx: &mut App) -> Self {
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
            },
        }
    }

    pub(in crate::pages::workspace) fn finish_path_edit(&mut self) {
        self.path.editing = false;
    }

    pub(in crate::pages::workspace) fn start_path_edit(
        &mut self,
        value: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.path.editing = true;
        self.path.input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn on_path_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Blur) {
            if self.toolbar.path.editing && !notifications::is_focused(window, cx) {
                self.toolbar.finish_path_edit();
                cx.gds().files().clipboard().refresh();
                cx.notify();
            }
            return;
        }
        if !matches!(event, InputEvent::PressEnter { .. }) {
            return;
        }
        let value = input.read(cx).value();
        if crate::entities::location::scheme::recognizes(&value) {
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
            resolve_input(value.as_ref(), &current, Some(&self.locations.home))
        };
        match target {
            Ok(path) => {
                self.navigate(path, cx);
                Self::restore_file_focus(cx.entity().downgrade(), self.tabs.active_id, window);
                cx.gds().files().clipboard().refresh();
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

    pub(in crate::pages::workspace) fn on_filter_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Blur) {
            cx.gds().files().clipboard().refresh();
            return;
        }
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = input.read(cx).value().to_string();
        if let Some(slot) = self.current_tab_mut() {
            slot.set_filter(value);
            slot.refresh_visible();
        }
        cx.notify();
    }

    pub(in crate::pages::workspace) fn sync_filter_input(&self, window: &mut Window, cx: &mut App) {
        let filter = self
            .current_tab()
            .map(|tab| tab.filter().to_owned())
            .unwrap_or_default();
        if self.toolbar.filter.input.read(cx).value().as_ref() != filter {
            self.toolbar
                .filter
                .input
                .update(cx, |input, cx| input.set_value(filter, window, cx));
        }
    }

    pub(in crate::pages::workspace) fn request_filter_sync(&mut self, cx: &mut Context<Self>) {
        let handle = self.window.handle;
        if handle
            .update(cx, |_, window, cx| {
                self.sync_filter_input(window, cx);
            })
            .is_err()
        {
            cx.spawn(async move |this, cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |this, cx| this.sync_filter_input(window, cx));
                });
            })
            .detach();
        }
    }
    /// A path or filter edit owns its keys; workspace shortcuts stay out of it.
    pub(in crate::pages::workspace) fn editing_input_focused(
        &self,
        window: &Window,
        cx: &App,
    ) -> bool {
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
    pub(in crate::pages::workspace) fn editing_path(&self, window: &Window, cx: &App) -> bool {
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
