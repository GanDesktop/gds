use zgui::reactive::{LocalStorage, Signal, prelude::*};

use crate::appearance::{motion, tokens};

/// Application-controlled sidebar state shared across layout, shortcuts, and persistence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarState {
    #[default]
    Expanded,
    Collapsed,
}

impl SidebarState {
    #[must_use]
    pub const fn is_collapsed(self) -> bool {
        matches!(self, Self::Collapsed)
    }

    #[must_use]
    pub const fn from_collapsed(collapsed: bool) -> Self {
        if collapsed {
            Self::Collapsed
        } else {
            Self::Expanded
        }
    }
}

/// Host-sampled sidebar reveal geometry shared across sidebar sections and navigation items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarPresentation {
    width: f32,
    progress: f32,
}

impl Default for SidebarPresentation {
    fn default() -> Self {
        Self {
            width: tokens::SIDEBAR_WIDTH,
            progress: 1.0,
        }
    }
}

impl SidebarPresentation {
    #[must_use]
    pub fn from_state(state: SidebarState) -> Self {
        Self::from_collapsed(state.is_collapsed())
    }

    #[must_use]
    pub fn from_collapsed(collapsed: bool) -> Self {
        if collapsed {
            Self {
                width: tokens::COLLAPSED_SIDEBAR_WIDTH,
                progress: 0.0,
            }
        } else {
            Self::default()
        }
    }

    #[must_use]
    pub fn from_width(width: f32, expanded: f32, collapsed: f32) -> Self {
        let lo = collapsed.min(expanded);
        let hi = expanded.max(collapsed);
        let width = if width.is_finite() {
            width.clamp(lo, hi)
        } else {
            expanded
        };
        let span = expanded - collapsed;
        let progress = if span.abs() <= f32::EPSILON {
            1.0
        } else {
            ((width - collapsed) / span).clamp(0.0, 1.0)
        };
        Self { width, progress }
    }

    #[must_use]
    pub fn width(self) -> f32 {
        self.width
    }

    #[must_use]
    pub fn progress(self) -> f32 {
        self.progress
    }

    #[must_use]
    pub fn is_collapsed(self) -> bool {
        self.progress == 0.0
    }

    #[must_use]
    pub fn collapsed(self) -> bool {
        self.is_collapsed()
    }

    #[must_use]
    pub fn interpolate(self, expanded: f32, collapsed: f32) -> f32 {
        collapsed + (expanded - collapsed) * self.progress
    }

    #[must_use]
    pub fn label_opacity(self) -> f32 {
        self.progress
    }

    #[must_use]
    pub fn actions_opacity(self) -> f32 {
        self.progress
    }

    #[must_use]
    pub fn actions_visible(self) -> bool {
        self.progress == 1.0
    }
}

/// Continuously interpolate [`SidebarPresentation`] over `160ms` (`ease-out cubic`) using standard
/// GDS sidebar widths (`224px` expanded / `56px` collapsed).
#[must_use]
pub fn use_sidebar_presentation(
    state: Signal<SidebarState, LocalStorage>,
) -> Signal<SidebarPresentation, LocalStorage> {
    use_sidebar_presentation_widths(
        state,
        tokens::SIDEBAR_WIDTH,
        tokens::COLLAPSED_SIDEBAR_WIDTH,
    )
}

/// Continuously interpolate [`SidebarPresentation`] over `160ms` (`ease-out cubic`) between custom
/// `expanded_width` and `collapsed_width` endpoints.
#[must_use]
pub fn use_sidebar_presentation_widths(
    state: Signal<SidebarState, LocalStorage>,
    expanded_width: f32,
    collapsed_width: f32,
) -> Signal<SidebarPresentation, LocalStorage> {
    let target = Signal::derive_local(move || match state.get() {
        SidebarState::Expanded => expanded_width,
        SidebarState::Collapsed => collapsed_width,
    });
    let width = motion::use_transition(target, motion::WORKSPACE_DURATION_MS);
    Signal::derive_local(move || {
        SidebarPresentation::from_width(width.get(), expanded_width, collapsed_width)
    })
}
