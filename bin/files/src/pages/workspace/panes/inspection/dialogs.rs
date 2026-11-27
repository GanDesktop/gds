//! Per-workspace dialog slots and unified item editor modal.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

use gds::i18n;
use gds::ui::feedback::dialog::{
    DialogActionsProps, DialogDecision, DialogDescriptionProps, DialogFooterProps,
    DialogHeaderProps, DialogSize, DialogTitleProps, ModalDialogProps,
};
use gds::ui::feedback::tooltip::HintTooltipProps;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::{current_host, install_stylesheet};
use zgui::{css, view};
use zgui_ui_primitives::Binding;

use crate::entities::file::directory::Entry;

use super::Inspection;
use super::editor::{self, EditTab};
use crate::pages::workspace::state::context::{Context, WeakContext};

const DIALOGS_SHEET: &str = "gds-files-workspace-dialogs";

const DIALOGS_CSS: &str = css!(
    r#"
    .gds-files-dialog-body {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    .gds-files-dialog-desc-wrap .zui-tooltip__trigger {
        display: inline-flex;
        max-width: 100%;
        min-width: 0;
    }

    .gds-files-dialog-desc {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 100%;
    }
    "#
);

fn install_dialogs_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(DIALOGS_SHEET, DIALOGS_CSS);
    }
}

#[derive(Clone)]
struct DialogSlots {
    ctx: WeakContext,
    editor: RwSignal<Option<editor::ItemEditor>, LocalStorage>,
}

thread_local! {
    static DIALOGS: RefCell<HashMap<usize, DialogSlots>> = RefCell::new(HashMap::new());
}

fn dialog_slots(ctx: &Context) -> DialogSlots {
    let key = Rc::as_ptr(&ctx.0) as usize;
    if let Some(existing) = DIALOGS.with(|map| {
        map.borrow().get(&key).and_then(|slots| {
            slots
                .ctx
                .upgrade()
                .filter(|existing_ctx| Rc::ptr_eq(&existing_ctx.0, &ctx.0))
                .map(|_| slots.clone())
        })
    }) {
        return existing;
    }

    let created = ctx.0.owner.with(|| {
        on_cleanup_local(move || {
            DIALOGS.with(|map| {
                map.borrow_mut().remove(&key);
            });
        });
        DialogSlots {
            ctx: ctx.downgrade(),
            editor: RwSignal::new_local(None),
        }
    });
    DIALOGS.with(|map| {
        map.borrow_mut().insert(key, created.clone());
    });
    created
}

impl Inspection<'_> {
    pub(in crate::pages::workspace) fn has_open_inspection_dialog(&self) -> bool {
        dialog_slots(self.0).editor.get_untracked().is_some()
    }

    pub(in crate::pages::workspace) fn cancel_inspection_dialog(&self) -> bool {
        let slots = dialog_slots(self.0);
        if let Some(editor) = slots.editor.get_untracked() {
            if !editor.is_saving_untracked() {
                slots.editor.set(None);
            }
            return true;
        }
        false
    }

    pub(in crate::pages::workspace) fn can_edit_item_properties(&self) -> bool {
        editor::has_editable_tabs(self.0)
    }

    pub(in crate::pages::workspace) fn edit_item_properties(
        &self,
        entry: Entry,
        initial_tab: Option<EditTab>,
    ) {
        self.0.0.menus.close();
        let slots = dialog_slots(self.0);
        if slots
            .editor
            .get_untracked()
            .as_ref()
            .is_some_and(editor::ItemEditor::is_saving_untracked)
        {
            return;
        }
        if let Some(item_editor) = editor::ItemEditor::from_entry(self.0, entry, initial_tab) {
            slots.editor.set(Some(item_editor));
        }
    }

    pub(in crate::pages::workspace) fn edit_path_properties(
        &self,
        path: PathBuf,
        initial_tab: Option<EditTab>,
    ) {
        self.0.0.menus.close();
        let slots = dialog_slots(self.0);
        if slots
            .editor
            .get_untracked()
            .as_ref()
            .is_some_and(editor::ItemEditor::is_saving_untracked)
        {
            return;
        }
        if let Some(item_editor) = editor::ItemEditor::from_path(self.0, path, initial_tab) {
            slots.editor.set(Some(item_editor));
        }
    }

    #[cfg(feature = "files-e2e")]
    pub(in crate::pages::workspace) fn edit_tags(&self, path: PathBuf) {
        self.edit_path_properties(path, Some(EditTab::Tags));
    }

    #[cfg(all(unix, feature = "files-e2e"))]
    pub(in crate::pages::workspace) fn edit_permissions(&self, entry: Entry) {
        self.edit_item_properties(entry, Some(EditTab::Permissions));
    }

    pub(in crate::pages::workspace) fn item_editor_dialog(&self) -> AnyView {
        install_dialogs_stylesheet();
        let slots = dialog_slots(self.0);
        let editor_sig = slots.editor;
        let open = Signal::derive_local(move || editor_sig.get().is_some());
        let can_dismiss = Signal::derive_local(move || {
            !editor_sig
                .get()
                .as_ref()
                .is_some_and(editor::ItemEditor::is_saving)
        });
        let modal_disabled = Signal::derive_local(move || {
            !editor_sig
                .get()
                .as_ref()
                .is_some_and(editor::ItemEditor::can_save)
        });
        let on_modal_decision =
            UnsyncCallback::new(move |decision: DialogDecision| match decision {
                DialogDecision::Confirm => {
                    if let Some(editor) = editor_sig.get_untracked()
                        && !editor.is_saving_untracked()
                    {
                        editor.save(move || editor_sig.set(None));
                    }
                }
                DialogDecision::Cancel | DialogDecision::Dismiss => {
                    if !editor_sig
                        .get_untracked()
                        .as_ref()
                        .is_some_and(editor::ItemEditor::is_saving_untracked)
                    {
                        editor_sig.set(None);
                    }
                }
            });

        AnyView::new(view! {
            ModalDialog(
                open = Binding::controlled(open, move |next: bool| {
                    if !next
                        && !editor_sig
                            .get_untracked()
                            .as_ref()
                            .is_some_and(editor::ItemEditor::is_saving_untracked)
                    {
                        editor_sig.set(None);
                    }
                }),
                size = DialogSize::Md,
                disabled = modal_disabled,
                dismiss_on_outside_press = can_dismiss,
                dismiss_on_escape = can_dismiss,
                close_on_confirm = false,
                on_decision = on_modal_decision,
            ) {
                {move || match editor_sig.get() {
                    None => AnyView::new(()),
                    Some(editor) => {
                        let full_path = editor.path().display().to_string();
                        let title_path = editor
                            .path()
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| full_path.clone());
                        let submit_sig = editor_sig;
                        let fields = editor.fields(move || {
                            if let Some(active) = submit_sig.get_untracked()
                                && !active.is_saving_untracked()
                            {
                                active.save(move || submit_sig.set(None));
                            }
                        });
                        let disabled = Signal::derive_local(move || {
                            !editor_sig
                                .get()
                                .as_ref()
                                .is_some_and(editor::ItemEditor::can_save)
                        });
                        AnyView::new(view! {
                            column(class = "gds-files-dialog-body") {
                                DialogHeader {
                                    DialogTitle {{i18n::text("details-edit-properties-title")}}
                                    box(class = "gds-files-dialog-desc-wrap") {
                                        HintTooltip(text = full_path) {
                                            DialogDescription(class = "gds-files-dialog-desc") {
                                                {title_path}
                                            }
                                        }
                                    }
                                }
                                {fields}
                                DialogFooter {
                                    DialogActions(
                                        primary_label = i18n::text("permissions-save"),
                                        cancel_label = i18n::text("operation-cancel"),
                                        disabled = disabled,
                                        on_decision = on_modal_decision,
                                    )
                                }
                            }
                        })
                    }
                }}
            }
        })
    }
}
