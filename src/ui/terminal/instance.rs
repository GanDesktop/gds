//! GPUI ownership, focus and rendering for a terminal service.
use super::{Event, INPUT_CONTEXT, Preferences, view::View};
use crate::service::terminal::{self as service, Shutdown, SpawnOptions, Status};
use crate::{appearance::theme, i18n};
use gds::app::AppExt as _;
use gpui_kit::{prelude::*, *};
use std::path::{Path, PathBuf};
use tokio::sync::watch;

#[cfg(all(test, feature = "gui-tests", unix))]
mod tests;

/// One terminal surface; the service owns startup, process state and cleanup.
pub struct Terminal {
    scope: crate::service::task_manager::Owner,
    service: service::Terminal,
    preferences: Preferences,
    focus: FocusHandle,
    view: Option<Entity<View>>,
    updates: Option<Subscription>,
    _theme: Subscription,
    shutdown_done: watch::Receiver<bool>,
    observed: Option<(Status, Option<SharedString>)>,
}
impl Terminal {
    pub fn new(
        cwd: PathBuf,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let preferences = preferences.normalized();
        let options = SpawnOptions {
            cwd,
            shell: preferences.shell.clone(),
            args: preferences.shell_args.clone(),
            login: preferences.login_shell,
            columns: 80,
            rows: 24,
            scrollback: preferences.scrollback_lines,
            palette: Some(super::view::palette::Palette::new(cx).query_colors()),
        };
        let scope = cx
            .gds()
            .tasks()
            .scope()
            .named_child(format_args!("Terminal · {}", options.cwd.display()));
        let (service, launch) = service::Terminal::prepare((*scope).clone(), options);
        let cancel = scope.token();
        let cleanup_service = service.clone();
        let (finished, done) = watch::channel(false);
        scope.spawn(async move {
            cancel.cancelled().await;
            cleanup_service.shutdown().await;
            finished.send_replace(true);
        });
        let ready = service.ready();
        scope.spawn_blocking(move || launch.run());
        cx.spawn_in(window, async move |terminal, cx| {
            let session = ready.await;
            let _ = terminal.update_in(cx, |this, window, cx| {
                if !this.service.is_closing() {
                    if let Some(session) = session {
                        let view =
                            cx.new(|cx| View::new(session, this.preferences.clone(), window, cx));
                        this.updates = Some(cx.observe(&view, |this, _, cx| this.changed(cx)));
                        if this.focus.is_focused(window) {
                            view.read(cx).focus_handle().focus(window, cx);
                        }
                        this.view = Some(view);
                    }
                }
                this.changed(cx);
            });
        })
        .detach();
        Self {
            scope,
            service,
            preferences,
            focus: cx.focus_handle(),
            view: None,
            updates: None,
            _theme: cx.observe_global::<theme::Colors>(|this, cx| {
                this.service
                    .set_palette(super::view::palette::Palette::new(cx).query_colors());
                if let Some(view) = &this.view {
                    view.update(cx, |_, cx| cx.notify());
                }
                cx.notify();
            }),
            shutdown_done: done,
            observed: None,
        }
    }
    pub fn cwd(&self) -> &Path {
        self.service.cwd()
    }
    pub fn status(&self, _: &App) -> Status {
        self.service.status()
    }
    pub fn is_running(&self, cx: &App) -> bool {
        self.status(cx).is_running()
    }
    /// # Errors
    /// Reports an unavailable session, invalid path or unverified shell input.
    pub fn change_directory(
        &self,
        cwd: PathBuf,
        _: &App,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static + use<> {
        let session = (!self.service.is_closing())
            .then(|| self.service.session())
            .flatten();
        async move {
            let session = session.ok_or(service::Error::ShellBusy)?;
            session.change_directory(&cwd).await?;
            Ok(())
        }
    }
    pub fn title(&self, _: &App) -> Option<SharedString> {
        self.service
            .title()
            .filter(|title| !title.is_empty())
            .map(Into::into)
    }
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus_handle(cx).focus(window, cx);
    }
    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
            || self.focus_handle(cx).contains_focused(window, cx)
    }
    /// Paste quoted file paths without submitting the command or modifying the clipboard.
    pub fn paste_paths(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.view.clone() else {
            return;
        };
        match crate::service::terminal::input::path_text(paths) {
            Ok(text) => {
                view.update(cx, |view, cx| view.paste_text(&text, cx));
                self.focus(window, cx);
            }
            Err(error) => tracing::warn!(%error, "cannot paste file paths into terminal"),
        }
    }

    /// Shell changes affect future sessions; existing processes are never restarted.
    pub fn set_preferences(&mut self, preferences: Preferences, cx: &mut Context<Self>) {
        let preferences = preferences.normalized();
        if self.preferences == preferences {
            return;
        }
        self.preferences = preferences;
        if let Some(view) = &self.view {
            view.update(cx, |view, cx| {
                view.set_preferences(self.preferences.clone(), cx)
            });
        }
        cx.notify();
    }
    /// Wait for the service's cleanup even if startup has not completed.
    pub fn shutdown(&mut self, cx: &mut Context<Self>) -> Shutdown {
        self.scope.cancel();
        // Request termination now; the tracked supervisor owns completion.
        drop(self.service.shutdown());
        let done = self.shutdown_done.clone();
        let mut completed = done.clone();
        cx.spawn(async move |terminal, cx| {
            let _ = completed.wait_for(|done| *done).await;
            let _ = terminal.update(cx, |this, cx| this.changed(cx));
        })
        .detach();
        self.changed(cx);
        wait_for_cleanup(done)
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        let state = (self.status(cx), self.title(cx));
        if self.observed.as_ref() != Some(&state) {
            self.observed = Some(state);
            cx.emit(Event::Changed);
        }
        cx.notify();
    }
}
fn wait_for_cleanup(mut done: watch::Receiver<bool>) -> Shutdown {
    Box::pin(async move {
        let _ = done.wait_for(|done| *done).await;
    })
}
impl Focusable for Terminal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.view
            .as_ref()
            .map_or_else(|| self.focus.clone(), |view| view.read(cx).focus_handle())
    }
}
impl EventEmitter<Event> for Terminal {}
impl Render for Terminal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.view {
            Some(view) => view.clone().into_any_element(),
            None => {
                let state = self.service.status();
                let (label, detail) = match state {
                    Status::Starting => ("gds-terminal-starting", None),
                    Status::Failed(error) => ("gds-terminal-start-failed", Some(error.clone())),
                    Status::Closed => ("gds-terminal-exited", None),
                    _ => ("gds-terminal-stopping", None),
                };
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .key_context(INPUT_CONTEXT)
                    .track_focus(&self.focus)
                    .bg(theme::content_background(cx))
                    .text_color(theme::Colors::current(cx).foreground)
                    .child(i18n::text(label, cx))
                    .when_some(detail, |this, detail| {
                        this.child(
                            div()
                                .text_color(theme::Colors::current(cx).error)
                                .child(detail),
                        )
                    })
                    .into_any_element()
            }
        }
    }
}
