//! The controller owns the PTY; mounting and hiding views does not restart it.
use super::{Preferences, view::Palette};
use crate::service::{
    task_manager::{Owner as TaskOwner, Scope},
    terminal::{self as service, Shutdown, SpawnOptions, Status},
};
use anti_webui::prelude::*;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const OUTPUT_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone)]
pub(super) struct Redraw {
    pub revision: Signal<u64>,
    pending: Rc<Cell<bool>>,
}

impl Redraw {
    pub fn request(&self) {
        if self.pending.replace(true) {
            return;
        }
        let pending = self.pending.clone();
        let revision = self.revision;
        request_frame(move |_| {
            pending.set(false);
            if revision.is_alive() {
                revision.update(|value| *value = value.wrapping_add(1));
            }
        });
    }
}

pub(super) struct Controller {
    pub service: service::Terminal,
    pub preferences: Signal<Preferences>,
    pub redraw: Redraw,
    status: Signal<Status>,
    title: Signal<Option<String>>,
    pub error: Signal<Option<String>>,
    pub focus: Cell<Option<NodeRef>>,
    pub focused: Signal<bool>,
    pub focus_requested: Cell<bool>,
    pub scope: TaskOwner,
    owner: Owner,
}
#[derive(Clone)]
pub struct Terminal(pub(super) Rc<Controller>);
impl Drop for Controller {
    fn drop(&mut self) {
        self.scope.cancel();
        self.owner.dispose();
    }
}
impl Terminal {
    pub fn new(cwd: PathBuf, preferences: Preferences, scope: Scope) -> Self {
        let (controller, _) = anti_webui::core::reactive::create_child_owner(|| {
            let preferences = preferences.normalized();
            let scope = scope.named_child(format_args!("Terminal · {}", cwd.display()));
            let options = SpawnOptions {
                cwd,
                shell: preferences.shell.clone(),
                args: preferences.shell_args.clone(),
                login: preferences.login_shell,
                scrollback: preferences.scrollback_lines,
                palette: Some(Palette::new().query_colors()),
                ..SpawnOptions::default()
            };
            let (service, launch) = service::Terminal::prepare((*scope).clone(), options);
            let redraw = Redraw {
                revision: signal(0),
                pending: Rc::new(Cell::new(false)),
            };
            let status = signal(Status::Starting);
            let title = signal(None);
            let error = signal(None);
            let observing = service.clone();
            let pending = Arc::new(AtomicBool::new(false));
            let painting = redraw.clone();
            let metadata = service.clone();
            let sender = ui_channel({
                let pending = pending.clone();
                move |()| {
                    pending.store(false, Ordering::Release);
                    status.set_if_changed(metadata.status());
                    title.set_if_changed(metadata.title().filter(|title| !title.is_empty()));
                    painting.request();
                }
            });
            scope
                .spawn_blocking_named("Launch terminal", move || launch.run())
                .on_complete(&scope, |result| {
                    if let Err(error) = result {
                        tracing::error!(%error, "terminal launch worker failed");
                    }
                });
            scope
                .spawn_cancellable_named("Observe terminal", async move {
                    let publish = || {
                        if !pending.swap(true, Ordering::AcqRel) {
                            sender.send(());
                        }
                    };
                    let Some(session) = observing.ready().await else {
                        publish();
                        return;
                    };
                    let mut updates = session.updates();
                    publish();
                    while !*updates.borrow() && updates.changed().await.is_ok() {
                        tokio::time::sleep(OUTPUT_INTERVAL).await;
                        let stopped = *updates.borrow_and_update();
                        publish();
                        if stopped {
                            break;
                        }
                    }
                    publish();
                })
                .on_complete(&scope, |result| {
                    if let Err(error) = result {
                        tracing::debug!(%error, "terminal observer stopped");
                    }
                });
            let cancel = scope.token();
            let cleanup = service.clone();
            scope.spawn_named("Shut down terminal", async move {
                cancel.cancelled().await;
                cleanup.shutdown().await;
            });
            let styled = service.clone();
            effect(move || {
                styled.set_palette(Palette::new().query_colors());
            });
            Controller {
                service,
                preferences: signal(preferences),
                redraw,
                status,
                title,
                error,
                focus: Cell::new(None),
                focused: signal(false),
                focus_requested: Cell::new(false),
                scope,
                owner: Owner::current().expect("terminal controller owner"),
            }
        });
        Self(Rc::new(controller))
    }
    pub fn cwd(&self) -> &Path {
        self.0.service.cwd()
    }
    pub fn status(&self) -> Status {
        self.0.status.get()
    }
    pub fn is_running(&self) -> bool {
        self.status().is_running()
    }
    pub fn title(&self) -> Option<String> {
        self.0.title.get()
    }
    pub fn status_label(&self) -> String {
        use crate::i18n;
        match self.status() {
            Status::Starting => i18n::text("gds-terminal-starting"),
            Status::Running => i18n::text("gds-terminal-running"),
            Status::Stopping => i18n::text("gds-terminal-stopping"),
            Status::Closed => i18n::text("gds-terminal-exited"),
            Status::Failed(error) => {
                format!("{}: {error}", i18n::text("gds-terminal-start-failed"))
            }
            Status::Exited { code, signal } => i18n::t!(
                "gds-terminal-exit-status",
                code = code,
                signal = signal.unwrap_or_default()
            ),
        }
    }
    pub fn error(&self) -> Option<String> {
        self.0.error.get()
    }
    pub fn focus(&self) {
        if !self.0.focused.is_alive() {
            return;
        }
        self.0.focus_requested.set(!self.0.focused.get_untracked());
        if self.0.service.status() != Status::Starting
            && let Some(focus) = self.0.focus.get()
        {
            focus.focus();
        }
    }
    pub fn is_focused(&self) -> bool {
        self.0.focused.get_untracked()
    }
    pub fn set_preferences(&self, preferences: Preferences) {
        self.0.preferences.set_if_changed(preferences.normalized());
    }
    pub fn shutdown(&self) -> Shutdown {
        self.0.scope.cancel();
        self.0.service.shutdown()
    }
    /// # Errors
    /// Reports an unavailable session or a shell that cannot safely accept directory input.
    pub fn change_directory(
        &self,
        path: PathBuf,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static + use<> {
        let session = self.0.service.session();
        async move {
            session
                .ok_or(service::Error::ShellBusy)?
                .change_directory(&path)
                .await?;
            Ok(())
        }
    }
    pub fn paste_paths(&self, paths: &[PathBuf]) {
        match service::input::path_text(paths) {
            Ok(text) => self.paste_text(&text),
            Err(error) => self.0.error.set(Some(error.to_string())),
        }
    }
    pub fn paste_text(&self, text: &str) {
        if let Some(session) = self.0.service.session() {
            let bytes = session.with_term(|term| service::input::paste_bytes(text, *term.mode()));
            self.send(&bytes);
        }
    }
    pub(super) fn send(&self, bytes: &[u8]) {
        if let Some(session) = self.0.service.session() {
            match session.write(bytes) {
                Ok(()) => self.0.error.set_if_changed(None),
                Err(error) => self.0.error.set_if_changed(Some(error.to_string())),
            }
        }
    }
    pub(super) fn changed(&self) {
        self.0.redraw.request();
    }
}
