use super::*;
use crate::shared::lib::clipboard;
use gds::ui::DialogActions;
use gds_uim::el;
use gpui_kit::component::WindowExt;
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

impl View {
    pub(super) fn name_prompt(
        &mut self,
        directory: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy || (!directory && self.selected.len() != 1) {
            return;
        }
        let source = self.selected.first().cloned();
        let current = if directory {
            String::new()
        } else {
            source.as_ref().map_or(String::new(), Address::name)
        };
        let input = cx.new(|cx| name_prompt::Form::new(current.clone(), window, cx));
        let focus = input.clone();
        let view = cx.entity().downgrade();
        let title = i18n::text(if directory { "new-folder" } else { "rename" }, cx);
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let input = input.clone();
            let view = view.clone();
            let source = source.clone();
            let current = current.clone();
            let footer = el! { .DialogActions::default {
                primary_label: $(i18n::text(
                    if directory {
                        "new-folder-confirm"
                    } else {
                        "rename-confirm"
                    },
                    cx,
                ));
                primary_icon: $(if directory {
                    gpui_kit::assets::IconName::FolderPlus
                } else {
                    gpui_kit::assets::IconName::Pencil
                });
            } };
            let content = input.clone();
            let on_ok = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let name = input.read(cx).value(cx);
                let result = view.update(cx, |view, cx| {
                    if name == current && !directory {
                        return true;
                    }
                    if let Err(error) = Address::validate_name(&name, view.address().protocol()) {
                        notifications::error(error.to_string(), cx);
                        return false;
                    }
                    let parent = if directory {
                        Some(view.address().clone())
                    } else {
                        source.as_ref().and_then(Address::parent)
                    };
                    let Some(parent) = parent else {
                        return false;
                    };
                    let target = match parent.child(&name) {
                        Ok(target) => target,
                        Err(error) => {
                            notifications::error(error.to_string(), cx);
                            return false;
                        }
                    };
                    let session = view.options.session.clone();
                    let source = source.clone();
                    view.mutate(
                        async move {
                            if directory {
                                session.mkdir(&target).await
                            } else {
                                session
                                    .rename(&source.expect("single rename source"), &target)
                                    .await
                            }
                        },
                        cx,
                    );
                    true
                });
                result.unwrap_or(true)
            };
            el! {
                dialog {
                    confirm;
                    width: $(px(460.));
                    title: $(title.clone());
                    footer: footer;
                    on_ok: on_ok;
                } [
                    content,
                ]
            }
        });
        window.on_next_frame(move |window, cx| {
            focus.update(cx, |form, cx| form.focus(window, cx));
        });
    }

    fn mutate(
        &mut self,
        work: impl Future<Output = Result<(), gds::service::remote::Error>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.cancellable = false;
        self.message = None;
        let work = AbortOnDropHandle::new(self.options.runtime.spawn(work));
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| v.map_err(|e| e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => cx.emit(Event::Refresh),
                    Err(error) => this.message = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some(i18n::text("remote-upload", cx).into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = async { paths.await? }.await;
            let _ = this.update_in(cx, |this, _, cx| match result {
                Ok(Some(paths)) => this.start_copy(
                    paths.into_iter().map(operations::Resource::Local).collect(),
                    this.target(),
                    cx,
                ),
                Ok(None) => (),
                Err(error) => {
                    this.message = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn download(
        &mut self,
        addresses: Vec<Address>,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if addresses.is_empty() {
            return;
        }
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(i18n::text("remote-download", cx).into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = async { paths.await? }.await;
            let _ = this.update_in(cx, |this, _, cx| match result {
                Ok(Some(paths)) => {
                    if let Some(destination) = paths.first() {
                        let sources = addresses
                            .into_iter()
                            .map(|address| operations::Resource::Remote {
                                address,
                                session: this.options.session.clone(),
                            })
                            .collect::<Vec<_>>();
                        let open_path = open.then(|| destination.join(sources_name(&sources)));
                        this.start_copy_then_open(
                            sources,
                            operations::Resource::Local(destination.clone()),
                            open_path,
                            cx,
                        );
                    }
                }
                Ok(None) => (),
                Err(error) => {
                    this.message = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn start_copy(
        &mut self,
        sources: Vec<operations::Resource>,
        target: operations::Resource,
        cx: &mut Context<Self>,
    ) {
        self.start_copy_then_open(sources, target, None, cx);
    }
    fn start_copy_then_open(
        &mut self,
        sources: Vec<operations::Resource>,
        target: operations::Resource,
        open: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.busy || sources.is_empty() {
            return;
        }
        self.cancel = CancellationToken::new();
        let (sender, receiver) = watch::channel(Default::default());
        let work = operations::copy(sources, target, self.cancel.clone(), sender);
        self.run_job(work, receiver, open, cx);
    }

    pub(super) fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() || self.busy {
            return;
        }
        let sources = self.resources();
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let sources = sources.clone();
            let view = view.clone();
            let footer = el! { .DialogActions::default {
                danger;
            } };
            let on_ok = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |view, cx| {
                    view.cancel = CancellationToken::new();
                    let (sender, receiver) = watch::channel(Default::default());
                    let work = operations::delete(sources.clone(), view.cancel.clone(), sender);
                    view.run_job(work, receiver, None, cx);
                });
                true
            };
            el! {
                dialog {
                    confirm;
                    title: $(i18n::text("delete-permanently", cx));
                    description: $(i18n::text("remote-delete-warning", cx));
                    footer: footer;
                    on_ok: on_ok;
                }
            }
        });
    }

    fn run_job(
        &mut self,
        work: impl Future<Output = operations::Report> + Send + 'static,
        mut progress: watch::Receiver<operations::Progress>,
        open: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.cancellable = true;
        self.message = None;
        self.progress = Default::default();
        self.progress_task = Some(cx.spawn(async move |this, cx| {
            while progress.changed().await.is_ok() {
                let value = progress.borrow_and_update().clone();
                if this
                    .update(cx, |this, cx| {
                        this.progress = value;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        // Cancellation completes protocol close/cleanup before the Tokio task is dropped.
        let work = self.options.runtime.spawn(work);
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(report) => {
                        let failed = !report.failures.is_empty() || report.cancelled;
                        if failed {
                            let mut messages = report
                                .failures
                                .into_iter()
                                .map(|(path, error)| format!("{path}: {error}"))
                                .collect::<Vec<_>>();
                            if report.cancelled {
                                messages.push(i18n::text("remote-cancelled", cx).into());
                            }
                            let message = messages.join("\n");
                            notifications::error(message.clone(), cx);
                            this.message = Some(message);
                        } else if let Some(path) = open {
                            this.open_download(path, cx);
                        }
                        if !report.completed.is_empty() {
                            cx.emit(Event::Refresh);
                        }
                    }
                    Err(error) => this.message = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn open_download(&self, path: PathBuf, cx: &mut Context<Self>) {
        let desktop = self.options.desktop.clone();
        let work = self.options.runtime.spawn(async move {
            desktop
                .open_file(&path, desktop::OpenMode::DefaultApplication)
                .await
        });
        cx.spawn(async move |_, cx| {
            let result = work
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| v.map_err(|e| e.to_string()));
            if let Err(error) = result {
                let _ = cx.update(|cx| notifications::error(error, cx));
            }
        })
        .detach();
    }

    pub(super) fn copy_uris(&self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        let work = clipboard::Bridge::new(cx).write_uris(
            self.selected.iter().map(|a| a.uri().to_owned()).collect(),
            cx,
        );
        cx.spawn(async move |_, cx| {
            if let Err(error) = work.await {
                let _ = cx.update(|cx| notifications::error(error.to_string(), cx));
            }
        })
        .detach();
    }

    pub(super) fn paste(&mut self, cx: &mut Context<Self>) {
        let work = clipboard::Bridge::new(cx).read_uris(cx);
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let uris = match result {
                    Ok(Some((clipboard::Mode::Copy, uris))) => uris,
                    Ok(Some((clipboard::Mode::Cut, _))) => {
                        this.message = Some(i18n::text("remote-cut-unavailable", cx).into());
                        cx.notify();
                        return;
                    }
                    Ok(None) => return,
                    Err(error) => {
                        this.message = Some(error.to_string());
                        cx.notify();
                        return;
                    }
                };
                let sources = uris
                    .into_iter()
                    .map(|uri| {
                        let uri = url::Url::parse(&uri).map_err(|e| e.to_string())?;
                        if uri.scheme() == "file" {
                            return uri
                                .to_file_path()
                                .map(operations::Resource::Local)
                                .map_err(|_| "Invalid local file URI".to_owned());
                        }
                        let address = Address::parse(uri.as_str()).map_err(|e| e.to_string())?;
                        let session = this
                            .options
                            .connector
                            .read(cx)
                            .session(&address)
                            .ok_or_else(|| i18n::text("remote-connect-source", cx).to_string())?;
                        Ok(operations::Resource::Remote { address, session })
                    })
                    .collect::<Result<Vec<_>, String>>();
                match sources {
                    Ok(sources) => this.start_copy(sources, this.target(), cx),
                    Err(error) => {
                        this.message = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

fn sources_name(sources: &[operations::Resource]) -> String {
    sources
        .first()
        .and_then(|r| r.name().ok())
        .unwrap_or_default()
}
