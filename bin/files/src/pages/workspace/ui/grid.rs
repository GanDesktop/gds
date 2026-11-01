//! Virtual grid rows over the same path-based file selection as the table.
use super::*;
use std::{cell::RefCell, collections::HashSet, rc::Rc};

use gds::ui::thumbnail::Thumbnail;
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Icon, menu::ContextMenuExt},
    prelude::*,
};

use super::{
    drag_drop::{DragPreview, DragSnapshot, FileDrag},
    entry_appearance::EntryAppearance,
    file_icon::FileIcon,
};

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
pub(super) struct Geometry {
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

    pub fn navigate(self, key: &str, current: Option<usize>, count: usize) -> Option<usize> {
        if count == 0 {
            return None;
        }
        if !matches!(key, "left" | "right" | "up" | "down" | "home" | "end") {
            return None;
        }
        if current.is_none() {
            return Some(if key == "end" { count - 1 } else { 0 });
        }
        let current = current.expect("a selected item was checked above");
        Some(match key {
            "left" => current.saturating_sub(1),
            "right" => (current + 1).min(count - 1),
            "up" => current.saturating_sub(self.columns),
            "down" => (current + self.columns).min(count - 1),
            "home" => 0,
            "end" => count - 1,
            _ => return None,
        })
    }
}

struct Data {
    rows: Arc<[directory::Entry]>,
    selected: HashSet<PathBuf>,
    drag: DragSnapshot,
    geometry: Geometry,
}

#[derive(Clone)]
struct TileContent {
    entry: directory::Entry,
    geometry: Geometry,
    thumbnails: Entity<ui::thumbnail::Cache>,
    selected: bool,
    appearance: EntryAppearance,
}

impl TileContent {
    fn render(&self, index: Option<usize>, play: Option<AnyElement>, cx: &App) -> Div {
        let entry = &self.entry;
        let geometry = self.geometry;
        let icon = FileIcon::for_entry(entry);
        let appearance = self.appearance;
        let foreground = if self.selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        };
        let icon_color = if self.selected {
            foreground
        } else if icon.base() == IconName::Folder {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        let media = match crate::features::file_preview::source(entry) {
            Some(source) => Thumbnail::new(self.thumbnails.clone(), source)
                .fallback(icon.render(px(geometry.icon_size), icon_color))
                .into_any_element(),
            None => icon.render(px(geometry.icon_size), icon_color),
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
                .tooltip(table::EntryPresentation::name_hint(
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
                when: $self.selected, $(|tile| tile.bg(cx.theme().selection).border_color(cx.theme().primary));
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
    pub(super) fn view_mode(&self) -> ViewMode {
        self.places.effective_view_mode(self.current_location())
    }

    pub(super) fn grid_mode(&self) -> bool {
        self.view_mode() == ViewMode::Grid
    }

    pub(super) fn set_view_mode(
        &mut self,
        mode: ViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.folder_settled() || self.editing_path(window, cx) || window.has_active_dialog(cx) {
            return;
        }
        if self.view_mode() == mode {
            return;
        }
        self.discard_marquee();
        self.keyboard_menu = None;
        self.places.set_view_mode(self.current_location(), mode);
        self.save_places(cx);
        if let Some(slot) = self.active_mut() {
            slot.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), px(0.)));
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn render_file_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(slot) = self.active() else {
            return div().into_any_element();
        };
        let geometry = slot.grid;
        let rows = slot.table_rows.clone();
        let selected = slot.navigation.view().selected.clone();
        let drag = DragSnapshot::new(&rows, &selected);
        let data = Data {
            rows: rows.clone(),
            selected: selected.into_iter().collect(),
            drag,
            geometry,
        };
        let list = uniform_list("file-grid", geometry.rows(rows.len()), cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
            range.map(|row| {
                let tiles = (row * geometry.columns..((row + 1) * geometry.columns).min(rows.len()))
                    .map(|index| this.grid_tile(index, &data, cx));
                el! { .div { h: $(geometry.row_height).px; w_full; flex; items_center; gap: GAP.px; px: PADDING.px; } [..tiles] }.into_any_element()
            }).collect()
        }))
        .track_scroll(&slot.scroll)
        .size_full();
        let scrollbar = gpui_kit::base::Scrollbar::vertical(&slot.scroll).viewport_from_layout();
        el! {
            .div { # "file-grid-area"; relative; size_full; min_h_0; } [
                list,
                .div { absolute; top_0; bottom_0; right_0; w: view::SCROLLBAR_LANE.px; } [scrollbar],
            ]
        }.into_any_element()
    }

    fn grid_tile(&self, index: usize, data: &Data, cx: &mut Context<Self>) -> AnyElement {
        let entry = &data.rows[index];
        let selected = data.selected.contains(&entry.path);
        let content = TileContent {
            entry: entry.clone(),
            geometry: data.geometry,
            thumbnails: self.thumbnails.clone(),
            selected,
            appearance: EntryAppearance::of(entry, cx),
        };
        let source = crate::features::file_preview::source(entry);
        let is_video = source
            .as_ref()
            .is_some_and(|source| source.kind == ui::thumbnail::Kind::Video);
        let path = entry.path.clone();
        let target = path.clone();
        let browser = cx.entity().downgrade();
        let on_click = cx.listener(move |this, event: &ClickEvent, window, cx| {
            file_clipboard::refresh(cx);
            let Some(index) = this.active().and_then(|slot| {
                slot.table_rows
                    .iter()
                    .position(|entry| entry.path == target)
            }) else {
                return;
            };
            this.select(index, event.modifiers(), cx);
            this.focus.focus(window, cx);
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
            this.focus.focus(window, cx);
            this.activate_path(play_path.clone(), cx);
        });
        let play = (is_video && self.picker.is_none()).then(|| {
            ui::button::Button::icon_button(("video-play", index))
                .control_icon(IconName::Play)
                .accessibility_label(i18n::text("video-play", cx))
                .tooltip(i18n::text("video-play", cx))
                .on_click(on_play)
        });
        let menu_path = path.clone();
        let menu_browser = browser.clone();
        let mut tile = el! {
            $(content.render(Some(index), play.map(IntoElement::into_any_element), cx)) { # $("file-tile", index);
                role: Role::ListItem; aria_label: $(entry.display_name.clone()); aria_selected: selected;
                cursor_pointer;
                on_click: on_click;
                when: $(!selected), $(|tile| tile.hover(|tile| tile.bg(cx.theme().list_hover)));
            }
        };
        tile = tile
            .on_drag(data.drag.drag_for(entry), move |drag, offset, _, cx| {
                let mut content = content.clone();
                content.appearance = content.appearance.preview();
                DragPreview::new(
                    drag,
                    offset,
                    move |_, cx| content.render(None, None, cx).into_any_element(),
                    cx,
                )
            })
            .external_drag_payload::<FileDrag>(|drag, _, _| Some(drag.external_payload()));
        if entry.kind == directory::EntryKind::Directory {
            tile = drag_drop::drop_target(tile, path, browser);
        }
        tile.test_support()
            .context_menu(move |menu, window, cx| {
                let Some(browser) = menu_browser.upgrade() else {
                    return menu;
                };
                browser.update(cx, |this, cx| {
                    this.entry_context_menu(menu_path.clone(), menu, window, cx)
                })
            })
            .into_any_element()
    }

    pub(super) fn grid_surface(&self, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let bounds = Rc::new(RefCell::new(Bounds::default()));
        let measured = bounds.clone();
        let measure = canvas(
            move |bounds, _, _| *measured.borrow_mut() = bounds,
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        let handler = cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            let Some(slot) = this.active() else {
                return;
            };
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
                        slot.table_rows.len(),
                    )
                    .is_none()
            {
                this.on_list_background_press(event, window, cx);
            }
        });
        el! { .div { # "file-grid-surface"; relative; size_full; flex; flex_col; min_h_0; capture_any_mouse_down: handler; } [content, measure] }.into_any_element()
    }
}

#[cfg(test)]
mod tests;
