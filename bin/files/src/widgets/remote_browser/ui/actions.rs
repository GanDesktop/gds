use super::*;
use crate::shared::lib::clipboard;
use crate::shared::lib::clipboard::ClipboardExt as _;
use gds::app::AppExt as _;
use gds::ui::DialogActions;
use gds_uim::el;
use gpui_kit::component::WindowExt;
use tokio::sync::watch;

#[derive(Debug, thiserror::Error)]
enum PasteError {
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error("invalid local file URI")]
    LocalUri,
    #[error(transparent)]
    Remote(#[from] gds::service::remote::Error),
    #[error("{0}")]
    Unconnected(SharedString),
}

impl View {
    pub(super) fn name_prompt(
        &mut self,
        directory: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() || (!directory && self.selected.len() != 1) {
            return;
        }
        let source = self.selected.first().cloned();
        let current = if directory {
            String::new()
        } else {
            source.as_ref().map_or(String::new(), Address::name)
        };
        let protocol = self.address().protocol();
        let folder_name = self.address().name();
        let input =
            cx.new(|cx| name_prompt::Form::new(current.clone(), protocol, directory, window, cx));
        let focus = input.clone();
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let title = if directory {
                t!(@cx, "new-folder-title", folder = folder_name.clone())
            } else {
                t!(@cx, "rename-title", name = current.clone())
            };
            let description = if directory {
                t!(@cx, "new-folder-message")
            } else {
                t!(@cx, "rename-message")
            };
            let footer = el! { .DialogActions::default {
                primary_label: $(if directory {
                    t!(@cx, "new-folder-confirm")
                } else {
                    t!(@cx, "rename-confirm")
                });
                primary_icon: $(if directory {
                    IconName::FolderPlus
                } else {
                    IconName::Pencil
                });
                cancel_label: $(t!(@cx, "operation-cancel"));
            } };
            let content = input.clone();
            let on_ok =
                Self::name_prompt_confirm(view.clone(), input.clone(), source.clone(), directory);
            let on_close = Self::dialog_close(view.clone());
            el! {
                dialog {
                    confirm;
                    width: $(px(460.));
                    title: title;
                    description: description;
                    footer: footer;
                    on_ok: on_ok;
                    on_close: on_close;
                } [
                    content,
                ]
            }
        });
        window.on_next_frame(move |window, cx| {
            focus.update(cx, |form, cx| form.focus(window, cx));
        });
    }

    fn name_prompt_confirm(
        view: WeakEntity<Self>,
        input: Entity<name_prompt::Form>,
        source: Option<Address>,
        directory: bool,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, window, cx| {
            let valid = input.update(cx, |form, cx| form.validate(cx));
            if !valid {
                return false;
            }
            let name = input.read(cx).value(cx);
            let result = view.update(cx, |view, cx| {
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
                let label = if directory {
                    t!(@cx, "new-folder-working")
                } else {
                    t!(@cx, "rename-working")
                };
                view.options.focus.focus(window, cx);
                view.mutate(
                    if directory {
                        "Create remote folder"
                    } else {
                        "Rename remote file"
                    },
                    label,
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
        }
    }

    fn dialog_close(view: WeakEntity<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            let _ = view.update(cx, |view, cx| {
                view.options.focus.focus(window, cx);
            });
        }
    }

    fn mutate(
        &mut self,
        name: &str,
        label: SharedString,
        work: impl Future<Output = Result<(), gds::service::remote::Error>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.message = None;
        let work = self.tasks.spawn_named(name, work);
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.job = None;
                match result {
                    Ok(Ok(())) => cx.emit(Event::Refresh),
                    Ok(Err(error)) => this.message = Some(error.to_string()),
                    Err(error) => this.message = Some(error.to_string()),
                }
                cx.notify();
            });
        });
        self.job = Some(Job::Mutating { label, _task: task });
        cx.notify();
    }

    pub(super) fn upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some(t!(@cx, "remote-upload").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = paths.await else {
                return;
            };
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
            prompt: Some(t!(@cx, "remote-download").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = paths.await else {
                return;
            };
            let _ = this.update_in(cx, |this, _, cx| match result {
                Ok(Some(paths)) => {
                    if let Some(destination) = paths.first() {
                        let open_path = if open {
                            addresses
                                .first()
                                .map(Address::name)
                                .filter(|name| !name.is_empty())
                                .map(|name| destination.join(name))
                        } else {
                            None
                        };
                        let sources = addresses
                            .into_iter()
                            .map(|address| operations::Resource::Remote {
                                address,
                                session: this.options.session.clone(),
                            })
                            .collect::<Vec<_>>();
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
        if self.busy() || sources.is_empty() {
            return;
        }
        let cancel = self.tasks.token().child_token();
        let (sender, receiver) = watch::channel(Default::default());
        let work = operations::copy(
            (*self.tasks).clone(),
            sources,
            target,
            cancel.clone(),
            sender,
        );
        self.run_job(work, receiver, cancel, open, cx);
    }

    pub(super) fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() || self.busy() {
            return;
        }
        let sources = self.resources();
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let footer = el! { .DialogActions::default {
                danger;
                cancel_label: $(t!(@cx, "operation-cancel"));
            } };
            let on_ok = Self::delete_confirm(view.clone(), sources.clone());
            let on_close = Self::dialog_close(view.clone());
            el! {
                dialog {
                    confirm;
                    title: $(t!(@cx, "delete-permanently"));
                    description: $(t!(@cx, "remote-delete-warning"));
                    footer: footer;
                    on_ok: on_ok;
                    on_close: on_close;
                }
            }
        });
    }

    fn delete_confirm(
        view: WeakEntity<Self>,
        sources: Vec<operations::Resource>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, window, cx| {
            let _ = view.update(cx, |view, cx| {
                view.options.focus.focus(window, cx);
                let cancel = view.tasks.token().child_token();
                let (sender, receiver) = watch::channel(Default::default());
                let work = operations::delete(sources.clone(), cancel.clone(), sender);
                view.run_job(work, receiver, cancel, None, cx);
            });
            true
        }
    }

    fn run_job(
        &mut self,
        work: impl Future<Output = operations::Report> + Send + 'static,
        mut progress: watch::Receiver<operations::Progress>,
        cancel: CancellationToken,
        open: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.message = None;
        let progress_task = cx.spawn(async move |this, cx| {
            while progress.changed().await.is_ok() {
                let value = progress.borrow_and_update().clone();
                if this
                    .update(cx, |this, cx| {
                        if let Some(Job::Transferring { progress, .. }) = &mut this.job {
                            *progress = value;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        // Cancellation completes protocol close/cleanup before the Tokio task is dropped.
        let work = self.tasks.spawn_named("Transfer remote files", work);
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.job = None;
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
                                messages.push(t!(@cx, "remote-cancelled").into());
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
        });
        self.job = Some(Job::Transferring {
            cancel,
            progress: Default::default(),
            _progress_task: progress_task,
            _task: task,
        });
        cx.notify();
    }

    fn open_download(&self, path: PathBuf, cx: &mut Context<Self>) {
        let desktop = self.options.desktop.clone();
        let work = self.tasks.spawn_named("Open downloaded file", async move {
            desktop
                .open_file(&path, desktop::OpenMode::DefaultApplication)
                .await
        });
        cx.spawn(async move |_, cx| match work.await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                let _ = cx.update(|cx| notifications::error(error.to_string(), cx));
            }
            Err(error) => {
                let _ = cx.update(|cx| notifications::error(error.to_string(), cx));
            }
        })
        .detach();
    }

    pub(super) fn copy_uris(&self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        let work = cx.gds_mut().clipboard().write_uris(
            self.selected
                .iter()
                .map(|address| address.uri().to_owned())
                .collect(),
        );
        cx.spawn(async move |_, cx| {
            if let Err(error) = work.await {
                let _ = cx.update(|cx| notifications::error(error.to_string(), cx));
            }
        })
        .detach();
    }

    /// # Errors
    /// Returns [`PasteError`] if the URI is malformed, refers to a non-local `file:` URI,
    /// or references a remote server without an active session.
    fn resolve_paste_source(
        &self,
        uri: &str,
        cx: &App,
    ) -> Result<operations::Resource, PasteError> {
        let parsed = url::Url::parse(uri)?;
        if parsed.scheme() == "file" {
            return parsed
                .to_file_path()
                .map(operations::Resource::Local)
                .map_err(|()| PasteError::LocalUri);
        }
        let address = Address::parse(parsed.as_str())?;
        let session = self
            .options
            .connector
            .read(cx)
            .session(&address)
            .ok_or_else(|| PasteError::Unconnected(t!(@cx, "remote-connect-source")))?;
        Ok(operations::Resource::Remote { address, session })
    }

    pub(super) fn paste(&mut self, cx: &mut Context<Self>) {
        let work = cx.gds_mut().clipboard().read_uris();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let uris = match result {
                    Ok(Some((clipboard::Mode::Copy, uris))) => uris,
                    Ok(Some((clipboard::Mode::Cut, _))) => {
                        this.message = Some(t!(@cx, "remote-cut-unavailable").into());
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
                    .iter()
                    .map(|uri| this.resolve_paste_source(uri, cx))
                    .collect::<Result<Vec<_>, PasteError>>();
                match sources {
                    Ok(sources) => this.start_copy(sources, this.target(), cx),
                    Err(error) => {
                        this.message = Some(error.to_string());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}
