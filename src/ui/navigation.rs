//! Navigation anatomy shared by settings and workspace sidebars.
use super::{Icon, container::container_traits};
use crate::appearance::tokens::{SIDEBAR_ICON_SIZE, SIDEBAR_ITEM_HEIGHT};
use anti_webui::prelude::*;
use gds_uim::el;
use std::rc::Rc;

macro_rules! part {
    ($name:ident, $base:expr) => {
        pub struct $name {
            base: Div,
        }
        impl Default for $name {
            fn default() -> Self {
                Self { base: $base }
            }
        }
        container_traits!($name);
    };
}
part!(
    NavItemContent,
    el! { .div.{ relative; flex; flex_col; w_full; min_w:0.0_f32 } }
);
part!(
    NavItemHeader,
    el! { .div.{ flex; items_center; flex_none; w_full; min_w:0.0_f32; h:SIDEBAR_ITEM_HEIGHT; px:8.0_f32; gap:8.0_f32 } }
);
part!(
    NavItemLabel,
    el! { .div.{ flex_1; min_w:0.0_f32; overflow_hidden; whitespace_nowrap } }
);
part!(NavItemDetails, el! { .div.{ w_full; min_w:0.0_f32 } });
part!(
    NavItemActions,
    el! { .div.{ absolute; right:0.0_f32; top:3.0_f32; flex } }
);

pub struct NavItem {
    base: Div,
    icon: Option<Icon>,
    label: Rc<dyn Fn() -> String>,
    active: Rc<dyn Fn() -> bool>,
    expanded: Rc<dyn Fn() -> bool>,
    presentation: Option<super::sidebar::SidebarPresentation>,
    keytips: Vec<super::keytips::Anchor>,
    trailing: Vec<View>,
    corner: Option<View>,
}
impl NavItem {
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            base: div(),
            label: Rc::new(move || label.clone()),
            icon: None,
            active: Rc::new(|| false),
            expanded: Rc::new(|| true),
            presentation: None,
            keytips: Vec::new(),
            trailing: Vec::new(),
            corner: None,
        }
    }
    pub fn label_fn(mut self, label: impl Fn() -> String + 'static) -> Self {
        self.label = Rc::new(label);
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = Rc::new(move || active);
        self
    }
    pub fn active_when(mut self, active: impl Fn() -> bool + 'static) -> Self {
        self.active = Rc::new(active);
        self
    }
    pub fn expanded_when(mut self, expanded: impl Fn() -> bool + 'static) -> Self {
        self.expanded = Rc::new(expanded);
        self
    }
    pub fn presentation(mut self, presentation: super::sidebar::SidebarPresentation) -> Self {
        self.presentation = Some(presentation);
        self
    }
    pub fn keytip(self, stroke: &'static str) -> Self {
        self.keytip_when(stroke, || true)
    }
    pub fn keytip_when(
        mut self,
        stroke: &'static str,
        available: impl Fn() -> bool + 'static,
    ) -> Self {
        self.keytips.push(
            super::keytips::Anchor::new(stroke)
                .trailing()
                .when(available),
        );
        self
    }
    pub fn trailing(mut self, control: impl IntoView) -> Self {
        self.trailing.push(control.into_view());
        self
    }
    pub fn corner_action(mut self, control: impl IntoView) -> Self {
        self.corner = Some(control.into_view());
        self
    }
    pub fn on_click(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.base = self.base.on_click(callback);
        self
    }
    pub fn on_aux_click(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.base = self.base.on_aux_click(callback);
        self
    }
}
impl IntoView for NavItem {
    fn into_view(self) -> View {
        let Self {
            base,
            icon,
            label,
            active,
            expanded,
            presentation,
            keytips,
            trailing,
            corner,
        } = self;
        let accessible_label = label.clone();
        let tooltip_label = label.clone();
        let accessible_active = active.clone();
        let hovered = signal(false);
        let focused = signal(false);
        let layout = expanded.clone();
        let actions = expanded.clone();
        let trailing_width = trailing.len() as f32 * 32.0;
        let item = el! { base.{
            relative; flex; w_full; min_w: 0.0_f32; rounded: 6.0_f32;
            cursor: CursorIcon::Pointer; focusable; select_none;
            on_hover: move value => hovered.{ set: value };
            on_focus_visible: move value => focused.{ set: value };
            access: move a => $(a.role(Role::Button).label(accessible_label()).selected(accessible_active()));
            dyn_style: move s => ${
                let palette = crate::app::Gds::current().appearance().theme().colors();
                s.bg(if active() { palette.selection } else if hovered.get() { palette.sidebar_hover() } else { Color::TRANSPARENT })
                    .text_color(if active() { palette.selection_foreground } else { palette.sidebar_foreground })
                    .outline(if focused.get() { 1.0_f32 } else { 0.0_f32 })
                    .outline_offset(-1.0_f32)
                    .outline_color(palette.primary)
            };
        }.[
            .NavItemContent::default.[
                .NavItemHeader::default.{
                    dyn_style: move s => $({ let progress = presentation.map_or_else(|| if layout() {1.0} else {0.0}, |p| p.progress()); s.pl(10.0 - 2.0 * progress).pr(10.0 + (trailing_width - 2.0) * progress) });
                }.[
                    if let Some(icon) = icon [$(icon.view(SIDEBAR_ICON_SIZE))],
                    .NavItemLabel::default.{
                        dyn_style: move s => $(if let Some(presentation) = presentation { s.opacity(presentation.progress()) } else if expanded() { s } else { s.hidden() });
                    }.[(move () => .label).text.{ ellipsis }],
                ],
                .NavItemActions::default.{
                    dyn_style: move s => $(if presentation.map_or_else(||actions(), |p| p.actions_visible()) && (hovered.get() || focused.get()) { s.flex() } else { s.hidden() });
                }.[..trailing],
                if let Some(corner) = corner [
                    .div.{absolute; top: 0.0_f32; right: 0.0_f32;
                        dyn_style: move s => $(if presentation.is_some_and(|p| p.progress() == 0.0)
                            && (hovered.get() || focused.get()) {s.flex()} else {s.hidden()});
                    }.[corner],
                ],
            ],
            ..keytips,
        ] };
        tooltip_with(item, TooltipOptions::new(), move |_| {
            let label = tooltip_label.clone();
            move || {
                let label = label.clone();
                el! { .div.{
                    max_w:280.0_f32; px:8.0_f32; py:4.0_f32; rounded:4.0_f32;
                    dyn_style:s=>$(s.bg(theme().tooltip).text_color(theme().on_tooltip)
                        .text_size(crate::app::Gds::current().appearance().typography().scaled(12.0)));
                }.[(move()=>.label).text] }
            }
        }).into_view()
    }
}
