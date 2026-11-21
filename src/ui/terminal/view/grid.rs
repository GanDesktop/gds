//! Device-pixel terminal cell grid rendered via `zgui::custom::CustomElement`.
//!
//! The emulator owns columns and rows. Shaping batches contiguous styled spans
//! per row, then anchors every shaped cluster back to its exact grid column so
//! fallback fonts and combining diacritics never shift neighboring cells.

use std::cell::Cell as StdCell;
use std::rc::Rc;

use alacritty_terminal::index::{Line, Point as GridPoint, Side};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors as TerminalColors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use zgui::app::Shaper;
use zgui::canvas::zgui_color::Color as ZguiColor;
use zgui::custom::{CustomElement, CustomLayoutCx, CustomMeasured, PaintSlot, ScenePainter, Space};
use zgui::geom::{Device, DevicePx, Point, Rect, Size};

use super::fonts::TerminalFonts;
use super::ime::PreeditState;
use super::palette::Palette;
use crate::appearance::tokens::TERMINAL_CONTENT_INSET;
use crate::service::terminal::Session;
use crate::ui::terminal::Preferences;

const INSET_X_CSS: f32 = TERMINAL_CONTENT_INSET;
const INSET_Y_CSS: f32 = 4.0;
const LINE_HEIGHT_MULTIPLIER: f32 = 1.2;

#[derive(Clone)]
struct SnapshotCell {
    point: GridPoint,
    cell: Cell,
    selected: bool,
}

#[derive(Clone, Default)]
struct Snapshot {
    cells: Vec<SnapshotCell>,
    display_offset: usize,
    cursor: Option<(GridPoint, CursorShape)>,
    colors: TerminalColors,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui::terminal) struct GridGeometry {
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
    fn from_bounds(
        bounds_width: f32,
        bounds_height: f32,
        fonts: &TerminalFonts,
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
        let columns = ((avail_w / fonts.cell_width).floor() as usize).clamp(2, u16::MAX as usize);
        let rows = ((avail_h / fonts.cell_height).floor() as usize).clamp(1, u16::MAX as usize);
        Self {
            origin_x: inset_x,
            origin_y: inset_y,
            cell_width: fonts.cell_width,
            cell_height: fonts.cell_height,
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

#[derive(Clone, Copy)]
struct ClusterSpan {
    end: usize,
    column: usize,
}

struct StyledSpan {
    text: String,
    trailing_spaces: usize,
    fg: ZguiColor,
    bold: bool,
    italic: bool,
    clusters: Vec<ClusterSpan>,
}

impl StyledSpan {
    fn new(fg: ZguiColor, bold: bool, italic: bool) -> Self {
        Self {
            text: String::new(),
            trailing_spaces: 0,
            fg,
            bold,
            italic,
            clusters: Vec::new(),
        }
    }

    fn matches(&self, fg: ZguiColor, bold: bool, italic: bool) -> bool {
        self.fg == fg && self.bold == bold && self.italic == italic
    }

    fn push_cell(&mut self, column: usize, value: &str, is_plain_space: bool) {
        if is_plain_space {
            if self.text.is_empty() {
                return;
            }
            self.trailing_spaces += 1;
            self.text.push(' ');
            let end = self.text.len();
            self.clusters.push(ClusterSpan { end, column });
            return;
        }
        self.trailing_spaces = 0;
        self.text.push_str(value);
        let end = self.text.len();
        self.clusters.push(ClusterSpan { end, column });
    }

    fn trim_trailing_spaces(&mut self) {
        if self.trailing_spaces > 0 {
            let keep = self.clusters.len().saturating_sub(self.trailing_spaces);
            self.clusters.truncate(keep);
            let byte_len = self.clusters.last().map_or(0, |cluster| cluster.end);
            self.text.truncate(byte_len);
            self.trailing_spaces = 0;
        }
    }
}

fn push_background(
    runs: &mut Vec<(Rect<DevicePx, Device>, ZguiColor)>,
    rect: Rect<DevicePx, Device>,
    color: ZguiColor,
) {
    if let Some((prev_rect, prev_color)) = runs.last_mut()
        && *prev_color == color
        && (prev_rect.origin.y.0 - rect.origin.y.0).abs() <= 0.01
        && (prev_rect.size.height.0 - rect.size.height.0).abs() <= 0.01
        && ((prev_rect.origin.x.0 + prev_rect.size.width.0) - rect.origin.x.0).abs() <= 0.5
    {
        prev_rect.size.width = DevicePx(prev_rect.size.width.0 + rect.size.width.0);
    } else {
        runs.push((rect, color));
    }
}

fn shape_and_paint_span(
    span: &StyledSpan,
    origin: Point<DevicePx, Device>,
    cell_width: f32,
    fonts: &TerminalFonts,
    shaper: &mut Shaper,
    painter: &mut ScenePainter<'_>,
) {
    if span.text.is_empty() || span.clusters.is_empty() {
        return;
    }
    let request = fonts.line_request(span.bold, span.italic);
    let mut shaped = shaper.shape_line(&span.text, &request);
    for run in &mut shaped {
        let mut anchors = vec![None::<f32>; span.clusters.len()];
        for (&cluster_byte, glyph) in run.clusters.iter().zip(&run.glyphs) {
            let idx = span
                .clusters
                .partition_point(|cluster| cluster.end <= cluster_byte as usize)
                .min(span.clusters.len() - 1);
            if anchors[idx].is_none() {
                anchors[idx] = Some(glyph.x);
            }
        }
        for (&cluster_byte, glyph) in run.clusters.iter().zip(&mut run.glyphs) {
            let idx = span
                .clusters
                .partition_point(|cluster| cluster.end <= cluster_byte as usize)
                .min(span.clusters.len() - 1);
            let anchor = anchors[idx].unwrap_or(glyph.x);
            let target_x = span.clusters[idx].column as f32 * cell_width;
            glyph.x = target_x + (glyph.x - anchor);
        }
        let paint_run = run.as_run(PaintSlot(0));
        painter.glyphs(&paint_run, origin, span.fg);
    }
}

pub(super) struct TerminalGrid {
    pub session: Option<Session>,
    pub palette: Palette,
    pub paint_surface: bool,
    shaper: Shaper,
    fonts: TerminalFonts,
    pub preferences: Preferences,
    pub focused: bool,
    pub cursor_on: bool,
    pub reduce_motion: bool,
    pub preedit: PreeditState,
    snapshot: Snapshot,
    geometry: GridGeometry,
    geometry_out: Rc<StdCell<GridGeometry>>,
    last_pty_size: Option<(u16, u16, u16, u16)>,
}

impl TerminalGrid {
    #[expect(
        clippy::too_many_arguments,
        reason = "TerminalGrid captures explicit initial state from TerminalView"
    )]
    pub fn new(
        session: Option<Session>,
        palette: Palette,
        paint_surface: bool,
        shaper: Shaper,
        preferences: Preferences,
        focused: bool,
        reduce_motion: bool,
        geometry_out: Rc<StdCell<GridGeometry>>,
    ) -> Self {
        let fonts = TerminalFonts::resolve(
            &preferences.font_family,
            preferences.font_size,
            LINE_HEIGHT_MULTIPLIER,
            1.0,
            &shaper,
        );
        let initial_geometry = GridGeometry {
            cell_width: fonts.cell_width,
            cell_height: fonts.cell_height,
            ..GridGeometry::default()
        };
        geometry_out.set(initial_geometry);
        Self {
            session,
            palette,
            paint_surface,
            shaper,
            fonts,
            preferences,
            focused,
            cursor_on: true,
            reduce_motion,
            preedit: PreeditState::default(),
            snapshot: Snapshot::default(),
            geometry: initial_geometry,
            geometry_out,
            last_pty_size: None,
        }
    }

    pub(in crate::ui::terminal) fn should_paint_surface(&self) -> bool {
        self.paint_surface || self.snapshot.colors[NamedColor::Background].is_some()
    }

    pub fn sync_pty_size(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let inset_x = (INSET_X_CSS * self.geometry.scale).round();
        let inset_y = (INSET_Y_CSS * self.geometry.scale).round();
        if self.geometry.bounds_width <= inset_x * 2.0
            || self.geometry.bounds_height <= inset_y * 2.0
        {
            return;
        }
        let next = (
            self.geometry.columns.min(u16::MAX as usize) as u16,
            self.geometry.rows.min(u16::MAX as usize) as u16,
            self.fonts
                .cell_width
                .round()
                .clamp(1.0, f32::from(u16::MAX)) as u16,
            self.fonts
                .cell_height
                .round()
                .clamp(1.0, f32::from(u16::MAX)) as u16,
        );
        if self.last_pty_size != Some(next)
            && session
                .resize_with_pixels(next.0, next.1, next.2, next.3)
                .is_ok()
        {
            self.last_pty_size = Some(next);
        }
    }

    fn ensure_fonts(&mut self, scale: f32) {
        if !self.fonts.matches(
            &self.preferences.font_family,
            self.preferences.font_size,
            LINE_HEIGHT_MULTIPLIER,
            scale,
        ) {
            self.fonts = TerminalFonts::resolve(
                &self.preferences.font_family,
                self.preferences.font_size,
                LINE_HEIGHT_MULTIPLIER,
                scale,
                &self.shaper,
            );
        }
    }
}

impl CustomElement for TerminalGrid {
    fn layout(&mut self, cx: &mut CustomLayoutCx<'_>) -> CustomMeasured {
        self.ensure_fonts(cx.scale);
        let inset_x = (INSET_X_CSS * cx.scale).round();
        let inset_y = (INSET_Y_CSS * cx.scale).round();
        let intrinsic_w =
            (self.geometry.columns.max(80) as f32 * self.fonts.cell_width + 2.0 * inset_x).ceil();
        let intrinsic_h =
            (self.geometry.rows.max(24) as f32 * self.fonts.cell_height + 2.0 * inset_y).ceil();

        let width = cx
            .known_width
            .or(match cx.available.0 {
                Space::Definite(value) => Some(value),
                Space::MinContent | Space::MaxContent => None,
            })
            .unwrap_or(intrinsic_w)
            .max(0.0);
        let height = cx
            .known_height
            .or(match cx.available.1 {
                Space::Definite(value) => Some(value),
                Space::MinContent | Space::MaxContent => None,
            })
            .unwrap_or(intrinsic_h)
            .max(0.0);

        if cx.final_pass {
            self.geometry = GridGeometry::from_bounds(
                width,
                height,
                &self.fonts,
                cx.scale,
                self.snapshot.display_offset,
            );
            self.geometry_out.set(self.geometry);
            self.sync_pty_size();
        }

        CustomMeasured {
            width,
            height,
            first_baseline: None,
            last_baseline: None,
        }
    }

    fn paint(&mut self, painter: &mut ScenePainter<'_>) {
        let size = painter.size();
        let scale = painter.scale();
        self.ensure_fonts(scale);
        if (size.width.0 - self.geometry.bounds_width).abs() > 0.5
            || (size.height.0 - self.geometry.bounds_height).abs() > 0.5
            || (self.fonts.cell_width - self.geometry.cell_width).abs() > f32::EPSILON
            || (self.fonts.cell_height - self.geometry.cell_height).abs() > f32::EPSILON
        {
            self.geometry = GridGeometry::from_bounds(
                size.width.0,
                size.height.0,
                &self.fonts,
                scale,
                self.snapshot.display_offset,
            );
            self.geometry_out.set(self.geometry);
            self.sync_pty_size();
        }

        let palette = self.palette;
        if let Some(session) = &self.session
            && let Some(snapshot) = session.try_with_term(|term| {
                let content = term.renderable_content();
                Snapshot {
                    display_offset: content.display_offset,
                    cursor: Some((content.cursor.point, content.cursor.shape)),
                    colors: *content.colors,
                    cells: content
                        .display_iter
                        .map(|indexed| SnapshotCell {
                            point: indexed.point,
                            selected: content.selection.is_some_and(|selection| {
                                selection.contains_cell(
                                    &indexed,
                                    content.cursor.point,
                                    content.cursor.shape,
                                )
                            }),
                            cell: indexed.cell.clone(),
                        })
                        .collect(),
                }
            })
        {
            self.snapshot = snapshot;
            self.geometry.display_offset = self.snapshot.display_offset;
            self.geometry_out.set(self.geometry);
        }

        if self.should_paint_surface() {
            let surface_bg = palette.background(&self.snapshot.colors);
            painter.fill(
                Rect::new(Point::new(DevicePx(0.0), DevicePx(0.0)), size),
                0.0,
                surface_bg,
            );
        }

        let visible_cursor = self
            .snapshot
            .cursor
            .filter(|(_, shape)| *shape != CursorShape::Hidden)
            .filter(|_| {
                !self.focused
                    || self.cursor_on
                    || !self.preferences.cursor_blink
                    || self.reduce_motion
            })
            .filter(|_| self.preedit.is_empty());

        let composition = (!self.preedit.is_empty())
            .then(|| {
                self.snapshot.cursor.map(|(point, _)| {
                    let row = point.line.0 + self.snapshot.display_offset as i32;
                    let col = self
                        .geometry
                        .composition_column(point.column.0, &self.preedit.text);
                    (row, col, col + self.preedit.text.width().max(1))
                })
            })
            .flatten();

        let mut background_runs: Vec<(Rect<DevicePx, Device>, ZguiColor)> = Vec::new();
        let mut decorations: Vec<(Rect<DevicePx, Device>, ZguiColor)> = Vec::new();
        let mut cursor_fills: Vec<(Rect<DevicePx, Device>, ZguiColor)> = Vec::new();
        let mut cursor_strokes: Vec<(Rect<DevicePx, Device>, f32, ZguiColor)> = Vec::new();
        let mut row_spans: Vec<Vec<StyledSpan>> =
            (0..self.geometry.rows).map(|_| Vec::new()).collect();

        let cursor_color = palette.resolve(Color::Named(NamedColor::Cursor), &self.snapshot.colors);
        let pixel = 1.0_f32.max(scale.round());

        for item in &self.snapshot.cells {
            let row_signed = item.point.line.0 + self.snapshot.display_offset as i32;
            let column = item.point.column.0;
            if row_signed < 0
                || row_signed as usize >= self.geometry.rows
                || column >= self.geometry.columns
            {
                continue;
            }
            let row = row_signed as usize;
            let flags = item.cell.flags;
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let cell_cols = if flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            let cell_x = self.geometry.origin_x + column as f32 * self.geometry.cell_width;
            let cell_y = self.geometry.origin_y + row as f32 * self.geometry.cell_height;
            let cell_w = cell_cols as f32 * self.geometry.cell_width;
            let cell_rect = Rect::new(
                Point::new(DevicePx(cell_x), DevicePx(cell_y)),
                Size::new(DevicePx(cell_w), DevicePx(self.geometry.cell_height)),
            );

            let mut fg = palette.foreground(item.cell.fg, flags, &self.snapshot.colors);
            let mut bg = palette.resolve(item.cell.bg, &self.snapshot.colors);
            let mut explicit_bg = item.cell.bg != Color::Named(NamedColor::Background);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
                explicit_bg = true;
            }
            if item.selected {
                fg = palette.selected_text;
                bg = palette.selection;
                explicit_bg = true;
            }

            let at_cursor = visible_cursor.filter(|(point, _)| *point == item.point);
            if self.focused && at_cursor.is_some_and(|(_, shape)| shape == CursorShape::Block) {
                bg = cursor_color;
                fg = palette.cursor_text;
                explicit_bg = true;
            }

            let obscured = composition.is_some_and(|(comp_row, start, end)| {
                row_signed == comp_row && column < end && column + cell_cols > start
            });
            if explicit_bg && !obscured {
                push_background(&mut background_runs, cell_rect, bg);
            }

            if let Some((_, shape)) = at_cursor {
                if !self.focused || shape == CursorShape::HollowBlock {
                    cursor_strokes.push((cell_rect, pixel, cursor_color));
                } else if shape == CursorShape::Beam {
                    let beam_w = (1.5 * scale).round().max(1.0);
                    cursor_fills.push((
                        Rect::new(
                            cell_rect.origin,
                            Size::new(DevicePx(beam_w), cell_rect.size.height),
                        ),
                        cursor_color,
                    ));
                } else if shape == CursorShape::Underline {
                    let ul_h = (2.0 * scale).round().max(1.0);
                    cursor_fills.push((
                        Rect::new(
                            Point::new(
                                cell_rect.origin.x,
                                DevicePx(cell_rect.origin.y.0 + cell_rect.size.height.0 - ul_h),
                            ),
                            Size::new(cell_rect.size.width, DevicePx(ul_h)),
                        ),
                        cursor_color,
                    ));
                }
            }

            let hidden = flags.contains(Flags::HIDDEN) || obscured;
            let base = if hidden || item.cell.c == '\t' {
                ' '
            } else {
                item.cell.c
            };
            let mut utf8_buf = [0_u8; 4];
            let mut combined = String::new();
            let (value, is_plain_space): (&str, bool) = if !hidden
                && let Some(combining) = item.cell.zerowidth()
                && !combining.is_empty()
            {
                combined.push(base);
                combined.extend(combining);
                (combined.as_str(), false)
            } else {
                (base.encode_utf8(&mut utf8_buf), base == ' ')
            };

            let bold = flags.contains(Flags::BOLD);
            let italic = flags.contains(Flags::ITALIC);
            let spans = &mut row_spans[row];
            let start_new = match spans.last() {
                Some(last) => {
                    !last.matches(fg, bold, italic) || (is_plain_space && last.trailing_spaces >= 3)
                }
                None => true,
            };
            if start_new {
                if !is_plain_space {
                    let mut span = StyledSpan::new(fg, bold, italic);
                    span.push_cell(column, value, false);
                    spans.push(span);
                }
            } else if let Some(last) = spans.last_mut() {
                last.push_cell(column, value, is_plain_space);
            }

            if !hidden {
                let underline_color = item.cell.underline_color().map_or(fg, |color| {
                    palette.foreground(color, flags, &self.snapshot.colors)
                });
                let thickness = self.fonts.underline_thickness;
                let ul_y = cell_y + self.fonts.underline_y;

                if flags.contains(Flags::UNDERCURL) {
                    let step = (2.0 * scale).round().max(1.0);
                    let mut x = cell_x;
                    let mut up = false;
                    while x < cell_x + cell_w {
                        let seg_w = step.min(cell_x + cell_w - x);
                        let dy = if up { -step * 0.5 } else { step * 0.5 };
                        decorations.push((
                            Rect::new(
                                Point::new(DevicePx(x), DevicePx((ul_y + dy).max(cell_y))),
                                Size::new(DevicePx(seg_w), DevicePx(thickness)),
                            ),
                            underline_color,
                        ));
                        x += step;
                        up = !up;
                    }
                } else if flags.intersects(Flags::DOTTED_UNDERLINE | Flags::DASHED_UNDERLINE) {
                    let dash = if flags.contains(Flags::DOTTED_UNDERLINE) {
                        (2.0 * scale).round().max(1.0)
                    } else {
                        (4.0 * scale).round().max(2.0)
                    };
                    let mut x = cell_x;
                    while x < cell_x + cell_w {
                        let seg_w = dash.min(cell_x + cell_w - x);
                        decorations.push((
                            Rect::new(
                                Point::new(DevicePx(x), DevicePx(ul_y)),
                                Size::new(DevicePx(seg_w), DevicePx(thickness)),
                            ),
                            underline_color,
                        ));
                        x += dash * 2.0;
                    }
                } else if flags.intersects(Flags::ALL_UNDERLINES) {
                    decorations.push((
                        Rect::new(
                            Point::new(DevicePx(cell_x), DevicePx(ul_y)),
                            Size::new(DevicePx(cell_w), DevicePx(thickness)),
                        ),
                        underline_color,
                    ));
                    if flags.contains(Flags::DOUBLE_UNDERLINE) {
                        let upper_y = (ul_y - thickness * 2.0).max(cell_y);
                        decorations.push((
                            Rect::new(
                                Point::new(DevicePx(cell_x), DevicePx(upper_y)),
                                Size::new(DevicePx(cell_w), DevicePx(thickness)),
                            ),
                            underline_color,
                        ));
                    }
                }

                if flags.contains(Flags::STRIKEOUT) {
                    decorations.push((
                        Rect::new(
                            Point::new(DevicePx(cell_x), DevicePx(cell_y + self.fonts.strikeout_y)),
                            Size::new(DevicePx(cell_w), DevicePx(self.fonts.strikeout_thickness)),
                        ),
                        fg,
                    ));
                }
            }
        }

        for (rect, color) in background_runs {
            painter.fill(rect, 0.0, color);
        }

        for (row_idx, spans) in row_spans.iter_mut().enumerate() {
            let line_origin = Point::new(
                DevicePx(self.geometry.origin_x),
                DevicePx(
                    self.geometry.origin_y
                        + row_idx as f32 * self.geometry.cell_height
                        + self.fonts.line_offset_y,
                ),
            );
            for span in spans {
                span.trim_trailing_spaces();
                shape_and_paint_span(
                    span,
                    line_origin,
                    self.geometry.cell_width,
                    &self.fonts,
                    &mut self.shaper,
                    painter,
                );
            }
        }

        for (rect, color) in decorations {
            painter.fill(rect, 0.0, color);
        }
        for (rect, color) in cursor_fills {
            painter.fill(rect, 0.0, color);
        }
        for (rect, width, color) in cursor_strokes {
            painter.stroke(rect, 0.0, width, color);
        }

        if let Some((comp_row_signed, comp_col, _)) = composition
            && comp_row_signed >= 0
            && (comp_row_signed as usize) < self.geometry.rows
        {
            let comp_row = comp_row_signed as usize;
            let row_top = self.geometry.origin_y + comp_row as f32 * self.geometry.cell_height;
            let selected = &self.preedit.cursor;
            let mut width_cols = 0_usize;
            let mut normal_span = StyledSpan::new(palette.foreground, false, false);
            let mut selected_span = StyledSpan::new(palette.selected_text, false, false);

            for (offset, grapheme) in self.preedit.text.grapheme_indices(true) {
                let cols = grapheme.width().max(1);
                if comp_col + width_cols + cols > self.geometry.columns {
                    break;
                }
                let cell_x = self.geometry.origin_x
                    + (comp_col + width_cols) as f32 * self.geometry.cell_width;
                let cell_w = cols as f32 * self.geometry.cell_width;
                let cell_rect = Rect::new(
                    Point::new(DevicePx(cell_x), DevicePx(row_top)),
                    Size::new(DevicePx(cell_w), DevicePx(self.geometry.cell_height)),
                );
                let marked = offset < selected.end && offset + grapheme.len() > selected.start;
                if marked {
                    painter.fill(cell_rect, 0.0, palette.selection);
                    selected_span.push_cell(width_cols, grapheme, false);
                } else {
                    normal_span.push_cell(width_cols, grapheme, false);
                }
                width_cols += cols;
            }

            if width_cols > 0 {
                let comp_x = self.geometry.origin_x + comp_col as f32 * self.geometry.cell_width;
                let line_origin = Point::new(
                    DevicePx(comp_x),
                    DevicePx(row_top + self.fonts.line_offset_y),
                );
                shape_and_paint_span(
                    &normal_span,
                    line_origin,
                    self.geometry.cell_width,
                    &self.fonts,
                    &mut self.shaper,
                    painter,
                );
                shape_and_paint_span(
                    &selected_span,
                    line_origin,
                    self.geometry.cell_width,
                    &self.fonts,
                    &mut self.shaper,
                    painter,
                );

                let caret_offset = self.preedit.text[..selected.end].width().min(width_cols);
                let caret_x = comp_x + caret_offset as f32 * self.geometry.cell_width;
                let caret_w = (1.5 * scale).round().max(1.0);
                painter.fill(
                    Rect::new(
                        Point::new(DevicePx(caret_x), DevicePx(row_top)),
                        Size::new(DevicePx(caret_w), DevicePx(self.geometry.cell_height)),
                    ),
                    0.0,
                    palette.foreground,
                );

                let ul_h = pixel;
                painter.fill(
                    Rect::new(
                        Point::new(
                            DevicePx(comp_x),
                            DevicePx(row_top + self.geometry.cell_height - ul_h),
                        ),
                        Size::new(
                            DevicePx(width_cols as f32 * self.geometry.cell_width),
                            DevicePx(ul_h),
                        ),
                    ),
                    0.0,
                    palette.foreground,
                );
            }
        }
    }
}

/// Convert `Line` + visible display offset to grid point cleanly.
pub(super) fn grid_point(
    geometry: GridGeometry,
    display_offset: usize,
    local_dev_x: f32,
    local_dev_y: f32,
) -> (GridPoint, Side, usize, usize) {
    let (column, row, side) = geometry.cell_at(local_dev_x, local_dev_y);
    let line = row as i32 - display_offset as i32;
    (
        GridPoint::new(Line(line), alacritty_terminal::index::Column(column)),
        side,
        column,
        row,
    )
}
