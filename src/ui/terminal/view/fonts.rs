//! Terminal monospace font resolution and snapped device-pixel cell metrics.

use zgui::app::{LineRequest, Shaper};
use zgui::geom::CssPx;
use zgui::view::Ident;

use crate::appearance::typography::resolve_monospace_family;

#[derive(Clone, Debug)]
pub(super) struct TerminalFonts {
    requested: String,
    families: Vec<Ident>,
    font_size_css_px: f32,
    line_height_multiplier: f32,
    scale: f32,
    pub device_font_size: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub line_offset_y: f32,
    pub underline_y: f32,
    pub underline_thickness: f32,
    pub strikeout_y: f32,
    pub strikeout_thickness: f32,
}

impl TerminalFonts {
    pub fn resolve(
        requested: &str,
        font_size_css_px: f32,
        line_height_multiplier: f32,
        scale: f32,
        shaper: &Shaper,
    ) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let font_size_css_px = font_size_css_px.clamp(8.0, 48.0);
        let line_height_multiplier = line_height_multiplier.clamp(1.0, 2.0);
        let device_font_size = (font_size_css_px * scale).max(1.0);

        let resolved = resolve_monospace_family(requested);
        let mut families = vec![Ident::new(resolved.as_str())];
        if !resolved.eq_ignore_ascii_case("monospace") {
            families.push(Ident::new("monospace"));
        }

        let metrics_request = LineRequest {
            families: &families,
            weight: 400,
            italic: false,
            size_device_px: device_font_size,
            letter_spacing: 0.0,
            ligatures: false,
        };

        let (
            cell_width,
            cell_height,
            line_offset_y,
            underline_y,
            underline_thickness,
            strikeout_y,
            strikeout_thickness,
        ) = if let Some(resolved_metrics) = shaper.line_metrics(&metrics_request) {
            let face = resolved_metrics.metrics;
            let cell_width = resolved_metrics.cell_advance.0.ceil().max(1.0);
            let raw_height = (face.ascent.0 + face.descent.0 + face.line_gap.0).max(1.0);
            let cell_height = (device_font_size * line_height_multiplier)
                .max(raw_height)
                .ceil()
                .max(1.0);
            let line_offset_y = ((cell_height - raw_height).max(0.0) * 0.5).floor();
            let baseline = (line_offset_y + face.ascent.0).clamp(1.0, cell_height);

            let underline_thickness = face
                .underline_thickness_or_fallback(CssPx(device_font_size))
                .0
                .round()
                .max(1.0);
            let underline_offset = face
                .underline_offset
                .map_or(2.0 * scale, |offset| -offset.0);
            let underline_y = (baseline + underline_offset)
                .clamp(0.0, (cell_height - underline_thickness).max(0.0));

            let strikeout_thickness = face
                .strikeout_thickness
                .map_or(underline_thickness, |thickness| {
                    thickness.0.round().max(1.0)
                });
            let strikeout_offset = face
                .strikeout_offset
                .map_or(face.ascent.0 * 0.35, |offset| offset.0);
            let strikeout_y = (baseline - strikeout_offset)
                .clamp(0.0, (cell_height - strikeout_thickness).max(0.0));

            (
                cell_width,
                cell_height,
                line_offset_y,
                underline_y,
                underline_thickness,
                strikeout_y,
                strikeout_thickness,
            )
        } else {
            let cell_width = (device_font_size * 0.6).ceil().max(1.0);
            let cell_height = (device_font_size * line_height_multiplier).ceil().max(1.0);
            let line_offset_y = 0.0;
            let baseline = (cell_height * 0.8).round().clamp(1.0, cell_height);
            let thickness = scale.round().max(1.0);
            let underline_y =
                (baseline + 2.0 * scale).clamp(0.0, (cell_height - thickness).max(0.0));
            let strikeout_y = (baseline * 0.65).clamp(0.0, (cell_height - thickness).max(0.0));
            (
                cell_width,
                cell_height,
                line_offset_y,
                underline_y,
                thickness,
                strikeout_y,
                thickness,
            )
        };

        Self {
            requested: requested.to_owned(),
            families,
            font_size_css_px,
            line_height_multiplier,
            scale,
            device_font_size,
            cell_width,
            cell_height,
            line_offset_y,
            underline_y,
            underline_thickness,
            strikeout_y,
            strikeout_thickness,
        }
    }

    #[must_use]
    pub fn matches(
        &self,
        requested: &str,
        font_size_css_px: f32,
        line_height_multiplier: f32,
        scale: f32,
    ) -> bool {
        self.requested == requested
            && (self.font_size_css_px - font_size_css_px.clamp(8.0, 48.0)).abs() <= f32::EPSILON
            && (self.line_height_multiplier - line_height_multiplier.clamp(1.0, 2.0)).abs()
                <= f32::EPSILON
            && (self.scale - scale).abs() <= f32::EPSILON
    }

    #[must_use]
    pub fn line_request(&self, bold: bool, italic: bool) -> LineRequest<'_> {
        LineRequest {
            families: &self.families,
            weight: if bold { 700 } else { 400 },
            italic,
            size_device_px: self.device_font_size,
            letter_spacing: 0.0,
            ligatures: false,
        }
    }
}
