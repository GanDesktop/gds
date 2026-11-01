//! Blank-space interaction shared by uniform content lists and tables.

use std::{cell::RefCell, rc::Rc};

use gds_uim::el;
use gpui_kit::{prelude::*, *};

use crate::appearance::tokens::DataListStyle;

#[derive(Clone, Copy)]
struct Geometry {
    rows: usize,
    row_height: Pixels,
    header_height: Pixels,
    inset_x: Pixels,
    bottom_inset: Pixels,
    scrollbar_width: Pixels,
}

impl Geometry {
    fn is_blank(self, bounds: Bounds<Pixels>, position: Point<Pixels>, scroll_y: Pixels) -> bool {
        if !bounds.contains(&position) {
            return false;
        }
        let local = position - bounds.origin;
        if local.y < self.header_height || local.y >= bounds.size.height - self.bottom_inset {
            return false;
        }
        let right = bounds.size.width - self.inset_x;
        if local.x >= right - self.scrollbar_width && local.x < right {
            return false;
        }
        local.x < self.inset_x
            || local.x >= right
            || local.y >= self.header_height + self.row_height * self.rows as f32 + scroll_y
    }
}

type BlankPress = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct ListSurface {
    id: ElementId,
    content: AnyElement,
    geometry: Geometry,
    scroll: ScrollHandle,
    on_blank_press: Option<BlankPress>,
}

impl ListSurface {
    pub fn new(
        id: impl Into<ElementId>,
        content: AnyElement,
        rows: usize,
        scroll: ScrollHandle,
    ) -> Self {
        Self {
            id: id.into(),
            content,
            scroll,
            geometry: Geometry {
                rows,
                row_height: px(DataListStyle::ROW_HEIGHT),
                header_height: px(DataListStyle::HEADER_HEIGHT),
                inset_x: px(0.),
                bottom_inset: px(DataListStyle::SCROLLBAR_LANE),
                scrollbar_width: px(DataListStyle::SCROLLBAR_LANE),
            },
            on_blank_press: None,
        }
    }

    pub fn row_height(mut self, height: Pixels) -> Self {
        self.geometry.row_height = height;
        self
    }

    pub fn header_height(mut self, height: Pixels) -> Self {
        self.geometry.header_height = height;
        self
    }

    pub fn inset_x(mut self, inset: Pixels) -> Self {
        self.geometry.inset_x = inset;
        self
    }

    pub fn bottom_inset(mut self, inset: Pixels) -> Self {
        self.geometry.bottom_inset = inset;
        self
    }

    pub fn on_blank_press(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_blank_press = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ListSurface {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let bounds = Rc::new(RefCell::new(Bounds::default()));
        let measured = bounds.clone();
        let measure = canvas(
            move |bounds, _, _| *measured.borrow_mut() = bounds,
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        let geometry = self.geometry;
        let scroll = self.scroll;
        let handler = self.on_blank_press;
        let id = self.id;
        let content = self.content;
        let on_press = move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            // Read the live scroll offset at the press, including scrolling
            // performed by a child table without rerendering its parent page.
            if matches!(event.button, MouseButton::Left | MouseButton::Right)
                && geometry.is_blank(*bounds.borrow(), event.position, scroll.offset().y)
                && let Some(handler) = &handler
            {
                handler(event, window, cx);
            }
        };
        el! { .div { # id; relative; flex; flex_col; flex_1; min_h_0; min_w_0; overflow_hidden;
            capture_any_mouse_down: on_press;
        } [content, measure] }
    }
}

#[cfg(test)]
mod tests;
