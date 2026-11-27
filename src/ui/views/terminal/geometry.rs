//! Device-pixel terminal cell grid geometry shared by the view and controller tests.

use alacritty_terminal::index::Side;
use unicode_width::UnicodeWidthStr;

use crate::appearance::tokens::TERMINAL_CONTENT_INSET;

pub(super) const INSET_X_CSS: f32 = TERMINAL_CONTENT_INSET;
pub(super) const INSET_Y_CSS: f32 = 4.0;

#[derive(Clone, Copy, Debug)]
pub(in crate::ui::views::terminal) struct GridGeometry {
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub columns: usize,
    pub rows: usize,
    pub scale: f32,
    pub bounds_width: f32,
    pub bounds_height: f32,
    pub display_offset: usize,
}

impl Default for GridGeometry {
    fn default() -> Self {
        Self {
            origin_x: INSET_X_CSS,
            origin_y: INSET_Y_CSS,
            cell_width: 8.0,
            cell_height: 18.0,
            columns: 80,
            rows: 24,
            scale: 1.0,
            bounds_width: 664.0,
            bounds_height: 440.0,
            display_offset: 0,
        }
    }
}

impl GridGeometry {
    pub(super) fn from_bounds(
        bounds_width: f32,
        bounds_height: f32,
        cell_width: f32,
        cell_height: f32,
        scale: f32,
        display_offset: usize,
    ) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let inset_x = (INSET_X_CSS * scale).round();
        let inset_y = (INSET_Y_CSS * scale).round();
        let avail_w = (bounds_width - 2.0 * inset_x).max(0.0);
        let avail_h = (bounds_height - 2.0 * inset_y).max(0.0);
        let columns = ((avail_w / cell_width).floor() as usize).clamp(2, u16::MAX as usize);
        let rows = ((avail_h / cell_height).floor() as usize).clamp(1, u16::MAX as usize);
        Self {
            origin_x: inset_x,
            origin_y: inset_y,
            cell_width,
            cell_height,
            columns,
            rows,
            scale,
            bounds_width,
            bounds_height,
            display_offset,
        }
    }

    pub fn cell_at(&self, local_dev_x: f32, local_dev_y: f32) -> (usize, usize, Side) {
        let x = (local_dev_x - self.origin_x) / self.cell_width.max(1.0);
        let y = (local_dev_y - self.origin_y) / self.cell_height.max(1.0);
        let col = (x.floor().max(0.0) as usize).min(self.columns.saturating_sub(1));
        let row = (y.floor().max(0.0) as usize).min(self.rows.saturating_sub(1));
        let side = if x.fract() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        (col, row, side)
    }

    pub fn composition_column(&self, cursor_column: usize, text: &str) -> usize {
        cursor_column.min(self.columns.saturating_sub(text.width().min(self.columns)))
    }
}
