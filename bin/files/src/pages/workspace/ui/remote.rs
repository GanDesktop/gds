//! Adapts remote widgets to workspace navigation; owns no protocol implementation.
use super::*;
use crate::{features::connect_server, widgets::remote_browser};
use crate::{features::file_operations::remote as operations, shared::lib::clipboard};
use gds::service::remote::{Address, Listing, Session};
use gds_uim::el;

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(cancel) = &self.remote_paste_cancel {
            cancel.cancel();
        }
    }
}

pub(super) struct Directory {
    pub listing: Listing,
    pub session: Session,
    pub view: Option<Entity<remote_browser::View>>,
}

impl Browser {
    pub(super) fn paste_remote_uris(
        &mut self,
        result: Result<Option<(clipboard::Mode, Vec<String>)>, clipboard::Error>,
        destination: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let sources = (|| -> Result<Vec<operations::Resource>, String> {
            let (mode, uris) = result
                .map_err(|e| e.to_string())?
                .ok_or_else(|| i18n::text("clipboard-no-files", cx).to_string())?;
            if mode == clipboard::Mode::Cut {
                return Err(i18n::text("remote-cut-unavailable", cx).to_string());
            }
            uris.into_iter()
                .map(|uri| {
                    let uri = url::Url::parse(&uri).map_err(|e| e.to_string())?;
                    if uri.scheme() == "file" {
                        return uri
                            .to_file_path()
                            .map(operations::Resource::Local)
                            .map_err(|_| "Invalid local file URI".to_owned());
                    }
                    let address = Address::parse(uri.as_str()).map_err(|e| e.to_string())?;
                    let session = self
                        .connector
                        .read(cx)
                        .session(&address)
                        .ok_or_else(|| i18n::text("remote-connect-source", cx).to_string())?;
                    Ok(operations::Resource::Remote { address, session })
                })
                .collect()
        })();
        let sources = match sources {
            Ok(sources) => sources,
            Err(error) => {
                self.file_operation_in_progress = false;
                notifications::Operation::finish(
                    notifications::Report::new(notifications::Level::Error, error),
                    cx,
                );
                cx.notify();
                return;
            }
        };
        self.file_operation_paths = vec![destination.clone()];
        let cancel = tokio_util::sync::CancellationToken::new();
        self.remote_paste_cancel = Some(cancel.clone());
        let (updates, _) = tokio::sync::watch::channel(Default::default());
        let target = operations::Resource::Local(destination.clone());
        let work = self
            .runtime
            .spawn(operations::copy(sources, target, cancel, updates));
        cx.spawn(async move |this, cx| {
            let report = work.await;
            let _ = this.update(cx, |this, cx| {
                this.file_operation_in_progress = false;
                this.remote_paste_cancel = None;
                this.file_operation_paths.clear();
                let (level, message) = match report {
                    Ok(report) => {
                        if !report.completed.is_empty() {
                            this.refresh_after_file_operation(&[destination], cx);
                        }
                        if report.cancelled {
                            (
                                notifications::Level::Warning,
                                i18n::text("remote-cancelled", cx).to_string(),
                            )
                        } else if report.failures.is_empty() {
                            (
                                notifications::Level::Success,
                                i18n::count("copy-done", report.completed.len(), cx).to_string(),
                            )
                        } else {
                            (
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
                    Err(error) => (notifications::Level::Error, error.to_string()),
                };
                notifications::Operation::finish(notifications::Report::new(level, message), cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn remote_view(&self) -> Option<Entity<remote_browser::View>> {
        if !matches!(self.current_location(), Location::Remote(_)) {
            return None;
        }
        self.current_tab()?.remote.as_ref()?.view.clone()
    }

    pub(super) fn install_remote_events(&mut self, cx: &mut Context<Self>) {
        self._subscriptions
            .push(cx.observe(&self.connector, |_, _, cx| cx.notify()));
        self._subscriptions.push(cx.subscribe(
            &self.connector,
            |this, _, event: &connect_server::Failed, cx| {
                if event.intent == connect_server::Intent::Add {
                    cx.notify();
                    return;
                }
                if let Some(request) = this.remote_requests.remove(&event.tab) {
                    if let Some(slot) = this.tab_slot_mut(event.tab) {
                        slot.navigation.cancel_request(&request);
                    }
                }
                cx.notify();
            },
        ));
        self._subscriptions.push(cx.subscribe(
            &self.connector,
            |this, _, event: &connect_server::Connected, cx| {
                if event.intent == connect_server::Intent::Add {
                    this.places.set_remote_favorite(event.address.clone(), true);
                    this.save_places(cx);
                    cx.notify();
                    return;
                }
                if let Some(request) = this.remote_requests.remove(&event.tab) {
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

    pub(super) fn add_network_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.active_id;
        self.connector.update(cx, |connector, cx| {
            connector.prompt(None, connect_server::Intent::Add, tab, window, cx)
        });
    }

    pub(super) fn cancel_location_load(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_id;
        self.remote_requests.remove(&tab);
        if let Some(slot) = self.tab_slot_mut(tab) {
            slot.remote_load = None;
            slot.navigation.cancel_pending();
        }
        if self
            .connector
            .read(cx)
            .pending()
            .is_some_and(|p| p.tab == tab && p.intent == connect_server::Intent::Open)
        {
            self.connector
                .update(cx, |connector, cx| connector.cancel(cx));
        }
        if self.tab_slot(tab).is_some_and(|slot| {
            matches!(slot.location(), Location::Remote(_)) && slot.remote.is_none()
        }) {
            self.navigate_location(tab, Location::Network, cx);
        }
        cx.notify();
    }

    pub(super) fn load_remote(
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
        let Some(session) = self.connector.read(cx).session(&address) else {
            let expected = request.clone();
            self.remote_requests.insert(id, request);
            let connector = self.connector.clone();
            let window = self.window;
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
        let work = AbortOnDropHandle::new(
            self.runtime
                .spawn(async move { worker_session.list(&worker_address).await }),
        );
        let task = cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
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
                        if let Some(slot) = this.tab_slot_mut(id) {
                            slot.remote = Some(Directory {
                                listing,
                                session,
                                view: None,
                            });
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
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.remote_load = Some(task);
        }
        cx.notify();
    }

    pub(super) fn render_remote(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab = self.active_id;
        let existing = self.remote_view();
        let view = if let Some(view) = existing {
            view
        } else {
            let Some(remote) = self.current_tab().and_then(|slot| slot.remote.as_ref()) else {
                return self.render_remote_unopened(cx);
            };
            let favorite = self
                .places
                .is_favorite(Location::Remote(remote.listing.address.clone()));
            let options = remote_browser::Options {
                runtime: self.runtime.clone(),
                session: remote.session.clone(),
                listing: remote.listing.clone(),
                connector: self.connector.clone(),
                desktop: self.desktop.clone(),
                favorite,
                filter: self.filter_input.clone(),
                focus: self.focus.clone(),
            };
            let address = options.listing.address.clone();
            let view = cx.new(|cx| remote_browser::View::new(options, window, cx));
            cx.subscribe(&view, move |this, _, event: &remote_browser::Event, cx| {
                match event {
                    remote_browser::Event::Navigate(target) => this.navigate_location(tab, Location::Remote(target.clone()), cx),
                    remote_browser::Event::Refresh if this.tab_slot(tab).is_some_and(|s| s.location() == Location::Remote(address.clone())) => this.refresh_tab(tab, cx),
                    remote_browser::Event::Favorite(target) => {
                        this.places.toggle_remote_favorite(target.clone()); this.save_places(cx); this.refresh_tab(tab, cx);
                    }
                    remote_browser::Event::Disconnect(target) => {
                        let key = target.connection_key();
                        if this.tabs.iter().filter_map(|slot| slot.remote.as_ref()).filter_map(|r| r.view.as_ref()).any(|v| v.read(cx).address().connection_key() == key && v.read(cx).busy()) {
                            notifications::info(i18n::text("remote-task-running", cx), cx); return;
                        }
                        this.connector.update(cx, |connector, cx| connector.disconnect(target, cx));
                        let ids = this.tabs.iter().filter(|s| matches!(s.location(), Location::Remote(a) if a.connection_key() == key)).map(|s| s.id).collect::<Vec<_>>();
                        for id in ids { this.navigate_location(id, Location::Network, cx); }
                    }
                    _ => (),
                }
            }).detach();
            if let Some(slot) = self.current_tab_mut() {
                if let Some(remote) = slot.remote.as_mut() {
                    remote.view = Some(view.clone());
                }
            }
            view
        };
        let filter = self
            .current_tab()
            .map_or(String::new(), |s| s.filter.clone());
        let hidden = self.preferences.files.show_hidden;
        view.update(cx, |view, cx| view.set_filter(filter, hidden, cx));
        el! { .div { flex_1; min_h_0 } [view] }.into_any_element()
    }
}

#[cfg(test)]
mod tests;
