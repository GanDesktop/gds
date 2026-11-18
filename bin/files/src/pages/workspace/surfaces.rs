//! Page-specific table metadata uses each domain's own values and service state.
mod network;

use super::{
    model::{Contents, Row},
    ui::Browser,
};
use crate::{
    entities::{
        file::ui::EntryPresentation,
        location::{Location, volumes},
    },
    features::file_operations::recycle,
};
use anti_webui::prelude::*;
use gds::{
    appearance::tokens::DataListStyle,
    i18n,
    ui::{
        Icon,
        menu::{dropdown_button, menu_item},
        time::Timestamp,
        tooltip::tooltip,
    },
};
use gds_uim::el;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct CollectionBodyState {
    icon: Option<Icon>,
    title: String,
    detail: Option<String>,
    retry: bool,
    loading: bool,
    reconnect: bool,
}

impl CollectionBodyState {
    pub(super) fn view(self, browser: &Browser) -> Div {
        let Self {
            icon,
            title,
            detail,
            retry,
            loading,
            reconnect,
        } = self;
        let fresh = browser.clone();
        el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32;
            items_center; justify_center; gap: 16.0_f32; px: 24.0_f32; text_align: TextAlign::Center;
        }.[
            if loading [.spinner.{size: 26.0_f32}],
            if let Some(icon) = icon [
                .div.{dyn_style: style => $(style.text_color(theme().text_muted))}.[ $(icon.view(26.0_f32)) ],
            ],
            title.text.{dyn_style: move style => $(if icon.is_none() {style.text_color(theme().text_muted)} else {style})},
            if let Some(detail) = detail [
                detail.text.{max_w: 480.0_f32; text_size: 12.0_f32;
                    dyn_style: style => $(style.text_color(theme().text_muted));
                },
            ],
            if retry [
                $(i18n::text(if reconnect { "device-reconnect" } else { "retry" })).button.{
                    on_click: move _ => $(if reconnect { fresh.reconnect_volume() } else { fresh.refresh() });
                },
            ],
        ] }
    }
}

impl Browser {
    pub(super) fn collection_notice(&self) -> Option<Div> {
        if !self.read(|state| matches!(state.tab().navigation.location(), Location::Volumes)) {
            return None;
        }
        let (message, error) = if let Some(error) = self.0.volumes.error() {
            (error, true)
        } else if self.0.volumes.discovering() {
            (i18n::text("device-discovering"), false)
        } else {
            return None;
        };
        Some(
            el! { .div.{flex_none; px: 24.0_f32; pb: 8.0_f32; text_size: 12.0_f32;
                dyn_style: move style => $(style.text_color(if error {theme().danger} else {theme().text_muted}));
            }.[message.text] },
        )
    }

    pub(super) fn collection_body_state(&self) -> Option<CollectionBodyState> {
        self.read(|state| {
            let tab = state.tab();
            if let Location::Volume { identity, .. } = tab.navigation.location()
                && self.resolve_path(tab.navigation.location()).is_none()
            {
                let label = if self.volume_at_location(identity).is_some() {
                    "device-not-mounted"
                } else {
                    "device-disconnected"
                };
                return Some(CollectionBodyState {
                    icon: Some(Icon::Drive),
                    title: i18n::text(label),
                    detail: Some(tab.navigation.location().label()),
                    retry: true,
                    loading: false,
                    reconnect: true,
                });
            }
            let (icon, title, detail, retry, loading) =
                match (tab.navigation.location(), &tab.contents) {
                    (Location::Trash, Contents::LoadingCollection) => {
                        (None, i18n::text("recycle-loading"), None, false, true)
                    }
                    (Location::Trash, Contents::Unavailable(error)) => (
                        Some(Icon::CircleAlert),
                        i18n::text("recycle-load-failed"),
                        Some(error.clone()),
                        recycle::supported(),
                        false,
                    ),
                    (Location::Trash, Contents::Trash(entries)) if entries.is_empty() => {
                        let loading = tab.navigation.is_loading();
                        let title = if loading {
                            "recycle-loading"
                        } else {
                            "recycle-empty"
                        };
                        (
                            (!loading).then_some(Icon::Trash),
                            i18n::text(title),
                            None,
                            false,
                            loading,
                        )
                    }
                    (Location::Tags(_), Contents::LoadingCollection) => {
                        (None, i18n::text("loading"), None, false, false)
                    }
                    (Location::Tags(_), Contents::Unavailable(error)) => {
                        let title = if self.0.preferences.snapshot().tags.enabled {
                            format!("{}: {error}", i18n::text("tags-unavailable"))
                        } else {
                            i18n::text("tags-disabled")
                        };
                        (None, title, None, false, false)
                    }
                    _ => return None,
                };
            Some(CollectionBodyState {
                icon,
                title,
                detail,
                retry,
                loading,
                reconnect: false,
            })
        })
    }

    pub(super) fn collection_empty_label(&self) -> Option<&'static str> {
        self.read(|state| {
            let tab = state.tab();
            if !tab.rows.is_empty() {
                return None;
            }
            match &tab.contents {
                Contents::Volumes => Some("device-none"),
                Contents::Places(_) | Contents::Tags { .. } => Some("empty-collection"),
                _ => None,
            }
        })
    }

    pub(super) fn collection_columns(&self) -> Option<Vec<(&'static str, f32)>> {
        let width = self.table_viewport_width();
        self.read(|state| match &state.tab().contents {
            Contents::Places(_) => {
                let name = (width * 0.35).max(180.0);
                Some(vec![
                    ("name", name),
                    ("place-location", (width - name).max(280.0)),
                ])
            }
            Contents::Trash(_) => Some(vec![
                ("name", (width * 0.38).clamp(200.0, 520.0)),
                ("recycle-original-location", 320.0),
                ("recycle-deleted-at", 160.0),
                ("size", 120.0),
            ]),
            Contents::Tags { .. } => Some(
                if state
                    .tab()
                    .rows
                    .iter()
                    .any(|row| matches!(row, Row::File(_)))
                {
                    vec![
                        ("name", (width - 370.0).max(180.0)),
                        ("modified", 160.0),
                        ("type", 110.0),
                        ("size", 100.0),
                    ]
                } else {
                    vec![("name", width.max(180.0))]
                },
            ),
            Contents::Remote { .. } => Some(vec![
                ("name", 280.0_f32),
                ("type", 125.0_f32),
                ("size", 125.0_f32),
                ("modified", 150.0_f32),
            ]),
            Contents::Volumes => {
                let flexible = width.max(700.0) - 76.0;
                let name = (flexible * 0.22).max(172.0);
                let space = (flexible * 0.36).max(220.0);
                let mount = (flexible - name - space).max(170.0);
                Some(vec![
                    ("name", name),
                    ("volume-format", 76.0),
                    ("volume-space", space),
                    ("volume-default-mount", mount),
                ])
            }
            _ => None,
        })
    }
    pub(super) fn collection_header(&self) -> Option<Div> {
        let cells = self.collection_columns()?.into_iter().map(|(label, width)| {
            el! {
                .div.{w: width; min_w: 0.0_f32; flex_none; px: 12.0_f32}.[
                    $(i18n::text(label)).text.{ellipsis; w_full; text_color: $(theme().text_muted); whitespace_nowrap;
                        dyn_style: style => $(style.text_size(gds::app::Gds::current().appearance().typography().scaled(DataListStyle::HEADER_SIZE)))},
                ]
            }
        });
        Some(el! {
            .div.{flex; items_center; h: 30.0_f32; flex_none; border_b: 1.0_f32; border_color: $(theme().border)}.[..cells]
        })
    }
    pub(super) fn collection_cells(&self, row: &Row, selected: Binding<bool>) -> Option<Vec<View>> {
        let columns = self.collection_columns()?;
        let metadata = move |style: Style| {
            let gds = gds::app::Gds::current();
            let colors = gds.appearance().theme().colors();
            style
                .text_size(
                    gds.appearance()
                        .typography()
                        .scaled(DataListStyle::META_SIZE),
                )
                .text_color(if selected.get() {
                    colors.selection_foreground
                } else {
                    colors.muted_foreground
                })
        };
        let label = |value: String, width: f32, align: TextAlign| {
            let hint = value.clone();
            el! { .div.{w: width; min_w: 0.0_f32; flex_none; px: 12.0_f32; dyn_style: metadata}.[
                $(tooltip(el! { .div.{w_full; min_w: 0.0_f32}.[value.text.{ellipsis; w_full; min_w: 0.0_f32; text_align: align; whitespace_nowrap; overflow_hidden}] }, hint)),
            ] }.into_view()
        };
        let date = |value, width| {
            el! { .div.{w: width; flex_none; px: 12.0_f32; dyn_style: metadata}.[ $(Timestamp::new(value)) ] }.into_view()
        };
        match row {
            Row::Place(location) => Some(vec![label(location.display_address(), columns[1].1, TextAlign::Start)]),
            Row::Trash(entry) => Some(vec![
                label(
                    entry
                        .original_path
                        .parent()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                    columns[1].1, TextAlign::Start,
                ),
                date(entry.deleted_at, columns[2].1),
                label(
                    entry
                        .size
                        .map(EntryPresentation::format_size)
                        .or_else(|| {
                            entry
                                .items
                                .map(|count| i18n::count("directory-item-count", count))
                        })
                        .unwrap_or_default(),
                    columns[3].1, TextAlign::End,
                ),
            ]),
            Row::Tag(_) if columns.len() == 1 => Some(Vec::new()),
            Row::Tag(_) => Some(vec![
                date(None, columns[1].1),
                label(i18n::text("tags-title"), columns[2].1, TextAlign::Start),
                label("—".into(), columns[3].1, TextAlign::Start),
            ]),
            Row::File(entry) => Some(vec![
                date(entry.modified, columns[1].1),
                label(EntryPresentation::type_label(entry), columns[2].1, TextAlign::Start),
                label(EntryPresentation::size(entry), columns[3].1, TextAlign::Start),
            ]),
            Row::Remote(entry) => Some(vec![
                label(
                    i18n::text(
                        if entry.kind == gds::service::remote::EntryKind::Directory {
                            "kind-directory"
                        } else {
                            "kind-file"
                        },
                    ),
                    125.0_f32, TextAlign::Start,
                ),
                label(
                    entry
                        .size
                        .map(EntryPresentation::format_size)
                        .unwrap_or_default(),
                    125.0_f32, TextAlign::End,
                ),
                el! {.div.{w: 150.0_f32; flex_none; px: 12.0_f32; dyn_style: metadata}.[$(Timestamp::new(entry.modified))]}.into_view(),
            ]),
            Row::Volume(volume) => {
                let service = self.0.volumes.clone();
                let id = volume.id.clone();
                let space = columns[2].1;
                let capacity = el! {
                    .div.{w: space; flex_none; px: 12.0_f32; dyn_style: metadata}.[
                        $(move || match service.capacity(&id) {
                            Some(volumes::CapacityState::Ready(capacity)) => {
                                let available = EntryPresentation::format_size(capacity.available);
                                let total = EntryPresentation::format_size(capacity.total);
                                el! {
                                    .div.{flex; flex_col; gap: 3.0_f32}.[
                                        $(gds::t!("device-capacity", available = available, total = total)).text.{text_size: 11.0_f32},
                                        $(progress_bar(move || capacity.unavailable_fraction())).{h: 4.0_f32},
                                    ]
                                }
                            }
                            Some(volumes::CapacityState::Loading) => div().child(text(i18n::text("device-capacity-loading"))),
                            Some(volumes::CapacityState::Unavailable(error)) => tooltip(
                                div().child(text(i18n::text("device-capacity-unavailable"))), error,
                            ),
                            None => div().child(text(i18n::text("device-not-mounted"))),
                        }).dyn_view,
                    ]
                }.into_view();
                Some(vec![
                    label(volume.filesystem.clone(), columns[1].1, TextAlign::Start),
                    capacity,
                    self.mount_point_cell(volume, columns[3].1).into_view(),
                ])
            }
        }
    }

    fn mount_point_cell(&self, volume: &volumes::Volume, width: f32) -> Div {
        let stable = volume.id.stable.clone();
        let preferred = stable.as_ref().and_then(|id| {
            self.0
                .preferences
                .snapshot()
                .files
                .volume_mount_points
                .get(id)
                .cloned()
        });
        let root = preferred.as_ref().or_else(|| volume.mount_points.first());
        let value = root.map_or_else(|| "—".into(), |root| root.display().to_string());
        let unavailable = root.is_some_and(|root| !volume.mount_points.contains(root));
        let hint = if unavailable {
            format!("{value}\n{}", i18n::text("volume-default-unavailable"))
        } else {
            value.clone()
        };
        let cell = match stable {
            Some(id) if volume.mount_points.len() > 1 => {
                let entries = volume
                    .mount_points
                    .iter()
                    .map(|path| {
                        let checked = root.is_some_and(|root| root == path);
                        let browser = self.clone();
                        let path = path.clone();
                        let id = id.clone();
                        menu_item(path.display().to_string())
                            .checked_when(move || checked)
                            .on_click(move || {
                                browser.edit_preferences(|preferences| {
                                    preferences
                                        .files
                                        .volume_mount_points
                                        .insert(id.clone(), path.clone());
                                })
                            })
                            .into()
                    })
                    .collect();
                dropdown_button(value, entries)
                    .h(28.0)
                    .min_w(0.0)
                    .w_full()
                    .justify_start()
            }
            _ => {
                el! { .div.{w_full; min_w: 0.0_f32}.[ value.text.{w_full; ellipsis; whitespace_nowrap} ] }
            }
        };
        el! { .div.{w: width; flex_none; px: 12.0_f32; text_size: 12.0_f32}.[ $(tooltip(cell, hint)) ] }
    }
}
