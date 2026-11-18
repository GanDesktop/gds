//! The file surface shares one geometry for rendering, navigation, and hit testing.
use super::ui::Browser;
use anti_webui::prelude::*;
use gds::appearance::tokens::DataListStyle;

pub(super) const TABLE_INSET: f32 = gds::ui::ContentPage::TABLE_INSET;
pub(super) const GRID_PADDING: f32 = 8.0;
pub(super) const GRID_GAP: f32 = 8.0;
pub(super) const GRID_INSET: f32 = TABLE_INSET + GRID_PADDING;
pub(super) const SCROLLBAR_LANE: f32 = DataListStyle::SCROLLBAR_LANE;

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
pub(super) struct GridMotion {
    pub target: Memo<Grid>,
    from: Signal<Grid>,
    progress: Animated<f32>,
}

impl GridMotion {
    pub fn new(browser: &Browser) -> Self {
        let browser = browser.clone();
        let target = memo(move || browser.grid_geometry());
        let initial = untrack(|| target.get());
        let from = signal(initial);
        let progress = animated(1.0_f32);
        let previous = std::cell::Cell::new(initial);
        effect(move || {
            let next = target.get();
            let mut prior = previous.replace(next);
            if prior == next {
                return;
            }
            if !gds::appearance::motion::enabled() {
                progress.set(1.0);
                return;
            }
            let first = from.get_untracked();
            if first.columns == prior.columns {
                prior.width = first.width + (prior.width - first.width) * progress.get_untracked();
            }
            batch(|| {
                from.set(prior);
                progress.set(0.0);
                progress.to(1.0, super::view::WORKSPACE_MOTION);
            });
        });
        Self {
            target,
            from,
            progress,
        }
    }

    pub fn tile_style(self, style: Style, index: usize) -> Style {
        let target = self.target.get().tile(index);
        let tile = self.tile(index);
        style
            .w(tile.width)
            .translate_x(tile.x - target.x)
            .translate_y(tile.y - target.y)
    }

    pub fn tile(self, index: usize) -> RectF {
        let target = self.target.get().tile(index);
        let from = self.from.get().tile(index);
        let remainder = 1.0 - self.progress.get();
        RectF::new(
            target.x + (from.x - target.x) * remainder,
            target.y + (from.y - target.y) * remainder,
            target.width + (from.width - target.width) * remainder,
            target.height,
        )
    }

    pub fn candidates(self, top: f32, bottom: f32, count: usize) -> std::ops::Range<usize> {
        let range = |grid: Grid| {
            let height = virtual_row_height(grid.row_height);
            let first = (top.max(0.0) / height).floor() as usize * grid.columns;
            let end = (bottom.max(0.0) / height).ceil() as usize * grid.columns;
            first.min(count)..end.min(count)
        };
        let target = range(self.target.get());
        if self.progress.get() == 1.0 {
            return target;
        }
        let from = range(self.from.get());
        from.start.min(target.start)..from.end.max(target.end)
    }
}

pub(super) fn virtual_row_height(height: f32) -> f32 {
    let scale = window_scale();
    scale.to_logical(scale.px(height) as f32)
}
impl Grid {
    pub fn new(width: f32, scale: f32) -> Self {
        let artwork_height = 100.0 * scale;
        let line_height = (20.0 * scale).ceil();
        let name_height = line_height * 2.0 + 2.0;
        let tile_height = GRID_PADDING * 2.0 + 2.0 + artwork_height + GRID_GAP + name_height;
        let available = (width - GRID_PADDING * 2.0 - SCROLLBAR_LANE).max(1.0);
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
    pub fn tile(self, index: usize) -> RectF {
        RectF::new(
            GRID_INSET + (index % self.columns) as f32 * (self.width + GRID_GAP),
            GRID_PADDING + (index / self.columns) as f32 * virtual_row_height(self.row_height),
            self.width,
            self.tile_height,
        )
    }
}
impl Browser {
    pub(super) fn compact(&self) -> bool {
        self.0.window_width.get() < 1080.0
    }
    pub(super) fn sidebar_expanded(&self) -> bool {
        self.0.sidebar.get() && !self.compact()
    }
    pub(super) fn grid_geometry(&self) -> Grid {
        let scale = gds::app::Gds::current()
            .appearance()
            .typography()
            .scaled(1.0);
        Grid::new(self.0.content_width.get(), scale)
    }
    pub(super) fn table_viewport_width(&self) -> f32 {
        (self.0.content_width.get() - TABLE_INSET * 2.0).max(1.0)
    }
    pub(super) fn grid_columns(&self) -> usize {
        self.grid_geometry().columns
    }
    pub(super) fn row_height(&self) -> f32 {
        if self.read(|state| state.tab().mode == crate::entities::file::ViewMode::Grid) {
            self.grid_geometry().row_height
        } else {
            DataListStyle::ROW_HEIGHT
        }
    }
}
