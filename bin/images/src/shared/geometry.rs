//! Viewport transform, fit modes, cursor-anchored zoom, and coordinate conversions.

#[cfg(test)]
mod tests;

use gds::service::image::PixelRect;

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point2 {
    pub x: f32,
    pub y: f32,
}

impl Point2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size2 {
    pub width: f32,
    pub height: f32,
}

impl Size2 {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect2 {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect2 {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn right(self) -> f32 {
        self.x + self.width
    }

    pub const fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub const fn center(self) -> Point2 {
        Point2::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    pub fn normalized(self) -> Self {
        let (x, width) = if self.width < 0.0 {
            (self.x + self.width, -self.width)
        } else {
            (self.x, self.width)
        };
        let (y, height) = if self.height < 0.0 {
            (self.y + self.height, -self.height)
        } else {
            (self.y, self.height)
        };
        Self::new(x, y, width, height)
    }

    pub fn expand(self, pad: f32) -> Self {
        let n = self.normalized();
        Self::new(
            n.x - pad,
            n.y - pad,
            (n.width + 2.0 * pad).max(0.0),
            (n.height + 2.0 * pad).max(0.0),
        )
    }

    pub fn union(self, other: Self) -> Self {
        let a = self.normalized();
        let b = other.normalized();
        let x0 = a.x.min(b.x);
        let y0 = a.y.min(b.y);
        let x1 = a.right().max(b.right());
        let y1 = a.bottom().max(b.bottom());
        Self::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let a = self.normalized();
        let b = other.normalized();
        let x0 = a.x.max(b.x);
        let y0 = a.y.max(b.y);
        let x1 = a.right().min(b.right());
        let y1 = a.bottom().min(b.bottom());
        let width = x1 - x0;
        let height = y1 - y0;
        (width > 0.0 && height > 0.0).then(|| Self::new(x0, y0, width, height))
    }

    pub fn intersects_inclusive(self, other: Self) -> bool {
        let a = self.normalized();
        let b = other.normalized();
        a.x <= b.right() && a.right() >= b.x && a.y <= b.bottom() && a.bottom() >= b.y
    }

    pub fn contains(self, pt: Point2) -> bool {
        let n = self.normalized();
        pt.x >= n.x && pt.x <= n.right() && pt.y >= n.y && pt.y <= n.bottom()
    }

    pub fn to_clamped_pixel_rect(self, bounds_w: u32, bounds_h: u32) -> Option<PixelRect> {
        let n = self.normalized();
        if !n.x.is_finite() || !n.y.is_finite() || !n.width.is_finite() || !n.height.is_finite() {
            return None;
        }
        let bw = bounds_w as f32;
        let bh = bounds_h as f32;
        let x0 = n.x.floor().clamp(0.0, bw) as u32;
        let y0 = n.y.floor().clamp(0.0, bh) as u32;
        let x1 = n.right().ceil().clamp(0.0, bw) as u32;
        let y1 = n.bottom().ceil().clamp(0.0, bh) as u32;
        let width = x1.saturating_sub(x0);
        let height = y1.saturating_sub(y0);
        (width > 0 && height > 0).then(|| PixelRect::new(x0, y0, width, height))
    }
}

/// Liang-Barsky segment clipping against `rect` so off-canvas segments are rejected cleanly.
pub fn clip_segment_to_rect(p0: Point2, p1: Point2, rect: Rect2) -> Option<(Point2, Point2)> {
    let n = rect.normalized();
    if !p0.x.is_finite()
        || !p0.y.is_finite()
        || !p1.x.is_finite()
        || !p1.y.is_finite()
        || !n.x.is_finite()
        || !n.y.is_finite()
        || !n.width.is_finite()
        || !n.height.is_finite()
        || n.width <= 0.0
        || n.height <= 0.0
    {
        return None;
    }

    let dx = p1.x - p0.x;
    let dy = p1.y - p0.y;
    let mut t0 = 0.0_f32;
    let mut t1 = 1.0_f32;

    let half_planes = [
        (-dx, p0.x - n.x),
        (dx, n.right() - p0.x),
        (-dy, p0.y - n.y),
        (dy, n.bottom() - p0.y),
    ];

    for (p, q) in half_planes {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }
    }

    let start = Point2::new(
        (p0.x + t0 * dx).clamp(n.x, n.right()),
        (p0.y + t0 * dy).clamp(n.y, n.bottom()),
    );
    let end = Point2::new(
        (p0.x + t1 * dx).clamp(n.x, n.right()),
        (p0.y + t1 * dy).clamp(n.y, n.bottom()),
    );
    Some((start, end))
}

/// Active viewport fitting policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FitMode {
    #[default]
    FitToWindow,
    ActualPixels,
    FillWidth,
    FillHeight,
    Custom,
}

/// Affine screen transform mapping `screen = image * scale + (pan_x, pan_y)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportTransform {
    pub scale: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub fit_mode: FitMode,
}

impl Default for ViewportTransform {
    fn default() -> Self {
        Self {
            scale: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            fit_mode: FitMode::FitToWindow,
        }
    }
}

impl ViewportTransform {
    pub fn compute_fit(mode: FitMode, viewport: Size2, image: Size2) -> Self {
        let vw = viewport.width.max(1.0);
        let vh = viewport.height.max(1.0);
        let iw = image.width.max(1.0);
        let ih = image.height.max(1.0);
        let raw_scale = match mode {
            FitMode::FitToWindow => (vw / iw).min(vh / ih),
            FitMode::ActualPixels | FitMode::Custom => 1.0,
            FitMode::FillWidth => vw / iw,
            FitMode::FillHeight => vh / ih,
        };
        let scale = raw_scale.clamp(MIN_ZOOM, MAX_ZOOM);
        let pan_x = (vw - iw * scale) * 0.5;
        let pan_y = (vh - ih * scale) * 0.5;
        let (pan_x, pan_y) = if mode == FitMode::ActualPixels {
            (pan_x.round(), pan_y.round())
        } else {
            (pan_x, pan_y)
        };
        Self {
            scale,
            pan_x,
            pan_y,
            fit_mode: mode,
        }
    }

    pub fn recenter_if_fitted(&mut self, viewport: Size2, image: Size2) {
        if self.fit_mode != FitMode::Custom {
            *self = Self::compute_fit(self.fit_mode, viewport, image);
        }
    }

    pub fn zoom_around_cursor(&mut self, factor: f32, cursor: Point2) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.viewport_to_image(cursor);
        let next_scale = (self.scale.max(MIN_ZOOM) * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scale = next_scale;
        self.pan_x = cursor.x - anchor.x * next_scale;
        self.pan_y = cursor.y - anchor.y * next_scale;
        self.fit_mode = FitMode::Custom;
    }

    pub fn set_scale_centered(&mut self, scale: f32, viewport: Size2) {
        if !scale.is_finite() {
            return;
        }
        let center = Point2::new(viewport.width * 0.5, viewport.height * 0.5);
        let anchor = self.viewport_to_image(center);
        let next_scale = scale.clamp(MIN_ZOOM, MAX_ZOOM);
        self.scale = next_scale;
        self.pan_x = center.x - anchor.x * next_scale;
        self.pan_y = center.y - anchor.y * next_scale;
        self.fit_mode = FitMode::Custom;
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan_x += dx;
        self.pan_y += dy;
        self.fit_mode = FitMode::Custom;
    }

    pub fn viewport_to_image(&self, pt: Point2) -> Point2 {
        let scale = self.scale.max(MIN_ZOOM);
        Point2::new((pt.x - self.pan_x) / scale, (pt.y - self.pan_y) / scale)
    }

    pub fn image_to_viewport(&self, pt: Point2) -> Point2 {
        let scale = self.scale.max(MIN_ZOOM);
        Point2::new(pt.x * scale + self.pan_x, pt.y * scale + self.pan_y)
    }

    pub fn image_rect_to_viewport(&self, rect: Rect2) -> Rect2 {
        let scale = self.scale.max(MIN_ZOOM);
        let top_left = self.image_to_viewport(Point2::new(rect.x, rect.y));
        Rect2::new(
            top_left.x,
            top_left.y,
            rect.width * scale,
            rect.height * scale,
        )
    }

    pub fn normalize_drag_rect(start: Point2, end: Point2) -> Rect2 {
        let x0 = start.x.min(end.x);
        let y0 = start.y.min(end.y);
        let x1 = start.x.max(end.x);
        let y1 = start.y.max(end.y);
        Rect2::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }

    pub fn clamp_image_rect(rect: Rect2, image: Size2) -> Option<PixelRect> {
        let bounds_w = image.width.max(0.0).round() as u32;
        let bounds_h = image.height.max(0.0).round() as u32;
        let x0 = rect.x.max(0.0).floor() as u32;
        let y0 = rect.y.max(0.0).floor() as u32;
        let x1 = (rect.x + rect.width).max(0.0).ceil() as u32;
        let y1 = (rect.y + rect.height).max(0.0).ceil() as u32;
        let width = x1.saturating_sub(x0);
        let height = y1.saturating_sub(y0);
        PixelRect::new(x0, y0, width, height).clamp_to(bounds_w, bounds_h)
    }
}
