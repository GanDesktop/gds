//! Uniform borderless list surface, `:focus-visible` inset contour, blank-space hit geometry, and virtualization.

#[cfg(test)]
mod tests;

use zgui::geom::{Css, CssPx, Point, Rect, Size};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback};
use zgui::view::{AttrName, ListenerOptions, events};
use zgui::vocab::PointerButton;
use zgui::{component, css, view};

pub use crate::appearance::tokens::DataListStyle;
pub use zgui_ui::virtualize::{VirtualList, VirtualListProps, VirtualWindow, Virtualize};

const LIST_SHEET: &str = "gds-ui-list";

const LIST_CSS: &str = css!(
    ".gds-list-surface.gds-list-surface {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
        background: transparent;
        border: none;
        outline: none;
        box-shadow: none;
    }
    .gds-list-surface.gds-list-surface:focus:not(:focus-visible),
    .gds-list-surface__scroll.gds-list-surface__scroll:focus:not(:focus-visible),
    .gds-list-surface .zui-scroll-area__viewport:focus:not(:focus-visible) {
        outline: none;
        box-shadow: none;
    }
    .gds-list-surface.gds-list-surface:focus-visible,
    .gds-list-surface__scroll.gds-list-surface__scroll:focus-visible,
    .gds-list-surface .zui-scroll-area__viewport:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
        box-shadow: none;
    }
    .gds-list-surface__scroll.gds-list-surface__scroll {
        display: block;
        flex: 1 1 0px;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow-y: auto;
        overflow-x: hidden;
    }
    .gds-list-surface .zui-scroll-area[data-orientation=\"vertical\"] > .zui-scroll-area__viewport {
        overflow-x: hidden;
    }
    .gds-list-surface__items {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
    }
    .gds-list-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        width: 100%;
        min-width: 0px;
        min-height: var(--gds-list-row-height, 40px);
        padding: var(--gds-list-cell-padding-y, 4px) var(--gds-list-cell-padding-x, 12px);
        border-radius: var(--gds-list-row-radius, 6px);
        color: var(--gds-foreground);
        font-size: var(--gds-list-name-size, var(--gds-text-sm, 14px));
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        outline: none;
        box-shadow: none;
        transition:
            background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-list-surface[data-style=\"striped\"] .gds-list-row[data-odd=\"true\"],
    .gds-list-surface[data-style=\"striped\"] .gds-list-row:not([data-index]):nth-child(even) {
        background-color: color-mix(in oklab, var(--gds-foreground) 3%, transparent);
    }
    .gds-list-row:hover {
        background-color: var(--gds-hover);
    }
    .gds-list-row[data-selected=\"true\"],
    .gds-list-row[data-selected=\"true\"]:hover {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
    }
    .gds-list-row:focus:not(:focus-visible) {
        outline: none;
        box-shadow: none;
    }
    .gds-list-row:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
        box-shadow: none;
    }"
);

fn install_list_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(LIST_SHEET, LIST_CSS);
}

/// Visual density/alternating style for [`ListSurface`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListStyle {
    #[default]
    Plain,
    Striped,
}

impl ListStyle {
    const fn attr_value(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Striped => "striped",
        }
    }
}

/// Geometry helper determining whether a press inside a [`ListSurface`] hit blank space
/// rather than the header, a populated row, or scrollbar lanes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListGeometry {
    pub rows: usize,
    pub row_height: f32,
    pub header_height: f32,
    pub inset_x: f32,
    pub bottom_inset: f32,
    pub scrollbar_width: f32,
}

impl Default for ListGeometry {
    fn default() -> Self {
        Self {
            rows: 0,
            row_height: DataListStyle::ROW_HEIGHT,
            header_height: DataListStyle::HEADER_HEIGHT,
            inset_x: 0.0,
            bottom_inset: DataListStyle::SCROLLBAR_LANE,
            scrollbar_width: DataListStyle::SCROLLBAR_LANE,
        }
    }
}

impl ListGeometry {
    /// Returns whether `position` hits empty background outside populated rows
    /// (side `inset_x` margins or space below the last row), where `scroll_y`
    /// is the downward scroll offset in CSS px (`ScrollPosition::offset.y / scale`).
    #[must_use]
    pub fn is_blank(
        self,
        bounds: Rect<CssPx, Css>,
        position: Point<CssPx, Css>,
        scroll_y: CssPx,
    ) -> bool {
        if position.x.0 < bounds.origin.x.0
            || position.y.0 < bounds.origin.y.0
            || position.x.0 >= bounds.origin.x.0 + bounds.size.width.0
            || position.y.0 >= bounds.origin.y.0 + bounds.size.height.0
        {
            return false;
        }
        let local_x = position.x.0 - bounds.origin.x.0;
        let local_y = position.y.0 - bounds.origin.y.0;
        if local_y < self.header_height || local_y >= bounds.size.height.0 - self.bottom_inset {
            return false;
        }
        let right = bounds.size.width.0 - self.inset_x;
        if local_x >= right - self.scrollbar_width && local_x < right {
            return false;
        }
        local_x < self.inset_x
            || local_x >= right
            || local_y >= self.header_height + self.row_height * self.rows as f32 - scroll_y.0
    }
}

struct BlankPressConfig {
    scroll_ref: Option<NodeRef>,
    rows: Signal<usize, LocalStorage>,
    row_height: Signal<f32, LocalStorage>,
    header_height: Signal<f32, LocalStorage>,
    inset_x: Signal<f32, LocalStorage>,
    bottom_inset: Signal<f32, LocalStorage>,
    on_blank_press: Option<UnsyncCallback<()>>,
}

fn render_list_surface(
    style: ListStyle,
    node_ref: Option<NodeRef>,
    blank: BlankPressConfig,
    class: Classes,
    attrs: Attrs,
    content: AnyView,
) -> impl IntoView {
    install_list_stylesheets();
    let surface_ref = node_ref.unwrap_or_default();
    let mut own = Attrs::new().attribute(AttrName::new("data-style"), style.attr_value());

    if let Some(on_blank) = blank.on_blank_press {
        let BlankPressConfig {
            scroll_ref,
            rows,
            row_height,
            header_height,
            inset_x,
            bottom_inset,
            ..
        } = blank;
        own = own.listener(
            events::POINTER_DOWN,
            ListenerOptions::CAPTURE,
            move |ev: &mut EventCx<'_, events::PointerDown>| {
                if !matches!(
                    ev.button,
                    Some(PointerButton::Primary | PointerButton::Secondary)
                ) {
                    return;
                }
                let Some(bounds_dev) = surface_ref.window_bounds() else {
                    return;
                };
                let scale = surface_ref.scale();
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                let bounds = Rect::new(
                    Point::new(
                        CssPx(bounds_dev.origin.x.0 / scale),
                        CssPx(bounds_dev.origin.y.0 / scale),
                    ),
                    Size::new(
                        CssPx(bounds_dev.size.width.0 / scale),
                        CssPx(bounds_dev.size.height.0 / scale),
                    ),
                );
                let scroll_y = scroll_ref.map_or(CssPx(0.0), |scroll| {
                    let scroll_scale = scroll.scale();
                    let s = if scroll_scale.is_finite() && scroll_scale > 0.0 {
                        scroll_scale
                    } else {
                        scale
                    };
                    CssPx(scroll.scroll_position().offset.y.0 / s)
                });
                let geometry = ListGeometry {
                    rows: rows.get_untracked(),
                    row_height: row_height.get_untracked(),
                    header_height: header_height.get_untracked(),
                    inset_x: inset_x.get_untracked(),
                    bottom_inset: bottom_inset.get_untracked(),
                    scrollbar_width: DataListStyle::SCROLLBAR_LANE,
                };
                if geometry.is_blank(bounds, ev.position, scroll_y) {
                    on_blank.run(());
                }
            },
        );
    }

    view! {
        column(
            class = "gds-list-surface",
            node_ref = surface_ref,
            {..own},
            class = class,
            {..attrs}
        ) {
            {content}
        }
    }
}

fn default_row_height_signal() -> Signal<f32, LocalStorage> {
    Signal::derive_local(|| {
        crate::app::Gds::try_current().map_or(DataListStyle::ROW_HEIGHT, |gds| {
            gds.appearance()
                .typography()
                .scaled(DataListStyle::ROW_HEIGHT)
        })
    })
}

fn default_header_height_signal() -> Signal<f32, LocalStorage> {
    Signal::derive_local(|| {
        crate::app::Gds::try_current().map_or(DataListStyle::HEADER_HEIGHT, |gds| {
            gds.appearance()
                .typography()
                .scaled(DataListStyle::HEADER_HEIGHT)
        })
    })
}

/// Borderless list container with `:focus-visible`-only `1px` inset contour and
/// built-in capture-phase blank-space press detection.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn ListSurface(
    #[prop(default = ListStyle::Plain)] style: ListStyle,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(optional)] scroll_ref: Option<NodeRef>,
    #[prop(into, default = Signal::stored_local(0_usize))] rows: Signal<usize, LocalStorage>,
    #[prop(into, default = default_row_height_signal())] row_height: Signal<f32, LocalStorage>,
    #[prop(into, default = default_header_height_signal())] header_height: Signal<
        f32,
        LocalStorage,
    >,
    #[prop(into, default = Signal::stored_local(0.0_f32))] inset_x: Signal<f32, LocalStorage>,
    #[prop(into, default = Signal::stored_local(DataListStyle::SCROLLBAR_LANE))]
    bottom_inset: Signal<f32, LocalStorage>,
    #[prop(optional)] on_blank_press: Option<UnsyncCallback<()>>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    render_list_surface(
        style,
        node_ref,
        BlankPressConfig {
            scroll_ref,
            rows,
            row_height,
            header_height,
            inset_x,
            bottom_inset,
            on_blank_press,
        },
        class,
        attrs,
        children.into_view_once(),
    )
}

/// Standard data-list row carrying selection and optional alternating stripe attributes.
#[component]
pub fn ListRow(
    #[prop(optional)] index: Option<usize>,
    #[prop(into, default = Signal::stored_local(false))] selected: Signal<bool, LocalStorage>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_list_stylesheets();
    let mut own = Attrs::new()
        .attribute(AttrName::new("data-index"), index.map(|i| i.to_string()))
        .attribute(
            AttrName::new("data-odd"),
            index.is_some_and(|i| i % 2 == 1).then(|| "true".to_owned()),
        )
        .attribute(AttrName::new("data-selected"), move || {
            selected.get().then(|| "true".to_owned())
        })
        .a11y_from(A11yBinding::new(Role::ListItem).selected(move || selected.get()));
    if let Some(i) = index {
        own = own.a11y_from(A11yBinding::unspecified().row_index(i));
    }

    view! {
        row(
            class = "gds-list-row",
            node_ref = node_ref.unwrap_or_default(),
            {..own},
            {..attrs},
            class = class
        ) {
            {children.into_view_once()}
        }
    }
}

/// Virtualized [`ListSurface`] building only rows visible inside its scrollport plus `overscan`.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn VirtualizedListSurface<V, F>(
    #[prop(into)] count: Signal<usize, LocalStorage>,
    #[prop(into, default = default_row_height_signal())] row_size: Signal<f32, LocalStorage>,
    #[prop(default = 4)] overscan: usize,
    #[prop(default = ListStyle::Plain)] list_style: ListStyle,
    #[prop(into, optional)] label: Option<String>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(optional)] surface_ref: Option<NodeRef>,
    #[prop(into, default = Signal::stored_local(0.0_f32))] header_height: Signal<f32, LocalStorage>,
    #[prop(into, default = Signal::stored_local(0.0_f32))] inset_x: Signal<f32, LocalStorage>,
    #[prop(into, default = Signal::stored_local(DataListStyle::SCROLLBAR_LANE))]
    bottom_inset: Signal<f32, LocalStorage>,
    #[prop(optional)] on_blank_press: Option<UnsyncCallback<()>>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    row: F,
) -> impl IntoView
where
    V: IntoView + 'static,
    F: Fn(usize) -> V + 'static,
{
    install_list_stylesheets();
    let viewport = node_ref.unwrap_or_default();
    let seen = Virtualize::new(viewport, count, row_size, overscan);
    let window = Signal::derive_local(move || seen.window());
    let row = std::rc::Rc::new(row);

    let mut semantics = A11yBinding::new(Role::List);
    if let Some(text) = label {
        semantics = semantics.label(text);
    }

    let scroll_view = AnyView::new(view! {
        scroll(
            node_ref = viewport,
            class = "gds-list-surface__scroll",
            {..Attrs::new().a11y_from(semantics)}
        ) {
            column(
                class = "gds-list-surface__items",
                style:padding-top = move || Some(format!("{}px", window.get().lead)),
                style:padding-bottom = move || Some(format!("{}px", window.get().trail))
            ) {
                for index in move || window.get().indices(), key = |index: &usize| *index {
                    {row(index)}
                }
            }
        }
    });

    render_list_surface(
        list_style,
        surface_ref,
        BlankPressConfig {
            scroll_ref: Some(viewport),
            rows: count,
            row_height: row_size,
            header_height,
            inset_x,
            bottom_inset,
            on_blank_press,
        },
        class,
        attrs,
        scroll_view,
    )
}
