//! Sidebar anatomy with application-owned expansion state.
use super::container::container_traits;
use crate::appearance::{
    motion,
    tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH},
};
use anti_webui::prelude::*;
use gds_uim::el;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SidebarState {
    #[default]
    Expanded,
    Collapsed,
}
impl SidebarState {
    pub fn width(self) -> f32 {
        match self {
            Self::Expanded => SIDEBAR_WIDTH,
            Self::Collapsed => COLLAPSED_SIDEBAR_WIDTH,
        }
    }
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Expanded => Self::Collapsed,
            Self::Collapsed => Self::Expanded,
        };
    }
}
/// One window-owned animation shared by frame geometry, labels and controls.
#[derive(Clone, Copy)]
pub struct SidebarPresentation {
    width: Animated<f32>,
}
impl SidebarPresentation {
    pub fn new(target: Binding<f32>) -> Self {
        Self {
            width: follow(move || target.get(), motion::WORKSPACE),
        }
    }
    pub fn width_binding(self) -> Binding<f32> {
        Binding::new(move || self.width.get(), |_| {})
    }
    pub fn progress(self) -> f32 {
        ((self.width.get() - COLLAPSED_SIDEBAR_WIDTH) / (SIDEBAR_WIDTH - COLLAPSED_SIDEBAR_WIDTH))
            .clamp(0.0, 1.0)
    }
    pub fn actions_visible(self) -> bool {
        self.progress() == 1.0
    }
}
pub struct SideBar {
    base: Div,
    state: SidebarState,
}
impl Default for SideBar {
    fn default() -> Self {
        Self {
            base: el! { .div.{ flex; flex_col; h_full; min_h: 0.0_f32; min_w: 0.0_f32; overflow_hidden; w: SIDEBAR_WIDTH } },
            state: SidebarState::default(),
        }
    }
}
container_traits!(SideBar);
impl SideBar {
    pub fn state(mut self, state: SidebarState) -> Self {
        self.state = state;
        self.base = self.base.w(state.width());
        self
    }
    pub fn width(&self) -> f32 {
        self.state.width()
    }
    pub fn width_binding(mut self, width: Binding<f32>) -> Self {
        self.base = self.base.dyn_style(move |s| s.w(width.get()));
        self
    }
    pub fn state_binding(mut self, state: impl Into<Binding<SidebarState>>) -> Self {
        let state = state.into();
        self.base = self
            .base
            .dyn_style(move |style| style.w(state.get().width()));
        self
    }
}
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
    SideBarHeader,
    el! { .div.{ flex; flex_col; flex_none; w_full; min_w:0.0_f32 } }
);
part!(
    SideBarContent,
    el! { .div.{ flex; flex_col; flex_1; min_h: 0.0_f32; overflow_y_scroll; min_w:0.0_f32 } }
);
part!(
    SideBarFooter,
    el! { .div.{ flex; flex_col; flex_none; w_full; min_w:0.0_f32 } }
);
part!(
    SideBarSection,
    el! { .div.{ flex; flex_col; flex_none; w_full; min_w:0.0_f32 } }
);
part!(
    SideBarSectionHeader,
    el! { .div.{ flex; items_center; flex_none; w_full; min_w:0.0_f32; h: Self::HEIGHT } }
);
impl SideBarSectionHeader {
    pub const HEIGHT: f32 = 34.0;
}
part!(
    SideBarSectionTitle,
    el! { .div.{ flex_1; min_w: 0.0_f32; text_size: 11.0_f32; font_weight: FontWeight::SEMIBOLD;
        dyn_style: s => $(s.text_color(theme().text_muted));
    } }
);
part!(
    SideBarSectionContent,
    el! { .div.{ flex; flex_col; w_full; min_w:0.0_f32 } }
);
