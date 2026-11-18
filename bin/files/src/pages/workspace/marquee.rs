//! Blank-space rectangle selection uses native pointer capture and tab-owned identities.
use super::{
    geometry::{GridMotion, SCROLLBAR_LANE, virtual_row_height},
    model::{Origin, Row, RowKey},
    ui::Browser,
};
use crate::shared::lib::selection::Selection;
use anti_webui::prelude::*;
use gds::{app::Gds, appearance::tokens::DataListStyle};
use gds_uim::el;
use std::{cell::RefCell, ops::Range, rc::Rc};

#[derive(Clone, Copy)]
pub(super) enum Layout {
    List { viewport: NodeRef },
    Grid(GridMotion),
}

impl Layout {
    fn candidates(self, top: f32, bottom: f32, count: usize) -> Range<usize> {
        match self {
            Self::List { .. } => {
                let height = virtual_row_height(DataListStyle::ROW_HEIGHT);
                let first = (top.max(0.0) / height).floor() as usize;
                let end = (bottom.max(0.0) / height).ceil() as usize;
                first.min(count)..end.min(count)
            }
            Self::Grid(grid) => grid.candidates(top, bottom, count),
        }
    }
}

struct Viewport {
    surface: RectF,
    rows: RectF,
    visible: RectF,
    scroll: PointF,
}

impl Viewport {
    fn new(surface: NodeRef, list: NodeRef, layout: Layout) -> Option<Self> {
        let surface = surface.logical_rect()?;
        let rows = list.logical_rect()?;
        let clip = match layout {
            Layout::List { viewport } => viewport.logical_rect()?,
            Layout::Grid(_) => surface,
        };
        let visible = intersection(rows, clip)?;
        let visible = RectF {
            height: (visible.height - SCROLLBAR_LANE).max(0.0),
            ..visible
        };
        Some(Self {
            surface,
            rows,
            visible,
            scroll: list.scroll_offset(),
        })
    }

    fn blank_at(&self, point: PointF, layout: Layout, count: usize) -> bool {
        if point.x < self.surface.x
            || point.x >= self.visible.right()
            || point.y < self.visible.y
            || point.y >= self.visible.bottom()
        {
            return false;
        }
        let x = point.x - self.rows.x + self.scroll.x;
        let y = point.y - self.rows.y + self.scroll.y;
        match layout {
            Layout::List { .. } => {
                point.x < self.visible.x
                    || (y / virtual_row_height(DataListStyle::ROW_HEIGHT)).floor() as usize >= count
            }
            Layout::Grid(grid) => !grid.candidates(y, y, count).any(|index| {
                let tile = grid.tile(index);
                x >= tile.x && x < tile.right() && y >= tile.y && y < tile.bottom()
            }),
        }
    }

    fn selection(&self, start: PointF, current: PointF) -> Option<RectF> {
        intersection(
            RectF::new(
                start.x.min(current.x),
                start.y.min(current.y),
                (current.x - start.x).abs(),
                (current.y - start.y).abs(),
            ),
            self.visible,
        )
    }

    fn content_rect(&self, rect: RectF) -> RectF {
        RectF::new(
            rect.x - self.rows.x + self.scroll.x,
            rect.y - self.rows.y + self.scroll.y,
            rect.width,
            rect.height,
        )
    }

    fn overlay_rect(&self, rect: RectF) -> RectF {
        RectF::new(
            rect.x - self.surface.x,
            rect.y - self.surface.y,
            rect.width,
            rect.height,
        )
    }
}

fn intersection(left: RectF, right: RectF) -> Option<RectF> {
    let x = left.x.max(right.x);
    let y = left.y.max(right.y);
    let width = left.right().min(right.right()) - x;
    let height = left.bottom().min(right.bottom()) - y;
    (width > 0.0 && height > 0.0).then(|| RectF::new(x, y, width, height))
}

fn logical_point(position: PointF) -> PointF {
    let scale = window_scale();
    PointF::new(scale.to_logical(position.x), scale.to_logical(position.y))
}

struct Marquee {
    origin: Origin,
    start: PointF,
    selection: Selection<RowKey>,
    cursor: Option<RowKey>,
    anchor: Option<RowKey>,
    merge: bool,
}

impl Browser {
    fn marquee_blank(&self, surface: NodeRef, layout: Layout, position: PointF) -> bool {
        let list = self.0.list;
        !list.is_over_scrollbar(position)
            && Viewport::new(surface, list, layout).is_some_and(|viewport| {
                let count = self.read(|state| state.tab().rows.len());
                viewport.blank_at(logical_point(position), layout, count)
            })
    }

    fn marquee_current(&self, drag: &Marquee) -> bool {
        self.read(|state| {
            let tab = state.tab();
            tab.id == drag.origin.tab && tab.navigation.location() == &drag.origin.location
        })
    }

    fn apply_marquee(&self, drag: &Marquee, layout: Layout, rect: Option<RectF>) {
        self.update_selection(|state| {
            if !state.allows_multiple() {
                return;
            }
            let tab = state.tab();
            let hits = rect.map_or(0..0, |rect| {
                layout.candidates(rect.y, rect.bottom(), tab.rows.len())
            });
            if matches!(layout, Layout::List { .. }) && !drag.merge && state.picker.is_none() {
                let first = (!hits.is_empty()).then(|| tab.rows[hits.start].key());
                let last = (!hits.is_empty()).then(|| tab.rows[hits.end - 1].key());
                let tab = state.tab_mut();
                let view = tab.navigation.view_mut();
                if hits.is_empty() {
                    view.selection.clear();
                } else {
                    view.selection
                        .set_range(&tab.backing, hits.start, hits.end - 1);
                }
                view.anchor = first;
                view.cursor = last;
                return;
            }
            let keys = hits
                .filter_map(|index| {
                    let row = &tab.rows[index];
                    if let Layout::Grid(grid) = layout
                        && rect
                            .and_then(|rect| intersection(rect, grid.tile(index)))
                            .is_none()
                    {
                        return None;
                    }
                    state
                        .picker
                        .as_ref()
                        .is_none_or(
                            |picker| matches!(row, Row::File(entry) if picker.accepts_entry(entry)),
                        )
                        .then(|| row.key())
                })
                .collect::<Vec<_>>();
            let first = keys.first().cloned();
            let last = keys.last().cloned();
            let view = state.tab_mut().navigation.view_mut();
            view.selection.replace(
                drag.selection
                    .iter()
                    .filter(|_| drag.merge)
                    .cloned()
                    .chain(keys),
            );
            view.anchor = if drag.merge {
                drag.anchor.clone().or(first)
            } else {
                first
            };
            view.cursor = last.or_else(|| drag.merge.then(|| drag.cursor.clone()).flatten());
        });
    }

    pub(super) fn marquee_surface(&self, content: Div, layout: Layout) -> Div {
        let surface = node_ref();
        let selection = signal(None::<RectF>);
        let drag = Rc::new(RefCell::new(None::<Marquee>));
        let grab = self.clone();
        let click = self.clone();
        el! {
            .div.{
                relative; flex; flex_col; flex_1; min_w: 0.0_f32; min_h: 0.0_f32;
                node_ref: surface; overflow_hidden;
                on_click: move event => ${
                    if event.button == Some(PointerButton::Primary)
                        && click.marquee_blank(surface, layout, event.position)
                    {
                        if !event.modifiers.primary() && !event.modifiers.shift {
                            click.update_selection(|state| {
                                let view = state.tab_mut().navigation.view_mut();
                                view.selection.clear();
                                view.cursor = None;
                                view.anchor = None;
                            });
                        }
                        click.focus_content();
                    }
                };
                on_grab: move event => ${
                    match event.phase {
                        GrabPhase::Start => {
                            if !grab.marquee_blank(surface, layout, event.position)
                                || !grab.read(|state| state.allows_multiple())
                            {
                                return;
                            }
                            let baseline = grab.read(|state| {
                                let view = state.tab().navigation.view();
                                Marquee {
                                    origin: grab.origin(),
                                    start: logical_point(event.position),
                                    selection: view.selection.clone(),
                                    cursor: view.cursor.clone(),
                                    anchor: view.anchor.clone(),
                                    merge: event.modifiers.primary() || event.modifiers.shift,
                                }
                            });
                            *drag.borrow_mut() = Some(baseline);
                            grab.focus_content();
                        }
                        GrabPhase::Move => {
                            let drag = drag.borrow();
                            let Some(drag) = drag.as_ref() else { return; };
                            let Some(viewport) = Viewport::new(surface, grab.0.list, layout)
                                .filter(|_| grab.marquee_current(drag))
                                else {
                                    selection.set(None);
                                    return;
                                };
                            let rect = viewport.selection(drag.start, logical_point(event.position));
                            grab.apply_marquee(drag, layout, rect.map(|rect| viewport.content_rect(rect)));
                            selection.set(rect.map(|rect| viewport.overlay_rect(rect)));
                        }
                        GrabPhase::End => {
                            drag.borrow_mut().take();
                            selection.set(None);
                        }
                        GrabPhase::Cancel => {
                            if let Some(drag) = drag.borrow_mut().take()
                                && grab.marquee_current(&drag)
                            {
                                grab.update_selection(|state| {
                                    let view = state.tab_mut().navigation.view_mut();
                                    view.selection = drag.selection;
                                    view.cursor = drag.cursor;
                                    view.anchor = drag.anchor;
                                });
                            }
                            selection.set(None);
                        }
                    }
                };
            }.[
                content,
                .div.{
                    absolute; z_index: 1; pointer_events_none; border: 1.0_f32;
                    dyn_style: move style => ${
                        let Some(rect) = selection.get() else { return style.hidden(); };
                        let colors = Gds::current().appearance().theme().colors();
                        style.block().left(rect.x).top(rect.y).w(rect.width).h(rect.height)
                            .border_color(colors.primary).bg(colors.drag_target())
                    };
                },
            ]
        }
    }
}
