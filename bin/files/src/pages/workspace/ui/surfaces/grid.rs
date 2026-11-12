//! Virtual grid rows over the same path-based file selection as the table.
use super::*;
use gds::ui::button::Button;
use std::{cell::RefCell, rc::Rc};

use gds::ui::thumbnail::Thumbnail;
use gds_uim::el;
use gpui_kit::base::motion::Interpolate as _;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Icon, menu::ContextMenuExt},
    prelude::*,
};

use super::{
    drag_drop::{DragPreview, DragSnapshot, FileDrag},
    entry_appearance::EntryAppearance,
    selection,
};
use crate::entities::file::ui::{EntryPresentation, FileArtwork, FileIcon};

const MIN_WIDTH: f32 = 148.;
const PADDING: f32 = 8.;
const GAP: f32 = 8.;
const THUMBNAIL_HEIGHT: f32 = 100.;
const NAME_SIZE: f32 = 14.;
const NAME_LINE_HEIGHT: f32 = 20.;
const NAME_LINES: usize = 2;
const CONTENT_GAP: f32 = 8.;
const NAME_INSET: f32 = 1.;

#[derive(Clone, Copy, Debug)]
pub(in crate::pages::workspace) struct Geometry {
    pub columns: usize,
    width: f32,
    pub row_height: f32,
    tile_height: f32,
    thumbnail_height: f32,
    name_size: f32,
    line_height: f32,
    name_height: f32,
    icon_size: f32,
}

impl Geometry {
    pub fn new(width: f32, scale: f32) -> Self {
        let thumbnail_height = THUMBNAIL_HEIGHT * scale;
        let line_height = (NAME_LINE_HEIGHT * scale).ceil();
        let name_height = line_height * NAME_LINES as f32 + NAME_INSET * 2.;
        let tile_height = PADDING * 2. + 2. + thumbnail_height + CONTENT_GAP + name_height;
        let available = (width - PADDING * 2. - view::SCROLLBAR_LANE).max(1.);
        let columns = ((available + GAP) / (MIN_WIDTH * scale + GAP))
            .floor()
            .max(1.) as usize;
        Self {
            columns,
            width: (available - GAP * (columns - 1) as f32) / columns as f32,
            row_height: tile_height + PADDING * 2.,
            tile_height,
            thumbnail_height,
            name_size: NAME_SIZE * scale,
            line_height,
            name_height,
            icon_size: 48. * scale,
        }
    }

    pub fn rows(self, count: usize) -> usize {
        count.div_ceil(self.columns)
    }

    pub fn tile(self, index: usize, origin: Point<Pixels>, scroll: Pixels) -> Bounds<Pixels> {
        Bounds::new(
            origin
                + point(
                    px(view::TABLE_INSET
                        + PADDING
                        + (index % self.columns) as f32 * (self.width + GAP)),
                    px(PADDING + (index / self.columns) as f32 * self.row_height) + scroll,
                ),
            size(px(self.width), px(self.tile_height)),
        )
    }

    pub fn hit(
        self,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        scroll: Pixels,
        count: usize,
    ) -> Option<usize> {
        let local = position - bounds.origin;
        let x = f32::from(local.x) - view::TABLE_INSET - PADDING;
        let y = f32::from(local.y - scroll);
        if !bounds.contains(&position) || x < 0. || y < 0. {
            return None;
        }
        let col = (x / (self.width + GAP)).floor() as usize;
        let row = (y / self.row_height).floor() as usize;
        let index = row * self.columns + col;
        (col < self.columns
            && index < count
            && self.tile(index, bounds.origin, scroll).contains(&position))
        .then_some(index)
    }

    pub fn navigate(
        self,
        motion: selection::Motion,
        current: Option<usize>,
        count: usize,
    ) -> Option<usize> {
        if count == 0 {
            return None;
        }
        let Some(current) = current else {
            return Some(match motion {
                selection::Motion::Last => count - 1,
                selection::Motion::Previous
                | selection::Motion::Next
                | selection::Motion::First => 0,
            });
        };
        Some(match motion {
            selection::Motion::Previous => current.saturating_sub(self.columns),
            selection::Motion::Next => (current + self.columns).min(count - 1),
            selection::Motion::First => 0,
            selection::Motion::Last => count - 1,
        })
    }
}

#[derive(Default)]
struct SelectionCache {
    tab: Option<u64>,
    rows: Arc<[directory::Entry]>,
    selected: Selection<PathBuf>,
    drag: DragSnapshot,
}

impl SelectionCache {
    fn snapshot(
        &mut self,
        tab: u64,
        rows: &Arc<[directory::Entry]>,
        selected: &Selection<PathBuf>,
    ) -> DragSnapshot {
        if self.tab != Some(tab) || !Arc::ptr_eq(&self.rows, rows) || self.selected != *selected {
            self.tab = Some(tab);
            self.rows = Arc::clone(rows);
            self.selected = selected.clone();
            self.drag = DragSnapshot::new(rows, selected);
        }
        self.drag.clone()
    }
}

struct Data {
    rows: Arc<[directory::Entry]>,
    drag: DragSnapshot,
    geometry: Geometry,
}

#[derive(Clone)]
struct TileContent {
    entry: directory::Entry,
    geometry: Geometry,
    inspections: Entity<ui::file_inspection::Cache>,
    selection: f32,
    appearance: EntryAppearance,
}

impl TileContent {
    fn render(&self, index: Option<usize>, play: Option<AnyElement>, cx: &App) -> Div {
        let entry = &self.entry;
        let geometry = self.geometry;
        let icon = FileIcon::for_entry(entry);
        let appearance = self.appearance;
        let foreground = cx
            .theme()
            .foreground
            .interpolate(&cx.theme().accent_foreground, self.selection);
        let icon_color =
            FileIcon::color(entry, cx).interpolate(&cx.theme().accent_foreground, self.selection);
        let artwork = el! { .FileArtwork::default {
            icon: icon; size: $(geometry.icon_size).px; color: icon_color;
        } };
        let media = match entry.thumbnail_source() {
            Some(source) => el! { ($(self.inspections.clone()), source).Thumbnail::new {
                fallback: artwork;
            } }
            .into_any_element(),
            None => artwork.into_any_element(),
        };
        let name = el! {
            .div {
                w_full; min_w_0; h: $(geometry.name_height).px; flex_shrink_0;
                text_size: $(geometry.name_size).px; line_height: $(geometry.line_height).px;
                py: NAME_INSET.px; text_center; whitespace_normal; text_ellipsis; line_clamp: NAME_LINES;
                opacity: $appearance.opacity;
            } [$(entry.display_name.clone())]
        };
        let name = if let Some(index) = index {
            name.id(("file-tile-name", index))
                .tooltip(EntryPresentation::name_hint(
                    entry.display_name.clone().into(),
                ))
                .into_any_element()
        } else {
            name.into_any_element()
        };
        el! {
            .div {
                relative; flex; flex_col; flex_shrink_0; items_center;
                w: $(geometry.width).px; h: $(geometry.tile_height).px; p: PADDING.px; gap: CONTENT_GAP.px;
                rounded_lg; border_1; border_color: $(transparent_black());
                text_color: foreground; overflow_hidden;
                bg: $(cx.theme().selection.opacity(self.selection));
                border_color: $(cx.theme().primary.opacity(self.selection));
            } [
                .div { relative; w_full; h: $(geometry.thumbnail_height).px; flex_shrink_0; flex; items_center; justify_center;
                    opacity: $appearance.opacity;
                } [media,
                    @if (let Some(play) = play)
                        .div { absolute; right_1; bottom_1; rounded_md;
                            bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                        } [play],
                    @end,
                ],
                name,
                @if (appearance.cut)
                    .div { absolute; top_1; right_1; } [IconName::Scissors.Icon::new { size: 12.0.px; }],
                @end,
            ]
        }
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn view_mode(&self) -> ViewMode {
        self.locations
            .places
            .effective_view_mode(self.current_location())
    }

    pub(in crate::pages::workspace) fn grid_mode(&self) -> bool {
        self.view_mode() == ViewMode::Grid
    }

    pub(in crate::pages::workspace) fn set_view_mode(
        &mut self,
        mode: ViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.browse_settled() || self.editing_path(window, cx) || window.has_active_dialog(cx) {
            return;
        }
        if self.view_mode() == mode {
            return;
        }
        self.discard_marquee();
        self.menus.context = None;
        self.locations
            .places
            .set_view_mode(self.current_location(), mode);
        self.save_places(cx);
        if let Some(slot) = self.current_tab_mut() {
            slot.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), px(0.)));
        }
        self.schedule_enrichment(self.tabs.active_id, cx);
        self.window.focus.focus(window, cx);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn render_file_grid(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(slot) = self.active() else {
            return el! { .div }.into_any_element();
        };
        let geometry = slot.grid;
        let rows = slot.table_rows.clone();
        let tab = slot.id;
        let list = uniform_list(
            "file-grid",
            geometry.rows(rows.len()),
            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                let cache = window.use_keyed_state("file-grid-selection", cx, |_, _| {
                    SelectionCache::default()
                });
                let Some(slot) = this.tab_slot(tab) else {
                    return Vec::new();
                };
                let rows = slot.table_rows.clone();
                let drag = cache.update(cx, |cache, _| {
                    cache.snapshot(tab, &rows, &slot.navigation.view().selected)
                });
                let start_item = (range.start * geometry.columns).min(rows.len());
                let end_item = (range.end * geometry.columns).min(rows.len());
                if start_item < end_item {
                    this.request_viewport_enrichment(&rows[start_item..end_item], cx);
                }
                let data = Data {
                    rows: rows.clone(),
                    drag,
                    geometry,
                };
                range
                    .map(|row| {
                        let tiles = (row * geometry.columns
                            ..((row + 1) * geometry.columns).min(rows.len()))
                            .map(|index| this.grid_tile(index, &data, window, cx));
                        el! { .div { h: $(geometry.row_height).px; w_full; flex; items_center; gap: GAP.px; px: PADDING.px; } [..tiles] }
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&slot.scroll)
        .size_full();
        let scrollbar = gpui_kit::base::Scrollbar::vertical(&slot.scroll).viewport_from_layout();
        el! {
            .div { # "file-grid-area"; relative; size_full; min_h_0; } [
                list,
                .div { absolute; top_0; bottom_0; right_0; w: view::SCROLLBAR_LANE.px; } [scrollbar],
            ]
        }
        .into_any_element()
    }

    fn grid_tile(
        &self,
        index: usize,
        data: &Data,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = &data.rows[index];
        let selected = data.drag.is_selected(&entry.path);
        let target_bounds = data.geometry.tile(index, Point::default(), px(0.));
        let presented = gpui_kit::base::motion::transition(
            format!("file-grid-layout:{}", entry.path.display()),
            target_bounds,
            gpui_kit::base::motion::Transition::new(super::motion::DURATION),
            window,
            cx,
        );
        let mut geometry = data.geometry;
        geometry.width = presented.size.width.into();
        let selection = super::motion::state_progress(
            format!("file-grid-selection:{}", entry.path.display()),
            selected,
            window,
            cx,
        );
        let content = TileContent {
            entry: entry.clone(),
            geometry,
            inspections: self.files.inspections.clone(),
            selection,
            appearance: EntryAppearance::of(entry, cx),
        };
        let source = entry.thumbnail_source();
        let is_video = source
            .as_ref()
            .is_some_and(|source| source.kind == gds::service::file_inspection::Kind::Video);
        let path = entry.path.clone();
        let target = path.clone();
        let browser = cx.entity().downgrade();
        let on_click = cx.listener(move |this, event: &ClickEvent, window, cx| {
            cx.gds().files().clipboard().refresh();
            let Some(index) = this.active().and_then(|slot| {
                slot.table_rows
                    .iter()
                    .position(|entry| entry.path == target)
            }) else {
                return;
            };
            this.select(index, event.modifiers(), cx);
            this.window.focus.focus(window, cx);
            if event.click_count() == 2 {
                this.activate_path(target.clone(), cx);
            }
        });
        let play_path = path.clone();
        let on_play = cx.listener(move |this, _: &ClickEvent, window, cx| {
            cx.stop_propagation();
            let Some(index) = this.active().and_then(|slot| {
                slot.table_rows
                    .iter()
                    .position(|entry| entry.path == play_path)
            }) else {
                return;
            };
            this.select(index, Modifiers::default(), cx);
            this.window.focus.focus(window, cx);
            this.activate_path(play_path.clone(), cx);
        });
        let play = (is_video && self.picker.is_none()).then(|| {
            el! { $(("video-play", index)).Button::icon_button {
                control_icon: IconName::Play;
                accessibility_label: $(i18n::text("video-play", cx));
                tooltip: $(i18n::text("video-play", cx));
                on_click: on_play;
            } }
        });
        let menu_path = path.clone();
        let menu_browser = browser.clone();
        let mut tile = el! {
            $(content.render(Some(index), play.map(IntoElement::into_any_element), cx)) { # $("file-tile", index);
                role: Role::ListItem; aria_label: $(entry.display_name.clone()); aria_selected: selected;
                cursor_pointer;
                left: $(presented.origin.x - target_bounds.origin.x);
                top: $(presented.origin.y - target_bounds.origin.y);
                on_click: on_click;
                when: $(!selected && selection == 0.), $(|tile| tile.hover(|tile| tile.bg(cx.theme().list_hover)));
            }
        };
        tile = tile
            .on_drag(
                data.drag.drag_for_row(entry, selected),
                move |drag, offset, _, cx| {
                    let mut content = content.clone();
                    content.appearance = content.appearance.preview();
                    content.selection = if selected { 1. } else { 0. };
                    DragPreview::new(
                        drag,
                        offset,
                        move |_, cx| content.render(None, None, cx).into_any_element(),
                        cx,
                    )
                },
            )
            .external_drag_payload::<FileDrag>(|drag, _, _| Some(drag.external_payload()));
        if entry.kind == directory::EntryKind::Directory {
            tile = drag_drop::drop_target(tile, path, browser);
        }
        let tile = tile
            .test_support()
            .context_menu(move |menu, window, cx| {
                let Some(browser) = menu_browser.upgrade() else {
                    return menu;
                };
                browser.update(cx, |this, cx| {
                    this.entry_context_menu(menu_path.clone(), menu, window, cx)
                })
            })
            .into_any_element();
        el! { .div { w: $(data.geometry.width).px; h: $(data.geometry.tile_height).px; flex_shrink_0; } [tile] }
            .into_any_element()
    }

    pub(in crate::pages::workspace) fn grid_surface(
        &self,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bounds = Rc::new(RefCell::new(Bounds::default()));
        let measured = bounds.clone();
        let measure = canvas(
            move |bounds, _, _| *measured.borrow_mut() = bounds,
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        let handler = cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            let Some(slot) = this.current_tab() else {
                return;
            };
            let count = this
                .place_collection()
                .and_then(|collection| this.place_locations(collection))
                .map_or(slot.table_rows.len(), |rows| rows.len());
            let bounds = *bounds.borrow();
            if !bounds.contains(&event.position)
                || event.position.x > bounds.right() - px(view::TABLE_INSET + view::SCROLLBAR_LANE)
                || event.position.y > bounds.bottom() - px(view::SCROLLBAR_LANE)
            {
                return;
            }
            if matches!(event.button, MouseButton::Left | MouseButton::Right)
                && slot
                    .grid
                    .hit(
                        bounds,
                        event.position,
                        slot.scroll.0.borrow().base_handle.offset().y,
                        count,
                    )
                    .is_none()
            {
                this.on_list_background_press(event, window, cx);
            }
        });
        el! { .div { # "file-grid-surface"; relative; size_full; flex; flex_col; min_h_0; capture_any_mouse_down: handler; } [content, measure] }
            .into_any_element()
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn render_location_grid(
        &self,
        collection: Collection,
        rows: Vec<Location>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(slot) = self.current_tab() else {
            return el! { .div }.into_any_element();
        };
        let geometry = slot.grid;
        let list = uniform_list(
            "location-grid",
            geometry.rows(rows.len()),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let selected = this.selected_place();
                range
                    .map(|row| {
                        let tiles = (row * geometry.columns
                            ..((row + 1) * geometry.columns).min(rows.len()))
                            .map(|index| {
                                let location = rows[index].clone();
                                let selection = if selected.as_ref() == Some(&location) {
                                    1.
                                } else {
                                    0.
                                };
                                let body = if let Some(entry) = this.tag_entry(&location) {
                                    TileContent {
                                        entry: entry.clone(),
                                        geometry,
                                        inspections: this.files.inspections.clone(),
                                        selection,
                                        appearance: EntryAppearance::of(entry, cx),
                                    }
                                    .render(Some(index), None, cx)
                                } else {
                                    el! { .div { flex; flex_col; flex_shrink_0; items_center;
                                        w: $(geometry.width).px; h: $(geometry.tile_height).px; p: PADDING.px; gap: CONTENT_GAP.px;
                                        rounded_lg; border_1; border_color: $(cx.theme().primary.opacity(selection));
                                        bg: $(cx.theme().selection.opacity(selection)); overflow_hidden;
                                    } [
                                        .div { w_full; h: $(geometry.thumbnail_height).px; flex_shrink_0; flex; items_center; justify_center; } [
                                            $(location.icon()).Icon::new { size: $(geometry.icon_size).px; text_color: $(cx.theme().primary.interpolate(&cx.theme().accent_foreground, selection)); },
                                        ],
                                        .div { w_full; min_w_0; h: $(geometry.name_height).px; text_size: $(geometry.name_size).px;
                                            line_height: $(geometry.line_height).px; py: NAME_INSET.px; text_center; whitespace_normal; text_ellipsis; line_clamp: NAME_LINES;
                                        } [$(location.label(cx))],
                                    ] }
                                };
                                let target = location.clone();
                                let browser = cx.entity().downgrade();
                                let menu_target = location.clone();
                                body.id(("location-tile", index))
                                    .cursor_pointer()
                                    .tooltip(EntryPresentation::name_hint(
                                        location.display_address().into(),
                                    ))
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, window, cx| {
                                            if event.is_right_click() {
                                                return;
                                            }
                                            this.select_place(collection, target.clone(), window, cx);
                                            if event.click_count() == 2 {
                                                this.open_place_cursor(cx);
                                            }
                                        },
                                    ))
                                    .context_menu(move |menu, window, cx| {
                                        let Some(browser) = browser.upgrade() else {
                                            return menu;
                                        };
                                        browser.update(cx, |this, cx| {
                                            this.select_place(
                                                collection,
                                                menu_target.clone(),
                                                window,
                                                cx,
                                            );
                                            this.place_selection_menu(menu, cx)
                                        })
                                    })
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>();
                        el! { .div { h: $(geometry.row_height).px; w_full; flex; items_center; gap: GAP.px; px: PADDING.px; } [..tiles] }
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&slot.scroll)
        .size_full();
        let scrollbar = gpui_kit::base::Scrollbar::vertical(&slot.scroll).viewport_from_layout();
        el! { .div { # "location-grid-area"; relative; size_full; min_h_0; } [
            list,
            .div { absolute; top_0; bottom_0; right_0; w: view::SCROLLBAR_LANE.px; } [scrollbar],
        ] }
        .into_any_element()
    }
}
