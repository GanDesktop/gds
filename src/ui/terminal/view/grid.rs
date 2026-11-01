//! A device-pixel character grid with separate material, cell, glyph and cursor passes.
//!
//! The emulator owns columns. Shaping batches rows, then maps the shaped clusters
//! back to those columns so fallback fonts and combining marks cannot move a prompt.
use super::{View, fonts::Fonts, palette::Palette};
use alacritty_terminal::{
    index::{Point as GridPoint, Side},
    term::{
        cell::{Cell, Flags},
        color::Colors as TerminalColors,
    },
    vte::ansi::{Color, CursorShape, NamedColor},
};
use gpui_kit::{prelude::*, *};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

// Share the host's content-row baseline; do not add an independent grid gutter.
const INSET_X: f32 = crate::appearance::tokens::TERMINAL_CONTENT_INSET;
const INSET_Y: f32 = 4.;

#[derive(Clone)]
struct GridCell {
    point: GridPoint,
    cell: Cell,
    selected: bool,
}

#[derive(Clone, Default)]
pub(super) struct Snapshot {
    cells: Vec<GridCell>,
    pub display_offset: usize,
    pub(super) cursor: Option<(GridPoint, CursorShape)>,
    colors: TerminalColors,
}

#[derive(Clone, Copy)]
pub(super) struct Geometry {
    /// Actual grid, excluding the terminal's content padding.
    pub bounds: Bounds<Pixels>,
    pub cell: Size<Pixels>,
    columns: usize,
    rows: usize,
}
impl Default for Geometry {
    fn default() -> Self {
        Self {
            bounds: Bounds::default(),
            cell: size(px(8.), px(18.)),
            columns: 80,
            rows: 24,
        }
    }
}
impl Geometry {
    fn new(bounds: Bounds<Pixels>, cell: Size<Pixels>, scale: f32) -> Self {
        let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
        let origin = point(
            snap(bounds.left() + px(INSET_X)),
            snap(bounds.top() + px(INSET_Y)),
        );
        let width = (bounds.right() - origin.x - px(INSET_X)).max(px(0.));
        let height = (bounds.bottom() - origin.y - px(INSET_Y)).max(px(0.));
        let columns = (f32::from(width) / f32::from(cell.width)).floor().max(2.) as usize;
        let rows = (f32::from(height) / f32::from(cell.height)).floor().max(1.) as usize;
        let columns = columns.min(u16::MAX as usize);
        let rows = rows.min(u16::MAX as usize);
        Self {
            bounds: Bounds::new(
                origin,
                size(cell.width * columns as f32, cell.height * rows as f32),
            ),
            cell,
            columns,
            rows,
        }
    }
    pub fn cell_at(&self, position: Point<Pixels>) -> (usize, usize, Side) {
        let x = f32::from(position.x - self.bounds.left()) / f32::from(self.cell.width);
        let y = f32::from(position.y - self.bounds.top()) / f32::from(self.cell.height);
        (
            (x.floor().max(0.) as usize).min(self.columns.saturating_sub(1)),
            (y.floor().max(0.) as usize).min(self.rows.saturating_sub(1)),
            if x.fract() < 0.5 {
                Side::Left
            } else {
                Side::Right
            },
        )
    }
    fn cell_bounds(&self, column: usize, row: usize, width: usize) -> Bounds<Pixels> {
        Bounds::new(
            self.bounds.origin
                + point(
                    self.cell.width * column as f32,
                    self.cell.height * row as f32,
                ),
            size(self.cell.width * width as f32, self.cell.height),
        )
    }
    pub fn composition_column(&self, cursor_column: usize, text: &str) -> usize {
        cursor_column.min(self.columns.saturating_sub(text.width().min(self.columns)))
    }
}

pub(super) struct Grid {
    pub view: Entity<View>,
}
struct Glyph {
    font: FontId,
    id: GlyphId,
    position: Point<Pixels>,
    color: Hsla,
    emoji: bool,
}
struct Underline {
    origin: Point<Pixels>,
    width: Pixels,
    style: UnderlineStyle,
}
struct Cluster {
    start: usize,
    end: usize,
    column: usize,
    color: Hsla,
}
#[derive(Default)]
struct Row {
    text: String,
    runs: Vec<TextRun>,
    clusters: Vec<Cluster>,
}
impl Row {
    fn push(&mut self, column: usize, text: &str, font: Font, color: Hsla) {
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if let Some(run) = self.runs.last_mut()
            && run.font == font
            && run.color == color
        {
            run.len += end - start;
        } else {
            self.runs.push(TextRun {
                len: end - start,
                font,
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
        self.clusters.push(Cluster {
            start,
            end,
            column,
            color,
        });
    }
    fn shape(
        self,
        origin: Point<Pixels>,
        fonts: &Fonts,
        glyphs: &mut Vec<Glyph>,
        text_system: &WindowTextSystem,
    ) {
        if self.text.is_empty() {
            return;
        }
        let line = text_system.shape_line(self.text.into(), fonts.size, &self.runs, None);
        let cluster_for = |index| {
            self.clusters
                .partition_point(|c| c.end <= index)
                .min(self.clusters.len() - 1)
        };
        // Preserve each shaped cluster's internal offsets, including combining
        // glyphs, but never its proportional advance into the following cell.
        let mut anchors = vec![None::<Pixels>; self.clusters.len()];
        for run in &line.runs {
            for glyph in &run.glyphs {
                let index = cluster_for(glyph.index);
                if glyph.index == self.clusters[index].start {
                    anchors[index] =
                        Some(anchors[index].map_or(glyph.position.x, |x| x.min(glyph.position.x)));
                }
            }
        }
        for run in &line.runs {
            for glyph in &run.glyphs {
                let index = cluster_for(glyph.index);
                let cluster = &self.clusters[index];
                let anchor = anchors[index].unwrap_or(glyph.position.x);
                glyphs.push(Glyph {
                    font: run.font_id,
                    id: glyph.id,
                    position: origin
                        + point(
                            fonts.cell.width * cluster.column as f32 + glyph.position.x - anchor,
                            fonts.baseline + glyph.position.y,
                        ),
                    color: cluster.color,
                    emoji: glyph.is_emoji,
                });
            }
        }
    }
}

pub(super) struct Painted {
    background: Hsla,
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    glyphs: Vec<Glyph>,
    underlines: Vec<Underline>,
    decorations: Vec<PaintQuad>,
    cursor: Vec<PaintQuad>,
    font_size: Pixels,
}
impl Painted {
    fn background(&mut self, bounds: Bounds<Pixels>, color: Hsla) {
        // Merge adjacent equal backgrounds. The default background never enters
        // this list: it is painted once for the whole terminal surface.
        if let Some((previous, previous_color)) = self.backgrounds.last_mut()
            && *previous_color == color
            && previous.top() == bounds.top()
            && previous.right() == bounds.left()
            && previous.size.height == bounds.size.height
        {
            previous.size.width += bounds.size.width;
        } else {
            self.backgrounds.push((bounds, color));
        }
    }
}
impl IntoElement for Grid {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Grid {
    type RequestLayoutState = ();
    type PrepaintState = Painted;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        style.flex_grow = 1.;
        style.flex_shrink = 1.;
        style.min_size.height = px(0.).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Painted {
        let palette = Palette::new(cx);
        let scale = f32::from(window.scale_factor());
        self.view.update(cx, |view, _| {
            view.session.set_palette(palette.query_colors());
            if !view.fonts.as_ref().is_some_and(|fonts| {
                fonts.matches(
                    &view.preferences.font_family,
                    view.preferences.font_size,
                    scale,
                )
            }) {
                view.fonts = Some(Fonts::resolve(
                    &view.preferences.font_family,
                    view.preferences.font_size,
                    scale,
                    window.text_system(),
                ));
            }
            let fonts = view.fonts.as_ref().expect("terminal font resolved");
            let geometry = Geometry::new(bounds, fonts.cell, scale);
            if bounds.size.width > px(INSET_X * 2.)
                && bounds.size.height > px(INSET_Y * 2.)
                && (
                    view.geometry.columns,
                    view.geometry.rows,
                    view.geometry.cell,
                ) != (geometry.columns, geometry.rows, geometry.cell)
            {
                if let Err(error) = view.session.resize_with_pixels(
                    geometry.columns as u16,
                    geometry.rows as u16,
                    (f32::from(fonts.cell.width) * scale).round() as u16,
                    (f32::from(fonts.cell.height) * scale).round() as u16,
                ) {
                    view.error = Some(error.to_string());
                }
            }
            view.geometry = geometry;
            if let Some(snapshot) = view.session.try_with_term(|term| {
                let content = term.renderable_content();
                Snapshot {
                    display_offset: content.display_offset,
                    cursor: Some((content.cursor.point, content.cursor.shape)),
                    colors: *content.colors,
                    cells: content
                        .display_iter
                        .map(|indexed| GridCell {
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
            }) {
                view.snapshot = snapshot;
            }
            let snapshot = &view.snapshot;
            let background = palette.background(&snapshot.colors);
            let mut painted = Painted {
                background,
                backgrounds: Vec::new(),
                glyphs: Vec::new(),
                underlines: Vec::new(),
                decorations: Vec::new(),
                cursor: Vec::new(),
                font_size: fonts.size,
            };
            let cursor = snapshot
                .cursor
                .filter(|(_, shape)| *shape != CursorShape::Hidden)
                .filter(|_| !view.focused || view.cursor_on || !view.preferences.cursor_blink)
                .filter(|_| view.preedit.is_empty());
            let composition = (!view.preedit.is_empty())
                .then(|| {
                    snapshot.cursor.map(|(point, _)| {
                        let row = point.line.0 + snapshot.display_offset as i32;
                        let column = geometry.composition_column(point.column.0, &view.preedit);
                        (row, column, column + view.preedit.width().max(1))
                    })
                })
                .flatten();
            let mut rows: Vec<Row> = (0..geometry.rows).map(|_| Row::default()).collect();
            for item in &snapshot.cells {
                let row = item.point.line.0 + snapshot.display_offset as i32;
                let column = item.point.column.0;
                if row < 0 || row as usize >= geometry.rows || column >= geometry.columns {
                    continue;
                }
                let flags = item.cell.flags;
                if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                let width = if flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                let cell_bounds = geometry.cell_bounds(column, row as usize, width);
                let mut fg = palette.foreground(item.cell.fg, flags, &snapshot.colors);
                let mut bg = palette.resolve(item.cell.bg, &snapshot.colors);
                let mut explicit_background = item.cell.bg != Color::Named(NamedColor::Background);
                if flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                    explicit_background = true;
                }
                if item.selected {
                    fg = palette.selected_text;
                    bg = palette.selection;
                    explicit_background = true;
                }
                let at_cursor = cursor.filter(|(point, _)| *point == item.point);
                let cursor_color =
                    palette.resolve(Color::Named(NamedColor::Cursor), &snapshot.colors);
                if view.focused && at_cursor.is_some_and(|(_, shape)| shape == CursorShape::Block) {
                    bg = cursor_color;
                    fg = palette.resolve(Color::Named(NamedColor::Background), &snapshot.colors);
                    explicit_background = true;
                }
                let obscured = composition.is_some_and(|(r, start, end)| {
                    row == r && column < end && column + width > start
                });
                if explicit_background && !obscured {
                    painted.background(cell_bounds, bg);
                }
                if let Some((_, shape)) = at_cursor {
                    let pixel = px(1. / scale);
                    if !view.focused || shape == CursorShape::HollowBlock {
                        painted
                            .cursor
                            .push(outline(cell_bounds, cursor_color, BorderStyle::Solid));
                    } else if shape == CursorShape::Beam {
                        painted.cursor.push(fill(
                            Bounds::new(
                                cell_bounds.origin,
                                size(pixel.max(px(1.)), cell_bounds.size.height),
                            ),
                            cursor_color,
                        ));
                    } else if shape == CursorShape::Underline {
                        painted.cursor.push(fill(
                            Bounds::new(
                                point(cell_bounds.left(), cell_bounds.bottom() - pixel),
                                size(cell_bounds.size.width, pixel),
                            ),
                            cursor_color,
                        ));
                    }
                }
                let hidden = flags.contains(Flags::HIDDEN) || obscured;
                let mut value = if hidden || item.cell.c == '\t' {
                    " ".to_owned()
                } else {
                    item.cell.c.to_string()
                };
                if !hidden && let Some(combining) = item.cell.zerowidth() {
                    value.extend(combining);
                }
                let mut font = fonts.font.clone();
                if flags.contains(Flags::BOLD) {
                    font.weight = FontWeight::BOLD;
                }
                if flags.contains(Flags::ITALIC) {
                    font.style = FontStyle::Italic;
                }
                rows[row as usize].push(column, &value, font, fg);
                if !hidden {
                    let underline_color = item
                        .cell
                        .underline_color()
                        .map(|c| palette.foreground(c, flags, &snapshot.colors))
                        .unwrap_or(fg);
                    let thickness =
                        px((view.preferences.font_size / 14. * scale).round().max(1.) / scale);
                    let y = cell_bounds.top()
                        + fonts.baseline
                        + ((fonts.cell.height - fonts.baseline) / 2.).max(thickness);
                    if flags.intersects(Flags::ALL_UNDERLINES) {
                        painted.underlines.push(Underline {
                            origin: point(cell_bounds.left(), y),
                            width: cell_bounds.size.width,
                            style: UnderlineStyle {
                                color: Some(underline_color),
                                thickness,
                                wavy: flags.contains(Flags::UNDERCURL),
                            },
                        });
                        if flags.contains(Flags::DOUBLE_UNDERLINE) {
                            painted.decorations.push(fill(
                                Bounds::new(
                                    point(cell_bounds.left(), y - thickness * 2.),
                                    size(cell_bounds.size.width, thickness),
                                ),
                                underline_color,
                            ));
                        }
                    }
                    if flags.contains(Flags::STRIKEOUT) {
                        painted.decorations.push(fill(
                            Bounds::new(
                                point(
                                    cell_bounds.left(),
                                    cell_bounds.top() + fonts.baseline * 0.65,
                                ),
                                size(cell_bounds.size.width, thickness),
                            ),
                            fg,
                        ));
                    }
                }
            }
            for (row, content) in rows.into_iter().enumerate() {
                content.shape(
                    geometry.cell_bounds(0, row, 1).origin,
                    fonts,
                    &mut painted.glyphs,
                    window.text_system(),
                );
            }
            if let Some((row, column, _)) = composition
                && row >= 0
                && (row as usize) < geometry.rows
            {
                let mut content = Row::default();
                let mut width = 0;
                let selected =
                    super::ime::byte_range(&view.preedit, view.preedit_selection.clone());
                for (offset, grapheme) in view.preedit.grapheme_indices(true) {
                    let count = grapheme.width().max(1);
                    if column + width + count > geometry.columns {
                        break;
                    }
                    let marked = offset < selected.end && offset + grapheme.len() > selected.start;
                    if marked {
                        painted.background(
                            geometry.cell_bounds(column + width, row as usize, count),
                            palette.selection,
                        );
                    }
                    content.push(
                        width,
                        grapheme,
                        fonts.font.clone(),
                        if marked {
                            palette.selected_text
                        } else {
                            palette.foreground
                        },
                    );
                    width += count;
                }
                let caret = view.preedit[..selected.end].width().min(width);
                let caret_bounds = geometry.cell_bounds(column + caret, row as usize, 1);
                painted.cursor.push(fill(
                    Bounds::new(
                        caret_bounds.origin,
                        size(px(1. / scale), caret_bounds.size.height),
                    ),
                    palette.foreground,
                ));
                let bounds = geometry.cell_bounds(column, row as usize, width);
                content.shape(
                    bounds.origin,
                    fonts,
                    &mut painted.glyphs,
                    window.text_system(),
                );
                painted.underlines.push(Underline {
                    origin: point(bounds.left(), bounds.bottom() - px(1. / scale)),
                    width: bounds.size.width,
                    style: UnderlineStyle {
                        color: Some(palette.foreground),
                        thickness: px(1. / scale),
                        wavy: false,
                    },
                });
            }
            painted
        })
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        painted: &mut Painted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.view.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, painted.background));
            for (bounds, color) in &painted.backgrounds {
                window.paint_quad(fill(*bounds, *color));
            }
            for glyph in &painted.glyphs {
                let result = if glyph.emoji {
                    window.paint_emoji(glyph.position, glyph.font, glyph.id, painted.font_size)
                } else {
                    window.paint_glyph(
                        glyph.position,
                        glyph.font,
                        glyph.id,
                        painted.font_size,
                        glyph.color,
                    )
                };
                if let Err(error) = result {
                    eprintln!("terminal glyph paint: {error}");
                }
            }
            for line in &painted.underlines {
                window.paint_underline(line.origin, line.width, &line.style);
            }
            for quad in painted
                .decorations
                .drain(..)
                .chain(painted.cursor.drain(..))
            {
                window.paint_quad(quad);
            }
        });
    }
}
