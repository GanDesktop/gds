//! Tag views contain virtual folders and real file targets, never a fake local directory.
mod deletion;
use super::*;
use crate::entities::location::scheme::TagAddress;
use gds_uim::el;

pub(super) struct Contents {
    pub listing: crate::entities::tags::Listing,
    pub entries: Vec<directory::Entry>,
}

#[derive(Debug, thiserror::Error)]
enum LoadError {
    #[error("{0}")]
    Backend(String),
    #[error(transparent)]
    Task(#[from] gds::service::task_manager::Error),
}

use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::*,
};

impl Browser {
    pub(super) fn visible_location_targets(&self) -> Vec<LocationTarget> {
        LocationTarget::ALL
            .into_iter()
            .filter(|target| {
                *target != LocationTarget::Tags || self.settings.preferences.tags.enabled
            })
            .collect()
    }
    pub(super) fn load_tags(
        &mut self,
        id: u64,
        request: LoadRequest,
        address: TagAddress,
        cx: &mut Context<Self>,
    ) {
        self.remote.requests.remove(&id);
        if !self.settings.preferences.tags.enabled {
            self.finish_request(id, &request, Ok(None), None, cx);
            if let Some(slot) = self.tab_slot_mut(id) {
                slot.tags = None;
            }
            return;
        }
        let commands = self.settings.preferences.tags.commands.clone();
        let tasks = self.window.tasks.clone();
        let work = IoTask::cancel_on_drop(self.window.tasks.spawn_cancellable_named(
            "Query tagged files",
            async move {
                let listing = crate::entities::tags::connect(&commands)
                    .await
                    .map_err(LoadError::Backend)?
                    .browse(address.tag())
                    .await
                    .map_err(|error| LoadError::Backend(error.to_string()))?;
                tasks
                    .spawn_blocking_named("Inspect tagged files", move || {
                        let entries = listing
                            .files
                            .iter()
                            .filter_map(|path| directory::Entry::inspect(path).ok())
                            .collect();
                        Contents { listing, entries }
                    })
                    .await
                    .map_err(LoadError::from)
            },
        ));
        let task = cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(LoadError::from)
                .and_then(std::convert::identity)
                .map_err(|error| error.to_string());
            let _ = this.update(cx, |this, cx| {
                if !this
                    .tab_slot(id)
                    .is_some_and(|slot| slot.navigation.is_current_request(&request))
                {
                    return;
                }
                this.finish_request(id, &request, Ok(None), None, cx);
                if let Some(slot) = this.tab_slot_mut(id) {
                    slot.tags = Some(result);
                }
                cx.notify();
            });
        });
        if let Some(slot) = self.tab_slot_mut(id) {
            slot.tags_load = Some(task);
        }
        cx.notify();
    }

    pub(super) fn render_tag_breadcrumbs(
        &self,
        mut address: TagAddress,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut addresses = vec![address.clone()];
        while let Some(parent) = address.parent() {
            addresses.push(parent.clone());
            address = parent;
        }
        addresses.reverse();
        let crumbs = addresses
            .into_iter()
            .enumerate()
            .map(|(index, address)| {
                let location = Location::Tags(address);
                let label = location.label(cx);
                el! { ($( "tag-crumb", index)).Button::new { ghost; small; label: label;
                    on_click: $(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.activate_location_button(location.clone(), window, cx);
                    }));
                } }
            })
            .collect::<Vec<_>>();
        el! { .div { # "tag-breadcrumbs"; flex; flex_1; min_w_0; items_center; overflow_x_scroll; } [..crumbs] }.into_any_element()
    }

    pub(super) fn render_tags(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let result = self.current_tab().and_then(|slot| slot.tags.as_ref());
        let message = if !self.settings.preferences.tags.enabled {
            Some(i18n::text("tags-disabled", cx).to_string())
        } else {
            match result {
                Some(Err(e)) => Some(format!("{}: {e}", i18n::text("tags-unavailable", cx))),
                None => Some(i18n::text("loading", cx).to_string()),
                _ => None,
            }
        };
        if let Some(message) = message {
            let body = el! { .div { flex; items_center; justify_center; size_full; text_color: $(cx.theme().muted_foreground); } [message] }.into_any_element();
            return self.directory_page(body, window, cx).into_any_element();
        }
        self.render_places_page(Collection::Tags, window, cx)
    }

    pub(super) fn tag_entry(&self, location: &Location) -> Option<&directory::Entry> {
        let path = location.local_path()?;
        self.current_tab()?
            .tags
            .as_ref()?
            .as_ref()
            .ok()?
            .entries
            .iter()
            .find(|entry| entry.path == path)
    }
}
