use zgui::{
    elements::kurbo::Vec2,
    geom::{DevicePx, Point, Rect},
    prelude::*,
};

use crate::entities::document::RgbaColor;

use super::model::{Hsv, RING_INNER, TRIANGLE};

const DEFAULT_WHEEL_SIZE: f32 = 160.0;
const DEFAULT_SWATCH_SIZE: f32 = 14.0;

#[repr(C)]
#[derive(Clone, Copy, Default, ShaderParams)]
struct SwatchWheelParams {
    /// RGB in `0..=1`; alpha `>= 0.5` displays the chosen custom color inside the chromatic ring.
    custom: [f32; 4],
}

static SWATCH_WHEEL: ShaderEffect<SwatchWheelParams> = shader! {
    name: "gds-images-color-swatch-wheel",
    mode: Paint,
    params: SwatchWheelParams,
    source: r#"
    struct Params {
        custom: vec4<f32>,
    }
    fn shade(in: ShaderInput, params: Params) -> vec4<f32> {
        let radius = max(1., min(in.size.x, in.size.y) * 0.5);
        let p = (in.local - in.size * 0.5) / radius;
        let aa = 1. / radius;
        let d = length(p);
        let hue = fract(atan2(p.x, -p.y) / 6.2831853 + 1.);
        let outer = 1. - smoothstep(1. - aa, 1., d);
        if params.custom.a >= 0.5 {
            // Active custom color: chromatic outer ring + breathing gap + solid custom core.
            let ring_inner = 0.68;
            let core_radius = 0.54;
            let ring = smoothstep(ring_inner - aa, ring_inner, d) * outer;
            let core = 1. - smoothstep(core_radius - aa, core_radius, d);
            let ring_rgb = hsl(hue, 0.92, 0.55);
            // Faint neutral edge on the core keeps pure black/white custom colors legible on any surface.
            let core_rim = smoothstep(core_radius - aa * 1.5, core_radius - aa * 0.25, d) * core;
            let core_rgb = mix(params.custom.rgb, vec3<f32>(0.5), core_rim * 0.28);
            let alpha = clamp(ring + core, 0., 1.);
            let rgb = select(core_rgb, ring_rgb, ring > core);
            return premultiplied(rgb, alpha);
        }
        // Idle chromatic orb: vivid spectrum rim blending smoothly into a luminous center.
        let sat = smoothstep(0.06, 0.84, d) * 0.92;
        let light = mix(0.95, 0.55, smoothstep(0.0, 0.78, d));
        var rgb = hsl(hue, sat, light);
        let rim = smoothstep(1. - aa * 2.0, 1. - aa * 0.4, d);
        rgb = mix(rgb, rgb * 0.82, rim * 0.35);
        return premultiplied(rgb, outer);
    }
    "#,
};

pub(super) struct SwatchWheel {
    pub custom: Option<RgbaColor>,
    effect: ShaderHandle<SwatchWheelParams>,
}

impl SwatchWheel {
    pub fn new(custom: Option<RgbaColor>) -> Self {
        Self {
            custom,
            effect: SWATCH_WHEEL.register(),
        }
    }
}

impl CustomElement for SwatchWheel {
    fn layout(&mut self, cx: &mut CustomLayoutCx<'_>) -> CustomMeasured {
        let default_extent = DEFAULT_SWATCH_SIZE * cx.scale.max(0.01);
        CustomMeasured {
            width: cx.known_width.unwrap_or(default_extent),
            height: cx.known_height.unwrap_or(default_extent),
            ..Default::default()
        }
    }

    fn paint(&mut self, painter: &mut ScenePainter<'_>) {
        let custom = self.custom.map_or([0.0; 4], |c| {
            [
                f32::from(c.r) / 255.0,
                f32::from(c.g) / 255.0,
                f32::from(c.b) / 255.0,
                1.0,
            ]
        });
        self.effect.set_params(SwatchWheelParams { custom });
        painter.effect(
            Rect::new(Point::new(DevicePx(0.0), DevicePx(0.0)), painter.size()),
            0.0,
            &self.effect,
        );
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, ShaderParams)]
struct WheelParams {
    hue_dir: [f32; 2],
    white_dir: [f32; 2],
    marker: [f32; 2],
    radii: [f32; 2],
    tint: [f32; 4],
}

static WHEEL: ShaderEffect<WheelParams> = shader! {
    name: "gds-images-hsv-wheel",
    mode: Paint,
    params: WheelParams,
    source: r#"
    struct Params {
        hue_dir: vec2<f32>,
        white_dir: vec2<f32>,
        marker: vec2<f32>,
        radii: vec2<f32>,
        tint: vec4<f32>,
    }
    fn shade(in: ShaderInput, params: Params) -> vec4<f32> {
        let radius = max(1., min(in.size.x, in.size.y) * 0.47);
        let p = (in.local - in.size * 0.5) / radius;
        let aa = 1. / radius;
        let d = length(p);
        let hue = fract(atan2(p.x, -p.y) / 6.2831853 + 1.);
        let ring = smoothstep(params.radii.y - aa, params.radii.y, d) * (1. - smoothstep(1. - aa, 1., d));
        let factor = 2. / params.radii.x;
        let h = (1. + factor * dot(p, params.hue_dir)) / 3.;
        let w = (1. + factor * dot(p, params.white_dir)) / 3.;
        let black = 1. - h - w;
        let triangle = smoothstep(-aa, 0., min(h, min(w, black)));
        var rgb = select(clamp(params.tint.rgb * h + vec3<f32>(w), vec3<f32>(0.), vec3<f32>(1.)), hsl(hue, 1., 0.5), d >= params.radii.y - aa);
        var alpha = max(ring, triangle);
        let circles = min(abs(distance(p, params.hue_dir * (1. + params.radii.y) * 0.5) - 0.054), abs(distance(p, params.marker) - 0.055));
        let outline = 1. - smoothstep(0.015, 0.015 + aa, circles);
        let core = 1. - smoothstep(0.006, 0.006 + aa, circles);
        rgb = mix(rgb, vec3<f32>(core), outline);
        alpha = max(alpha, outline);
        return premultiplied(rgb, alpha);
    }
    "#,
};

fn vec2_to_array(vec: Vec2) -> [f32; 2] {
    [vec.x as f32, vec.y as f32]
}

pub(super) struct Wheel {
    pub hsv: Hsv,
    effect: ShaderHandle<WheelParams>,
}

impl Wheel {
    pub fn new(hsv: Hsv) -> Self {
        Self {
            hsv,
            effect: WHEEL.register(),
        }
    }
}

impl CustomElement for Wheel {
    fn layout(&mut self, cx: &mut CustomLayoutCx<'_>) -> CustomMeasured {
        let default_extent = DEFAULT_WHEEL_SIZE * cx.scale.max(0.01);
        CustomMeasured {
            width: cx.known_width.unwrap_or(default_extent),
            height: cx.known_height.unwrap_or(default_extent),
            ..Default::default()
        }
    }

    fn paint(&mut self, painter: &mut ScenePainter<'_>) {
        let [hue_axis, white_axis, _] = self.hsv.axes();
        let pure_hue = Hsv {
            saturation: 1.0,
            value: 1.0,
            ..self.hsv
        }
        .rgb();
        self.effect.set_params(WheelParams {
            hue_dir: vec2_to_array(hue_axis),
            white_dir: vec2_to_array(white_axis),
            marker: vec2_to_array(self.hsv.marker()),
            radii: [TRIANGLE as f32, RING_INNER as f32],
            tint: [
                f32::from(pure_hue.r) / 255.0,
                f32::from(pure_hue.g) / 255.0,
                f32::from(pure_hue.b) / 255.0,
                1.0,
            ],
        });
        painter.effect(
            Rect::new(Point::new(DevicePx(0.0), DevicePx(0.0)), painter.size()),
            0.0,
            &self.effect,
        );
    }
}
