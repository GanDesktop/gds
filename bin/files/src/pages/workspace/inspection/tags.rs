//! Chip creation, removal and draft typing share the original editing history.
mod history;
use crate::pages::workspace::ui::{Browser, WeakBrowser};
use anti_webui::prelude::*;
use gds::{
    i18n,
    service::file_tags,
    ui::{Icon, IconButton},
};
use gds_uim::el;
use history::History;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone)]
enum State {
    Loading,
    Editing {
        original: Vec<String>,
        error: Option<String>,
    },
    Saving,
    Failed(String),
}

#[derive(Clone)]
pub(in crate::pages::workspace) struct TagEditor {
    browser: WeakBrowser,
    path: PathBuf,
    state: Signal<State>,
    history: Rc<RefCell<History>>,
    revision: Signal<u64>,
    input: EditorRef,
    restoring: Rc<Cell<bool>>,
}

impl TagEditor {
    pub(super) fn new(browser: &Browser, path: PathBuf) -> Self {
        let editor = Self {
            browser: browser.downgrade(),
            path: path.clone(),
            state: signal(State::Loading),
            history: Rc::new(RefCell::new(History::default())),
            revision: signal(0),
            input: editor_ref(),
            restoring: Rc::new(Cell::new(false)),
        };
        let loaded = editor.clone();
        browser.blocking(
            "Read file tags",
            move || file_tags::read(&path).map_err(|error| error.to_string()),
            move |_, result| match result {
                Ok(tags) => {
                    loaded.history.borrow_mut().current.tags = tags.clone();
                    loaded.state.set(State::Editing {
                        original: tags,
                        error: None,
                    });
                    loaded.changed();
                    loaded.input.focus();
                }
                Err(error) => loaded.state.set(State::Failed(error)),
            },
        );
        editor
    }
    fn editable(&self) -> bool {
        matches!(self.state.get_untracked(), State::Editing { .. })
    }
    pub(in crate::pages::workspace) fn is_saving(&self) -> bool {
        matches!(self.state.get_untracked(), State::Saving)
    }
    pub(super) fn available(&self) -> bool {
        matches!(self.state.get(), State::Editing { .. })
    }
    pub(super) fn focus(&self) {
        self.input.focus();
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    fn changed(&self) {
        self.revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }
    fn error(&self, error: Option<String>) {
        self.state.update(|state| {
            if let State::Editing { error: current, .. } = state {
                *current = error;
            }
        });
    }
    fn record_input(&self, value: &str) {
        if self.restoring.get() || !self.editable() {
            return;
        }
        if self
            .input
            .with(|input| input.preedit.is_some())
            .unwrap_or(false)
        {
            return;
        }
        let mut history = self.history.borrow_mut();
        let mut next = history.current.clone();
        next.draft = value.to_owned();
        if let Some((anchor, focus)) = self.input.with(|input| (input.anchor, input.focus)) {
            next.selection = anchor.min(focus)..anchor.max(focus);
            next.reversed = anchor > focus;
        }
        history.record(next, true);
    }
    fn selection(&self, anchor: usize, focus: usize) {
        if self.restoring.get() {
            return;
        }
        let mut history = self.history.borrow_mut();
        history.current.selection = anchor.min(focus)..anchor.max(focus);
        history.current.reversed = anchor > focus;
    }
    fn restore_input(&self) {
        let state = self.history.borrow().current.clone();
        let (anchor, focus) = if state.reversed {
            (state.selection.end, state.selection.start)
        } else {
            (state.selection.start, state.selection.end)
        };
        self.restoring.set(true);
        self.input
            .replace_and_select(0..self.input.text().len(), &state.draft, anchor, focus);
        self.restoring.set(false);
        self.input.focus();
    }
    fn add(&self) -> bool {
        if !self.editable() {
            return false;
        }
        let draft = self.input.text();
        let tags = match file_tags::decode(draft.as_bytes()) {
            Ok(tags) => tags,
            Err(error) => {
                self.error(Some(error.to_string()));
                return false;
            }
        };
        self.record_input(&draft);
        {
            let mut history = self.history.borrow_mut();
            let mut next = history.current.clone();
            for tag in tags {
                if !next.tags.contains(&tag) {
                    next.tags.push(tag);
                }
            }
            next.draft.clear();
            next.selection = 0..0;
            next.reversed = false;
            history.record(next, false);
        }
        self.restore_input();
        self.error(None);
        self.changed();
        true
    }
    fn remove(&self, tag: &str) {
        if !self.editable() {
            return;
        }
        self.record_input(&self.input.text());
        {
            let mut history = self.history.borrow_mut();
            let mut next = history.current.clone();
            next.tags.retain(|value| value != tag);
            history.record(next, false);
        }
        self.error(None);
        self.changed();
        self.input.focus();
    }
    fn key(&self, event: &KeyboardEvent) {
        if self
            .input
            .with(|input| input.preedit.is_some())
            .unwrap_or(false)
        {
            return;
        }
        if !self.editable() {
            return;
        }
        if event.modifiers.primary()
            && !event.modifiers.alt
            && (event.key.is_char('z') || event.key.is_char('y'))
        {
            self.record_input(&self.input.text());
            let redo = event.modifiers.shift || event.key.is_char('y');
            let changed = self.history.borrow_mut().step(redo);
            if changed {
                self.restore_input();
                self.error(None);
                self.changed();
            }
        } else if event.modifiers == Modifiers::default()
            && event.key == Key::Named(NamedKey::Enter)
        {
            self.add();
        } else if event.modifiers == Modifiers::default()
            && event.key == Key::Named(NamedKey::Backspace)
            && self.input.text().is_empty()
        {
            let last = self.history.borrow().current.tags.last().cloned();
            if let Some(tag) = last {
                self.remove(&tag);
            }
        } else {
            return;
        }
        event.prevent_default();
        event.stop_propagation();
    }
    pub(super) fn save(&self, complete: impl FnOnce() + 'static) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        if !self.add() {
            return;
        }
        let State::Editing { original, .. } = self.state.get_untracked() else {
            return;
        };
        let edited = self.history.borrow().current.tags.clone();
        self.state.set(State::Saving);
        let state = self.state;
        let path = self.path.clone();
        let saved = edited.clone();
        let unchanged = original.clone();
        browser.blocking(
            "Save file tags",
            move || {
                file_tags::apply_edit(&path, &original, &edited).map_err(|error| error.to_string())
            },
            move |_, result| {
                let succeeded = result.is_ok();
                state.set(match result {
                    Ok(()) => State::Editing {
                        original: saved,
                        error: None,
                    },
                    Err(error) => State::Editing {
                        original: unchanged,
                        error: Some(error),
                    },
                });
                if succeeded {
                    complete();
                }
            },
        );
    }
    fn chip(&self, tag: String) -> Div {
        let remove = self.clone();
        let label = tag.clone();
        el! {
            .div.{flex;items_center;gap:4.0_f32;px:6.0_f32;rounded:6.0_f32;bg:$(theme().selected)}.[
                label.text,
                "remove-tag".IconButton::new.{
                    icon:Icon::Close;
                    label_fn:()=>"tags-remove".i18n::text;
                    on_click:move _=>$(remove.remove(&tag));
                },
            ]
        }
    }
    fn chips(&self) -> Div {
        self.revision.get();
        let tags = self.history.borrow().current.tags.clone();
        el! {.div.{flex;flex_wrap;gap:6.0_f32}.[..$(tags.into_iter().map(|tag|self.chip(tag)))]}
    }

    pub(super) fn fields(&self) -> Div {
        let state = self.state;
        let chips = self.clone();
        let input = self.clone();
        let selected = self.clone();
        let key = self.clone();
        let blur = self.clone();
        el! {
            .div.{flex;flex_col;gap:8.0_f32}.[
                (move()=>&chips.{chips}).dyn_view,
                (()=>"tags-add-placeholder".i18n::text).text,
                .text_editor.{
                    editor_ref:$(self.input); h:36.0_f32; min_h:36.0_f32; border:1.0_f32; p:6.0_f32;
                    dyn_style:s=>$(s.border_color(theme().border).bg(theme().surface));
                    disabled:move()=>$(!matches!(state.get(),State::Editing{..}));
                    on_change:move value=>&input.{record_input:value};
                    on_select:move (anchor,focus)=>&selected.{selection:anchor,focus};
                    on_key_down:move event=>&key.{key:event};
                    on_focus_change:move focused=>$(if !focused {blur.history.borrow_mut().break_typing();});
                },
                (move()=>${match state.get() {
                    State::Loading=>i18n::text("tags-loading"),
                    State::Saving=>i18n::text("tags-saving"),
                    State::Failed(error)=>error,
                    State::Editing{error,..}=>error.unwrap_or_default(),
                }}).text,
            ]
        }
    }
}
