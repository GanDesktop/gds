//! Adapts remote widgets to workspace navigation; owns no protocol implementation.
use super::*;
use crate::features::file_operations::history as operation_history;
use crate::{features::connect_server, widgets::remote_browser};
use crate::{features::file_operations::remote as operations, shared::lib::clipboard};
use gds::service::remote::{Address, Listing, Session};
use gds_uim::el;

#[derive(Debug, thiserror::Error)]
enum PasteError {
    #[error(transparent)]
    Clipboard(#[from] clipboard::Error),
    #[error(transparent)]
    Address(#[from] crate::entities::location::AddressError),
    #[error("{0}")]
    Message(SharedString),
}

pub(in crate::pages::workspace) struct State {
    pub connector: Entity<connect_server::Connector>,
    pub requests: std::collections::HashMap<u64, LoadRequest>,
}

impl State {
    pub(in crate::pages::workspace) fn new(tasks: Scope, cx: &mut App) -> Self {
        Self {
            connector: cx.new(|_| connect_server::Connector::new(tasks)),
            requests: std::collections::HashMap::new(),
        }
    }
}

pub(in crate::pages::workspace) struct Directory {
    #[allow(dead_code)]
    pub listing: Listing,
    #[allow(dead_code)]
    pub session: Session,
    pub view: Option<Entity<remote_browser::View>>,
    _subscriptions: Vec<Subscription>,
}

impl Browser {
    /// # Errors
    /// Returns [`PasteError`] if the URI is invalid or refers to an unconnected remote server.
    fn resolve_paste_source(
        &self,
        uri: &str,
        cx: &App,
    ) -> Result<operations::Resource, PasteError> {
        match Location::from_uri(uri)? {
            Location::Directory(path) => Ok(operations::Resource::Local(path.into())),
            Location::Remote(address) => {
                let session = self
                    .remote
                    .connector
                    .read(cx)
                    .session(&address)
                    .ok_or_else(|| PasteError::Message(i18n::text("remote-connect-source", cx)))?;
                Ok(operations::Resource::Remote { address, session })
            }
            _ => Err(PasteError::Address(
                crate::entities::location::AddressError::Invalid(uri.to_owned()),
            )),
        }
    }

    /// # Errors
    /// Returns [`PasteError`] when clipboard contents are missing, cut-mode, or invalid URIs.
    fn collect_paste_sources(
        &self,
        result: Result<Option<(clipboard::Mode, Vec<String>)>, clipboard::Error>,
        cx: &App,
    ) -> Result<Vec<operations::Resource>, PasteError> {
        let (mode, uris) =
            result?.ok_or_else(|| PasteError::Message(i18n::text("clipboard-no-files", cx)))?;
        if mode == clipboard::Mode::Cut {
            return Err(PasteError::Message(i18n::text(
                "remote-cut-unavailable",
                cx,
            )));
        }
        uris.iter()
            .map(|uri| self.resolve_paste_source(uri, cx))
            .collect()
    }

    pub(in crate::pages::workspace) fn paste_remote_uris(
        &mut self,
        result: Result<Option<(clipboard::Mode, Vec<String>)>, clipboard::Error>,
        destination: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let sources = match self.collect_paste_sources(result, cx) {
            Ok(sources) => sources,
            Err(error) => {
                self.operations.in_progress = false;
                self.finish_background(
                    notifications::Report::new(notifications::Level::Error, error.to_string()),
                    cx,
                );
                cx.notify();
                return;
            }
        };
        self.operations.paths = vec![destination.clone()];
        let cancel = self.background_tasks.read(cx).token("files");
        self.operations.remote_paste_cancel = Some(cancel.clone());
        let (updates, _) = tokio::sync::watch::channel(Default::default());
        let target = operations::Resource::Local(destination.clone());
        let work = self.window.tasks.spawn_named(
            "Copy remote files",
            operations::copy(self.window.tasks.clone(), sources, target, cancel, updates),
        );
        cx.spawn(async move |this, cx| {
            let report = work.await;
            let _ = this.update(cx, |this, cx| {
                this.operations.in_progress = false;
                this.operations.remote_paste_cancel = None;
                this.operations.paths.clear();
                let notice = match report {
                    Ok(report) => {
                        this.operations
                            .history
                            .record(operation_history::Entry::new(
                                operation_history::Kind::Copy,
                                report
                                    .completed
                                    .iter()
                                    .filter_map(|source| source.name().ok())
                                    .map(|name| {
                                        operation_history::Step::trash(destination.join(name))
                                    })
                                    .collect(),
                            ));
                        if !report.completed.is_empty() {
                            this.refresh_after_file_operation(&[destination], cx);
                        }
                        if report.cancelled {
                            notifications::Report::new(
                                notifications::Level::Warning,
                                i18n::text("remote-cancelled", cx).to_string(),
                            )
                        } else if report.failures.is_empty() {
                            notifications::Report::new(
                                notifications::Level::Success,
                                i18n::count("copy-done", report.completed.len(), cx).to_string(),
                            )
                        } else {
                            notifications::Report::new(
                                notifications::Level::Error,
                                report
                                    .failures
                                    .into_iter()
                                    .map(|(path, error)| format!("{path}: {error}"))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            )
                        }
                    }
                    Err(error) => {
                        notifications::Report::new(notifications::Level::Error, error.to_string())
                    }
                };
                this.finish_background(notice, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::pages::workspace) fn remote_view(&self) -> Option<Entity<remote_browser::View>> {
        if !matches!(self.current_location(), Location::Remote(_)) {
            return None;
        }
        self.current_tab()?.remote()?.view.clone()
    }

    pub(in crate::pages::workspace) fn install_remote_events(&mut self, cx: &mut Context<Self>) {
        self.window
            .subscriptions
            .push(cx.observe(&self.remote.connector, |_, _, cx| cx.notify()));
        self.window.subscriptions.push(cx.subscribe(
            &self.remote.connector,
            |this, _, event: &connect_server::Failed, cx| {
                if event.intent == connect_server::Intent::Add {
                    cx.notify();
                    return;
                }
                if let Some(request) = this.remote.requests.remove(&event.tab) {
                    if let Some(slot) = this.tab_slot_mut(event.tab) {
                        slot.navigation.cancel_request(&request);
                    }
                }
                cx.notify();
            },
        ));
        self.window.subscriptions.push(cx.subscribe(
            &self.remote.connector,
            |this, _, event: &connect_server::Connected, cx| {
                if event.intent == connect_server::Intent::Add {
                    this.locations
                        .places
                        .set_remote_favorite(event.address.clone(), true);
                    this.save_places(cx);
                    cx.notify();
                    return;
                }
                if let Some(request) = this.remote.requests.remove(&event.tab) {
                    if this
                        .tab_slot(event.tab)
                        .is_some_and(|slot| slot.navigation.is_current_request(&request))
                    {
                        this.start_request(
                            event.tab,
                            request.resolve_remote(event.address.clone()),
                            cx,
                        );
                    }
                }
            },
        ));
    }

    pub(in crate::pages::workspace) fn add_network_location(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.tabs.active_id;
        self.remote.connector.update(cx, |connector, cx| {
            connector.prompt(None, connect_server::Intent::Add, tab, window, cx)
        });
    }

    pub(in crate::pages::workspace) fn cancel_location_load(&mut self, cx: &mut Context<Self>) {
        let tab = self.tabs.active_id;
        self.remote.requests.remove(&tab);
        if let Some(slot) = self.tab_slot_mut(tab) {
            slot.set_remote_load(None);
            slot.set_tags_load(None);
            slot.navigation.cancel_pending();
        }
        if self
            .remote
            .connector
            .read(cx)
            .pending()
            .is_some_and(|p| p.tab == tab && p.intent == connect_server::Intent::Open)
        {
            self.remote
                .connector
                .update(cx, |connector, cx| connector.cancel(cx));
        }
        if self.tab_slot(tab).is_some_and(|slot| {
            matches!(slot.location(), Location::Remote(_)) && slot.remote().is_none()
        }) {
            self.navigate_location(tab, Location::Network, cx);
        }
        cx.notify();
    }

    pub(in crate::pages::workspace) fn load_remote(
        &mut self,
        id: u64,
        request: LoadRequest,
        address: Address,
        cx: &mut Context<Self>,
    ) {
        if self.picker.is_some() {
            if let Some(slot) = self.tab_slot_mut(id) {
                slot.navigation.cancel_request(&request);
            }
            notifications::info(i18n::text("remote-picker-local", cx), cx);
            return;
        }
        let window = self.window.handle;
        let Some(session) = self.remote.connector.read(cx).session(&address) else {
            let expected = request.clone();
            self.remote.requests.insert(id, request);
            let connector = self.remote.connector.clone();
            let browser = cx.entity().downgrade();
            cx.defer(move |cx| {
                let Some(browser) = browser.upgrade() else {
                    return;
                };
                if !browser
                    .read(cx)
                    .tab_slot(id)
                    .is_some_and(|slot| slot.navigation.is_current_request(&expected))
                {
                    return;
                }
                let _ = window.update(cx, |_, window, cx| {
                    connector.update(cx, |connector, cx| {
                        connector.prompt(
                            Some(address),
                            connect_server::Intent::Open,
                            id,
                            window,
                            cx,
                        )
                    })
                });
            });
            cx.notify();
            return;
        };
        let worker_address = address.clone();
        let worker_session = session.clone();
        let work = IoTask::cancel_on_drop(
            self.window
                .tasks
                .spawn_cancellable_named("Read remote directory", async move {
                    worker_session.list(&worker_address).await
                }),
        );
        let task = cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            let _ = window.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    if !this
                        .tab_slot(id)
                        .is_some_and(|slot| slot.navigation.is_current_request(&request))
                    {
                        return;
                    }
                    match result {
                        Ok(listing) => {
                            this.finish_request(id, &request, Ok(None), None, cx);
                            let directory =
                                this.build_remote_directory(id, listing, session, window, cx);
                            if let Some(slot) = this.tab_slot_mut(id) {
                                slot.set_remote(Some(directory));
                            }
                        }
                        Err(error) => {
                            if let Some(slot) = this.tab_slot_mut(id) {
                                slot.navigation.cancel_request(&request);
                            }
                            report_load_failure(error, cx);
                        }
                    }
                    cx.notify();
                });
            });
        });
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.set_remote_load(Some(task));
        }
        cx.notify();
    }

    fn build_remote_directory(
        &mut self,
        tab: u64,
        listing: Listing,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Directory {
        let address = listing.address.clone();
        let favorite = self
            .locations
            .places
            .is_favorite(Location::Remote(address.clone()));
        let options = remote_browser::Options {
            tasks: self.window.tasks.clone(),
            session: session.clone(),
            listing: listing.clone(),
            connector: self.remote.connector.clone(),
            desktop: self.window.desktop.clone(),
            favorite,
            filter: self.toolbar.filter.input.clone(),
            focus: self.window.focus.clone(),
        };
        let filter = self
            .tab_slot(tab)
            .map_or_else(String::new, |slot| slot.filter().to_owned());
        let hidden = self.settings.preferences.files.show_hidden;
        let compact = self.window.layout.compact;
        let view = cx.new(|cx| {
            let mut view = remote_browser::View::new(options, window, cx);
            view.set_filter(filter, hidden, cx);
            view.set_compact(compact, cx);
            view
        });
        let tasks = self.background_tasks.clone();
        let task_key: SharedString = format!("remote-task-{}", view.entity_id()).into();
        let observe_view = cx.observe(&view, move |_, view, cx| {
            let task = view.read(cx).background_task();
            tasks.update(cx, |tasks, cx| {
                tasks.sync_remote(task_key.clone(), task, cx)
            });
            cx.notify();
        });
        let subscribe_view =
            cx.subscribe(&view, move |this, _, event: &remote_browser::Event, cx| {
                this.on_remote_browser_event(tab, &address, event, cx);
            });
        let subscribe_filter = cx.subscribe(
            &self.toolbar.filter.input,
            move |this, _, _: &gpui_kit::component::input::InputEvent, cx| {
                this.sync_remote_directory(tab, cx);
            },
        );
        let observe_preferences = cx.observe_global::<configuration::Current>(move |this, cx| {
            this.sync_remote_directory(tab, cx);
        });
        let observe_bounds = cx.observe_window_bounds(window, move |this, window, cx| {
            let compact = responsive::Layout::for_width(window.bounds().size.width).compact;
            if let Some(view) = this
                .tab_slot(tab)
                .and_then(|slot| slot.remote())
                .and_then(|remote| remote.view.clone())
            {
                view.update(cx, |view, cx| view.set_compact(compact, cx));
            }
        });
        Directory {
            listing,
            session,
            view: Some(view),
            _subscriptions: vec![
                observe_view,
                subscribe_view,
                subscribe_filter,
                observe_preferences,
                observe_bounds,
            ],
        }
    }

    fn sync_remote_directory(&self, tab: u64, cx: &mut Context<Self>) {
        let Some(slot) = self.tab_slot(tab) else {
            return;
        };
        let Some(view) = slot.remote().and_then(|remote| remote.view.clone()) else {
            return;
        };
        let filter = slot.filter().to_owned();
        let hidden = self.settings.preferences.files.show_hidden;
        let compact = self.window.layout.compact;
        view.update(cx, |view, cx| {
            view.set_filter(filter, hidden, cx);
            view.set_compact(compact, cx);
        });
    }

    fn on_remote_browser_event(
        &mut self,
        tab: u64,
        address: &Address,
        event: &remote_browser::Event,
        cx: &mut Context<Self>,
    ) {
        match event {
            remote_browser::Event::Navigate(target) => {
                self.navigate_location(tab, Location::Remote(target.clone()), cx);
            }
            remote_browser::Event::Refresh
                if self
                    .tab_slot(tab)
                    .is_some_and(|slot| slot.location() == Location::Remote(address.clone())) =>
            {
                self.refresh_tab(tab, cx);
            }
            remote_browser::Event::Favorite(target) => {
                self.locations.places.toggle_remote_favorite(target.clone());
                self.save_places(cx);
                self.refresh_tab(tab, cx);
            }
            remote_browser::Event::Disconnect(target) => {
                let key = target.connection_key();
                if self
                    .tabs
                    .items
                    .iter()
                    .filter_map(|slot| slot.remote())
                    .filter_map(|remote| remote.view.as_ref())
                    .any(|view| {
                        view.read(cx).address().connection_key() == key && view.read(cx).busy()
                    })
                {
                    notifications::info(i18n::text("remote-task-running", cx), cx);
                    return;
                }
                self.remote
                    .connector
                    .update(cx, |connector, cx| connector.disconnect(target, cx));
                let ids = self
                    .tabs
                    .items
                    .iter()
                    .filter(|slot| {
                        matches!(slot.location(), Location::Remote(addr) if addr.connection_key() == key)
                    })
                    .map(|slot| slot.id)
                    .collect::<Vec<_>>();
                for id in ids {
                    self.navigate_location(id, Location::Network, cx);
                }
            }
            remote_browser::Event::Refresh => {}
        }
    }

    pub(in crate::pages::workspace) fn render_remote(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(view) = self.remote_view() else {
            return self.render_remote_unopened(cx);
        };
        el! { .div { flex_1; min_h_0 } [view] }.into_any_element()
    }
}
