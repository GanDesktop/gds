//! A staged permission editor; only Save writes to the filesystem.

use super::Browser;
use crate::{features::file_operations::permissions, shared::i18n::t};
use gds::{i18n, ui::DialogActions, ui::notifications};
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Disableable, Icon, StyledExt, WindowExt, checkbox::Checkbox},
    prelude::*,
    *,
};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

const DIALOG_WIDTH: f32 = 520.;

struct Editor {
    path: PathBuf,
    mode: Option<u32>,
    error: Option<SharedString>,
    saving: bool,
    is_directory: bool,
    recursive: bool,
    browser: WeakEntity<Browser>,
    origin_tab: u64,
    tasks: gds::service::task_manager::Scope,
    cancel: CancellationToken,
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Editor {
    fn load(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let work = self
            .tasks
            .spawn_blocking_named("Read file permissions", move || {
                let mode = permissions::read(&path)?;
                let is_directory = std::fs::symlink_metadata(&path)?.is_dir();
                Ok::<_, permissions::Error>((mode, is_directory))
            });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok((mode, is_directory))) => {
                        this.mode = Some(mode);
                        this.is_directory = is_directory;
                    }
                    Ok(Err(error)) => this.error = Some(Self::file_error(error, cx)),
                    Err(error) => this.error = Some(error.to_string().into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn file_error(error: permissions::Error, cx: &App) -> SharedString {
        match error {
            permissions::Error::Symlink => i18n::text("permissions-symlink", cx),
            error => error.to_string().into(),
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.mode.filter(|_| !self.saving) else {
            return;
        };
        self.saving = true;
        self.error = None;
        let path = self.path.clone();
        let recursive = self.recursive;
        let cancel = self.cancel.child_token();
        let work = self
            .tasks
            .spawn_blocking_named("Change file permissions", move || {
                permissions::apply_cancellable(&path, mode, recursive, &cancel)
            });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(report) => {
                        if let Some(browser) = this.browser.upgrade() {
                            browser.update(cx, |browser, cx| {
                                browser.refresh_after_file_operation(&[this.path.clone()], cx);
                            });
                        }
                        if report.failures.is_empty() {
                            this.release(window, cx);
                            window.close_dialog(cx);
                            notifications::success(i18n::text("permissions-saved", cx), cx);
                        } else {
                            let summary = t!(
                                @cx,
                                "permissions-partial",
                                succeeded = report.succeeded as i64,
                                failed = report.failures.len() as i64,
                            );
                            this.error = Some(summary.clone());
                            let mut notice =
                                notifications::Report::new(notifications::Level::Error, summary);
                            for failure in report.failures {
                                notice = notice.detail(format!(
                                    "{}: {}",
                                    failure.path.display(),
                                    Self::file_error(failure.error, cx)
                                ));
                            }
                            notice.show(cx);
                        }
                    }
                    Err(error) => this.error = Some(error.to_string().into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn release(&self, window: &mut Window, cx: &mut App) {
        self.cancel.cancel();
        if let Some(browser) = self.browser.upgrade() {
            browser.update(cx, |browser, cx| {
                browser.operations.in_progress = false;
                cx.notify();
            });
        }
        Browser::restore_file_focus(self.browser.clone(), self.origin_tab, window);
    }

    fn permission_group(
        &self,
        title: &'static str,
        shift: u32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut checks = Vec::new();
        for (label, bit) in [
            ("permissions-read", 0o4),
            ("permissions-write", 0o2),
            ("permissions-execute", 0o1),
        ] {
            let mask = bit << shift;
            let checked = self.mode.is_some_and(|mode| mode & mask != 0);
            let disabled = self.mode.is_none() || self.saving;
            let on_change = cx.listener(move |this, checked: &bool, _, cx| {
                if !this.saving
                    && let Some(mode) = this.mode.as_mut()
                {
                    if *checked {
                        *mode |= mask;
                    } else {
                        *mode &= !mask;
                    }
                    cx.notify();
                }
            });
            let id: ElementId = format!("permission-{mask}").into();
            checks.push(el! {
                .div { flex_1; min_w_0 } [
                    id.Checkbox::new {
                        label: $(i18n::text(label, cx)); checked: checked; disabled: disabled;
                        accessibility_label: $(format!("{} — {}", i18n::text(title, cx), i18n::text(label, cx)));
                        on_change: on_change;
                    },
                ]
            });
        }
        el! {
            .div { flex; flex_col; w_full; min_w_0; gap_3; pb_4; border_b_1; border_color: $(cx.theme().border) } [
                .div { text_sm; font_semibold } [$(i18n::text(title, cx))],
                .div { flex; items_center; gap_4; w_full } [..checks],
            ]
        }
        .into_any_element()
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = [
            self.permission_group("permissions-owner", 6, cx),
            self.permission_group("permissions-group", 3, cx),
            self.permission_group("permissions-others", 0, cx),
        ];
        let status = if self.saving {
            Some(i18n::text("permissions-saving", cx))
        } else if self.mode.is_none() && self.error.is_none() {
            Some(i18n::text("permissions-loading", cx))
        } else {
            None
        };
        let on_recursive = cx.listener(|this, checked: &bool, _, cx| {
            if !this.saving {
                this.recursive = *checked;
                cx.notify();
            }
        });
        let error = self.error.clone();
        el! {
            .div { flex; flex_col; gap_4; w_full } [
                ..groups,
                @if (self.is_directory)
                    "permissions-recursive".Checkbox::new {
                        label: $(i18n::text("permissions-recursive", cx));
                        checked: $self.recursive; disabled: $self.saving;
                        on_change: on_recursive;
                    },
                    @if (self.recursive)
                        .div { text_sm; text_color: $(cx.theme().muted_foreground) } [$(i18n::text("permissions-recursive-hint", cx))],
                    @end,
                @end,
                @if (let Some(status) = status)
                    .div { text_sm; text_color: $(cx.theme().muted_foreground) } [status],
                @end,
                @if (let Some(error) = error)
                    .div { text_sm; text_color: $(cx.theme().danger) } [error],
                @end,
            ]
        }
    }
}

impl Browser {
    pub(super) fn edit_permissions(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.in_progress || window.has_active_dialog(cx) {
            return;
        }
        self.operations.in_progress = true;
        self.operations.paths = vec![path.clone()];
        let browser = cx.entity().downgrade();
        let tasks = self.window.tasks.clone();
        let origin_tab = self.tabs.active_id;
        let editor = cx.new(|cx| {
            let mut editor = Editor {
                path,
                mode: None,
                error: None,
                saving: false,
                is_directory: false,
                recursive: false,
                browser: browser.clone(),
                origin_tab,
                tasks,
                cancel: CancellationToken::new(),
            };
            editor.load(cx);
            editor
        });
        // Let the context menu dismiss before recording the dialog's return focus.
        window.on_next_frame(move |window, cx| {
            let Some(browser) = browser.upgrade() else {
                return;
            };
            browser.read(cx).window.focus.clone().focus(window, cx);
            window.open_alert_dialog(cx, move |dialog, _, cx| {
                let save = editor.clone();
                let cancel = editor.clone();
                let close = editor.clone();
                let footer = el! { .DialogActions::default {
                    primary_label: $(i18n::text("permissions-save", cx));
                    cancel_label: $(i18n::text("operation-cancel", cx));
                } };
                let on_ok = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    save.update(cx, |editor, cx| editor.save(window, cx));
                    false
                };
                let on_cancel = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    cancel.read(cx).cancel.cancel();
                    true
                };
                let on_close = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    close.update(cx, |editor, cx| editor.release(window, cx));
                };
                el! {
                    dialog {
                        confirm; width: DIALOG_WIDTH.px;
                        bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                        title: $(i18n::text("permissions-title", cx));
                        description: $(editor.read(cx).path.display().to_string());
                        icon: IconName::Lock.Icon::new;
                        footer: footer; on_ok: on_ok; on_cancel: on_cancel; on_close: on_close;
                    } [$(editor.clone())]
                }
            });
        });
        cx.notify();
    }
}
