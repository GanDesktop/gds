//! Comment tab editor backed by FreeDesktop `user.xdg.comment` extended attributes.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gds::{i18n, service::file_tags};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal};
use zgui::view::{FrameHandle, current_host, install_stylesheet, request_frame};
use zgui::vocab::{Key, Modifiers, NamedKey};
use zgui::{css, view};
use zgui_ui::TextareaProps;
use zgui_ui_primitives::Binding;

use crate::pages::workspace::state::context::{Context, WeakContext};

const COMMENT_SHEET: &str = "gds-files-comment-editor";

const COMMENT_CSS: &str = css!(
    r#"
    .gds-files-comment-editor {
        display: flex;
        flex-direction: column;
        gap: 8px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-comment-editor__textarea.zui-textarea {
        height: 176px;
        min-height: 176px;
        max-height: 176px;
        box-sizing: border-box;
        overflow-y: auto;
    }

    .gds-files-comment-editor__hint {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        min-height: 16px;
    }

    .gds-files-comment-editor__error {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-error);
    }
    "#
);

fn install_comment_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(COMMENT_SHEET, COMMENT_CSS);
    }
}

struct CommentEditorInner {
    ctx: WeakContext,
    path: RefCell<PathBuf>,
    input_ref: NodeRef,
    draft: RwSignal<String, LocalStorage>,
    original: RefCell<Option<String>>,
    loading: RwSignal<bool, LocalStorage>,
    saving: RwSignal<bool, LocalStorage>,
    load_failed: RwSignal<bool, LocalStorage>,
    composing: RwSignal<bool, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
    pending_focus: RefCell<Option<FrameHandle>>,
}

#[derive(Clone)]
pub(super) struct CommentEditor(Rc<CommentEditorInner>);

impl CommentEditor {
    pub(super) fn new(ctx: &Context, path: PathBuf, auto_focus: bool) -> Self {
        let editor = ctx.0.owner.with(|| {
            Self(Rc::new(CommentEditorInner {
                ctx: ctx.downgrade(),
                path: RefCell::new(path),
                input_ref: NodeRef::new(),
                draft: RwSignal::new_local(String::new()),
                original: RefCell::new(None),
                loading: RwSignal::new_local(true),
                saving: RwSignal::new_local(false),
                load_failed: RwSignal::new_local(false),
                composing: RwSignal::new_local(false),
                error: RwSignal::new_local(None),
                pending_focus: RefCell::new(None),
            }))
        });
        editor.load(auto_focus);
        editor
    }

    pub(super) fn set_path(&self, path: PathBuf) {
        *self.0.path.borrow_mut() = path;
    }

    fn load(&self, auto_focus: bool) {
        let Some(ctx) = self.0.ctx.upgrade() else {
            return;
        };
        let path = self.0.path.borrow().clone();
        let weak = Rc::downgrade(&self.0);
        ctx.blocking(
            "Read file comment",
            move || file_tags::read_comment(&path).map_err(|err| err.to_string()),
            move |_ctx, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                editor.0.loading.set(false);
                match result {
                    Ok(comment) => {
                        *editor.0.original.borrow_mut() = comment.clone();
                        editor.0.draft.set(comment.unwrap_or_default());
                        editor.0.load_failed.set(false);
                        editor.0.error.set(None);
                        if auto_focus {
                            editor.focus();
                        }
                    }
                    Err(message) => {
                        editor.0.load_failed.set(true);
                        editor.0.error.set(Some(message));
                    }
                }
            },
        );
    }

    fn normalized_draft(&self) -> Option<String> {
        let draft = self.0.draft.get_untracked();
        let trimmed = draft.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    pub(super) fn is_loading(&self) -> bool {
        self.0.loading.get()
    }

    pub(super) fn is_saving(&self) -> bool {
        self.0.saving.get()
    }

    pub(super) fn is_saving_untracked(&self) -> bool {
        self.0.saving.get_untracked()
    }

    pub(super) fn available(&self) -> bool {
        !self.0.loading.get() && !self.0.load_failed.get()
    }

    pub(super) fn available_untracked(&self) -> bool {
        !self.0.loading.get_untracked() && !self.0.load_failed.get_untracked()
    }

    pub(super) fn has_changes_untracked(&self) -> bool {
        self.available_untracked() && self.normalized_draft() != *self.0.original.borrow()
    }

    pub(super) fn focus(&self) {
        if self.0.input_ref.is_bound() {
            self.0.input_ref.focus();
            return;
        }
        let weak = Rc::downgrade(&self.0);
        if let Some(ctx) = self.0.ctx.upgrade() {
            let handle = ctx.0.owner.with(|| {
                request_frame(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        inner.input_ref.focus();
                    }
                })
            });
            *self.0.pending_focus.borrow_mut() = Some(handle);
        }
    }

    pub(super) fn save_result(&self, on_done: impl FnOnce(Result<(), ()>) + 'static) {
        if !self.available_untracked() || self.is_saving_untracked() {
            on_done(Ok(()));
            return;
        }
        let normalized = self.normalized_draft();
        if normalized == *self.0.original.borrow() {
            on_done(Ok(()));
            return;
        }
        let Some(ctx) = self.0.ctx.upgrade() else {
            on_done(Err(()));
            return;
        };
        let save_path = self.0.path.borrow().clone();
        let write_value = normalized.clone();
        let weak = Rc::downgrade(&self.0);

        self.0.saving.set(true);
        self.0.error.set(None);

        let done = RefCell::new(Some(Box::new(on_done) as Box<dyn FnOnce(Result<(), ()>)>));
        ctx.blocking(
            "Write file comment",
            move || {
                file_tags::write_comment(&save_path, write_value.as_deref())
                    .map_err(|err| err.to_string())
            },
            move |_ctx, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                editor.0.saving.set(false);
                match result {
                    Ok(()) => {
                        *editor.0.original.borrow_mut() = normalized;
                        if let Some(callback) = done.borrow_mut().take() {
                            callback(Ok(()));
                        }
                    }
                    Err(message) => {
                        editor.0.error.set(Some(message));
                        if let Some(callback) = done.borrow_mut().take() {
                            callback(Err(()));
                        }
                    }
                }
            },
        );
    }

    pub(super) fn fields(&self, on_enter_submit: impl Fn() + 'static) -> AnyView {
        install_comment_stylesheet();
        let loading = self.0.loading;
        let saving = self.0.saving;
        let load_failed = self.0.load_failed;
        let draft = self.0.draft;
        let composing = self.0.composing;
        let error = self.0.error;
        let input_ref = self.0.input_ref;
        let submit = Rc::new(on_enter_submit);
        let disabled =
            Signal::derive_local(move || loading.get() || saving.get() || load_failed.get());
        let invalid = Signal::derive_local(move || error.get().is_some());

        AnyView::new(view! {
            column(class = "gds-files-comment-editor") {
                Textarea(
                    node_ref = input_ref,
                    class = "gds-files-comment-editor__textarea",
                    value = Binding::controlled(
                        draft.read_only(),
                        move |next: String| {
                            draft.set(next);
                            error.set(None);
                        },
                    ),
                    placeholder = i18n::text("details-comment-placeholder"),
                    disabled = disabled,
                    invalid = invalid,
                    on:ime_start = move |_| composing.set(true),
                    on:ime_preedit = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::ImePreedit>| {
                        composing.set(ev.is_composing());
                    },
                    on:ime_commit = move |_| composing.set(false),
                    on:ime_end = move |_| composing.set(false),
                    on:key_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::KeyDown>| {
                        if ev.repeat || composing.get_untracked() {
                            return;
                        }
                        if ev.key == Key::Named(NamedKey::Enter)
                            && ev.modifiers == Modifiers::CONTROL
                        {
                            ev.prevent_default();
                            ev.stop_propagation();
                            submit();
                        }
                    },
                ) {}
                {move || {
                    if let Some(err) = error.get() {
                        AnyView::new(view! {
                            text(class = "gds-files-comment-editor__error") {{err}}
                        })
                    } else if loading.get() {
                        AnyView::new(view! {
                            text(class = "gds-files-comment-editor__hint") {{i18n::text("loading")}}
                        })
                    } else {
                        AnyView::new(view! {
                            text(class = "gds-files-comment-editor__hint") {
                                {i18n::text("details-comment-hint")}
                            }
                        })
                    }
                }}
            }
        })
    }
}
