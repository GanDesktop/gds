//! The file surface shares one geometry for rendering, navigation, and hit testing.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gds::{
    app::Gds,
    appearance::{
        motion::{WORKSPACE_DURATION_MS, use_transition},
        tokens::DataListStyle,
    },
    ui::window::{HeightTier, WidthTier},
};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal, Signal, on_cleanup_local};

use super::{model::RowKey, ui::Browser};

pub(super) const TABLE_INSET: f32 = gds::ui::page::TABLE_INSET;
pub(super) const GRID_PADDING: f32 = 8.0;
pub(super) const GRID_GAP: f32 = 8.0;
pub(super) const SCROLLBAR_LANE: f32 = DataListStyle::SCROLLBAR_LANE;
pub(super) const COMPACT_ACTIONS_WIDTH: f32 = 620.0;
pub(super) const NARROW_COLLECTION_WIDTH: f32 = 680.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectF {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Grid {
    pub columns: usize,
    pub width: f32,
    pub row_height: f32,
    pub tile_height: f32,
    pub artwork_height: f32,
    pub name_height: f32,
    pub name_size: f32,
    pub line_height: f32,
    pub icon_size: f32,
}

#[derive(Clone, Copy)]
pub(super) struct TilePresentation {
    pub width: Signal<f32, LocalStorage>,
    pub height: Signal<f32, LocalStorage>,
    pub offset: Signal<(f32, f32), LocalStorage>,
}

#[derive(Clone)]
pub(super) struct GridMotion {
    pub target: Signal<Grid, LocalStorage>,
    seen: Rc<RefCell<HashMap<RowKey, RectF>>>,
}

impl GridMotion {
    pub fn new(browser: &Browser) -> Self {
        let browser = browser.clone();
        let target = Signal::derive_local(move || browser.grid_geometry());
        Self {
            target,
            seen: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Interpolates visible tile bounds by stable [`RowKey`] over `160ms` (`motion.md`), while
    /// placing newly mounted items immediately without flying in from the origin.
    pub fn present_tile(&self, key: RowKey, index: usize) -> TilePresentation {
        let target_grid = self.target;
        let target_rect = Signal::derive_local(move || target_grid.get().tile(index, 1.0));
        let initial_target = target_rect.get_untracked();
        let prior = self.seen.borrow().get(&key).copied();
        self.seen.borrow_mut().insert(key.clone(), initial_target);

        let seed = prior.unwrap_or(initial_target);
        let goal_x = RwSignal::new_local(seed.x);
        let goal_y = RwSignal::new_local(seed.y);
        let goal_w = RwSignal::new_local(seed.width);

        let pres_x = use_transition(goal_x.into(), WORKSPACE_DURATION_MS);
        let pres_y = use_transition(goal_y.into(), WORKSPACE_DURATION_MS);
        let pres_w = use_transition(goal_w.into(), WORKSPACE_DURATION_MS);

        let seen = Rc::clone(&self.seen);
        let sync_fx = RenderEffect::new(move |_| {
            let next = target_rect.get();
            seen.borrow_mut().insert(key.clone(), next);
            if (goal_x.get_untracked() - next.x).abs() > 1e-3 {
                goal_x.set(next.x);
            }
            if (goal_y.get_untracked() - next.y).abs() > 1e-3 {
                goal_y.set(next.y);
            }
            if (goal_w.get_untracked() - next.width).abs() > 1e-3 {
                goal_w.set(next.width);
            }
        });
        on_cleanup_local(move || drop(sync_fx));

        let offset = Signal::derive_local(move || {
            let rect = target_rect.get();
            (pres_x.get() - rect.x, pres_y.get() - rect.y)
        });
        let height = Signal::derive_local(move || target_grid.get().tile_height);

        TilePresentation {
            width: pres_w,
            height,
            offset,
        }
    }
}

pub(super) fn virtual_row_height(height: f32, scale: f32) -> f32 {
    let scale = scale.max(1.0);
    (height * scale).round() / scale
}

impl Grid {
    pub fn new(width: f32, scale: f32) -> Self {
        let artwork_height = 100.0 * scale;
        let line_height = (20.0 * scale).ceil();
        let name_height = line_height * 2.0 + 2.0;
        let tile_height = GRID_PADDING * 2.0 + 2.0 + artwork_height + GRID_GAP + name_height;
        let available = (width - TABLE_INSET * 2.0 - GRID_PADDING * 2.0 - SCROLLBAR_LANE).max(1.0);
        let columns = ((available + GRID_GAP) / (148.0 * scale + GRID_GAP))
            .floor()
            .max(1.0) as usize;
        Self {
            columns,
            width: (available - GRID_GAP * (columns - 1) as f32) / columns as f32,
            row_height: tile_height + GRID_PADDING * 2.0,
            tile_height,
            artwork_height,
            name_height,
            name_size: 14.0 * scale,
            line_height,
            icon_size: 48.0 * scale,
        }
    }

    pub fn tile(self, index: usize, scale: f32) -> RectF {
        RectF::new(
            GRID_PADDING + (index % self.columns) as f32 * (self.width + GRID_GAP),
            GRID_PADDING
                + (index / self.columns) as f32 * virtual_row_height(self.row_height, scale),
            self.width,
            self.tile_height,
        )
    }
}

impl Browser {
    pub(super) fn width_tier(&self) -> WidthTier {
        WidthTier::from_width(self.0.window_width.get())
    }

    pub(super) fn width_tier_untracked(&self) -> WidthTier {
        WidthTier::from_width(self.0.window_width.get_untracked())
    }

    pub(super) fn height_tier(&self) -> HeightTier {
        HeightTier::from_height(self.0.window_height.get())
    }

    pub(super) fn height_tier_untracked(&self) -> HeightTier {
        HeightTier::from_height(self.0.window_height.get_untracked())
    }

    pub(super) fn actions_collapsed(&self) -> bool {
        self.width_tier() == WidthTier::Micro || self.0.content_width.get() < COMPACT_ACTIONS_WIDTH
    }

    pub(super) fn actions_collapsed_untracked(&self) -> bool {
        self.width_tier_untracked() == WidthTier::Micro
            || self.0.content_width.get_untracked() < COMPACT_ACTIONS_WIDTH
    }

    pub(super) fn narrow_collection(&self) -> bool {
        self.width_tier() == WidthTier::Micro
            || self.0.content_width.get() < NARROW_COLLECTION_WIDTH
    }

    pub(super) fn details_permitted(&self) -> bool {
        self.width_tier() == WidthTier::Wide
    }

    pub(super) fn details_permitted_untracked(&self) -> bool {
        self.width_tier_untracked() == WidthTier::Wide
    }

    pub(super) fn list_row_height(&self) -> f32 {
        Gds::try_current().map_or(DataListStyle::ROW_HEIGHT, |gds| {
            gds.appearance()
                .typography()
                .scaled(DataListStyle::ROW_HEIGHT)
        })
    }

    pub(super) fn list_row_height_untracked(&self) -> f32 {
        Gds::try_current().map_or(DataListStyle::ROW_HEIGHT, |gds| {
            gds.appearance()
                .typography()
                .scaled_untracked(DataListStyle::ROW_HEIGHT)
        })
    }

    pub(super) fn list_header_height(&self) -> f32 {
        Gds::try_current().map_or(DataListStyle::HEADER_HEIGHT, |gds| {
            gds.appearance()
                .typography()
                .scaled(DataListStyle::HEADER_HEIGHT)
        })
    }

    pub(super) fn grid_geometry(&self) -> Grid {
        let scale = Gds::current().appearance().typography().scaled(1.0);
        Grid::new(self.0.content_width.get(), scale)
    }

    pub(super) fn grid_geometry_untracked(&self) -> Grid {
        let scale = Gds::current()
            .appearance()
            .typography()
            .scaled_untracked(1.0);
        Grid::new(self.0.content_width.get_untracked(), scale)
    }

    pub(super) fn table_viewport_width(&self) -> f32 {
        (self.0.content_width.get() - TABLE_INSET * 2.0)
            .floor()
            .max(1.0)
    }

    pub(super) fn grid_columns_untracked(&self) -> usize {
        self.grid_geometry_untracked().columns
    }

    pub(super) fn row_height_untracked(&self) -> f32 {
        if self.read_untracked(|state| state.tab().mode == crate::entities::file::ViewMode::Grid) {
            self.grid_geometry_untracked().row_height
        } else {
            self.list_row_height_untracked()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GRID_GAP, GRID_PADDING, Grid, SCROLLBAR_LANE, TABLE_INSET};

    #[test]
    fn grid_geometry_fits_available_viewport_and_aligns_first_tile_at_padding() {
        let content_width = 1018.0;
        let grid = Grid::new(content_width, 1.0);
        assert!(grid.columns >= 1);
        let available = content_width - TABLE_INSET * 2.0 - GRID_PADDING * 2.0 - SCROLLBAR_LANE;
        let occupied = grid.columns as f32 * grid.width + (grid.columns - 1) as f32 * GRID_GAP;
        assert!((occupied - available).abs() < 1e-3);

        let first = grid.tile(0, 1.0);
        assert!((first.x - GRID_PADDING).abs() < 1e-3);
        assert!((first.y - GRID_PADDING).abs() < 1e-3);
    }
}
