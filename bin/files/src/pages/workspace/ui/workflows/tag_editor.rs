//! Single-file associations. Save applies the user's delta to freshly read tags.

mod history;

use super::*;
use gds::ui::button::Button;
use gds::{
    service::file_tags,
    ui::{DialogActions, field::input as input_field},
};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Backspace, Redo, Undo};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, Sizable, button::ButtonVariants, input::InputEvent,
        tag::Tag,
    },
    prelude::*,
};
use history::History;
use std::sync::Once;

static KEY_BINDINGS: Once = Once::new();

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("symbolic links and special files cannot be tagged directly")]
    UnsupportedTarget,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
}

struct Editor {
    path: PathBuf,
    input: Entity<InputState>,
    original: Option<Vec<String>>,
    history: History,
    error: Option<SharedString>,
    saving: bool,
    browser: WeakEntity<Browser>,
    tasks: Scope,
}

impl Editor {
    fn error_message(error: Error, cx: &App) -> SharedString {
        match error {
            Error::UnsupportedTarget => i18n::text("permissions-symlink", cx),
            Error::Io(error) => error.to_string().into(),
            Error::Task(error) => error.to_string().into(),
        }
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let work = self.tasks.spawn_blocking_named("Read file tags", move || {
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.is_file() && !metadata.is_dir() {
                return Err(Error::UnsupportedTarget);
            }
            Ok(file_tags::read(&path)?)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = work
                .await
                .map_err(Error::from)
                .and_then(std::convert::identity);
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(tags) => {
                        this.history.current.tags = tags.clone();
                        this.original = Some(tags);
                        this.input.update(cx, |input, cx| input.focus(window, cx));
                    }
                    Err(error) => this.error = Some(Self::error_message(error, cx)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.saving || self.original.is_none() {
            return false;
        }
        let tags = match file_tags::decode(self.input.read(cx).value().as_bytes()) {
            Ok(tags) => tags,
            Err(error) => {
                self.error = Some(error.to_string().into());
                cx.notify();
                return false;
            }
        };
        self.record_input(window, cx);
        let mut next = self.history.current.clone();
        for tag in tags {
            if !next.tags.contains(&tag) {
                next.tags.push(tag);
            }
        }
        next.draft.clear();
        next.selection = 0..0;
        next.reversed = false;
        self.history.record(next, false);
        self.restore_input(window, cx);
        self.error = None;
        cx.notify();
        true
    }

    fn record_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.original.is_none()
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            return;
        }
        let input = self.input.read(cx);
        let mut next = self.history.current.clone();
        next.draft = input.value().to_string();
        next.selection = input.selected_range();
        next.reversed = input.is_selection_reversed();
        self.history.record(next, true);
    }

    fn restore_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.history.current.clone();
        self.input.update(cx, |input, cx| {
            input.set_value(state.draft, window, cx);
            input.set_selected_range_with_direction(state.selection, state.reversed, cx);
            input.focus(window, cx);
        });
    }

    fn remove(&mut self, tag: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.original.is_none() {
            return;
        }
        self.record_input(window, cx);
        let mut next = self.history.current.clone();
        next.tags.retain(|old| old != tag);
        self.history.record(next, false);
        self.input.update(cx, |input, cx| input.focus(window, cx));
        self.error = None;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).value().is_empty()
            && self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_none()
            })
        {
            if let Some(tag) = self.history.current.tags.last().cloned() {
                self.remove(&tag, window, cx);
            }
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }

    fn history_step(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.saving || self.original.is_none() {
            return;
        }
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            cx.propagate();
            return;
        }
        self.record_input(window, cx);
        if self.history.step(redo) {
            self.restore_input(window, cx);
            self.error = None;
            cx.notify();
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(original) = self.original.clone() else {
            return;
        };
        if !self
            .browser
            .upgrade()
            .is_some_and(|browser| browser.read(cx).settings.preferences.tags.enabled)
        {
            return;
        }
        if !self.add(window, cx) {
            return;
        }
        let edited = self.history.current.tags.clone();
        let path = self.path.clone();
        self.saving = true;
        let work = self
            .tasks
            .spawn_blocking_named("Update file tags", move || {
                file_tags::apply_edit(&path, &original, &edited).map_err(Error::from)
            });
        cx.spawn_in(window, async move |this, cx| {
            let result = work
                .await
                .map_err(Error::from)
                .and_then(std::convert::identity);
            let _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        if let Some(browser) = this.browser.upgrade() {
                            browser.update(cx, |browser, cx| {
                                browser.refresh_after_file_operation(&[this.path.clone()], cx);
                                let ids: Vec<_> = browser
                                    .tabs
                                    .items
                                    .iter()
                                    .filter(|s| matches!(s.location(), Location::Tags(_)))
                                    .map(|s| s.id)
                                    .collect();
                                for id in ids {
                                    browser.refresh_tab(id, cx);
                                }
                            });
                        }
                        window.close_dialog(cx);
                        notifications::success(i18n::text("tags-saved", cx), cx);
                    }
                    Err(error) => this.error = Some(Self::error_message(error, cx)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let chips: Vec<_> = self.history.current.tags.iter().enumerate().map(|(index, tag)| {
            let tag = tag.clone();
            let hint = tag.clone();
            let remove = cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.remove(&tag, window, cx);
            });
            el! { .Tag::secondary { h: 24.0.px; py_0; px_2; gap_1; max_w_full; border_0; rounded: 4.0.px; text_size: 13.0.px; } [
                .div { # $("tag-label", index); min_w_0; truncate;
                    tooltip: $(move |window, cx| gds::ui::tooltip::view(hint.clone(), window, cx));
                } [$(self.history.current.tags[index].clone())],
                ($("remove-tag", index)).Button::new { ghost; compact; w: 18.0.px; h: 18.0.px; p_0; flex_shrink_0;
                    icon: $(Icon::new(IconName::X).size(px(12.)));
                    accessibility_label: $(i18n::text("tags-remove", cx)); disabled: $(self.saving); on_click: remove;
                },
            ] }
        }).collect();
        el! { .div { flex; flex_col; gap_3; key_context: "TagEditor";
            capture_action: $(cx.listener(Self::backspace));
            capture_action: $(cx.listener(|this, _: &Undo, window, cx| this.history_step(false, window, cx)));
            capture_action: $(cx.listener(|this, _: &Redo, window, cx| this.history_step(true, window, cx)));
            capture_key_down: $(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.record_input(window, cx);
                if matches!(event.keystroke.key.as_str(), "left" | "right" | "home" | "end") { this.history.break_typing(); }
            }));
            on_action: $(cx.listener(|this, _: &gpui_kit::base::actions::Confirm, window, cx| {
                this.add(window, cx);
                cx.stop_propagation();
            }));
        } [
            @if (self.original.is_some())
                .div { # "tag-editor-field"; flex; flex_wrap; items_center; gap: 6.0.px; p: 6.0.px; min_h: 38.0.px; max_h: 180.0.px;
                    overflow_y_scroll; border_1; rounded: $(cx.theme().radius);
                    bg: $(cx.gds().appearance().theme().colors().content);
                    border_color: $(if focused { cx.theme().primary } else { cx.theme().input });
                    cursor_text;
                    on_click: $(cx.listener(|this, _, window, cx| {
                        if !this.saving {
                            this.history.break_typing(); this.record_input(window, cx);
                            this.input.update(cx, |input, cx| input.focus(window, cx));
                        }
                    }));
                } [
                    ..chips,
                    .div { flex_1; min_w: 112.0.px; h: 24.0.px; } [
                        ($&self.input, cx).input_field { small; appearance: false; px_0; py_0; text_size: 13.0.px;
                            bg: $(transparent_black()); disabled: $(self.saving); aria_label: $(i18n::text("tags-add", cx));
                        },
                    ],
                ],
            @end,
            @if (self.original.is_none() && self.error.is_none()) .div [$(i18n::text("tags-loading", cx))], @end,
            @if (self.saving) .div [$(i18n::text("tags-saving", cx))], @end,
            @if (let Some(error) = self.error.clone()) .div { text_color: $(cx.theme().danger); } [error], @end,
        ] }
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn edit_tags(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.preferences.tags.enabled
            || self.operations.in_progress
            || window.has_active_dialog(cx)
        {
            return;
        }
        let browser = cx.entity().downgrade();
        let tasks = self.window.tasks.clone();
        let editor = cx.new(|cx| {
            KEY_BINDINGS.call_once(|| {
                cx.bind_keys([
                    KeyBinding::new("ctrl-z", Undo, Some("TagEditor")),
                    KeyBinding::new("ctrl-y", Redo, Some("TagEditor")),
                    KeyBinding::new("ctrl-shift-z", Redo, Some("TagEditor")),
                    #[cfg(target_os = "macos")]
                    KeyBinding::new("cmd-z", Undo, Some("TagEditor")),
                    #[cfg(target_os = "macos")]
                    KeyBinding::new("cmd-shift-z", Redo, Some("TagEditor")),
                    KeyBinding::new("backspace", Backspace, Some("TagEditor")),
                ]);
            });
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder(i18n::text("tags-add-placeholder", cx))
            });
            cx.subscribe_in(&input, window, |this: &mut Editor, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.record_input(window, cx);
                }
                if matches!(event, InputEvent::Blur) {
                    this.history.break_typing();
                }
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add(window, cx);
                }
                if matches!(event, InputEvent::Focus | InputEvent::Blur) {
                    cx.notify();
                }
            })
            .detach();
            Editor {
                path,
                input,
                original: None,
                history: History::default(),
                error: None,
                saving: false,
                browser,
                tasks,
            }
        });
        window.on_next_frame(move |window, cx| {
            let loading = editor.clone();
            window.open_alert_dialog(cx, move |dialog, _, cx| {
                let save = editor.clone(); let cancel = editor.clone();
                let footer = el! { .DialogActions::default {
                    primary_label: $(i18n::text("permissions-save", cx)); cancel_label: $(i18n::text("operation-cancel", cx));
                } };
                let path = editor.read(cx).path.clone();
                let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
                let description = el! { .div { # "tag-editor-file"; min_w_0; truncate;
                    tooltip: $(move |window, cx| gds::ui::tooltip::view(path.display().to_string(), window, cx));
                } [name] };
                el! { dialog { confirm; width: 520.0.px; title: $(i18n::text("tags-editor-title", cx));
                    description: description; footer: footer;
                    on_ok: $(move |_: &ClickEvent, window: &mut Window, cx: &mut App| { save.update(cx, |editor, cx| editor.save(window, cx)); false });
                    on_cancel: $(move |_: &ClickEvent, _: &mut Window, cx: &mut App| !cancel.read(cx).saving);
                } [$(editor.clone())] }
            });
            loading.update(cx, |editor, cx| editor.load(window, cx));
        });
    }
}
