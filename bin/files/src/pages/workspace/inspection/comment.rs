//! Modal comment editor backed by FreeDesktop `user.xdg.comment` extended attributes.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use gds::{i18n, service::file_tags};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal};
use zgui::view::{FrameHandle, current_host, install_stylesheet, request_frame};
use zgui::vocab::{Key, NamedKey};
use zgui::{css, view};
use zgui_ui::{InputProps, SpinnerProps};
use zgui_ui_primitives::Binding;

use super::super::ui::{Browser, WeakBrowser};

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

    .gds-files-comment-editor__status {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-comment-editor__hint {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
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
    browser: WeakBrowser,
    path: PathBuf,
    input_ref: NodeRef,
    draft: RwSignal<String, LocalStorage>,
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
    pub(super) fn new(browser: &Browser, path: PathBuf) -> Self {
        let editor = browser.0.owner.with(|| {
            Self(Rc::new(CommentEditorInner {
                browser: browser.downgrade(),
                path,
                input_ref: NodeRef::new(),
                draft: RwSignal::new_local(String::new()),
                loading: RwSignal::new_local(true),
                saving: RwSignal::new_local(false),
                load_failed: RwSignal::new_local(false),
                composing: RwSignal::new_local(false),
                error: RwSignal::new_local(None),
                pending_focus: RefCell::new(None),
            }))
        });
        editor.load();
        editor
    }

    fn load(&self) {
        let Some(browser) = self.0.browser.upgrade() else {
            return;
        };
        let path = self.0.path.clone();
        let weak = Rc::downgrade(&self.0);
        browser.blocking(
            "Read file comment",
            move || file_tags::read_comment(&path).map_err(|err| err.to_string()),
            move |_browser, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                editor.0.loading.set(false);
                match result {
                    Ok(comment) => {
                        editor.0.draft.set(comment.unwrap_or_default());
                        editor.0.load_failed.set(false);
                        editor.0.error.set(None);
                        editor.focus();
                    }
                    Err(message) => {
                        editor.0.load_failed.set(true);
                        editor.0.error.set(Some(message));
                    }
                }
            },
        );
    }

    pub(super) fn path(&self) -> &Path {
        &self.0.path
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

    pub(super) fn save(&self, complete: impl FnOnce() + 'static) {
        if !self.available_untracked() || self.is_saving_untracked() {
            return;
        }
        let Some(browser) = self.0.browser.upgrade() else {
            return;
        };
        let draft = self.0.draft.get_untracked();
        let normalized = {
            let trimmed = draft.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        };
        let save_path = self.0.path.clone();
        let refresh_path = self.0.path.clone();
        let weak = Rc::downgrade(&self.0);

        self.0.saving.set(true);
        self.0.error.set(None);

        let done = RefCell::new(Some(Box::new(complete) as Box<dyn FnOnce()>));
        browser.blocking(
            "Write file comment",
            move || {
                file_tags::write_comment(&save_path, normalized.as_deref())
                    .map_err(|err| err.to_string())
            },
            move |browser, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                editor.0.saving.set(false);
                match result {
                    Ok(()) => {
                        browser.report_success(i18n::text_untracked("details-comment-saved"));
                        browser.refresh_affected(&[refresh_path]);
                        super::bump_details_revision(browser);
                        if let Some(callback) = done.borrow_mut().take() {
                            callback();
                        }
                    }
                    Err(message) => {
                        editor.0.error.set(Some(message));
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

        AnyView::new(view! {
            column(class = "gds-files-comment-editor") {
                {move || if loading.get() {
                    AnyView::new(view! {
                        row(class = "gds-files-comment-editor__status") {
                            Spinner {}
                            text {{i18n::text("loading")}}
                        }
                    })
                } else {
                    let submit_key = Rc::clone(&submit);
                    let disabled = Signal::derive_local(move || saving.get() || load_failed.get());
                    AnyView::new(view! {
                        Input(
                            node_ref = input_ref,
                            value = Binding::controlled(
                                draft.read_only(),
                                move |next: String| {
                                    draft.set(next);
                                    error.set(None);
                                },
                            ),
                            placeholder = i18n::text("details-comment-placeholder"),
                            disabled = disabled,
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
                                if ev.key == Key::Named(NamedKey::Enter) && ev.modifiers.is_empty() {
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                    submit_key();
                                }
                            },
                        ) {}
                        text(class = "gds-files-comment-editor__hint") {
                            {i18n::text("details-comment-hint")}
                        }
                    })
                }}
                {move || error.get().map(|err| {
                    view! {
                        text(class = "gds-files-comment-editor__error") {{err}}
                    }
                    .into_view()
                })}
            }
        })
    }
}
