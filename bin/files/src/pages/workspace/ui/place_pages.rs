//! Table pages and selection actions for recent items, favorites and network folders.

mod network;
mod network_view;
pub(super) mod table;
pub(super) use network::State as Network;

use super::*;
use gds::ui::page::*;
use gds::{appearance::tokens::DataListStyle, ui::ContentPage};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Sizable, table::DataTable};
use gpui_kit::prelude::*;

impl Browser {
    pub(super) fn place_table(
        &self,
        collection: Collection,
    ) -> Option<&Entity<gpui_kit::component::table::TableState<table::LocationTable>>> {
        match collection {
            Collection::Recent => Some(&self.recent_table),
            Collection::Favorites => Some(&self.favorites_table),
            Collection::Network => Some(&self.network_table),
            _ => None,
        }
    }

    pub(super) fn place_locations(&self, collection: Collection) -> Option<Vec<Location>> {
        let locations = match collection {
            Collection::Favorites => self.places.favorites.clone(),
            Collection::Recent => self.places.recent.clone(),
            Collection::Network => self.network.paths.iter().map(Location::from).collect(),
            _ => return None,
        };
        Some(
            locations
                .into_iter()
                .filter(|location| {
                    self.picker.is_none() || !matches!(location, Location::Remote(_))
                })
                .collect(),
        )
    }

    pub(super) fn select_place(
        &mut self,
        collection: Collection,
        location: Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.collection() != Some(collection)
            || !self
                .place_locations(collection)
                .is_some_and(|locations| locations.contains(&location))
        {
            return;
        }
        if let Some(slot) = self.current_tab_mut() {
            slot.navigation.view_mut().place = Some(location);
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn favorite_selected_place(&mut self, remove_only: bool, cx: &mut Context<Self>) {
        let Some(location) = self.selected_place() else {
            return;
        };
        if remove_only && !self.places.is_favorite(&location) {
            return;
        }
        self.places.toggle_favorite(&location);
        if self.collection() == Some(Collection::Favorites) && !self.places.is_favorite(&location) {
            if let Some(slot) = self.current_tab_mut() {
                slot.navigation.view_mut().place = None;
            }
        }
        self.save_places(cx);
        cx.notify();
    }

    pub(super) fn copy_place_address(&self, location: &Location, cx: &mut Context<Self>) {
        match location.uri() {
            Ok(uri) => {
                cx.write_to_clipboard(ClipboardItem::new_string(uri));
                file_clipboard::clear(cx);
                cx.notify();
            }
            Err(error) => notifications::error(error.to_string(), cx),
        }
    }

    pub(super) fn refresh_network(&mut self, cx: &mut Context<Self>) {
        if self.network.loading {
            return;
        }
        self.network.loading = true;
        self.network.error = None;
        let work = cx.background_executor().spawn(async { network::read() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ =
                this.update(cx, |browser, cx| {
                    browser.network.loading = false;
                    browser.network.loaded = true;
                    match result {
                        Ok(paths) => {
                            for slot in &mut browser.tabs {
                                if slot.navigation.location() == &Location::Network {
                                    if slot.navigation.view().place.as_ref().is_some_and(
                                        |location| {
                                            !paths
                                                .iter()
                                                .any(|path| location == &Location::from(path))
                                        },
                                    ) {
                                        slot.navigation.view_mut().place = None;
                                    }
                                }
                            }
                            browser.network.paths = paths;
                        }
                        Err(error) => browser.network.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn refresh_favorites(&mut self, cx: &mut Context<Self>) {
        if self.favorites_loading {
            return;
        }
        let Some(work) = self.places.reload() else {
            return;
        };
        let work = AbortOnDropHandle::new(self.runtime.spawn(work));
        self.favorites_loading = true;
        let previous = self.places.favorites.clone();
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(crate::entities::location::places::Error::from)
                .and_then(std::convert::identity);
            let _ = this.update(cx, |browser, cx| {
                browser.favorites_loading = false;
                match result {
                    Ok(places)
                        if browser.places.favorites == previous
                            && !browser.places_saving
                            && !browser.places.has_pending() =>
                    {
                        browser.places.refresh_favorites(places);
                        let favorites = &browser.places.favorites;
                        for slot in &mut browser.tabs {
                            if slot.navigation.location() == &Location::Favorites {
                                if slot
                                    .navigation
                                    .view()
                                    .place
                                    .as_ref()
                                    .is_some_and(|location| !favorites.contains(location))
                                {
                                    slot.navigation.view_mut().place = None;
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => notifications::error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_places_page(
        &self,
        collection: Collection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if collection == Collection::Network {
            return self.render_network_page(cx);
        }
        let locations = self.place_locations(collection).unwrap_or_default();
        let selected = self.selected_place();
        let has_selection = selected.is_some();
        let favorite = selected
            .as_ref()
            .is_some_and(|path| self.places.is_favorite(path));
        let loading = self.favorites_loading;
        let table_state = self
            .place_table(collection)
            .expect("folder collection has a table");
        let width =
            f32::from(window.bounds().size.width) - self.width() - ContentPage::TABLE_INSET * 2.;
        table::LocationTable::sync(table_state, locations.clone(), selected, width, cx);
        let table = el! {
            ($table_state).DataTable::new {
                bordered: false; with_size: $(gpui_kit::component::Size::Size(px(DataListStyle::ROW_HEIGHT)));
                header_row_height: DataListStyle::HEADER_HEIGHT.px; into_any_element;
            }
        };
        let scroll = table_state
            .read(cx)
            .vertical_scroll_handle
            .0
            .borrow()
            .base_handle
            .clone();
        let area = PageTableArea::default()
            .id("locations-table-area")
            .child(table);
        let list = ui::ListSurface::new(
            "locations-selection-surface",
            area.into_any_element(),
            locations.len(),
            scroll,
        )
        .inset_x(px(ContentPage::TABLE_INSET))
        .on_blank_press(cx.listener(Self::on_list_background_press));
        let on_copy_path = cx.listener(Self::click_command(Command::CopyPaths));
        let mut actions = Vec::new();
        if collection != Collection::Recent {
            let on_copy = cx.listener(Self::click_command(Command::Copy));
            actions.push(el! {
                "place-copy".ui::IconButton::new { icon: IconName::Copy; label: $i18n::text("copy-files", cx);
                    command: Command::Copy; disabled: $(self.selected_place_path().is_none()); on_click: on_copy;
                }
            }.into_any_element());
        }
        actions.push(el! {
            "place-copy-path".ui::IconButton::new { icon: IconName::ClipboardCopy; label: $i18n::text(Command::CopyPaths.label_id(), cx);
                command: Command::CopyPaths; disabled: $(!has_selection); on_click: on_copy_path;
            }
        }.into_any_element());
        let on_favorite = cx.listener(Self::click_command(Command::Favorite));
        actions.push(el! {
            "place-favorite".ui::IconButton::new { icon: $(if favorite { IconName::StarOff } else { IconName::Star });
                label: $i18n::text(if favorite { "place-favorite-remove" } else { "place-favorite-add" }, cx);
                command: Command::Favorite; disabled: $(!has_selection); on_click: on_favorite;
            }
        }.into_any_element());
        if collection == Collection::Recent {
            let on_clear = cx.listener(Self::click_command(Command::ClearRecent));
            actions.push(el! {
                "clear-recent".ui::IconButton::new { icon: IconName::Broom; label: $i18n::text("clear-recent", cx);
                    command: Command::ClearRecent; disabled: $(locations.is_empty()); on_click: on_clear;
                }
            }.into_any_element());
        } else {
            let on_refresh = cx.listener(Self::click_command(Command::Refresh));
            actions.push(el! {
                "place-refresh".ui::IconButton::new { icon: IconName::RefreshCw; label: $i18n::text("refresh", cx);
                    command: Command::Refresh; disabled: loading; on_click: on_refresh;
                }
            }.into_any_element());
        }
        el! {
            .ContentPage::default { # $(collection.label_id()) } [
                .PageHeader::default [
                    .PageTitle::default [$i18n::text(collection.label_id(), cx)],
                    @if (!actions.is_empty())
                        .PageActions::default [..actions],
                    @end,
                ],
                .PageContent::default [
                    list,
                ],
            ]
        }
        .into_any_element()
    }
}

#[cfg(test)]
mod tests;
