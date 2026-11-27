//! Remote directory listing and connection-prompt orchestration for workspace tabs.

use gds::service::remote;
use tokio::sync::mpsc;
use zgui::reactive::spawn_local;

use super::super::{
    projection::rows::{Loaded, Rows, sync_pinned_navigation},
    state::{context::Context, model::Contents},
};
use crate::{
    entities::location::{
        Location,
        navigation::{LoadOutcome, LoadRequest},
    },
    features::connect_server::Intent,
    shared::i18n,
};

pub(in crate::pages::workspace) struct RemoteLoader<'a>(pub &'a Context);

impl RemoteLoader<'_> {
    pub(in crate::pages::workspace) fn load_remote(
        &self,
        id: u64,
        request: LoadRequest,
        address: remote::Address,
        on_fallback_network: fn(&Context, u64),
    ) {
        if self.0.read_untracked(|state| state.picker.is_some()) {
            self.0.update(|state| {
                if let Some(tab) = state.tab_by_id(id) {
                    if tab.navigation.is_current_request(&request) {
                        tab.navigation.cancel_pending();
                    }
                    tab.work = None;
                }
            });
            self.0
                .report_notice(i18n::text_untracked("remote-picker-local"));
            Rows(self.0).sync_address();
            return;
        }

        let Some(session) = self.0.0.connector.session(&address) else {
            let prompt_ctx = self.0.clone();
            let prompt_request = request.clone();
            self.0
                .0
                .connector
                .prompt(Some(address), Intent::Open, move |result| match result {
                    Ok(connected) => {
                        RemoteLoader(&prompt_ctx).load_remote(
                            id,
                            prompt_request.clone(),
                            connected,
                            on_fallback_network,
                        );
                    }
                    Err(error) if error.is_empty() => {
                        let unopened_remote = prompt_ctx.update(|state| {
                            let Some(tab) = state.tab_by_id(id) else {
                                return false;
                            };
                            if tab.navigation.is_current_request(&prompt_request) {
                                tab.navigation.cancel_pending();
                            }
                            tab.work = None;
                            matches!(tab.navigation.location(), Location::Remote(_))
                                && !matches!(tab.contents, Contents::Remote { .. })
                        });
                        if unopened_remote {
                            on_fallback_network(&prompt_ctx, id);
                        } else {
                            Rows(&prompt_ctx).sync_address();
                            prompt_ctx.changed();
                        }
                    }
                    Err(error) => {
                        Rows(&prompt_ctx).finish_collection_load(
                            id,
                            &prompt_request,
                            Loaded::Unavailable(error),
                        );
                    }
                });
            return;
        };

        let work = self.0.0.tasks.named_child("Load remote folder");
        let scope = (*work).clone();
        {
            let mut state = self.0.0.state.borrow_mut();
            let Some(tab) = state.tab_by_id(id) else {
                return;
            };
            if !matches!(tab.contents, Contents::Remote { .. }) {
                tab.contents = Contents::LoadingCollection;
                tab.rows.clear();
            }
            tab.work = Some(work);
        }
        self.0.changed();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let list_address = address.clone();
        let list_session = session.clone();
        drop(scope.spawn_named("List remote directory", async move {
            let result = list_session.list(&list_address).await;
            let _ = tx.send(result);
        }));

        let weak = self.0.downgrade();
        self.0.0.owner.with(|| {
            drop(spawn_local(async move {
                let Some(result) = rx.recv().await else {
                    return;
                };
                let Some(ctx) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(listing) => {
                        let mut need_save = false;
                        let applied_loc = ctx.update(|state| {
                            let tab = state.tab_by_id(id)?;
                            let prior_pin = tab.pinned.then(|| tab.navigation.location().clone());
                            match tab.navigation.finish_load(&request, Ok(None)) {
                                Ok(LoadOutcome::Applied) => {
                                    tab.work = None;
                                    tab.enrichment = None;
                                    tab.contents = Contents::Remote { listing, session };
                                    let loc = Location::Remote(address);
                                    if request.records_recent() {
                                        state.places.record(loc.clone());
                                        need_save = true;
                                    }
                                    if let Some(old_loc) = prior_pin
                                        && sync_pinned_navigation(state, &old_loc, &loc)
                                    {
                                        need_save = true;
                                    }
                                    Some(loc)
                                }
                                Ok(LoadOutcome::Stale) | Err(_) => None,
                            }
                        });
                        if let Some(loc) = applied_loc {
                            if need_save {
                                ctx.save_places();
                            }
                            let rows = Rows(&ctx);
                            rows.apply_location_view_settings(id, &loc);
                            rows.reproject(id);
                            rows.restore_active_scroll();
                        }
                    }
                    Err(error) => {
                        Rows(&ctx).finish_collection_load(
                            id,
                            &request,
                            Loaded::Unavailable(error.to_string()),
                        );
                    }
                }
            }));
        });
    }
}
