//! Chip creation, removal and draft typing share the original editing history.

mod history;

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gds::i18n;
use gds::service::file_tags;
use gds::ui::button::ButtonSize;
use gds::ui::feedback::tooltip::HintTooltipProps;
use gds::ui::{IconButtonProps, IconName};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal};
use zgui::view::{FrameHandle, current_host, install_stylesheet, request_frame};
use zgui::vocab::{ImeEvent, Key, Modifiers, NamedKey};
use zgui::{css, view};
use zgui_ui::InputProps;
use zgui_ui_primitives::Binding;

use super::super::ui::{Browser, WeakBrowser};
use history::{History, Snapshot};

const TAGS_SHEET: &str = "gds-files-workspace-tags";

const TAGS_CSS: &str = css!(
    r#"
    .gds-files-tag-editor {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .gds-files-tag-editor__field {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: center;
        align-content: flex-start;
        gap: 6px;
        min-height: 176px;
        max-height: 184px;
        align-content: flex-start;
        overflow-y: auto;
        padding: 8px;
        border-radius: 6px;
        border: 1px solid var(--gds-border);
        background: var(--gds-content-bg, var(--gds-surface));
        cursor: text;
        box-sizing: border-box;
    }

    .gds-files-tag-editor__field:focus-within {
        border-color: var(--gds-primary);
    }

    .gds-files-tag-editor__field[data-invalid="true"] {
        border-color: var(--gds-error);
    }

    .gds-files-tag-editor__field[data-disabled="true"] {
        opacity: 0.6;
        cursor: default;
    }

    .gds-files-tag-chip-wrap {
        display: inline-flex;
        max-width: 100%;
    }

    .gds-files-tag-chip-wrap .zui-tooltip__trigger {
        display: inline-flex;
        max-width: 100%;
    }

    .gds-files-tag-chip {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        height: 24px;
        max-width: 240px;
        gap: 4px;
        padding: 0 3px 0 8px;
        border-radius: 4px;
        background: var(--gds-surface);
        border: 1px solid var(--gds-border);
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-foreground);
        box-sizing: border-box;
    }

    .gds-files-tag-chip__label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
    }

    .gds-files-tag-chip__remove.zui-button.gds-icon-button {
        width: 18px;
        height: 18px;
        min-width: 18px;
        min-height: 18px;
        padding: 0;
    }

    .gds-files-tag-editor__inline-input.zui-input,
    .gds-files-tag-editor__inline-input {
        flex: 1 1 112px;
        min-width: 112px;
        height: 24px;
        min-height: 24px;
        padding: 0 4px;
        border: none !important;
        background: transparent !important;
        box-shadow: none !important;
        outline: none !important;
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-files-tag-editor__status {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-tag-editor__error {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-error);
    }
    "#
);

fn install_tags_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(TAGS_SHEET, TAGS_CSS);
    }
}

pub(super) fn parse_draft_tags(raw: &str) -> Result<Vec<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let parts = file_tags::decode(trimmed.as_bytes()).map_err(|err| err.to_string())?;
    let mut parsed = Vec::with_capacity(parts.len());
    for part in parts {
        let candidate = part.trim();
        if candidate.is_empty() {
            continue;
        }
        file_tags::validate(candidate).map_err(|err| err.to_string())?;
        if !parsed.iter().any(|existing| existing == candidate) {
            parsed.push(candidate.to_owned());
        }
    }
    Ok(parsed)
}

struct TagEditorInner {
    browser: WeakBrowser,
    path: PathBuf,
    input_ref: NodeRef,
    draft: RwSignal<String, LocalStorage>,
    tags: RwSignal<Vec<String>, LocalStorage>,
    original: RefCell<Vec<String>>,
    history: RefCell<History>,
    loading: RwSignal<bool, LocalStorage>,
    saving: RwSignal<bool, LocalStorage>,
    load_failed: RwSignal<bool, LocalStorage>,
    composing: RwSignal<bool, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
    pending_focus: RefCell<Option<FrameHandle>>,
}

#[derive(Clone)]
pub(in crate::pages::workspace) struct TagEditor(Rc<TagEditorInner>);

impl TagEditor {
    pub(super) fn new(browser: &Browser, path: PathBuf, auto_focus: bool) -> Self {
        let inner = browser.0.owner.with(|| {
            Rc::new(TagEditorInner {
                browser: browser.downgrade(),
                path: path.clone(),
                input_ref: NodeRef::new(),
                draft: RwSignal::new_local(String::new()),
                tags: RwSignal::new_local(Vec::new()),
                original: RefCell::new(Vec::new()),
                history: RefCell::new(History::default()),
                loading: RwSignal::new_local(true),
                saving: RwSignal::new_local(false),
                load_failed: RwSignal::new_local(false),
                composing: RwSignal::new_local(false),
                error: RwSignal::new_local(None),
                pending_focus: RefCell::new(None),
            })
        });
        let editor = Self(inner);
        let weak = Rc::downgrade(&editor.0);
        let read_path = path;
        let symlink_warning = i18n::text_untracked("permissions-symlink");
        browser.blocking(
            "Read file tags",
            move || {
                let metadata =
                    std::fs::symlink_metadata(&read_path).map_err(|err| err.to_string())?;
                if !metadata.is_file() && !metadata.is_dir() {
                    return Err(symlink_warning);
                }
                file_tags::read(&read_path).map_err(|err| err.to_string())
            },
            move |_, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let target = Self(inner);
                match result {
                    Ok(loaded) => {
                        *target.0.original.borrow_mut() = loaded.clone();
                        let mut fresh = History::default();
                        fresh.current = Snapshot {
                            tags: loaded.clone(),
                            draft: String::new(),
                            selection: 0..0,
                            reversed: false,
                        };
                        *target.0.history.borrow_mut() = fresh;
                        target.0.tags.set(loaded);
                        target.0.loading.set(false);
                        target.0.load_failed.set(false);
                        target.0.error.set(None);
                        if auto_focus {
                            target.focus();
                        }
                    }
                    Err(message) => {
                        target.0.loading.set(false);
                        target.0.load_failed.set(true);
                        target.0.error.set(Some(message));
                    }
                }
            },
        );
        editor
    }

    fn editable(&self) -> bool {
        !self.0.loading.get_untracked()
            && !self.0.load_failed.get_untracked()
            && !self.0.saving.get_untracked()
    }

    pub(super) fn is_loading(&self) -> bool {
        self.0.loading.get()
    }

    pub(in crate::pages::workspace) fn is_saving(&self) -> bool {
        self.0.saving.get()
    }

    pub(in crate::pages::workspace) fn is_saving_untracked(&self) -> bool {
        self.0.saving.get_untracked()
    }

    pub(super) fn available(&self) -> bool {
        !self.0.loading.get() && !self.0.load_failed.get()
    }

    pub(super) fn available_untracked(&self) -> bool {
        !self.0.loading.get_untracked() && !self.0.load_failed.get_untracked()
    }

    pub(super) fn has_changes_untracked(&self) -> bool {
        self.available_untracked()
            && (self.0.tags.get_untracked() != *self.0.original.borrow()
                || !self.0.draft.get_untracked().trim().is_empty())
    }

    pub(super) fn validate_and_commit_draft(&self) -> Result<(), ()> {
        if !self.editable() {
            return Ok(());
        }
        self.add().map(|_| ()).map_err(|_| ())
    }

    pub(super) fn focus(&self) {
        if self.0.input_ref.is_bound() {
            self.0.input_ref.focus();
            return;
        }
        let weak = Rc::downgrade(&self.0);
        if let Some(browser) = self.0.browser.upgrade() {
            let handle = browser.0.owner.with(|| {
                request_frame(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        inner.input_ref.focus();
                    }
                })
            });
            *self.0.pending_focus.borrow_mut() = Some(handle);
        }
    }

    fn record_input(&self, value: String) {
        self.0.draft.set(value.clone());
        if !self.editable() || self.0.composing.get_untracked() {
            return;
        }
        let selection = self
            .0
            .input_ref
            .selection()
            .unwrap_or(value.len()..value.len());
        let tags = self.0.tags.get_untracked();
        {
            let mut history = self.0.history.borrow_mut();
            history.record(
                Snapshot {
                    tags,
                    draft: value,
                    selection,
                    reversed: false,
                },
                true,
            );
        }
        self.0.error.set(None);
    }

    fn restore_snapshot(&self, snapshot: Snapshot) {
        self.0.tags.set(snapshot.tags);
        self.0.draft.set(snapshot.draft.clone());
        self.0.input_ref.set_value(&snapshot.draft);
        self.0.input_ref.set_selection(snapshot.selection);
        self.0.error.set(None);
    }

    fn add(&self) -> Result<bool, String> {
        if !self.editable() {
            return Ok(false);
        }
        let raw = self.0.draft.get_untracked();
        let parsed = match parse_draft_tags(&raw) {
            Ok(parsed) => parsed,
            Err(msg) => {
                self.0.error.set(Some(msg.clone()));
                return Err(msg);
            }
        };
        if parsed.is_empty() {
            self.0.draft.set(String::new());
            self.0.input_ref.set_value("");
            self.0.input_ref.set_selection(0..0);
            self.0.history.borrow_mut().break_typing();
            return Ok(false);
        }
        let mut tags = self.0.tags.get_untracked();
        for candidate in parsed {
            if !tags.iter().any(|existing| existing == &candidate) {
                tags.push(candidate);
            }
        }
        {
            let mut history = self.0.history.borrow_mut();
            history.record(
                Snapshot {
                    tags: tags.clone(),
                    draft: String::new(),
                    selection: 0..0,
                    reversed: false,
                },
                false,
            );
        }
        self.0.tags.set(tags);
        self.0.draft.set(String::new());
        self.0.input_ref.set_value("");
        self.0.input_ref.set_selection(0..0);
        self.0.error.set(None);
        Ok(true)
    }

    fn remove(&self, tag: &str) {
        if !self.editable() {
            return;
        }
        let mut tags = self.0.tags.get_untracked();
        let before = tags.len();
        tags.retain(|item| item != tag);
        if tags.len() == before {
            return;
        }
        let draft = self.0.draft.get_untracked();
        let selection = self
            .0
            .input_ref
            .selection()
            .unwrap_or(draft.len()..draft.len());
        {
            let mut history = self.0.history.borrow_mut();
            history.record(
                Snapshot {
                    tags: tags.clone(),
                    draft,
                    selection,
                    reversed: false,
                },
                false,
            );
        }
        self.0.tags.set(tags);
        self.0.error.set(None);
    }

    fn handle_key(&self, event: &mut zgui::view::EventCx<'_, zgui::view::events::KeyDown>) {
        if self.0.composing.get_untracked() || !self.editable() {
            return;
        }
        let modifiers = event.modifiers;
        let has_chord = modifiers.contains(Modifiers::CONTROL)
            && !modifiers.contains(Modifiers::ALT)
            && !modifiers.contains(Modifiers::META);

        if has_chord && let Key::Character(ch) = &event.key {
            let redo = if ch.eq_ignore_ascii_case("z") {
                modifiers.contains(Modifiers::SHIFT)
            } else {
                ch.eq_ignore_ascii_case("y") && !modifiers.contains(Modifiers::SHIFT)
            };
            if ch.eq_ignore_ascii_case("z") || (redo && ch.eq_ignore_ascii_case("y")) {
                let next = {
                    let mut history = self.0.history.borrow_mut();
                    history.step(redo).then(|| history.current.clone())
                };
                if let Some(snapshot) = next {
                    self.restore_snapshot(snapshot);
                }
                event.prevent_default();
                event.stop_propagation();
                return;
            }
        }

        if !modifiers.is_empty() {
            return;
        }

        match &event.key {
            Key::Named(NamedKey::Enter) => {
                let _ = self.add();
                event.prevent_default();
                event.stop_propagation();
            }
            Key::Named(NamedKey::Backspace) => {
                if self.0.draft.get_untracked().is_empty()
                    && let Some(last) = self.0.tags.get_untracked().last().cloned()
                {
                    self.remove(&last);
                    event.prevent_default();
                    event.stop_propagation();
                }
            }
            Key::Named(
                NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::Home | NamedKey::End,
            ) => {
                self.0.history.borrow_mut().break_typing();
            }
            _ => {}
        }
    }

    pub(super) fn save_result(&self, on_done: impl FnOnce(Result<(), ()>) + 'static) {
        if !self.editable() {
            on_done(Ok(()));
            return;
        }
        if self.add().is_err() {
            on_done(Err(()));
            return;
        }
        let Some(browser) = self.0.browser.upgrade() else {
            on_done(Err(()));
            return;
        };
        let original = self.0.original.borrow().clone();
        let edited = self.0.tags.get_untracked();
        if edited == original {
            on_done(Ok(()));
            return;
        }
        let save_path = self.0.path.clone();
        let weak = Rc::downgrade(&self.0);

        self.0.saving.set(true);
        self.0.error.set(None);

        let done = RefCell::new(Some(Box::new(on_done) as Box<dyn FnOnce(Result<(), ()>)>));
        browser.blocking(
            "Save file tags",
            move || {
                file_tags::apply_edit(&save_path, &original, &edited)
                    .map(|()| edited)
                    .map_err(|err| err.to_string())
            },
            move |_browser, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let target = Self(inner);
                target.0.saving.set(false);
                match result {
                    Ok(saved) => {
                        *target.0.original.borrow_mut() = saved;
                        if let Some(callback) = done.borrow_mut().take() {
                            callback(Ok(()));
                        }
                    }
                    Err(message) => {
                        target.0.error.set(Some(message));
                        if let Some(callback) = done.borrow_mut().take() {
                            callback(Err(()));
                        }
                    }
                }
            },
        );
    }

    fn chip(&self, tag: String) -> AnyView {
        let tooltip_tag = tag.clone();
        let display = tag.clone();
        let remove_tag = tag;
        let editor = self.clone();
        let disabled = Signal::derive_local(move || editor.is_saving());
        let remove_editor = self.clone();

        AnyView::new(view! {
            box(class = "gds-files-tag-chip-wrap") {
                HintTooltip(text = tooltip_tag) {
                    row(
                        class = "gds-files-tag-chip",
                        on:click = |ev: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                            ev.stop_propagation();
                        },
                    ) {
                        text(class = "gds-files-tag-chip__label") {{display}}
                        IconButton(
                            icon = IconName::X,
                            label = i18n::text("tags-remove"),
                            size = ButtonSize::IconXs,
                            disabled = disabled,
                            class = "gds-files-tag-chip__remove",
                            on:click = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                                ev.stop_propagation();
                                remove_editor.remove(&remove_tag);
                                remove_editor.focus();
                            },
                        )
                    }
                }
            }
        })
    }

    pub(super) fn fields(&self) -> AnyView {
        install_tags_stylesheet();
        let tags_sig = self.0.tags;
        let draft_sig = self.0.draft;
        let loading_sig = self.0.loading;
        let saving_sig = self.0.saving;
        let load_failed_sig = self.0.load_failed;
        let composing_sig = self.0.composing;
        let error_sig = self.0.error;
        let input_ref = self.0.input_ref;

        let disabled = Signal::derive_local(move || {
            loading_sig.get() || saving_sig.get() || load_failed_sig.get()
        });
        let invalid = Signal::derive_local(move || error_sig.get().is_some());

        let chips_editor = self.clone();
        let input_editor = self.clone();
        let key_editor = self.clone();
        let field_focus_editor = self.clone();

        AnyView::new(view! {
            column(class = "gds-files-tag-editor") {
                box(
                    class = "gds-files-tag-editor__field",
                    attr:data-invalid = move || invalid.get().then(|| "true".to_owned()),
                    attr:data-disabled = move || disabled.get().then(|| "true".to_owned()),
                    on:click = move |_| {
                        if !disabled.get_untracked() {
                            field_focus_editor.focus();
                        }
                    },
                ) {
                    {move || {
                        tags_sig
                            .get()
                            .into_iter()
                            .map(|tag| chips_editor.chip(tag))
                            .collect::<Vec<_>>()
                    }}
                    Input(
                        class = "gds-files-tag-editor__inline-input",
                        node_ref = input_ref,
                        placeholder = i18n::text("tags-add-placeholder"),
                        disabled = disabled,
                        invalid = invalid,
                        value = Binding::controlled(draft_sig, move |next| {
                            input_editor.record_input(next);
                        }),
                        on:ime_start = move |_| composing_sig.set(true),
                        on:ime_preedit = move |ev| {
                            let active = match &**ev {
                                ImeEvent::Preedit { text, .. } => !text.is_empty(),
                                _ => ev.is_composing(),
                            };
                            composing_sig.set(active);
                        },
                        on:ime_commit = move |_| composing_sig.set(false),
                        on:ime_end = move |_| composing_sig.set(false),
                        on:key_down = move |ev| key_editor.handle_key(ev),
                    )
                }
                {move || {
                    if loading_sig.get() {
                        Some(AnyView::new(view! {
                            text(class = "gds-files-tag-editor__status") {{i18n::text("tags-loading")}}
                        }))
                    } else if saving_sig.get() {
                        Some(AnyView::new(view! {
                            text(class = "gds-files-tag-editor__status") {{i18n::text("tags-saving")}}
                        }))
                    } else {
                        None
                    }
                }}
                {move || {
                    error_sig.get().map(|err| {
                        view! {
                            text(class = "gds-files-tag-editor__error") {{err}}
                        }
                        .into_view()
                    })
                }}
            }
        })
    }
}
