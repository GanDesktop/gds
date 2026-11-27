//! Window translucency and surface material roles.
use std::fmt::Write as _;
use zgui::canvas::zgui_color::Color;

use super::color::color_css;

/// Resolved surface materials separating window translucency from opaque control backgrounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub content: Color,
    pub sidebar: Color,
    pub content_opacity: f32,
    pub sidebar_opacity: f32,
}

impl Material {
    /// Background color for the primary content surface with configured alpha applied.
    pub fn content_background(self) -> Color {
        self.content.with_alpha(self.content_opacity)
    }

    /// Background color for the sidebar and toolbar chrome with configured alpha applied.
    pub fn sidebar_background(self) -> Color {
        self.sidebar.with_alpha(self.sidebar_opacity)
    }

    /// Whether either surface asks the compositor for alpha translucency.
    pub fn is_translucent(self) -> bool {
        self.content_opacity < 1.0 || self.sidebar_opacity < 1.0
    }

    pub(crate) fn write_css_declarations(self, out: &mut String) {
        let _ = writeln!(
            out,
            "    --gds-content-bg: {};",
            color_css(self.content_background())
        );
        let _ = writeln!(out, "    --gds-content-opacity: {};", self.content_opacity);
        let _ = writeln!(
            out,
            "    --gds-sidebar-bg: {};",
            color_css(self.sidebar_background())
        );
        let _ = writeln!(out, "    --gds-sidebar-opacity: {};", self.sidebar_opacity);
    }
}
