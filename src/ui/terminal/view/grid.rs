//! Paint the visible VT cells as a single native drawing.
use super::Palette;
use crate::service::terminal::Session;
use alacritty_terminal::{
    grid::GridCell as _,
    index::{Point as GridPoint, Side},
    term::{
        cell::{Cell, Flags},
        color::Colors,
    },
    vte::ansi::{Color as TerminalColor, CursorShape, NamedColor},
};
use anti_webui::{
    core::{
        drawing::{Drawing, DrawingItem},
        style::ComputedStyle,
        text::{ShapeStyle, ShapedText, TextLayout, shape_glyphs, with_text},
    },
    prelude::*,
};
use std::collections::HashMap;

const INSET_X: f32 = crate::appearance::tokens::TERMINAL_CONTENT_INSET;
const INSET_Y: f32 = 4.0;
const GLYPH_CACHE_LIMIT: usize = 4096;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Geometry {
    pub width: f32,
    pub height: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    scale: Scale,
    inset: PointF,
    baseline: f32,
}

impl Geometry {
    pub fn new(width: f32, height: f32, font: &Style, scale: Scale) -> Self {
        let mut computed = ComputedStyle::default();
        font.apply_to(&mut computed);
        let shape = ShapeStyle::from_computed(&computed, scale);
        let (advance, height_px, baseline) = with_text(|text| {
            let mut layout = TextLayout::default();
            layout.shape(text, "0", 1, &shape);
            layout.break_lines(None);
            let advance = layout.layout.width();
            (
                advance,
                layout.size().height as f32,
                layout.first_baseline(),
            )
        });
        Self {
            width,
            height,
            cell_width: scale.to_logical(advance.floor().max(1.0)),
            cell_height: scale.to_logical(height_px.max(1.0)),
            scale,
            inset: PointF::new(
                scale.to_logical(scale.px(INSET_X) as f32),
                scale.to_logical(scale.px(INSET_Y) as f32),
            ),
            baseline: scale.to_logical(baseline.round()),
        }
    }

    pub fn with_size(self, width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            ..self
        }
    }

    pub fn pty_size(self) -> Option<[u16; 4]> {
        (self.width > self.inset.x + INSET_X && self.height > self.inset.y + INSET_Y).then(|| {
            [
                self.columns(),
                self.rows(),
                self.scale.px(self.cell_width) as u16,
                self.scale.px(self.cell_height) as u16,
            ]
        })
    }

    fn columns(self) -> u16 {
        ((self.width - self.inset.x - INSET_X) / self.cell_width)
            .floor()
            .clamp(2.0, u16::MAX as f32) as u16
    }

    fn rows(self) -> u16 {
        ((self.height - self.inset.y - INSET_Y) / self.cell_height)
            .floor()
            .clamp(1.0, u16::MAX as f32) as u16
    }

    pub fn point(self, position: PointF) -> (usize, usize, Side) {
        let column = (self.scale.to_logical(position.x) - self.inset.x) / self.cell_width;
        let row = (self.scale.to_logical(position.y) - self.inset.y) / self.cell_height;
        (
            column.floor().clamp(0.0, f32::from(self.columns() - 1)) as usize,
            row.floor().clamp(0.0, f32::from(self.rows() - 1)) as usize,
            if column.fract() < 0.5 {
                Side::Left
            } else {
                Side::Right
            },
        )
    }

    fn origin(self, row: i32, column: usize) -> PointF {
        PointF::new(
            self.inset.x + column as f32 * self.cell_width,
            self.inset.y + row as f32 * self.cell_height,
        )
    }

    fn visible(self, row: i32, column: usize) -> bool {
        (0..i32::from(self.rows())).contains(&row) && column < usize::from(self.columns())
    }
}

struct VisibleCell {
    point: GridPoint,
    cell: Cell,
    selected: bool,
}

#[derive(Default)]
pub(super) struct Snapshot {
    cells: Vec<VisibleCell>,
    colors: Colors,
    offset: i32,
    cursor: Option<(GridPoint, CursorShape)>,
}

impl Snapshot {
    pub fn refresh(&mut self, session: &Session) -> bool {
        session
            .try_with_term(|term| {
                let content = term.renderable_content();
                self.offset = content.display_offset as i32;
                self.colors = *content.colors;
                self.cursor = Some((content.cursor.point, content.cursor.shape));
                self.cells.clear();
                self.cells
                    .extend(content.display_iter.filter_map(|indexed| {
                        let cell = indexed.cell;
                        let selected = content.selection.is_some_and(|selection| {
                            selection.contains_cell(
                                &indexed,
                                content.cursor.point,
                                content.cursor.shape,
                            )
                        });
                        let visible =
                            !cell.is_empty() || selected || content.cursor.point == indexed.point;
                        visible.then(|| VisibleCell {
                            point: indexed.point,
                            cell: cell.clone(),
                            selected,
                        })
                    }));
            })
            .is_some()
    }

    pub fn cursor(&self, geometry: Geometry) -> PointF {
        self.cursor.map_or(geometry.inset, |(point, _)| {
            geometry.origin((point.line.0 + self.offset).max(0), point.column.0)
        })
    }

    pub fn background(&self, palette: &Palette) -> Color {
        palette.background(&self.colors)
    }
}

#[derive(Default)]
pub(super) struct Painter {
    family: String,
    size: f32,
    glyphs: HashMap<(char, Flags), ShapedText>,
}

impl Painter {
    pub fn font(&mut self, family: &str, size: f32) {
        if self.family != family || self.size != size {
            self.family = family.to_owned();
            self.size = size;
            self.glyphs.clear();
        }
    }

    pub fn draw(
        &mut self,
        snapshot: &Snapshot,
        geometry: Geometry,
        palette: &Palette,
        focused: bool,
        cursor_on: bool,
    ) -> Drawing {
        let cursor = snapshot.cursor.filter(|(point, shape)| {
            (!focused || cursor_on)
                && *shape != CursorShape::Hidden
                && geometry.visible(point.line.0 + snapshot.offset, point.column.0)
        });
        let cursor_color =
            palette.resolve(TerminalColor::Named(NamedColor::Cursor), &snapshot.colors);
        let mut drawing = Drawing::new(geometry.width, 0.0, geometry.height);
        let mut glyphs = Vec::new();
        let mut decorations = Vec::new();
        for item in &snapshot.cells {
            let row = item.point.line.0 + snapshot.offset;
            let column = item.point.column.0;
            let cell = &item.cell;
            let flags = cell.flags;
            if !geometry.visible(row, column)
                || flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let position = geometry.origin(row, column);
            let width = if flags.contains(Flags::WIDE_CHAR) {
                2.0
            } else {
                1.0
            } * geometry.cell_width;
            let mut foreground = palette.foreground(cell.fg, flags, &snapshot.colors);
            let mut background = palette.resolve(cell.bg, &snapshot.colors);
            let mut has_background = cell.bg != TerminalColor::Named(NamedColor::Background);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
                has_background = true;
            }
            if item.selected {
                foreground = palette.selected_text;
                background = palette.selection;
                has_background = true;
            }
            if focused && cursor == Some((item.point, CursorShape::Block)) {
                foreground = palette.cursor_text;
                background = cursor_color;
                has_background = true;
            }
            if has_background {
                Self::rect(
                    &mut drawing.items,
                    position,
                    width,
                    geometry.cell_height,
                    background,
                );
            }
            if flags.contains(Flags::HIDDEN) {
                continue;
            }
            if !matches!(cell.c, ' ' | '\t') || cell.zerowidth().is_some() {
                let baseline = PointF::new(position.x, position.y + geometry.baseline);
                self.glyphs(cell, baseline, foreground, &mut glyphs);
            }
            if flags.intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE) {
                let bottom = position.y + geometry.cell_height - 1.0;
                let color = cell.underline_color().map_or(foreground, |color| {
                    palette.foreground(color, flags, &snapshot.colors)
                });
                Self::rect(
                    &mut decorations,
                    PointF::new(position.x, bottom),
                    width,
                    1.0,
                    color,
                );
                if flags.contains(Flags::DOUBLE_UNDERLINE) {
                    Self::rect(
                        &mut decorations,
                        PointF::new(position.x, bottom - 2.0),
                        width,
                        1.0,
                        color,
                    );
                }
            }
        }
        drawing.items.extend(glyphs);
        drawing.items.extend(decorations);
        if let Some((point, shape)) = cursor {
            let position = geometry.origin(point.line.0 + snapshot.offset, point.column.0);
            Self::caret(
                &mut drawing.items,
                geometry,
                position,
                cursor_color,
                if focused {
                    shape
                } else {
                    CursorShape::HollowBlock
                },
            );
        }
        drawing
    }

    fn glyphs(
        &mut self,
        cell: &Cell,
        baseline: PointF,
        color: Color,
        output: &mut Vec<DrawingItem>,
    ) {
        let flags = cell.flags & (Flags::BOLD | Flags::ITALIC);
        let key = (cell.c, flags);
        let shape = |text: &str| {
            let mut style = Style::default()
                .font_family(self.family.as_str())
                .text_size(self.size);
            if flags.contains(Flags::BOLD) {
                style = style.font_weight(FontWeight::BOLD);
            }
            if flags.contains(Flags::ITALIC) {
                style = style.italic();
            }
            shape_glyphs(text, &style)
        };
        let combined;
        let shaped = if let Some(combining) = cell.zerowidth().filter(|marks| !marks.is_empty()) {
            let mut text = cell.c.to_string();
            text.extend(combining);
            combined = shape(&text);
            &combined
        } else {
            if self.glyphs.len() >= GLYPH_CACHE_LIMIT {
                self.glyphs.clear();
            }
            self.glyphs
                .entry(key)
                .or_insert_with(|| shape(cell.c.encode_utf8(&mut [0; 4])))
        };
        for run in &shaped.runs {
            let glyphs = run.glyphs.iter().map(|glyph| {
                let mut glyph = *glyph;
                glyph.x += baseline.x;
                glyph.y += baseline.y;
                glyph
            });
            if let Some(DrawingItem::Glyphs {
                font,
                size,
                color: ink,
                glyphs: previous,
            }) = output.last_mut()
                && *ink == Some(color)
                && *size == run.size
                && *font == run.font
            {
                previous.extend(glyphs);
            } else {
                output.push(DrawingItem::Glyphs {
                    font: run.font.clone(),
                    size: run.size,
                    color: Some(color),
                    glyphs: glyphs.collect(),
                });
            }
        }
    }

    fn rect(items: &mut Vec<DrawingItem>, at: PointF, width: f32, height: f32, color: Color) {
        if let Some(DrawingItem::Rect {
            x,
            y,
            width: previous,
            height: previous_height,
            color: ink,
        }) = items.last_mut()
            && (*x + *previous - at.x).abs() < 0.01
            && *y == at.y
            && *previous_height == height
            && *ink == Some(color)
        {
            *previous = at.x + width - *x;
        } else {
            items.push(DrawingItem::Rect {
                x: at.x,
                y: at.y,
                width,
                height,
                color: Some(color),
            });
        }
    }

    fn caret(
        items: &mut Vec<DrawingItem>,
        geometry: Geometry,
        at: PointF,
        color: Color,
        shape: CursorShape,
    ) {
        let pixel = geometry.scale.to_logical(1.0);
        let width = geometry.cell_width;
        let height = geometry.cell_height;
        let bottom = PointF::new(at.x, at.y + height - pixel);
        match shape {
            CursorShape::Underline => Self::rect(items, bottom, width, pixel, color),
            CursorShape::Beam => Self::rect(items, at, 1.0, height, color),
            CursorShape::HollowBlock => {
                let right = PointF::new(at.x + width - pixel, at.y);
                Self::rect(items, at, width, pixel, color);
                Self::rect(items, bottom, width, pixel, color);
                Self::rect(items, at, pixel, height, color);
                Self::rect(items, right, pixel, height, color);
            }
            CursorShape::Block | CursorShape::Hidden => {}
        }
    }
}
