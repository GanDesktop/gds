//! POSIX file and directory permission editor tab.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use gds::i18n;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal};
use zgui::view::{current_host, install_stylesheet};
use zgui::{css, view};
use zgui_ui::{CheckboxProps, Checked, LabelProps};
use zgui_ui_primitives::Binding;

use crate::entities::file::directory::{Entry, EntryKind};
use crate::features::file_operations::permissions;

use crate::pages::workspace::{
    operations::Lifecycle,
    state::context::{Context, WeakContext},
};

const PERMISSIONS_SHEET: &str = "gds-files-workspace-permissions";

const PERMISSIONS_CSS: &str = css!(
    r#"
    .gds-files-permissions-grid {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-permissions-table {
        display: flex;
        flex-direction: column;
        width: 100%;
        border: 1px solid var(--gds-border);
        border-radius: var(--gds-radius-control, 8px);
        background: var(--gds-surface-container-low, transparent);
        overflow: hidden;
    }

    .gds-files-permissions-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 12px;
        padding: 11px 14px;
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-files-permissions-row--last {
        border-bottom: none;
    }

    .gds-files-permissions-row__title {
        width: 68px;
        flex-shrink: 0;
        font-size: var(--gds-text-xs, 12px);
        font-weight: 500;
        color: var(--gds-muted-foreground);
    }

    .gds-files-permissions-row__checks {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: center;
        gap: 16px;
        flex: 1 1 0%;
        min-width: 0;
    }

    .gds-files-permissions-check {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
    }

    .gds-files-permissions-recursive {
        display: flex;
        flex-direction: column;
        gap: 4px;
        min-height: 44px;
    }

    .gds-files-permissions-note {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-permissions-error {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-error);
    }
    "#
);

fn install_permissions_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(PERMISSIONS_SHEET, PERMISSIONS_CSS);
    }
}

struct PermissionEditorInner {
    ctx: WeakContext,
    path: RefCell<PathBuf>,
    directory: RwSignal<bool, LocalStorage>,
    mode: RwSignal<u32, LocalStorage>,
    original_mode: RefCell<u32>,
    recursive: RwSignal<bool, LocalStorage>,
    loading: RwSignal<bool, LocalStorage>,
    saving: RwSignal<bool, LocalStorage>,
    load_failed: RwSignal<bool, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
}

#[derive(Clone)]
pub(super) struct PermissionEditor(Rc<PermissionEditorInner>);

impl PermissionEditor {
    pub(super) fn new(ctx: &Context, entry: Entry) -> Self {
        let directory = entry.kind == EntryKind::Directory;
        let is_symlink = entry.kind == EntryKind::SymbolicLink;
        let initial_mode = entry
            .details
            .permissions
            .as_ref()
            .map_or(0o644, |perms| perms.mode);
        let editor = ctx.0.owner.with(|| {
            Self(Rc::new(PermissionEditorInner {
                ctx: ctx.downgrade(),
                path: RefCell::new(entry.path.clone()),
                directory: RwSignal::new_local(directory),
                mode: RwSignal::new_local(initial_mode),
                original_mode: RefCell::new(initial_mode),
                recursive: RwSignal::new_local(false),
                loading: RwSignal::new_local(true),
                saving: RwSignal::new_local(false),
                load_failed: RwSignal::new_local(false),
                error: RwSignal::new_local(None),
            }))
        });

        if is_symlink {
            editor.0.loading.set(false);
            editor.0.load_failed.set(true);
            editor
                .0
                .error
                .set(Some(i18n::text_untracked("permissions-symlink")));
            return editor;
        }

        editor.load(ctx);
        editor
    }

    pub(super) fn from_path(ctx: &Context, path: PathBuf) -> Self {
        let editor = ctx.0.owner.with(|| {
            Self(Rc::new(PermissionEditorInner {
                ctx: ctx.downgrade(),
                path: RefCell::new(path),
                directory: RwSignal::new_local(false),
                mode: RwSignal::new_local(0o644),
                original_mode: RefCell::new(0o644),
                recursive: RwSignal::new_local(false),
                loading: RwSignal::new_local(true),
                saving: RwSignal::new_local(false),
                load_failed: RwSignal::new_local(false),
                error: RwSignal::new_local(None),
            }))
        });
        editor.load(ctx);
        editor
    }

    pub(super) fn set_path(&self, path: PathBuf) {
        *self.0.path.borrow_mut() = path;
    }

    fn load(&self, ctx: &Context) {
        let weak = Rc::downgrade(&self.0);
        let read_path = self.0.path.borrow().clone();
        let symlink_warning = i18n::text_untracked("permissions-symlink");
        ctx.blocking(
            "Read file permissions",
            move || {
                let meta = std::fs::symlink_metadata(&read_path).map_err(|err| err.to_string())?;
                if meta.file_type().is_symlink() {
                    return Err(symlink_warning);
                }
                let is_dir = meta.is_dir();
                let mode = permissions::read(&read_path).map_err(|err| err.to_string())?;
                Ok((mode, is_dir))
            },
            move |_, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let target = Self(inner);
                target.0.loading.set(false);
                match result {
                    Ok((mode, is_dir)) => {
                        target.0.directory.set(is_dir);
                        target.0.mode.set(mode);
                        *target.0.original_mode.borrow_mut() = mode;
                        target.0.load_failed.set(false);
                        target.0.error.set(None);
                    }
                    Err(message) => {
                        target.0.load_failed.set(true);
                        target.0.error.set(Some(message));
                    }
                }
            },
        );
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

    pub(super) fn can_save(&self) -> bool {
        !self.0.loading.get() && !self.0.load_failed.get() && !self.0.saving.get()
    }

    fn can_save_untracked(&self) -> bool {
        !self.0.loading.get_untracked()
            && !self.0.load_failed.get_untracked()
            && !self.0.saving.get_untracked()
    }

    pub(super) fn has_changes_untracked(&self) -> bool {
        self.can_save_untracked()
            && (((self.0.mode.get_untracked() & 0o777) != (*self.0.original_mode.borrow() & 0o777))
                || (self.0.directory.get_untracked() && self.0.recursive.get_untracked()))
    }

    pub(super) fn save_result(&self, on_done: impl FnOnce(Result<(), ()>) + 'static) {
        if !self.can_save_untracked() {
            on_done(Ok(()));
            return;
        }
        let Some(ctx) = self.0.ctx.upgrade() else {
            on_done(Err(()));
            return;
        };
        let path = self.0.path.borrow().clone();
        // `permissions::save` and `permissions::apply_cancellable` read fstat and preserve
        // all high bits (`!0o777`, including `0o7000` setuid/setgid/sticky) while applying `access`.
        let access = self.0.mode.get_untracked() & 0o777;
        let recursive = self.0.directory.get_untracked() && self.0.recursive.get_untracked();
        let weak = Rc::downgrade(&self.0);

        self.0.saving.set(true);
        self.0.error.set(None);

        let done = RefCell::new(Some(Box::new(on_done) as Box<dyn FnOnce(Result<(), ()>)>));
        if recursive {
            let Some(cancel) =
                Lifecycle(&ctx).begin_operation(&i18n::text_untracked("permissions-saving"))
            else {
                self.0.saving.set(false);
                if let Some(cb) = done.borrow_mut().take() {
                    cb(Err(()));
                }
                return;
            };
            ctx.blocking(
                "Apply recursive permissions",
                move || Ok(permissions::apply_cancellable(&path, access, true, &cancel)),
                move |ctx, result| {
                    Lifecycle(ctx).end_operation();
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let target = Self(inner);
                    target.0.saving.set(false);
                    match result {
                        Ok(report) => {
                            if report.failures.is_empty() {
                                *target.0.original_mode.borrow_mut() = access;
                                target.0.recursive.set(false);
                                if let Some(cb) = done.borrow_mut().take() {
                                    cb(Ok(()));
                                }
                            } else {
                                for failure in &report.failures {
                                    tracing::warn!(
                                        path = %failure.path.display(),
                                        error = %failure.error,
                                        "recursive permission update failed"
                                    );
                                }
                                let succeeded = report.succeeded;
                                let failed = report.failures.len();
                                let msg =
                                    gds::t_untracked!("permissions-partial", succeeded, failed);
                                ctx.report_notice(msg.clone());
                                target.0.error.set(Some(msg));
                                if let Some(cb) = done.borrow_mut().take() {
                                    cb(Err(()));
                                }
                            }
                        }
                        Err(err) => {
                            target.0.error.set(Some(err));
                            if let Some(cb) = done.borrow_mut().take() {
                                cb(Err(()));
                            }
                        }
                    }
                },
            );
        } else {
            ctx.blocking(
                "Save file permissions",
                move || permissions::save(&path, access).map_err(|err| err.to_string()),
                move |_ctx, result| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let target = Self(inner);
                    target.0.saving.set(false);
                    match result {
                        Ok(()) => {
                            *target.0.original_mode.borrow_mut() = access;
                            if let Some(cb) = done.borrow_mut().take() {
                                cb(Ok(()));
                            }
                        }
                        Err(err) => {
                            target.0.error.set(Some(err));
                            if let Some(cb) = done.borrow_mut().take() {
                                cb(Err(()));
                            }
                        }
                    }
                },
            );
        }
    }

    fn bit_checkbox(&self, label_key: &'static str, mask: u32) -> AnyView {
        let mode_sig = self.0.mode;
        let loading_sig = self.0.loading;
        let saving_sig = self.0.saving;
        let failed_sig = self.0.load_failed;
        let disabled =
            Signal::derive_local(move || loading_sig.get() || saving_sig.get() || failed_sig.get());
        let checked = Binding::controlled(
            Signal::derive_local(move || {
                if (mode_sig.get() & mask) != 0 {
                    Checked::Yes
                } else {
                    Checked::No
                }
            }),
            move |next| {
                mode_sig.update(|bits| {
                    if matches!(next, Checked::Yes) {
                        *bits |= mask;
                    } else {
                        *bits &= !mask;
                    }
                });
            },
        );
        let box_ref = NodeRef::new();
        let label_ref = NodeRef::new();
        AnyView::new(view! {
            row(class = "gds-files-permissions-check") {
                Checkbox(
                    node_ref = box_ref,
                    checked = checked,
                    disabled = disabled,
                    labelled_by = label_ref,
                )
                Label(node_ref = label_ref, control = box_ref) {{i18n::text(label_key)}}
            }
        })
    }

    fn group(&self, title_key: &'static str, shift: u32, last: bool) -> AnyView {
        let read = self.bit_checkbox("permissions-read", 0o4 << shift);
        let write = self.bit_checkbox("permissions-write", 0o2 << shift);
        let execute = self.bit_checkbox("permissions-execute", 0o1 << shift);
        let row_class = if last {
            "gds-files-permissions-row gds-files-permissions-row--last"
        } else {
            "gds-files-permissions-row"
        };
        AnyView::new(view! {
            row(class = row_class) {
                text(class = "gds-files-permissions-row__title") {{i18n::text(title_key)}}
                row(class = "gds-files-permissions-row__checks") {
                    {read}
                    {write}
                    {execute}
                }
            }
        })
    }

    pub(super) fn fields(&self) -> AnyView {
        install_permissions_stylesheet();
        let loading_sig = self.0.loading;
        let saving_sig = self.0.saving;
        let failed_sig = self.0.load_failed;
        let error_sig = self.0.error;
        let rec_sig = self.0.recursive;
        let dir_sig = self.0.directory;

        let owner_group = self.group("permissions-owner", 6, false);
        let group_group = self.group("permissions-group", 3, false);
        let other_group = self.group("permissions-others", 0, true);

        let recursive_view = move || {
            dir_sig.get().then(|| {
                let disabled = Signal::derive_local(move || {
                    loading_sig.get() || saving_sig.get() || failed_sig.get()
                });
                let checked = Binding::controlled(
                    Signal::derive_local(move || {
                        if rec_sig.get() {
                            Checked::Yes
                        } else {
                            Checked::No
                        }
                    }),
                    move |next| rec_sig.set(matches!(next, Checked::Yes)),
                );
                let rec_ref = NodeRef::new();
                let rec_label = NodeRef::new();
                view! {
                    column(class = "gds-files-permissions-recursive") {
                        row(class = "gds-files-permissions-check") {
                            Checkbox(
                                node_ref = rec_ref,
                                checked = checked,
                                disabled = disabled,
                                labelled_by = rec_label,
                            )
                            Label(node_ref = rec_label, control = rec_ref) {{i18n::text("permissions-recursive")}}
                        }
                        {move || {
                            rec_sig.get().then(|| {
                                view! {
                                    text(class = "gds-files-permissions-note") {{i18n::text("permissions-recursive-hint")}}
                                }
                                .into_view()
                            })
                        }}
                    }
                }
                .into_view()
            })
        };

        AnyView::new(view! {
            column(class = "gds-files-permissions-grid") {
                column(class = "gds-files-permissions-table") {
                    {owner_group}
                    {group_group}
                    {other_group}
                }
                {recursive_view}
                {move || {
                    if loading_sig.get() {
                        Some(AnyView::new(view! {
                            text(class = "gds-files-permissions-note") {{i18n::text("permissions-loading")}}
                        }))
                    } else if saving_sig.get() {
                        Some(AnyView::new(view! {
                            text(class = "gds-files-permissions-note") {{i18n::text("permissions-saving")}}
                        }))
                    } else {
                        None
                    }
                }}
                {move || {
                    error_sig.get().map(|err| {
                        view! {
                            text(class = "gds-files-permissions-error") {{err}}
                        }
                        .into_view()
                    })
                }}
            }
        })
    }
}
