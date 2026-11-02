use std::time::Duration;

use gpui_kit::{
    App, Window,
    base::motion::{self, Transition, TransitionId},
};

use super::SideBar;

const DURATION: Duration = Duration::from_millis(160);

/// One host-owned sample shared by every part of the sidebar column.
/// This is presentation only: selection, section state and scrolling stay with the host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarPresentation {
    width: f32,
    progress: f32,
}

impl Default for SidebarPresentation {
    fn default() -> Self {
        Self {
            width: crate::appearance::tokens::SIDEBAR_WIDTH,
            progress: 1.0,
        }
    }
}

impl SidebarPresentation {
    pub fn width(self) -> f32 {
        self.width
    }

    /// Zero at the narrow endpoint, one at the expanded endpoint.
    pub fn progress(self) -> f32 {
        self.progress
    }

    /// Interpolate a local horizontal measurement using the column's single clock.
    pub fn interpolate(self, expanded: f32, collapsed: f32) -> f32 {
        collapsed + (expanded - collapsed) * self.progress
    }

    /// Only text fades; surfaces and main icons retain their original opacity.
    pub fn label_opacity(self) -> f32 {
        self.progress
    }

    /// A clipped or faded trailing control must not retain a hit target.
    pub fn actions_visible(self) -> bool {
        self.progress == 1.0
    }
}

impl SideBar {
    /// Sample once in the host's render, then pass the value to every column part.
    /// A stable host-local ID preserves reversal continuity without another clock.
    pub fn sample(
        &self,
        id: impl Into<TransitionId>,
        window: &mut Window,
        cx: &mut App,
    ) -> SidebarPresentation {
        let width = motion::transition(
            id,
            self.target_width(),
            Transition::new(DURATION),
            window,
            cx,
        );
        let span = self.expanded_width - self.collapsed_width;
        let progress = if span == 0.0 {
            1.0
        } else {
            ((width - self.collapsed_width) / span).clamp(0.0, 1.0)
        };
        SidebarPresentation { width, progress }
    }
}
