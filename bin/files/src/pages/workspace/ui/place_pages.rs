//! Table pages and selection actions for recent items, favorites and network folders.

mod network;
mod network_view;
pub(super) mod table;
use gds::ui::ListSurface;
pub(super) use network::State as Network;

use super::*;
use gds::ui::page::*;
use gds::{appearance::tokens::DataListStyle, ui::ContentPage};
use gds_uim::el;
use gpui_kit::component::{ActiveTheme, Sizable, table::DataTable};
use gpui_kit::prelude::*;

pub(super) struct State {
    pub tags: Entity<gpui_kit::component::table::TableState<table::LocationTable>>,
    pub preview_location: Option<Location>,
    pub preview_task: Option<Task<()>>,
    pub recent: Entity<gpui_kit::component::table::TableState<table::LocationTable>>,
    pub favorites: Favorites,
    pub network: NetworkPage,
    pub trash: recycle::State,
}

pub(super) struct Favorites {
    pub table: Entity<gpui_kit::component::table::TableState<table::LocationTable>>,
    pub loading: bool,
}

pub(super) struct NetworkPage {
    pub table: Entity<gpui_kit::component::table::TableState<table::LocationTable>>,
    pub discovery: Network,
}

impl State {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Browser>) -> Self {
        let mut table = |collection| {
            table::LocationTable::make_state(cx.entity().downgrade(), collection, window, cx)
        };
        Self {
            tags: table(Collection::Tags),
            preview_location: None,
            preview_task: None,
            recent: table(Collection::Recent),
            favorites: Favorites {
                table: table(Collection::Favorites),
                loading: false,
            },
            network: NetworkPage {
                table: table(Collection::Network),
                discovery: Network::default(),
            },
            trash: recycle::State::new(cx.entity().downgrade(), window, cx),
        }
    }
}

impl Browser {
    pub(super) fn place_table(
        &self,
        collection: Collection,
    ) -> Option<&Entity<gpui_kit::component::table::TableState<table::LocationTable>>> {
        match collection {
            Collection::Tags => Some(&self.collections.tags),
            Collection::Recent => Some(&self.collections.recent),
            Collection::Favorites => Some(&self.collections.favorites.table),
            Collection::Network => Some(&self.collections.network.table),
            _ => None,
        }
    }

    pub(super) fn place_locations(&self, collection: Collection) -> Option<Vec<Location>> {
        let locations = match collection {
            Collection::Tags => {
                let contents = self.current_tab()?.tags.as_ref()?.as_ref().ok()?;
                contents
                    .listing
                    .tags
                    .iter()
                    .filter_map(|name| {
                        crate::entities::location::scheme::TagAddress::new(name)
                            .ok()
                            .map(Location::Tags)
                    })
                    .chain(
                        contents
                            .entries
                            .iter()
                            .map(|entry| Location::from(&entry.path)),
                    )
                    .collect()
            }
            Collection::Favorites => self.locations.places.favorites.clone(),
            Collection::Recent => self.locations.places.recent.clone(),
            Collection::Network => self
                .collections
                .network
                .discovery
                .paths
                .iter()
                .map(Location::from)
                .collect(),
            _ => return None,
        };
        let filter = self
            .current_tab()
            .map(|slot| slot.filter.to_lowercase())
            .unwrap_or_default();
        Some(
            locations
                .into_iter()
                .filter(|location| {
                    filter.is_empty() || location.display_address().to_lowercase().contains(&filter)
                })
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
        self.window.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn favorite_selected_place(&mut self, remove_only: bool, cx: &mut Context<Self>) {
        let Some(location) = self.selected_place() else {
            return;
        };
        if remove_only && !self.locations.places.is_favorite(&location) {
            return;
        }
        self.locations.places.toggle_favorite(&location);
        if self.collection() == Some(Collection::Favorites)
            && !self.locations.places.is_favorite(&location)
        {
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
        if self.collections.network.discovery.loading {
            return;
        }
        self.collections.network.discovery.loading = true;
        self.collections.network.discovery.error = None;
        let work = self.window.tasks.spawn_blocking(network::read);
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(network::Error::from)
                .and_then(std::convert::identity);
            let _ =
                this.update(cx, |browser, cx| {
                    browser.collections.network.discovery.loading = false;
                    browser.collections.network.discovery.loaded = true;
                    match result {
                        Ok(paths) => {
                            for slot in &mut browser.tabs.items {
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
                            browser.collections.network.discovery.paths = paths;
                        }
                        Err(error) => {
                            browser.collections.network.discovery.error = Some(error.to_string())
                        }
                    }
                    cx.notify();
                });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn refresh_favorites(&mut self, cx: &mut Context<Self>) {
        if self.collections.favorites.loading {
            return;
        }
        let Some(work) = self.locations.places.reload() else {
            return;
        };
        let work = IoTask::cancel_on_drop(self.window.tasks.spawn_cancellable(work));
        self.collections.favorites.loading = true;
        let previous = self.locations.places.favorites.clone();
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(crate::entities::location::places::Error::from)
                .and_then(std::convert::identity);
            let _ = this.update(cx, |browser, cx| {
                browser.collections.favorites.loading = false;
                match result {
                    Ok(places)
                        if browser.locations.places.favorites == previous
                            && !browser.locations.saving
                            && !browser.locations.places.has_pending() =>
                    {
                        browser.locations.places.refresh_favorites(places);
                        let favorites = &browser.locations.places.favorites;
                        for slot in &mut browser.tabs.items {
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
        &mut self,
        collection: Collection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if collection == Collection::Network {
            return self.render_network_page(cx);
        }
        self.sync_preview(cx);
        let locations = self.place_locations(collection).unwrap_or_default();
        let selected = self.selected_place();
        let progress = if self.details_visible() { 1. } else { 0. };
        let width = f32::from(window.bounds().size.width)
            - self.width()
            - ContentPage::TABLE_INSET * 2.
            - super::view::PREVIEW_WIDTH * progress;
        let scale = f32::from(gds::appearance::typography::scaled(1., cx));
        let mode = self.view_mode();
        if let Some(slot) = self.current_tab_mut() {
            slot.grid = grid::Geometry::new(width, scale);
            slot.view_mode = mode;
        }
        let list = if self.grid_mode() {
            let grid = self.render_location_grid(collection, locations.clone(), cx);
            self.grid_surface(grid, cx)
        } else {
            let table_state = self
                .place_table(collection)
                .expect("location collection has a table");
            table::LocationTable::sync(table_state, locations.clone(), selected, width, cx);
            let table = el! { ($table_state).DataTable::new {
                bordered: false; with_size: $(gpui_kit::component::Size::Size(px(DataListStyle::ROW_HEIGHT)));
                header_row_height: DataListStyle::HEADER_HEIGHT.px; into_any_element;
            } };
            let scroll = table_state
                .read(cx)
                .vertical_scroll_handle
                .0
                .borrow()
                .base_handle
                .clone();
            let area = el! { .PageTableArea::default { # "locations-table-area"; } [table] };
            el! { ("locations-selection-surface", $(area.into_any_element()), $(locations.len()), scroll).ListSurface::new {
                inset_x: $(px(ContentPage::TABLE_INSET));
                on_blank_press: $(cx.listener(Self::on_list_background_press));
            } }.into_any_element()
        };
        let content = self.directory_page(list, window, cx);
        let panel = (progress > 0.).then(|| {
            el! { .div {
                # "preview-panel"; w: super::view::PREVIEW_WIDTH.px; flex_shrink_0; h_full; min_h_0;
                flex; flex_col; overflow_hidden; border_l_1; border_color: $cx.theme().border;
            } [$(self.files.preview.clone())] }
        });
        el! { .div { flex; size_full; min_h_0; min_w_0; } [content, ..panel] }.into_any_element()
    }
}
