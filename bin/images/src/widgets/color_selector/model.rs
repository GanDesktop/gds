//! Barycentric HSV triangle and hex RGB parsing. Its vertices are the pure hue, white and black.

#[cfg(test)]
mod tests;

use std::f64::consts::TAU;

use zgui::elements::kurbo::Vec2;

use crate::entities::document::RgbaColor;

pub(super) const TRIANGLE: f64 = 0.73;
pub(super) const RING_INNER: f64 = 0.80;

pub(super) fn format_hex(color: RgbaColor) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

pub(super) fn parse_hex_rgb(input: &str) -> Option<RgbaColor> {
    let trimmed = input.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(RgbaColor::rgb(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Hsv {
    pub hue: f64,
    pub saturation: f64,
    pub value: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Part {
    Hue,
    Triangle,
}

impl Hsv {
    pub fn from_rgb(rgb: RgbaColor, previous_hue: f64) -> Self {
        let [r, g, b] = [rgb.r, rgb.g, rgb.b].map(|channel| f64::from(channel) / 255.0);
        let value = r.max(g).max(b);
        let chroma = value - r.min(g).min(b);
        let hue = if chroma == 0.0 {
            previous_hue
        } else {
            let sector = if value == r {
                (g - b) / chroma
            } else if value == g {
                2.0 + (b - r) / chroma
            } else {
                4.0 + (r - g) / chroma
            };
            (sector / 6.0).rem_euclid(1.0)
        };
        Self {
            hue,
            saturation: if value == 0.0 { 0.0 } else { chroma / value },
            value,
        }
    }

    pub fn rgb(self) -> RgbaColor {
        let channel = |offset: f64| {
            let k = (offset + self.hue * 6.0).rem_euclid(6.0);
            (255.0 * self.value * (1.0 - self.saturation * k.min(4.0 - k).clamp(0.0, 1.0))).round()
                as u8
        };
        RgbaColor::rgb(channel(5.0), channel(3.0), channel(1.0))
    }

    pub fn axes(self) -> [Vec2; 3] {
        [0.0, -1.0 / 3.0, 1.0 / 3.0].map(|offset| {
            let (sin, cos) = ((self.hue + offset) * TAU).sin_cos();
            Vec2::new(sin, -cos)
        })
    }

    fn weights(self, point: Vec2) -> [f64; 3] {
        self.axes()
            .map(|axis| (1.0 + 2.0 * point.dot(axis) / TRIANGLE) / 3.0)
    }

    pub fn marker(self) -> Vec2 {
        let [hue, white, black] = self.axes();
        let chroma = self.value * self.saturation;
        TRIANGLE * (hue * chroma + white * (self.value - chroma) + black * (1.0 - self.value))
    }

    pub fn hit(self, point: Vec2) -> Option<Part> {
        if (RING_INNER..=1.0).contains(&point.hypot()) {
            Some(Part::Hue)
        } else if self.weights(point).iter().all(|weight| *weight >= -1e-6) {
            Some(Part::Triangle)
        } else {
            None
        }
    }

    pub fn pick(mut self, part: Part, point: Vec2) -> Self {
        match part {
            Part::Hue => {
                if point.hypot() > 1e-6 {
                    self.hue = (point.x.atan2(-point.y) / TAU).rem_euclid(1.0);
                }
            }
            Part::Triangle => {
                let mut weights = self.weights(point);
                if weights.iter().any(|weight| *weight < 0.0) {
                    let vertices = self.axes().map(|axis| axis * TRIANGLE);
                    let projected = (0..3)
                        .map(|index| {
                            let start = vertices[index];
                            let edge = vertices[(index + 1) % 3] - start;
                            let factor =
                                ((point - start).dot(edge) / edge.dot(edge)).clamp(0.0, 1.0);
                            start + edge * factor
                        })
                        .min_by(|left, right| {
                            (*left - point).hypot().total_cmp(&(*right - point).hypot())
                        })
                        .expect("a triangle has three edges");
                    weights = self.weights(projected);
                }
                self.value = (weights[0] + weights[1]).clamp(0.0, 1.0);
                if self.value > 1e-6 {
                    self.saturation = (weights[0] / self.value).clamp(0.0, 1.0);
                }
            }
        }
        self
    }
}
