//! Driven E2E and real-GPU visual gallery harness for headless/offscreen workspace verification.

use std::process::ExitCode;

#[cfg(not(feature = "files-e2e"))]
pub(crate) fn run() -> ExitCode {
    eprintln!("gds-files built without `files-e2e`; rebuild with `--features files-e2e`");
    ExitCode::FAILURE
}

#[cfg(feature = "files-e2e")]
pub(crate) fn run() -> ExitCode {
    let mode = std::env::var("GDS_E2E_MODE").unwrap_or_else(|_| "driven".to_owned());
    let outcome = match mode.as_str() {
        "gallery" => driven::execute_gallery(),
        _ => driven::execute(),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Files E2E {mode} runner failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "files-e2e")]
mod driven {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::fmt::Write as _;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, ensure};
    use gds::app::Gds;
    use gds::appearance::theme::{Appearance, BuiltinTheme, ModePreference, Source};
    use gds::i18n::Language;
    use gds::service::desktop::protocol::chooser;
    use gds::service::remote::Address;
    use gds::service::task_manager::Supervisor;
    use gds::ui::frame::FrameProps;
    use gds::ui::picker::directory::{
        DirectoryPicker, DirectoryPickerDialogProps, Entry as DirEntry, Location as DirLocation,
        Page as DirPage,
    };
    use gds::ui::window::{TitleBarProps, workspace_options};
    use zgui::app::app;
    use zgui::atlas::{MemorySink, TextureId, TextureKind, TextureSink};
    use zgui::bits::DamageSet;
    use zgui::geom::{Css, CssPx, DevicePx, Matrix4, Point, Rect, Size, transformed_bounds};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent, SurfaceId};
    use zgui::prelude::{AnyView, GetUntracked, RwSignal, Set, UnsyncCallback};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, ScrollShift, TextureHandle, VectorStatus,
    };
    use zgui::runtime::try_use_windows;
    use zgui::scene::{ClipId, ColorSprite, Paint, PaintId, PaintKind, Placements, Scene};
    use zgui::view;
    use zgui::vocab::{
        ImeEvent, KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction,
        PointerButton, PointerEvent, Role, Timestamp,
    };
    use zgui_platform_headless::{Harness, OffscreenSurface};
    use zgui_render_wgpu::{Pixels, SharedGraphics, WgpuRenderer, wgpu};

    use crate::entities::file::{ViewMode, directory::Entry};
    use crate::entities::location::{Location, places::Places, scheme::TagAddress, volumes};
    use crate::entities::preferences::{Preferences, Service as PreferencesService};
    use crate::entities::tags;
    use crate::features::pick_files;
    use crate::pages::workspace::{
        inspection::Properties,
        menus::Surface as MenuSurface,
        status::Kind as StatusKind,
        ui::{Browser, LaunchOptions},
    };
    use crate::shared::{i18n as files_i18n, lib::clipboard::Clipboard};

    const WINDOW_WIDTH: f32 = 1280.0;
    const WINDOW_HEIGHT: f32 = 900.0;
    const GALLERY_WIDTH: f32 = 1280.0;
    const GALLERY_HEIGHT: f32 = 800.0;
    const COMPACT_WIDTH: f32 = 860.0;
    const COMPACT_HEIGHT: f32 = 640.0;
    const CLEAR_PIXEL: image::Rgba<u8> = image::Rgba([24, 24, 28, 255]);

    #[derive(Default)]
    struct CaptureState {
        latest_rgba: Option<image::RgbaImage>,
        frames: u64,
        gpu_backed: bool,
    }

    #[derive(Clone, Default)]
    struct CaptureRegistry {
        primary: Rc<RefCell<CaptureState>>,
        by_surface: Rc<RefCell<HashMap<SurfaceId, Rc<RefCell<CaptureState>>>>>,
        warned_fallback: Rc<Cell<bool>>,
    }

    impl CaptureRegistry {
        fn state_for(&self, id: SurfaceId) -> Rc<RefCell<CaptureState>> {
            let mut map = self.by_surface.borrow_mut();
            if let Some(existing) = map.get(&id) {
                return Rc::clone(existing);
            }
            let state = if map.is_empty() {
                Rc::clone(&self.primary)
            } else {
                Rc::new(RefCell::new(CaptureState::default()))
            };
            map.insert(id, Rc::clone(&state));
            state
        }
    }

    fn pixels_to_rgba(pixels: &Pixels) -> image::RgbaImage {
        let size = pixels.size();
        let width = u32::try_from(size.width.max(1)).unwrap_or(1);
        let height = u32::try_from(size.height.max(1)).unwrap_or(1);
        let raw = pixels.bytes();
        let mut rgba = Vec::with_capacity(raw.len());
        let [bg_r, bg_g, bg_b, _] = CLEAR_PIXEL.0;
        // `WgpuRenderer` composites into a premultiplied offscreen target; blend over the
        // headless desktop backdrop (`CLEAR_PIXEL`) so PNG captures store unpremultiplied
        // opaque RGBA matching a Wayland compositor screenshot.
        let blend = |r: u8, g: u8, b: u8, a: u8| -> [u8; 4] {
            if a == 255 {
                [r, g, b, 255]
            } else {
                let inv = u16::from(255 - a);
                [
                    (u16::from(r) + (u16::from(bg_r) * inv + 127) / 255).min(255) as u8,
                    (u16::from(g) + (u16::from(bg_g) * inv + 127) / 255).min(255) as u8,
                    (u16::from(b) + (u16::from(bg_b) * inv + 127) / 255).min(255) as u8,
                    255,
                ]
            }
        };
        if pixels.is_bgra() {
            for &[b, g, r, a] in raw.as_chunks::<4>().0 {
                rgba.extend_from_slice(&blend(r, g, b, a));
            }
        } else {
            for &[r, g, b, a] in raw.as_chunks::<4>().0 {
                rgba.extend_from_slice(&blend(r, g, b, a));
            }
        }
        image::RgbaImage::from_raw(width, height, rgba)
            .unwrap_or_else(|| image::RgbaImage::from_pixel(width, height, CLEAR_PIXEL))
    }

    struct GpuCaptureRenderer {
        renderer: WgpuRenderer,
        capture: Rc<RefCell<CaptureState>>,
    }

    impl Renderer for GpuCaptureRenderer {
        fn capabilities(&self) -> RenderCapabilities {
            self.renderer.capabilities()
        }

        fn configure(&mut self, target: RenderTarget) {
            self.renderer.configure(target);
        }

        fn composed_survives_configure(&self) -> bool {
            self.renderer.composed_survives_configure()
        }

        fn shifts_composed_pixels(&self) -> bool {
            self.renderer.shifts_composed_pixels()
        }

        fn shift_composed(&mut self, shift: ScrollShift) {
            self.renderer.shift_composed(shift);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.renderer.target()
        }

        fn vector_status(&self) -> VectorStatus {
            self.renderer.vector_status()
        }

        fn draw(&mut self, scene: &Scene, damage: &DamageSet) -> FrameOutcome {
            let outcome = self.renderer.draw(scene, damage);
            let mut guard = self.capture.borrow_mut();
            guard.frames = guard.frames.saturating_add(1);
            if matches!(outcome, FrameOutcome::Presented(_)) || guard.latest_rgba.is_none() {
                let pixels = self
                    .renderer
                    .read_presented()
                    .unwrap_or_else(|| self.renderer.read_composed());
                guard.latest_rgba = Some(pixels_to_rgba(&pixels));
            }
            outcome
        }

        fn register_external(&mut self, texture: ExternalTexture) -> TextureHandle {
            self.renderer.register_external(texture)
        }

        fn release_external(&mut self, handle: TextureHandle) {
            self.renderer.release_external(handle);
        }

        fn memory(&self) -> MemoryReport {
            self.renderer.memory()
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            self.renderer.texture_sink()
        }

        fn as_any_mut(&mut self) -> Option<&mut dyn core::any::Any> {
            self.renderer.as_any_mut()
        }

        fn as_any(&self) -> Option<&dyn core::any::Any> {
            self.renderer.as_any()
        }
    }

    fn renderer_factory(
        registry: CaptureRegistry,
    ) -> Box<dyn FnMut(&Arc<dyn Surface>, RenderTarget) -> Result<Box<dyn Renderer>, zgui::Error>>
    {
        let graphics = SharedGraphics::new();
        Box::new(move |surface, target| {
            let state = registry.state_for(surface.id());
            match graphics.renderer_offscreen(target, wgpu::TextureFormat::Bgra8Unorm, false) {
                Ok(mut renderer) => {
                    zgui_render_vector_vello::attach(&mut renderer, target.size);
                    state.borrow_mut().gpu_backed = true;
                    Ok(Box::new(GpuCaptureRenderer {
                        renderer,
                        capture: state,
                    }) as Box<dyn Renderer>)
                }
                Err(failure) => {
                    if !registry.warned_fallback.replace(true) {
                        eprintln!(
                            "Files E2E: no usable wgpu adapter ({failure}); falling back to CPU CaptureRenderer"
                        );
                    }
                    let mut fallback = CaptureRenderer::with_state(state);
                    fallback.configure(target);
                    Ok(Box::new(fallback) as Box<dyn Renderer>)
                }
            }
        })
    }

    #[derive(Default)]
    struct CaptureRenderer {
        target: Option<RenderTarget>,
        atlas: MemorySink,
        canvas: Option<image::RgbaImage>,
        capture: Rc<RefCell<CaptureState>>,
    }

    impl CaptureRenderer {
        fn with_state(capture: Rc<RefCell<CaptureState>>) -> Self {
            Self {
                target: None,
                atlas: MemorySink::new(),
                canvas: None,
                capture,
            }
        }
    }

    struct DamageRegion {
        full: bool,
        rects: Vec<[i32; 4]>,
        bbox: [i32; 4],
    }

    impl DamageRegion {
        fn full(max_w: i32, max_h: i32) -> Self {
            let bbox = [0, 0, max_w, max_h];
            Self {
                full: true,
                rects: vec![bbox],
                bbox,
            }
        }

        fn from_rects(rects: Vec<[i32; 4]>) -> Option<Self> {
            if rects.is_empty() {
                return None;
            }
            let mut bbox = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for &[x0, y0, x1, y1] in &rects {
                bbox[0] = bbox[0].min(x0);
                bbox[1] = bbox[1].min(y0);
                bbox[2] = bbox[2].max(x1);
                bbox[3] = bbox[3].max(y1);
            }
            Some(Self {
                full: false,
                rects,
                bbox,
            })
        }

        fn contains(&self, x: i32, y: i32) -> bool {
            self.full
                || self.rects.len() == 1
                || self
                    .rects
                    .iter()
                    .any(|&[x0, y0, x1, y1]| x >= x0 && x < x1 && y >= y0 && y < y1)
        }

        fn clipped_span(&self, bounds: [f32; 4], clip: [i32; 4]) -> Option<[i32; 4]> {
            if bounds[2] <= 0.0 || bounds[3] <= 0.0 {
                return None;
            }
            let x0 = (bounds[0].floor() as i32).max(clip[0]).max(self.bbox[0]);
            let y0 = (bounds[1].floor() as i32).max(clip[1]).max(self.bbox[1]);
            let x1 = ((bounds[0] + bounds[2]).ceil() as i32)
                .min(clip[2])
                .min(self.bbox[2]);
            let y1 = ((bounds[1] + bounds[3]).ceil() as i32)
                .min(clip[3])
                .min(self.bbox[3]);
            (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
        }
    }

    fn blend_premultiplied(dst: &mut [u8; 4], src: [f32; 4]) {
        let sa = src[3].clamp(0.0, 1.0);
        if sa <= 0.0 {
            return;
        }
        let inv = 1.0 - sa;
        let out_r = (src[0].clamp(0.0, 1.0) * 255.0 + f32::from(dst[0]) * inv).round();
        let out_g = (src[1].clamp(0.0, 1.0) * 255.0 + f32::from(dst[1]) * inv).round();
        let out_b = (src[2].clamp(0.0, 1.0) * 255.0 + f32::from(dst[2]) * inv).round();
        let out_a = (sa * 255.0 + f32::from(dst[3]) * inv).round();
        dst[0] = out_r.clamp(0.0, 255.0) as u8;
        dst[1] = out_g.clamp(0.0, 255.0) as u8;
        dst[2] = out_b.clamp(0.0, 255.0) as u8;
        dst[3] = out_a.clamp(0.0, 255.0) as u8;
    }

    fn fill_rect(
        img: &mut image::RgbaImage,
        bounds: [f32; 4],
        clip: [i32; 4],
        damage: &DamageRegion,
        color: [f32; 4],
    ) {
        if color[3] <= 0.0 {
            return;
        }
        let Some([x0, y0, x1, y1]) = damage.clipped_span(bounds, clip) else {
            return;
        };
        for y in y0..y1 {
            for x in x0..x1 {
                if !damage.contains(x, y) {
                    continue;
                }
                let px = img.get_pixel_mut(x as u32, y as u32);
                blend_premultiplied(&mut px.0, color);
            }
        }
    }

    fn place_bounds(matrices: &[Matrix4], slot: u32, bounds: [f32; 4]) -> [f32; 4] {
        if bounds[2] <= 0.0 || bounds[3] <= 0.0 {
            return [0.0; 4];
        }
        let Some(matrix) = matrices.get(slot as usize) else {
            return bounds;
        };
        let rect = Rect::new(
            Point::new(DevicePx(bounds[0]), DevicePx(bounds[1])),
            Size::new(DevicePx(bounds[2]), DevicePx(bounds[3])),
        );
        let placed = transformed_bounds(matrix, rect);
        [
            placed.origin.x.0,
            placed.origin.y.0,
            placed.size.width.0,
            placed.size.height.0,
        ]
    }

    fn resolve_clip_rect(
        scene: &Scene,
        placements: &Placements,
        clip: u32,
        max_w: i32,
        max_h: i32,
    ) -> [i32; 4] {
        let resolved = scene
            .clips
            .resolve_placed(ClipId(clip), &|space| placements.get(space).copied());
        let cx0 = f64::from(resolved.aabb[0])
            .floor()
            .clamp(0.0, f64::from(max_w)) as i32;
        let cy0 = f64::from(resolved.aabb[1])
            .floor()
            .clamp(0.0, f64::from(max_h)) as i32;
        let cx1 = (f64::from(resolved.aabb[0]) + f64::from(resolved.aabb[2]))
            .ceil()
            .clamp(0.0, f64::from(max_w)) as i32;
        let cy1 = (f64::from(resolved.aabb[1]) + f64::from(resolved.aabb[3]))
            .ceil()
            .clamp(0.0, f64::from(max_h)) as i32;
        [cx0, cy0, cx1, cy1]
    }

    fn intersect_clip(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
        [
            a[0].max(b[0]),
            a[1].max(b[1]),
            a[2].min(b[2]),
            a[3].min(b[3]),
        ]
    }

    fn texture_id_from_word(word: u32) -> Option<TextureId> {
        let kind = match word >> 16 {
            0 => TextureKind::Mono,
            1 => TextureKind::Subpixel,
            2 => TextureKind::Color,
            3 => TextureKind::Image,
            _ => return None,
        };
        Some(TextureId::new(kind, word & 0xffff))
    }

    fn paint_color(scene: &Scene, kind: u32, index: u32) -> Option<[f32; 4]> {
        if kind == PaintKind::None as u32 {
            return None;
        }
        match scene.paints.get(PaintId(index)) {
            Some(Paint::Solid(color)) => Some(color.to_premultiplied_srgb()),
            Some(Paint::Gradient { stops, .. }) => {
                stops.first().map(|stop| stop.color.to_premultiplied_srgb())
            }
            _ => None,
        }
    }

    impl Renderer for CaptureRenderer {
        fn capabilities(&self) -> RenderCapabilities {
            RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.target
        }

        fn draw(&mut self, scene: &Scene, damage: &DamageSet) -> FrameOutcome {
            let size = self
                .target
                .map(|target| target.size)
                .unwrap_or_else(|| scene.viewport());
            let width = u32::try_from(size.width.max(1)).unwrap_or(1);
            let height = u32::try_from(size.height.max(1)).unwrap_or(1);
            let max_w = i32::try_from(width).unwrap_or(i32::MAX);
            let max_h = i32::try_from(height).unwrap_or(i32::MAX);

            let resized = self
                .canvas
                .as_ref()
                .is_none_or(|canvas| canvas.width() != width || canvas.height() != height);
            if resized {
                self.canvas = Some(image::RgbaImage::from_pixel(width, height, CLEAR_PIXEL));
            }
            let img = self
                .canvas
                .get_or_insert_with(|| image::RgbaImage::from_pixel(width, height, CLEAR_PIXEL));

            let full_clear = resized || damage.is_full();
            let region = if full_clear {
                for px in img.pixels_mut() {
                    *px = CLEAR_PIXEL;
                }
                DamageRegion::full(max_w, max_h)
            } else if damage.is_empty() {
                let mut guard = self.capture.borrow_mut();
                guard.latest_rgba = Some(img.clone());
                guard.frames = guard.frames.saturating_add(1);
                return FrameOutcome::Presented(FrameStats {
                    vector_passes: 0,
                    draw_calls: 0,
                    damage_px: 0,
                    bytes_uploaded: 0,
                    memory: MemoryReport::ZERO,
                });
            } else {
                let rects: Vec<[i32; 4]> = damage
                    .rects()
                    .iter()
                    .filter_map(|r| {
                        let x0 = r.origin.x.clamp(0, max_w);
                        let y0 = r.origin.y.clamp(0, max_h);
                        let x1 = r.origin.x.saturating_add(r.size.width).clamp(0, max_w);
                        let y1 = r.origin.y.saturating_add(r.size.height).clamp(0, max_h);
                        (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
                    })
                    .collect();
                let Some(region) = DamageRegion::from_rects(rects) else {
                    let mut guard = self.capture.borrow_mut();
                    guard.latest_rgba = Some(img.clone());
                    guard.frames = guard.frames.saturating_add(1);
                    return FrameOutcome::Presented(FrameStats {
                        vector_passes: 0,
                        draw_calls: 0,
                        damage_px: 0,
                        bytes_uploaded: 0,
                        memory: MemoryReport::ZERO,
                    });
                };
                for &[x0, y0, x1, y1] in &region.rects {
                    for py in y0..y1 {
                        for px in x0..x1 {
                            img.put_pixel(px as u32, py as u32, CLEAR_PIXEL);
                        }
                    }
                }
                region
            };

            let placements = scene.placements();
            let matrices: Vec<Matrix4> = placements.matrices().collect();

            #[derive(Clone, Copy)]
            enum DrawItem {
                Quad(usize, u32),
                Mono(usize, u32),
                Color(usize, u32),
            }

            let mut items = Vec::with_capacity(
                scene.primitives.quads.len()
                    + scene.primitives.mono_sprites.len()
                    + scene.primitives.color_sprites.len(),
            );
            for (idx, quad) in scene.primitives.quads.iter().enumerate() {
                items.push(DrawItem::Quad(idx, quad.order));
            }
            for (idx, sprite) in scene.primitives.mono_sprites.iter().enumerate() {
                items.push(DrawItem::Mono(idx, sprite.order));
            }
            for (idx, sprite) in scene.primitives.color_sprites.iter().enumerate() {
                items.push(DrawItem::Color(idx, sprite.order));
            }
            items.sort_by_key(|item| match *item {
                DrawItem::Quad(idx, order) => (order, 0_u8, idx),
                DrawItem::Color(idx, order) => (order, 1_u8, idx),
                DrawItem::Mono(idx, order) => (order, 2_u8, idx),
            });

            for item in items {
                match item {
                    DrawItem::Quad(idx, _) => {
                        let quad = &scene.primitives.quads[idx];
                        let placed = place_bounds(&matrices, quad.transform, quad.bounds);
                        let clip = resolve_clip_rect(scene, &placements, quad.clip, max_w, max_h);
                        if let Some(fill) = paint_color(scene, quad.fill.kind, quad.fill.index) {
                            fill_rect(img, placed, clip, &region, fill);
                        }
                        if let Some(stroke) =
                            paint_color(scene, quad.stroke.kind, quad.stroke.index)
                        {
                            let [x, y, w, h] = placed;
                            let [t, r, b, l] = quad.border;
                            if t > 0.0 {
                                fill_rect(img, [x, y, w, t], clip, &region, stroke);
                            }
                            if b > 0.0 {
                                fill_rect(img, [x, y + h - b, w, b], clip, &region, stroke);
                            }
                            if l > 0.0 {
                                fill_rect(img, [x, y, l, h], clip, &region, stroke);
                            }
                            if r > 0.0 {
                                fill_rect(img, [x + w - r, y, r, h], clip, &region, stroke);
                            }
                        }
                    }
                    DrawItem::Mono(idx, _) => {
                        let sprite = &scene.primitives.mono_sprites[idx];
                        if sprite.tile.is_unresolved() {
                            continue;
                        }
                        let Some(tex_id) = texture_id_from_word(sprite.tile.texture) else {
                            continue;
                        };
                        let [tx, ty, tw, th] = sprite.tile.bounds;
                        let placed = place_bounds(&matrices, sprite.transform, sprite.bounds);
                        let [dx, dy, dw, dh] = placed;
                        if tw <= 0 || th <= 0 || dw <= 0.0 || dh <= 0.0 {
                            continue;
                        }
                        let clip = resolve_clip_rect(scene, &placements, sprite.clip, max_w, max_h);
                        let Some([start_x, start_y, end_x, end_y]) =
                            region.clipped_span(placed, clip)
                        else {
                            continue;
                        };
                        for py in start_y..end_y {
                            let v = ((py as f32 + 0.5 - dy) / dh) * th as f32;
                            let sy = ty + (v.floor() as i32).clamp(0, th - 1);
                            for px in start_x..end_x {
                                if !region.contains(px, py) {
                                    continue;
                                }
                                let u = ((px as f32 + 0.5 - dx) / dw) * tw as f32;
                                let sx = tx + (u.floor() as i32).clamp(0, tw - 1);
                                if let Some(cov_bytes) = self.atlas.texel(tex_id, sx, sy)
                                    && let Some(&cov_u8) = cov_bytes.first()
                                    && cov_u8 > 0
                                {
                                    let cov = f32::from(cov_u8) / 255.0;
                                    let tinted = [
                                        sprite.color[0] * cov,
                                        sprite.color[1] * cov,
                                        sprite.color[2] * cov,
                                        sprite.color[3] * cov,
                                    ];
                                    let px_dst = img.get_pixel_mut(px as u32, py as u32);
                                    blend_premultiplied(&mut px_dst.0, tinted);
                                }
                            }
                        }
                    }
                    DrawItem::Color(idx, _) => {
                        let sprite = &scene.primitives.color_sprites[idx];
                        if sprite.tile.is_unresolved() {
                            continue;
                        }
                        let Some(tex_id) = texture_id_from_word(sprite.tile.texture) else {
                            continue;
                        };
                        let [tx, ty, tw, th] = sprite.tile.bounds;
                        let placed = place_bounds(&matrices, sprite.transform, sprite.bounds);
                        let [dx, dy, dw, dh] = placed;
                        let opacity = sprite.opacity.clamp(0.0, 1.0);
                        if tw <= 0 || th <= 0 || dw <= 0.0 || dh <= 0.0 || opacity <= 0.0 {
                            continue;
                        }
                        let frame = place_bounds(&matrices, sprite.transform, sprite.frame);
                        let frame_clip = [
                            frame[0].floor().clamp(0.0, max_w as f32) as i32,
                            frame[1].floor().clamp(0.0, max_h as f32) as i32,
                            (frame[0] + frame[2]).ceil().clamp(0.0, max_w as f32) as i32,
                            (frame[1] + frame[3]).ceil().clamp(0.0, max_h as f32) as i32,
                        ];
                        let scene_clip =
                            resolve_clip_rect(scene, &placements, sprite.clip, max_w, max_h);
                        let clip = intersect_clip(scene_clip, frame_clip);
                        let Some([start_x, start_y, end_x, end_y]) =
                            region.clipped_span(placed, clip)
                        else {
                            continue;
                        };
                        let grayscale = (sprite.flags & ColorSprite::GRAYSCALE) != 0;
                        for py in start_y..end_y {
                            let v = ((py as f32 + 0.5 - dy) / dh) * th as f32;
                            let sy = ty + (v.floor() as i32).clamp(0, th - 1);
                            for px in start_x..end_x {
                                if !region.contains(px, py) {
                                    continue;
                                }
                                let u = ((px as f32 + 0.5 - dx) / dw) * tw as f32;
                                let sx = tx + (u.floor() as i32).clamp(0, tw - 1);
                                if let Some(texel) = self.atlas.texel(tex_id, sx, sy)
                                    && texel.len() >= 4
                                    && texel[3] > 0
                                {
                                    let mut r = (f32::from(texel[0]) / 255.0) * opacity;
                                    let mut g = (f32::from(texel[1]) / 255.0) * opacity;
                                    let mut b = (f32::from(texel[2]) / 255.0) * opacity;
                                    let a = (f32::from(texel[3]) / 255.0) * opacity;
                                    if grayscale {
                                        let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                                        r = lum;
                                        g = lum;
                                        b = lum;
                                    }
                                    let px_dst = img.get_pixel_mut(px as u32, py as u32);
                                    blend_premultiplied(&mut px_dst.0, [r, g, b, a]);
                                }
                            }
                        }
                    }
                }
            }

            {
                let mut guard = self.capture.borrow_mut();
                guard.latest_rgba = Some(img.clone());
                guard.frames = guard.frames.saturating_add(1);
            }

            let damage_px = if full_clear {
                u64::from(width) * u64::from(height)
            } else {
                region
                    .rects
                    .iter()
                    .map(|&[x0, y0, x1, y1]| {
                        u64::try_from((x1 - x0).max(0)).unwrap_or(0)
                            * u64::try_from((y1 - y0).max(0)).unwrap_or(0)
                    })
                    .sum()
            };

            FrameOutcome::Presented(FrameStats {
                vector_passes: 0,
                draw_calls: u32::try_from(
                    scene.primitives.quads.len()
                        + scene.primitives.mono_sprites.len()
                        + scene.primitives.color_sprites.len(),
                )
                .unwrap_or(u32::MAX),
                damage_px,
                bytes_uploaded: 0,
                memory: MemoryReport::ZERO,
            })
        }

        fn register_external(&mut self, _texture: ExternalTexture) -> TextureHandle {
            TextureHandle(0)
        }

        fn release_external(&mut self, _handle: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::ZERO
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            &mut self.atlas
        }
    }

    struct NodeEntry {
        role: Role,
        label: Option<String>,
        value: Option<String>,
        bounds: Option<(f64, f64, f64, f64)>,
        transform: Option<[f64; 6]>,
        children: Vec<u64>,
        disabled: bool,
    }

    struct ResolvedNode {
        id: u64,
        depth: usize,
        role: Role,
        label: Option<String>,
        value: Option<String>,
        bounds: Option<(f64, f64, f64, f64)>,
        disabled: bool,
    }

    const IDENTITY_AFFINE: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    fn compose_affine(outer: [f64; 6], inner: [f64; 6]) -> [f64; 6] {
        [
            outer[0] * inner[0] + outer[2] * inner[1],
            outer[1] * inner[0] + outer[3] * inner[1],
            outer[0] * inner[2] + outer[2] * inner[3],
            outer[1] * inner[2] + outer[3] * inner[3],
            outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
            outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
        ]
    }

    fn apply_affine(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    fn transform_a11y_rect(m: [f64; 6], r: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        let corners = [
            apply_affine(m, r.0, r.1),
            apply_affine(m, r.2, r.1),
            apply_affine(m, r.2, r.3),
            apply_affine(m, r.0, r.3),
        ];
        let min_x = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let min_y = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let max_x = corners
            .iter()
            .map(|c| c.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = corners
            .iter()
            .map(|c| c.1)
            .fold(f64::NEG_INFINITY, f64::max);
        (min_x, min_y, max_x, max_y)
    }

    #[derive(Default)]
    struct SemanticStore {
        root: Option<u64>,
        focus: Option<u64>,
        nodes: HashMap<u64, NodeEntry>,
    }

    impl SemanticStore {
        fn ingest_from(&mut self, harness: &Harness<Box<dyn AppHandler>>) {
            if let Some(offscreen) = harness.platform().offscreens().first() {
                self.ingest(offscreen);
            }
        }

        fn ingest(&mut self, surface: &OffscreenSurface) {
            for update in surface.take_a11y_log() {
                if let Some(tree) = update.tree {
                    self.root = Some(tree.root.0);
                }
                self.focus = Some(update.focus.0);
                for (id, node) in update.nodes {
                    let bounds = node
                        .bounds()
                        .map(|rect| (rect.x0, rect.y0, rect.x1, rect.y1));
                    let transform = node.transform().map(|t| t.as_coeffs());
                    let children = node.children().iter().map(|child| child.0).collect();
                    self.nodes.insert(
                        id.0,
                        NodeEntry {
                            role: node.role(),
                            label: node.label().map(str::to_owned),
                            value: node.value().map(str::to_owned),
                            bounds,
                            transform,
                            children,
                            disabled: node.is_disabled(),
                        },
                    );
                }
            }
        }

        fn resolved_live_nodes(&self) -> Vec<ResolvedNode> {
            let mut out = Vec::new();
            let Some(root) = self.root else {
                return out;
            };
            let mut visited = HashSet::new();
            self.collect_live(root, 0, IDENTITY_AFFINE, &mut visited, &mut out);
            out
        }

        fn collect_live(
            &self,
            id: u64,
            depth: usize,
            world: [f64; 6],
            visited: &mut HashSet<u64>,
            out: &mut Vec<ResolvedNode>,
        ) {
            if !visited.insert(id) {
                return;
            }
            let Some(node) = self.nodes.get(&id) else {
                return;
            };
            let local_to_world = match node.transform {
                Some(t) => compose_affine(world, t),
                None => world,
            };
            let resolved_bounds = node.bounds.map(|b| transform_a11y_rect(local_to_world, b));
            out.push(ResolvedNode {
                id,
                depth,
                role: node.role,
                label: node.label.clone(),
                value: node.value.clone(),
                bounds: resolved_bounds,
                disabled: node.disabled,
            });
            for &child in &node.children {
                self.collect_live(child, depth + 1, local_to_world, visited, out);
            }
        }

        fn dump(&self) -> String {
            let live = self.resolved_live_nodes();
            if live.is_empty() {
                return String::from("(empty accessibility tree)\n");
            }
            let mut out = String::new();
            let _ = writeln!(out, "focus={:?}", self.focus);
            for node in live {
                let indent = "  ".repeat(node.depth);
                let _ = write!(out, "{indent}- {:?} (id={})", node.role, node.id);
                if let Some(label) = &node.label {
                    let _ = write!(out, " label={label:?}");
                }
                if let Some(value) = &node.value {
                    let _ = write!(out, " value={value:?}");
                }
                if let Some((x0, y0, x1, y1)) = node.bounds {
                    let _ = write!(out, " bounds=[{x0:.1},{y0:.1},{x1:.1},{y1:.1}]");
                }
                if node.disabled {
                    let _ = write!(out, " [disabled]");
                }
                let _ = writeln!(out);
            }
            out
        }

        fn has_role(&self, expected: Role) -> bool {
            self.resolved_live_nodes()
                .into_iter()
                .any(|node| node.role == expected)
        }

        fn find_entry_center(&self, needle: &str) -> Option<Point<CssPx, Css>> {
            self.resolved_live_nodes().into_iter().find_map(|node| {
                if !matches!(node.role, Role::Label | Role::ListItem) {
                    return None;
                }
                let matches_label = node
                    .label
                    .as_deref()
                    .is_some_and(|label| label.contains(needle));
                let matches_value = node
                    .value
                    .as_deref()
                    .is_some_and(|value| value.contains(needle));
                if !(matches_label || matches_value) {
                    return None;
                }
                let (x0, y0, x1, y1) = node.bounds?;
                if x0 < 220.0 || y0 < 75.0 || x1 <= x0 || y1 <= y0 {
                    return None;
                }
                Some(Point::new(
                    CssPx(((x0 + x1) * 0.5) as f32),
                    CssPx(((y0 + y1) * 0.5) as f32),
                ))
            })
        }

        fn find_button_center(&self, needle: &str) -> Option<Point<CssPx, Css>> {
            self.resolved_live_nodes().into_iter().find_map(|node| {
                if !matches!(node.role, Role::Button | Role::Tab) {
                    return None;
                }
                let matches_label = node
                    .label
                    .as_deref()
                    .is_some_and(|label| label.contains(needle));
                if !matches_label {
                    return None;
                }
                let (x0, y0, x1, y1) = node.bounds?;
                if x1 <= x0 || y1 <= y0 {
                    return None;
                }
                Some(Point::new(
                    CssPx(((x0 + x1) * 0.5) as f32),
                    CssPx(((y0 + y1) * 0.5) as f32),
                ))
            })
        }

        fn find_tab_center(&self, region_label: &str, title: &str) -> Option<Point<CssPx, Css>> {
            let nodes = self.resolved_live_nodes();
            nodes.iter().enumerate().find_map(|(index, region)| {
                if region.label.as_deref() != Some(region_label) {
                    return None;
                }
                let tab = nodes[index + 1..]
                    .iter()
                    .take_while(|node| node.depth > region.depth)
                    .find(|node| {
                        node.role == Role::Button && node.label.as_deref() == Some(title)
                    })?;
                let (x0, y0, x1, y1) = tab.bounds?;
                (x1 > x0 && y1 > y0).then(|| {
                    Point::new(
                        CssPx(((x0 + x1) * 0.5) as f32),
                        CssPx(((y0 + y1) * 0.5) as f32),
                    )
                })
            })
        }
    }

    fn pump_frames(harness: &mut Harness<Box<dyn AppHandler>>) {
        for _ in 0..8 {
            harness.pump();
            harness.advance(Duration::from_millis(30));
            harness.pump();
        }
    }

    fn send_click(
        harness: &mut Harness<Box<dyn AppHandler>>,
        pos: Point<CssPx, Css>,
        button: PointerButton,
        ts: &Cell<u64>,
    ) {
        let now = Timestamp::from_origin(Duration::from_millis(ts.get()));
        ts.set(ts.get() + 16);
        for (action, event) in [
            (PointerAction::Moved, PointerEvent::mouse(pos)),
            (
                PointerAction::Pressed,
                PointerEvent::mouse(pos).with_button(button),
            ),
            (
                PointerAction::Released,
                PointerEvent::mouse(pos).with_button(button),
            ),
        ] {
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action,
                event,
                modifiers: Modifiers::NONE,
                timestamp: now,
            });
            harness.pump();
        }
        pump_frames(harness);
    }

    fn send_click_on(
        harness: &mut Harness<Box<dyn AppHandler>>,
        surface: SurfaceId,
        pos: Point<CssPx, Css>,
        button: PointerButton,
        ts: &Cell<u64>,
    ) {
        let now = Timestamp::from_origin(Duration::from_millis(ts.get()));
        ts.set(ts.get() + 16);
        for (action, event) in [
            (PointerAction::Moved, PointerEvent::mouse(pos)),
            (
                PointerAction::Pressed,
                PointerEvent::mouse(pos).with_button(button),
            ),
            (
                PointerAction::Released,
                PointerEvent::mouse(pos).with_button(button),
            ),
        ] {
            harness.deliver(
                surface,
                SurfaceEvent::Pointer {
                    action,
                    event,
                    modifiers: Modifiers::NONE,
                    timestamp: now,
                },
            );
            harness.pump();
        }
        pump_frames(harness);
    }

    fn park_pointer(harness: &mut Harness<Box<dyn AppHandler>>, ts: &Cell<u64>) {
        let now = Timestamp::from_origin(Duration::from_millis(ts.get()));
        ts.set(ts.get() + 16);
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Moved,
            event: PointerEvent::mouse(Point::new(CssPx(980.0), CssPx(640.0))),
            modifiers: Modifiers::NONE,
            timestamp: now,
        });
        pump_frames(harness);
    }

    fn send_key(
        harness: &mut Harness<Box<dyn AppHandler>>,
        event: KeyEvent,
        modifiers: Modifiers,
        ts: &Cell<u64>,
    ) {
        let down_ts = Timestamp::from_origin(Duration::from_millis(ts.get()));
        let up_ts = Timestamp::from_origin(Duration::from_millis(ts.get() + 8));
        ts.set(ts.get() + 16);
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: event.clone(),
            modifiers,
            timestamp: down_ts,
        });
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event,
            modifiers: Modifiers::NONE,
            timestamp: up_ts,
        });
        if modifiers.alt() {
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Released,
                event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
                modifiers: Modifiers::NONE,
                timestamp: up_ts,
            });
        }
        if modifiers.control() {
            harness.deliver_to_first(SurfaceEvent::Key {
                state: KeyState::Released,
                event: KeyEvent::named(NamedKey::Control, PhysicalKey::Code(KeyCode::ControlLeft)),
                modifiers: Modifiers::NONE,
                timestamp: up_ts,
            });
        }
        pump_frames(harness);
    }

    fn type_chars(harness: &mut Harness<Box<dyn AppHandler>>, text: &str, ts: &Cell<u64>) {
        for ch in text.chars() {
            let buf = ch.to_string();
            send_key(harness, KeyEvent::character(buf), Modifiers::NONE, ts);
        }
    }

    fn wait_until(
        harness: &mut Harness<Box<dyn AppHandler>>,
        timeout: Duration,
        mut condition: impl FnMut(&Harness<Box<dyn AppHandler>>) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        while !condition(harness) {
            if Instant::now() >= deadline {
                return false;
            }
            harness.advance(Duration::from_millis(30));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        pump_frames(harness);
        true
    }

    fn write_snapshot_files(
        name: &str,
        semantic: &SemanticStore,
        capture: &Rc<RefCell<CaptureState>>,
        artifacts: &Path,
    ) -> Result<()> {
        let a11y_path = artifacts.join(format!("{name}.accessibility.txt"));
        fs::write(&a11y_path, semantic.dump())
            .with_context(|| format!("write accessibility dump {}", a11y_path.display()))?;
        let img = capture
            .borrow()
            .latest_rgba
            .clone()
            .with_context(|| format!("expected rendered frame before capturing {name}"))?;
        let first = *img.get_pixel(0, 0);
        ensure!(
            img.pixels().any(|px| *px != first),
            "capture {name}: rendered frame is uniform solid color"
        );
        let png_path = artifacts.join(format!("{name}.png"));
        img.save_with_format(&png_path, image::ImageFormat::Png)
            .with_context(|| format!("write screenshot {}", png_path.display()))?;
        Ok(())
    }

    fn capture_checkpoint(
        name: &str,
        harness: &mut Harness<Box<dyn AppHandler>>,
        semantic: &mut SemanticStore,
        capture: &Rc<RefCell<CaptureState>>,
        artifacts: &Path,
    ) -> Result<()> {
        pump_frames(harness);
        semantic.ingest_from(harness);
        ensure!(
            !semantic.has_role(Role::Dialog),
            "checkpoint {name}: unexpected open Role::Dialog remained mounted"
        );
        write_snapshot_files(name, semantic, capture, artifacts)
    }

    fn prepare_artifacts() -> Result<(PathBuf, PathBuf)> {
        let artifacts = match std::env::var_os("GDS_E2E_ARTIFACTS") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => std::env::temp_dir().join(format!("gds-files-e2e-{}", std::process::id())),
        };
        fs::create_dir_all(&artifacts)?;
        let work = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("work"))
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(|| artifacts.join("work"));
        fs::create_dir_all(&work)?;
        if !work.join("original.txt").exists() {
            fs::write(work.join("original.txt"), b"original payload\n")?;
        }
        if !work.join("notes.txt").exists() {
            fs::write(work.join("notes.txt"), b"notes payload\n")?;
        }
        let sub = work.join("folder_a");
        fs::create_dir_all(&sub)?;
        if !sub.join("sub.txt").exists() {
            fs::write(sub.join("sub.txt"), b"sub payload\n")?;
        }
        Ok((artifacts, work))
    }

    pub(super) fn execute() -> Result<()> {
        let (artifacts, work) = prepare_artifacts()?;
        let home = work.parent().unwrap_or(&artifacts).to_path_buf();

        let supervisor = Supervisor::new(false).context("start Files E2E Tokio task supervisor")?;
        let scope = supervisor.scope();
        let registry = CaptureRegistry::default();
        let capture_state = Rc::clone(&registry.primary);
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let browser_for_view = Rc::clone(&browser_slot);
        let start_dir = work.clone();
        let home_dir = home.clone();

        let handler = app()
            .with_title("Files")
            .with_application_id("io.github.GanDesktop.GdsFiles")
            .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
            .with_renderer(renderer_factory(registry))
            .with_context(move || {
                let gds = Gds::install_with_logs(scope.clone(), None);
                gds.appearance().motion().set_enabled(false);
                let prefs = Preferences {
                    animations_enabled: Some(false),
                    ..Preferences::default()
                };
                let _ = gds.appearance().theme().apply(&prefs.appearance);
                if let Ok(catalog) = files_i18n::prepare(Language::English) {
                    gds.i18n().install_catalog(catalog);
                }
                let _ = PreferencesService::install(prefs, &scope);
            })
            .into_handler(move || {
                let _ = Gds::current().appearance().install_window_sheet();
                let root_scope = Gds::current().tasks().scope();
                let volumes = volumes::Service::new(root_scope);
                let places = Places::default();
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(places),
                        start: start_dir.clone(),
                        home: home_dir.clone(),
                        picker: None,
                    },
                    volumes,
                    Rc::new(|_| {}),
                );
                *browser_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .context("build headless Files application")?;

        let scenario_error: Rc<RefCell<Option<anyhow::Error>>> = Rc::new(RefCell::new(None));
        let scenario_error_out = Rc::clone(&scenario_error);

        handler
            .drive(move |app| -> Result<(), PlatformError> {
                let mut harness = Harness::new(app);
                harness.redraw_on_configure(true);
                if let Some(offscreen) = harness.platform().offscreens().first() {
                    offscreen.set_a11y_listener(true);
                }
                harness.deliver_to_first(SurfaceEvent::Focused(true));
                pump_frames(&mut harness);

                let result = (|| -> Result<()> {
                    let browser = browser_slot
                        .borrow()
                        .clone()
                        .context("Files Browser was not constructed")?;
                    let ts = Cell::new(100_u64);
                    let mut semantic = SemanticStore::default();

                    // 01: Open seeded workspace & verify rendered frame + semantic dump.
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |h| {
                            semantic.ingest_from(h);
                            capture_state.borrow().frames > 0
                                && semantic.find_entry_center("original.txt").is_some()
                        }),
                        "01: initial workspace listing did not render (frames={}, rows={}, loading={}, root={:?}, dump={})",
                        capture_state.borrow().frames,
                        browser.read_untracked(|s| s.tab().rows.len()),
                        browser.read_untracked(|s| s.tab().navigation.is_loading()),
                        semantic.root,
                        semantic.dump()
                    );
                    capture_checkpoint(
                        "01-open",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    // 02: Rename + Undo + Redo.
                    let orig_pos = semantic
                        .find_entry_center("original.txt")
                        .context("02: original.txt row missing")?;
                    send_click(&mut harness, orig_pos, PointerButton::Primary, &ts);
                    send_key(
                        &mut harness,
                        KeyEvent::named(NamedKey::F2, PhysicalKey::Code(KeyCode::F2)),
                        Modifiers::NONE,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |h| {
                            semantic.ingest_from(h);
                            semantic.has_role(Role::Dialog)
                        }),
                        "02: rename dialog did not open"
                    );
                    send_key(
                        &mut harness,
                        KeyEvent::character("a"),
                        Modifiers::CONTROL,
                        &ts,
                    );
                    type_chars(&mut harness, "renamed.txt", &ts);
                    send_key(
                        &mut harness,
                        KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
                        Modifiers::NONE,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |_| {
                            work.join("renamed.txt").exists() && !work.join("original.txt").exists()
                        }),
                        "02: rename failed to produce renamed.txt"
                    );
                    pump_frames(&mut harness);
                    semantic.ingest_from(&harness);
                    ensure!(
                        !semantic.has_role(Role::Dialog),
                        "02: rename dialog stayed open after submit"
                    );
                    send_key(
                        &mut harness,
                        KeyEvent::character("z"),
                        Modifiers::CONTROL,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |_| {
                            work.join("original.txt").exists() && !work.join("renamed.txt").exists()
                        }),
                        "02: Ctrl+Z undo failed to restore original.txt"
                    );
                    send_key(
                        &mut harness,
                        KeyEvent::character("z"),
                        Modifiers::CONTROL | Modifiers::SHIFT,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |_| {
                            work.join("renamed.txt").exists() && !work.join("original.txt").exists()
                        }),
                        "02: Ctrl+Shift+Z redo failed to re-apply renamed.txt"
                    );
                    capture_checkpoint(
                        "02-rename-undo-redo",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    // 03: New folder shortcut (Ctrl+Shift+N).
                    send_key(
                        &mut harness,
                        KeyEvent::character("n"),
                        Modifiers::CONTROL | Modifiers::SHIFT,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |h| {
                            semantic.ingest_from(h);
                            semantic.has_role(Role::Dialog)
                        }),
                        "03: new folder dialog did not open"
                    );
                    send_key(
                        &mut harness,
                        KeyEvent::character("a"),
                        Modifiers::CONTROL,
                        &ts,
                    );
                    type_chars(&mut harness, "created-dir", &ts);
                    send_key(
                        &mut harness,
                        KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
                        Modifiers::NONE,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |_| {
                            work.join("created-dir").is_dir()
                        }),
                        "03: new folder created-dir was not created"
                    );
                    pump_frames(&mut harness);
                    semantic.ingest_from(&harness);
                    ensure!(
                        !semantic.has_role(Role::Dialog),
                        "03: new folder dialog stayed open after submit"
                    );
                    capture_checkpoint(
                        "03-new-folder",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    // 04: Trash & restore flow.
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |h| {
                            semantic.ingest_from(h);
                            semantic.find_entry_center("created-dir").is_some()
                        }),
                        "04: created-dir row not visible before trashing"
                    );
                    let dir_pos = semantic
                        .find_entry_center("created-dir")
                        .context("04: created-dir row missing")?;
                    send_click(&mut harness, dir_pos, PointerButton::Primary, &ts);
                    send_key(
                        &mut harness,
                        KeyEvent::named(NamedKey::Delete, PhysicalKey::Code(KeyCode::Delete)),
                        Modifiers::NONE,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |_| {
                            !work.join("created-dir").exists()
                        }),
                        "04: created-dir was not moved to Trash"
                    );
                    browser.open(Location::Trash);
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |h| {
                            semantic.ingest_from(h);
                            semantic.find_entry_center("created-dir").is_some()
                        }),
                        "04: created-dir did not appear in Trash listing"
                    );
                    let trash_pos = semantic
                        .find_entry_center("created-dir")
                        .context("04: created-dir row missing in Trash")?;
                    send_click(&mut harness, trash_pos, PointerButton::Primary, &ts);
                    capture_checkpoint(
                        "04-trash-restore",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;
                    send_key(
                        &mut harness,
                        KeyEvent::character("r"),
                        Modifiers::ALT,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |_| {
                            work.join("created-dir").is_dir()
                        }),
                        "04: Alt+R restore from Trash failed to restore created-dir"
                    );

                    // 05: External filesystem change watch.
                    browser.open(Location::from(&work));
                    pump_frames(&mut harness);
                    fs::write(work.join("external-added.txt"), b"watched payload\n")?;
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(3), |h| {
                            semantic.ingest_from(h);
                            browser
                                .0
                                .state
                                .borrow()
                                .tab()
                                .rows
                                .iter()
                                .any(|row| row.label() == "external-added.txt")
                                && semantic.find_entry_center("external-added.txt").is_some()
                        }),
                        "05: filesystem watcher did not add external-added.txt"
                    );
                    capture_checkpoint(
                        "05-external-watch",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    // 06: Grid view switch.
                    browser.toggle_view();
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |_| {
                            browser.read_untracked(|state| state.tab().mode == ViewMode::Grid)
                        }),
                        "06: toggle_view did not switch to Grid"
                    );
                    capture_checkpoint(
                        "06-grid-switch",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    // 07: Tabs + pointer drag sequence across sidebar tabs.
                    send_key(
                        &mut harness,
                        KeyEvent::character("n"),
                        Modifiers::CONTROL,
                        &ts,
                    );
                    browser.new_tab(Some(Location::from(work.join("folder_a"))));
                    let tabs_label = browser.0.owner.with(|| files_i18n::text_untracked("tabs"));
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |h| {
                            semantic.ingest_from(h);
                            semantic.find_tab_center(&tabs_label, "folder_a").is_some()
                                && semantic.find_tab_center(&tabs_label, "work").is_some()
                        }),
                        "07: sidebar tabs did not mount"
                    );
                    let drag_start = semantic
                        .find_tab_center(&tabs_label, "folder_a")
                        .context("07: folder_a sidebar tab missing")?;
                    let drag_end = semantic
                        .find_tab_center(&tabs_label, "work")
                        .context("07: work sidebar tab missing")?;
                    let drag_nudge = Point::new(CssPx(drag_start.x.0), CssPx(drag_start.y.0 - 8.0));
                    let drag_mid = Point::new(
                        CssPx((drag_start.x.0 + drag_end.x.0) * 0.5),
                        CssPx((drag_start.y.0 + drag_end.y.0) * 0.5),
                    );
                    harness.deliver_to_first(SurfaceEvent::Pointer {
                        action: PointerAction::Moved,
                        event: PointerEvent::mouse(drag_start),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ts.get())),
                    });
                    harness.pump();
                    harness.deliver_to_first(SurfaceEvent::Pointer {
                        action: PointerAction::Pressed,
                        event: PointerEvent::mouse(drag_start)
                            .with_button(PointerButton::Primary),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ts.get() + 16)),
                    });
                    harness.pump();
                    harness.deliver_to_first(SurfaceEvent::Pointer {
                        action: PointerAction::Moved,
                        event: PointerEvent::mouse(drag_nudge),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ts.get() + 24)),
                    });
                    harness.pump();
                    harness.deliver_to_first(SurfaceEvent::Pointer {
                        action: PointerAction::Moved,
                        event: PointerEvent::mouse(drag_mid),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ts.get() + 32)),
                    });
                    harness.pump();
                    harness.deliver_to_first(SurfaceEvent::Pointer {
                        action: PointerAction::Moved,
                        event: PointerEvent::mouse(drag_end),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ts.get() + 40)),
                    });
                    harness.pump();
                    harness.deliver_to_first(SurfaceEvent::Pointer {
                        action: PointerAction::Released,
                        event: PointerEvent::mouse(drag_end).with_button(PointerButton::Primary),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ts.get() + 48)),
                    });
                    ts.set(ts.get() + 64);
                    pump_frames(&mut harness);
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |_| {
                            browser.read_untracked(|state| {
                                state.tabs.len() == 3
                                    && state
                                        .tabs
                                        .first()
                                        .and_then(|tab| tab.navigation.location().local_path())
                                        .is_some_and(|path| path.ends_with("folder_a"))
                            })
                        }),
                        "07: dragging folder_a sidebar tab onto work tab did not reorder tabs"
                    );
                    capture_checkpoint(
                        "07-tab-drag",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    // 08: Filter + IME composition & commit.
                    harness.deliver_to_first(SurfaceEvent::Pointer {
                        action: PointerAction::Moved,
                        event: PointerEvent::mouse(Point::new(CssPx(640.0), CssPx(220.0))),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ts.get())),
                    });
                    ts.set(ts.get() + 16);
                    pump_frames(&mut harness);
                    semantic.ingest_from(&harness);
                    let work_tab_pos = semantic
                        .find_tab_center(&tabs_label, "work")
                        .context("08: work sidebar tab missing")?;
                    send_click(&mut harness, work_tab_pos, PointerButton::Primary, &ts);
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |h| {
                            semantic.ingest_from(h);
                            semantic.find_entry_center("notes.txt").is_some()
                                && semantic.find_entry_center("renamed.txt").is_some()
                        }),
                        "08: switching back to work tab at ({:.1},{:.1}) did not render work entries (state={:?}, notes={:?}, renamed={:?})",
                        work_tab_pos.x.0,
                        work_tab_pos.y.0,
                        browser.read_untracked(|state| (
                            state.active,
                            state
                                .tabs
                                .iter()
                                .map(|t| (
                                    t.id,
                                    t.navigation.location().label(),
                                    t.mode,
                                    t.rows.len()
                                ))
                                .collect::<Vec<_>>()
                        )),
                        semantic.find_entry_center("notes.txt"),
                        semantic.find_entry_center("renamed.txt")
                    );
                    send_key(
                        &mut harness,
                        KeyEvent::character("f"),
                        Modifiers::CONTROL,
                        &ts,
                    );
                    harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Enabled));
                    harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Preedit {
                        text: "bi".into(),
                        cursor: Some(0..2),
                    }));
                    harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Commit("notes".into())));
                    harness.deliver_to_first(SurfaceEvent::Ime(ImeEvent::Disabled));
                    pump_frames(&mut harness);
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |h| {
                            semantic.ingest_from(h);
                            semantic.find_entry_center("notes.txt").is_some()
                                && semantic.find_entry_center("renamed.txt").is_none()
                        }),
                        "08: IME filter commit did not filter view"
                    );
                    capture_checkpoint(
                        "08-search-ime",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;
                    send_key(
                        &mut harness,
                        KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                        Modifiers::NONE,
                        &ts,
                    );

                    // 09: Details panel + embedded terminal dock.
                    browser.toggle_details();
                    browser.toggle_terminal();
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |_| {
                            browser.0.details.get_untracked() && browser.0.terminal.get_untracked()
                        }),
                        "09: details panel or terminal dock failed to open"
                    );
                    capture_checkpoint(
                        "09-details-terminal",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    // 10: Hierarchical tag address + mouse Back/Forward navigation.
                    let tag_root = TagAddress::new("work")
                        .map_err(|err| anyhow::anyhow!("invalid work tag: {err:?}"))?;
                    browser.open(Location::Tags(tag_root));
                    let tag_child = TagAddress::new("work/urgent")
                        .map_err(|err| anyhow::anyhow!("invalid work/urgent tag: {err:?}"))?;
                    browser.open(Location::Tags(tag_child));
                    browser.open(Location::from(&work));
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |_| {
                            browser.read_untracked(|state| {
                                state.tab().navigation.location().local_path()
                                    == Some(work.as_path())
                            })
                        }),
                        "10: failed to navigate back to work directory before history check"
                    );
                    send_click(
                        &mut harness,
                        Point::new(CssPx(640.0), CssPx(220.0)),
                        PointerButton::Back,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |_| {
                            browser.read_untracked(|state| {
                                matches!(state.tab().navigation.location(), Location::Tags(_))
                            })
                        }),
                        "10: mouse Back button did not return to tag location"
                    );
                    send_click(
                        &mut harness,
                        Point::new(CssPx(640.0), CssPx(220.0)),
                        PointerButton::Forward,
                        &ts,
                    );
                    ensure!(
                        wait_until(&mut harness, Duration::from_secs(2), |_| {
                            browser.read_untracked(|state| {
                                state.tab().navigation.location().local_path()
                                    == Some(work.as_path())
                            })
                        }),
                        "10: mouse Forward button did not return to work directory"
                    );
                    capture_checkpoint(
                        "10-location-mouse-history",
                        &mut harness,
                        &mut semantic,
                        &capture_state,
                        &artifacts,
                    )?;

                    let guard = capture_state.borrow();
                    let backend = if guard.gpu_backed {
                        "wgpu+vello"
                    } else {
                        "cpu-fallback"
                    };
                    let summary = format!(
                        "PASS driven\nrenderer={backend}\nframes={}\nartifacts={}\n",
                        guard.frames,
                        artifacts.display()
                    );
                    fs::write(artifacts.join("driven-passed.txt"), summary)?;
                    Ok(())
                })();

                if let Err(err) = result {
                    *scenario_error_out.borrow_mut() = Some(err);
                }
                harness.shut_down();
                Ok(())
            })
            .map_err(|error| anyhow::anyhow!("zgui harness failed: {error}"))?;

        if let Some(err) = scenario_error.borrow_mut().take() {
            return Err(err);
        }
        Ok(())
    }

    struct GalleryCtx<'a> {
        harness: &'a mut Harness<Box<dyn AppHandler>>,
        semantic: &'a mut SemanticStore,
        secondary_semantics: HashMap<SurfaceId, SemanticStore>,
        registry: &'a CaptureRegistry,
        artifacts: &'a Path,
        captured: &'a mut Vec<String>,
        ts: &'a Cell<u64>,
    }

    impl GalleryCtx<'_> {
        fn snap_primary(&mut self, name: &str) -> Result<()> {
            pump_frames(self.harness);
            self.semantic.ingest_from(self.harness);
            write_snapshot_files(name, self.semantic, &self.registry.primary, self.artifacts)?;
            self.captured.push(name.to_owned());
            Ok(())
        }

        fn snap_surface(&mut self, name: &str, surface: &Arc<OffscreenSurface>) -> Result<()> {
            pump_frames(self.harness);
            let id = zgui::platform::Surface::id(surface.as_ref());
            let sec_semantic = self.secondary_semantics.entry(id).or_default();
            sec_semantic.ingest(surface);
            let state = self.registry.state_for(id);
            write_snapshot_files(name, sec_semantic, &state, self.artifacts)?;
            self.captured.push(name.to_owned());
            Ok(())
        }
    }

    fn bamboo_appearance(mode: ModePreference) -> Appearance {
        Appearance {
            source: Source::Builtin,
            builtin: BuiltinTheme::Bamboo,
            mode,
            content_opacity: 1.0,
            sidebar_opacity: 0.85,
            rounded_corners: false,
        }
    }

    fn apply_bamboo_mode(browser: &Browser, mode: ModePreference) {
        let appearance = bamboo_appearance(mode);
        browser.edit_preferences(|prefs| {
            prefs.appearance = appearance.clone();
        });
        browser.0.owner.with(|| {
            let _ = Gds::current().appearance().theme().apply(&appearance);
        });
    }

    fn write_sample_png(path: &Path) -> Result<()> {
        let mut img = image::RgbaImage::new(160, 120);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let r = (48 + (x * 160 / 160)) as u8;
            let g = (118 + (y * 100 / 120)) as u8;
            let b = 96_u8;
            *px = image::Rgba([r, g, b, 255]);
        }
        img.save_with_format(path, image::ImageFormat::Png)
            .with_context(|| format!("write sample PNG {}", path.display()))
    }

    fn install_mock_baloo(bin_dir: &Path, home: &Path) -> Result<tags::Preferences> {
        fs::create_dir_all(bin_dir)?;
        let balooctl = bin_dir.join("balooctl6");
        fs::write(
            &balooctl,
            "#!/bin/sh\necho 'Baloo File Indexer is running'\n",
        )?;
        fs::set_permissions(&balooctl, fs::Permissions::from_mode(0o755))?;

        let docs_dir = home.join("Documents");
        let readme = docs_dir.join("README.md");
        let preview = docs_dir.join("preview.png");
        let kioclient = bin_dir.join("kioclient");
        let script = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"ls\" ]; then\n\
               case \"$2\" in\n\
                 tags:/milestone) printf 'v1\\nREADME.md\\n' ;;\n\
                 tags:/milestone/v1|tags:/design) printf 'README.md\\npreview.png\\n' ;;\n\
                 *) printf 'design\\ndocumentation\\nmilestone\\nurgent\\n' ;;\n\
               esac\n\
             elif [ \"$1\" = \"stat\" ]; then\n\
               case \"$2\" in\n\
                 *README*md) printf 'LOCAL_PATH            %s\\n' \"{}\" ;;\n\
                 *preview*png) printf 'LOCAL_PATH            %s\\n' \"{}\" ;;\n\
                 *) printf 'DISPLAY_TYPE          Tag Fragment\\n' ;;\n\
               esac\n\
             fi\n",
            readme.display(),
            preview.display()
        );
        fs::write(&kioclient, script)?;
        fs::set_permissions(&kioclient, fs::Permissions::from_mode(0o755))?;

        Ok(tags::Preferences {
            enabled: true,
            commands: tags::Commands {
                backend: tags::Backend::Baloo,
                control: Some(balooctl),
                kio_client: Some(kioclient),
            },
        })
    }

    fn open_secondary_and_wait(
        harness: &mut Harness<Box<dyn AppHandler>>,
        open_fn: impl FnOnce(),
    ) -> Result<Arc<OffscreenSurface>> {
        let existing: HashSet<SurfaceId> = harness
            .platform()
            .offscreens()
            .iter()
            .map(|s| zgui::platform::Surface::id(s.as_ref()))
            .collect();
        open_fn();
        ensure!(
            wait_until(harness, Duration::from_secs(3), |h| {
                h.platform()
                    .offscreens()
                    .iter()
                    .any(|s| !existing.contains(&zgui::platform::Surface::id(s.as_ref())))
            }),
            "secondary window surface did not open"
        );
        let surface = harness
            .platform()
            .offscreens()
            .into_iter()
            .find(|s| !existing.contains(&zgui::platform::Surface::id(s.as_ref())))
            .context("secondary window surface missing after wait")?;
        let id = zgui::platform::Surface::id(surface.as_ref());
        surface.set_a11y_listener(true);
        harness.deliver(id, SurfaceEvent::Focused(true));
        pump_frames(harness);
        Ok(surface)
    }

    fn close_secondary(
        harness: &mut Harness<Box<dyn AppHandler>>,
        surface: &Arc<OffscreenSurface>,
    ) {
        let id = zgui::platform::Surface::id(surface.as_ref());
        harness.deliver(id, SurfaceEvent::CloseRequested);
        pump_frames(harness);
        if let Some(primary) = harness.platform().offscreens().first() {
            let pid = zgui::platform::Surface::id(primary.as_ref());
            harness.deliver(pid, SurfaceEvent::Focused(true));
            pump_frames(harness);
        }
    }

    pub(super) fn execute_gallery() -> Result<()> {
        let artifacts = match std::env::var_os("GDS_E2E_ARTIFACTS") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => std::env::temp_dir().join("gds-gallery-baseline"),
        };
        fs::create_dir_all(&artifacts)?;

        // Re-exec inside a hermetic temporary HOME/XDG sandbox so Preferences, Trash, and
        // BaseDirs never touch the caller's real desktop environment.
        if std::env::var_os("GDS_E2E_GALLERY_SANDBOXED").is_none() {
            let sandbox = tempfile::Builder::new()
                .prefix("gds-gallery-sandbox-")
                .tempdir()
                .context("create temporary gallery sandbox")?;
            let home = sandbox.path().join("home");
            let runtime = sandbox.path().join("runtime");
            fs::create_dir_all(home.join(".config/gds/app/files"))?;
            fs::create_dir_all(home.join(".local/share"))?;
            fs::create_dir_all(home.join(".local/state"))?;
            fs::create_dir_all(home.join(".cache"))?;
            fs::create_dir_all(&runtime)?;
            fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))?;

            let exe = std::env::current_exe().context("resolve current gds-files binary")?;
            let status = std::process::Command::new(exe)
                .env_remove("XDG_CURRENT_DESKTOP")
                .env_remove("NIRI_SOCKET")
                .env_remove("SWAYSOCK")
                .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
                .env("GDS_E2E_MODE", "gallery")
                .env("GDS_E2E_GALLERY_SANDBOXED", "1")
                .env("GDS_E2E_ARTIFACTS", &artifacts)
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", home.join(".config"))
                .env("XDG_DATA_HOME", home.join(".local/share"))
                .env("XDG_STATE_HOME", home.join(".local/state"))
                .env("XDG_CACHE_HOME", home.join(".cache"))
                .env("XDG_RUNTIME_DIR", &runtime)
                .status()
                .context("spawn sandboxed gallery child process")?;
            ensure!(
                status.success(),
                "sandboxed gallery process exited with {status}"
            );
            return Ok(());
        }

        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("sandbox HOME missing")?;
        for folder in ["Desktop", "Documents", "Downloads", "Github", "Pictures"] {
            fs::create_dir_all(home.join(folder))?;
        }
        let _ = std::os::unix::fs::symlink(home.join("Documents"), home.join("Docs-Shortcut"));
        let tag_prefs = install_mock_baloo(&home.join(".local/bin"), &home)?;

        let logs_store = gds::service::logs::init_tracing("info,gds=debug,gds_files=debug", true)
            .context("initialize gallery tracing store")?;
        gds::ui::logs::init(gds::ui::logs::Options {
            app_id: Some("io.github.GanDesktop.GdsFiles".into()),
            app_name: "Files".into(),
            ignored_contexts: &["GdsTerminal", "Input", "textarea", "input"],
        });

        let supervisor = Supervisor::new(true).context("start gallery Tokio task supervisor")?;
        let scope = supervisor.scope();
        let registry = CaptureRegistry::default();
        let registry_for_scenario = registry.clone();
        let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
        let browser_for_view = Rc::clone(&browser_slot);
        let home_for_view = home.clone();

        let handler = app()
            .with_title("Files")
            .with_application_id("io.github.GanDesktop.GdsFiles")
            .with_size(GALLERY_WIDTH, GALLERY_HEIGHT)
            .with_renderer(renderer_factory(registry))
            .with_context(move || {
                let gds = Gds::install_with_logs(scope.clone(), logs_store.clone());
                gds.appearance().motion().set_enabled(false);
                let prefs = Preferences {
                    language: Some(Language::English),
                    animations_enabled: Some(false),
                    appearance: bamboo_appearance(ModePreference::Light),
                    ..Preferences::default()
                };
                let _ = gds.appearance().theme().apply(&prefs.appearance);
                if let Ok(catalog) = files_i18n::prepare(Language::English) {
                    gds.i18n().install_catalog(catalog);
                }
                let _ = PreferencesService::install(prefs, &scope);
                tracing::info!("Gallery session initialized on real zgui headless harness");
                tracing::debug!(target: "gds_files::gallery", "Prepared Bamboo Light/Dark palettes");
                tracing::warn!(target: "gds::storage", "Running inside isolated gallery sandbox");
            })
            .into_handler(move || {
                let _ = Gds::current().appearance().install_window_sheet();
                let root_scope = Gds::current().tasks().scope();
                let volumes = volumes::Service::new(root_scope);
                let places = Places::default();
                let browser = Browser::new(
                    LaunchOptions {
                        places: Ok(places),
                        start: home_for_view.clone(),
                        home: home_for_view.clone(),
                        picker: None,
                    },
                    volumes,
                    Rc::new(|_| {}),
                );
                *browser_for_view.borrow_mut() = Some(browser.clone());
                browser.view()
            })
            .context("build headless Files gallery application")?;

        let scenario_error: Rc<RefCell<Option<anyhow::Error>>> = Rc::new(RefCell::new(None));
        let scenario_error_out = Rc::clone(&scenario_error);

        handler
            .drive(move |app| -> Result<(), PlatformError> {
                let mut harness = Harness::new(app);
                harness.redraw_on_configure(true);
                if let Some(offscreen) = harness.platform().offscreens().first() {
                    offscreen.set_a11y_listener(true);
                }
                harness.deliver_to_first(SurfaceEvent::Focused(true));
                pump_frames(&mut harness);

                let result = (|| -> Result<()> {
                    let browser = browser_slot
                        .borrow()
                        .clone()
                        .context("Files Browser was not constructed")?;
                    let ts = Cell::new(100_u64);
                    let mut semantic = SemanticStore::default();
                    let mut captured = Vec::new();
                    let mut ctx = GalleryCtx {
                        harness: &mut harness,
                        semantic: &mut semantic,
                        secondary_semantics: HashMap::new(),
                        registry: &registry_for_scenario,
                        artifacts: &artifacts,
                        captured: &mut captured,
                        ts: &ts,
                    };

                    // Wait for initial Home listing (Desktop, Documents, Downloads, Github, Pictures).
                    ensure!(
                        wait_until(ctx.harness, Duration::from_secs(4), |h| {
                            ctx.semantic.ingest_from(h);
                            ctx.registry.primary.borrow().frames > 0
                                && ctx.semantic.find_entry_center("Desktop").is_some()
                                && ctx.semantic.find_entry_center("Pictures").is_some()
                        }),
                        "gallery 01: Home folder entries did not render"
                    );

                    // 01: Browser list in Light Bamboo, 1 row selected in pointer mode.
                    let desktop_pos = ctx
                        .semantic
                        .find_entry_center("Desktop")
                        .context("gallery 01: Desktop row missing")?;
                    send_click(ctx.harness, desktop_pos, PointerButton::Primary, ctx.ts);
                    park_pointer(ctx.harness, ctx.ts);
                    ctx.snap_primary("01-browser-list-light")?;

                    // 02: Browser list keyboard focus mode (0 rows selected, Alt+Space focuses content).
                    browser.update_selection(|state| {
                        let view = state.tab_mut().navigation.view_mut();
                        view.selection.clear();
                        view.anchor = None;
                        view.cursor = None;
                    });
                    send_key(
                        ctx.harness,
                        KeyEvent::named(NamedKey::Space, PhysicalKey::Code(KeyCode::Space)),
                        Modifiers::ALT,
                        ctx.ts,
                    );
                    ctx.snap_primary("02-browser-list-keyboard-focus")?;

                    // 03: Browser list in Dark Bamboo (advance timestamp past double-click window),
                    // selecting both Desktop and symlink Docs-Shortcut to verify multi-row 36px pills
                    // and selected symlink corner badge without any gray panel circle.
                    ctx.ts.set(ctx.ts.get() + 1_000);
                    apply_bamboo_mode(&browser, ModePreference::Dark);
                    send_click(ctx.harness, desktop_pos, PointerButton::Primary, ctx.ts);
                    ctx.semantic.ingest_from(ctx.harness);
                    if let Some(shortcut_pos) = ctx.semantic.find_entry_center("Docs-Shortcut") {
                        let now = Timestamp::from_origin(Duration::from_millis(ctx.ts.get()));
                        ctx.ts.set(ctx.ts.get() + 16);
                        for (action, event) in [
                            (PointerAction::Moved, PointerEvent::mouse(shortcut_pos)),
                            (
                                PointerAction::Pressed,
                                PointerEvent::mouse(shortcut_pos)
                                    .with_button(PointerButton::Primary),
                            ),
                            (
                                PointerAction::Released,
                                PointerEvent::mouse(shortcut_pos)
                                    .with_button(PointerButton::Primary),
                            ),
                        ] {
                            ctx.harness.deliver_to_first(SurfaceEvent::Pointer {
                                action,
                                event,
                                modifiers: Modifiers::CONTROL,
                                timestamp: now,
                            });
                            ctx.harness.pump();
                        }
                        pump_frames(ctx.harness);
                    }
                    park_pointer(ctx.harness, ctx.ts);
                    ctx.snap_primary("03-browser-list-dark")?;

                    // Seed rich Documents contents for Grid, Details, Properties, Tags, Permissions, Picker.
                    apply_bamboo_mode(&browser, ModePreference::Light);
                    let docs = home.join("Documents");
                    let readme_path = docs.join("README.md");
                    let arch_path = docs.join("architecture.rs");
                    let preview_png = docs.join("preview.png");
                    let csv_path = docs.join("metrics-2026.csv");
                    let hidden_path = docs.join(".env.example");
                    fs::create_dir_all(docs.join("Designs"))?;
                    fs::write(
                        &readme_path,
                        b"# GDS Files\nNative Wayland file manager built on zgui.\n",
                    )?;
                    fs::write(&arch_path, b"pub fn render_workspace() {}\n")?;
                    fs::write(&csv_path, b"page,fps,frame_ms\nbrowser,144,1.2\n")?;
                    fs::write(docs.join("release-notes.txt"), b"v0.1.0 release notes\n")?;
                    fs::write(&hidden_path, b"GDS_LOG=info\n")?;
                    let _ = std::os::unix::fs::symlink(&arch_path, docs.join("arch-link.rs"));
                    write_sample_png(&preview_png)?;
                    let _ = gds::service::file_tags::write(
                        &readme_path,
                        &["design".to_owned(), "milestone/v1".to_owned()],
                    );

                    // 04: Browser Grid view (Light + Dark).
                    browser.open(Location::from(&docs));
                    ensure!(
                        wait_until(ctx.harness, Duration::from_secs(3), |h| {
                            ctx.semantic.ingest_from(h);
                            ctx.semantic.find_entry_center("preview.png").is_some()
                        }),
                        "gallery 04: Documents entries did not render"
                    );
                    browser.toggle_view();
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |_| {
                        browser.read_untracked(|state| state.tab().mode == ViewMode::Grid)
                    });
                    // Allow directory enrichment + thumbnail pipeline to decode preview.png.
                    let _ = wait_until(ctx.harness, Duration::from_secs(3), |_| {
                        let cache = browser.0.inspections.get_untracked();
                        cache.entry_count() > 0 && cache.running_count() == 0 && cache.pending_count() == 0
                    });
                    for _ in 0..8 {
                        std::thread::sleep(Duration::from_millis(25));
                        pump_frames(ctx.harness);
                    }
                    ctx.semantic.ingest_from(ctx.harness);
                    if let Some(pos) = ctx.semantic.find_entry_center("preview.png") {
                        send_click(ctx.harness, pos, PointerButton::Primary, ctx.ts);
                        park_pointer(ctx.harness, ctx.ts);
                    }
                    ctx.snap_primary("04-browser-grid")?;
                    apply_bamboo_mode(&browser, ModePreference::Dark);
                    ctx.snap_primary("04b-browser-grid-dark")?;
                    apply_bamboo_mode(&browser, ModePreference::Light);
                    browser.toggle_view();
                    pump_frames(ctx.harness);

                    // 05: Compact responsive mode (860x640) + collapsed icon-rail sidebar.
                    ctx.harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                        DevicePx(COMPACT_WIDTH),
                        DevicePx(COMPACT_HEIGHT),
                    )));
                    browser.0.sidebar.set(false);
                    ctx.snap_primary("05-browser-compact-collapsed")?;
                    browser.0.sidebar.set(true);
                    apply_bamboo_mode(&browser, ModePreference::Dark);
                    ctx.snap_primary("05b-browser-compact-expanded-dark")?;
                    apply_bamboo_mode(&browser, ModePreference::Light);
                    ctx.harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                        DevicePx(GALLERY_WIDTH),
                        DevicePx(GALLERY_HEIGHT),
                    )));
                    pump_frames(ctx.harness);
                    browser.0.sidebar.set(false);
                    ctx.snap_primary("05c-browser-desktop-collapsed-rail")?;
                    browser.0.sidebar.set(true);
                    pump_frames(ctx.harness);

                    // 06: Recent page.
                    browser.update(|state| {
                        state.places.record(Location::from(&docs));
                        state.places.record(Location::from(home.join("Docs-Shortcut")));
                        state.places.record(Location::from(home.join("Pictures")));
                        state.places.record(Location::from(home.join("Downloads")));
                    });
                    browser.open(Location::Recent);
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.find_entry_center("Documents").is_some()
                    });
                    if let Some(pos) = ctx.semantic.find_entry_center("Documents") {
                        send_click(ctx.harness, pos, PointerButton::Primary, ctx.ts);
                        park_pointer(ctx.harness, ctx.ts);
                    }
                    ctx.snap_primary("06-page-recent")?;

                    // 07: Favorites page.
                    browser.update(|state| {
                        state
                            .places
                            .set_favorite(Location::from(&docs), true);
                        state
                            .places
                            .set_favorite(Location::from(home.join("Github")), true);
                        state
                            .places
                            .set_favorite(Location::from(home.join("Pictures")), true);
                    });
                    browser.open(Location::Favorites);
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.find_entry_center("Documents").is_some()
                    });
                    if let Some(pos) = ctx.semantic.find_entry_center("Documents") {
                        send_click(ctx.harness, pos, PointerButton::Primary, ctx.ts);
                        park_pointer(ctx.harness, ctx.ts);
                    }
                    ctx.snap_primary("07-page-favorites")?;

                    // 08: Volumes page.
                    browser.open(Location::Volumes);
                    std::thread::sleep(Duration::from_millis(150));
                    ctx.snap_primary("08-page-volumes")?;

                    // 09: Network page + Connect Server dialog.
                    if let Ok(sftp_addr) =
                        Address::parse("sftp://atlas@files.gandesktop.org:22/srv/shared")
                    {
                        browser.update(|state| {
                            state.places.set_remote_favorite(sftp_addr, true);
                        });
                    }
                    if let Ok(smb_addr) =
                        Address::parse("smb://atlas@nas.office.internal:445/media/archive")
                    {
                        browser.update(|state| {
                            state.places.set_remote_favorite(smb_addr, true);
                        });
                    }
                    browser.open(Location::Network);
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |_| {
                        browser.read_untracked(|state| {
                            matches!(
                                state.tab().contents,
                                crate::pages::workspace::model::Contents::Network { .. }
                            )
                        })
                    });
                    ctx.snap_primary("09a-page-network")?;
                    browser.connect_server();
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.has_role(Role::Dialog)
                    });
                    ctx.snap_primary("09b-dialog-connect-protocols")?;
                    if let Some(sftp_btn) = ctx.semantic.find_button_center("SFTP") {
                        send_click(ctx.harness, sftp_btn, PointerButton::Primary, ctx.ts);
                    }
                    ctx.snap_primary("09-page-network-and-connect-dialog")?;
                    browser.0.connector.cancel();
                    browser.0.notice.set(None);
                    pump_frames(ctx.harness);

                    // 10: Trash page with real trashed items inside sandbox XDG_DATA_HOME/Trash.
                    let trashed_file = docs.join("deprecated-draft.md");
                    let trashed_dir = docs.join("Archive-2025");
                    fs::write(&trashed_file, b"draft content\n")?;
                    fs::create_dir_all(&trashed_dir)?;
                    let _ = trash::delete_all([&trashed_file, &trashed_dir]);
                    browser.open(Location::Trash);
                    let _ = wait_until(ctx.harness, Duration::from_secs(3), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic
                            .find_entry_center("deprecated-draft.md")
                            .is_some()
                    });
                    if let Some(pos) = ctx.semantic.find_entry_center("deprecated-draft.md") {
                        send_click(ctx.harness, pos, PointerButton::Primary, ctx.ts);
                        park_pointer(ctx.harness, ctx.ts);
                    }
                    ctx.snap_primary("10-page-trash")?;

                    // 11: Tags catalogue page.
                    browser.edit_preferences(|prefs| {
                        prefs.tags = tag_prefs.clone();
                    });
                    pump_frames(ctx.harness);
                    let root_tags = TagAddress::new("")
                        .map_err(|err| anyhow::anyhow!("invalid root tag address: {err:?}"))?;
                    browser.open(Location::Tags(root_tags));
                    let _ = wait_until(ctx.harness, Duration::from_secs(3), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.find_entry_center("design").is_some()
                    });
                    ctx.snap_primary("11-page-tags-catalogue")?;

                    let nested_tag = TagAddress::new("milestone/v1")
                        .map_err(|err| anyhow::anyhow!("invalid nested tag address: {err:?}"))?;
                    browser.open(Location::Tags(nested_tag));
                    let _ = wait_until(ctx.harness, Duration::from_secs(3), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.find_entry_center("README.md").is_some()
                    });
                    ctx.snap_primary("11b-page-tags-items")?;

                    // 12: Tag Editor dialog + Destructive Confirm 5-item preview dialog.
                    browser.open(Location::from(&docs));
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.find_entry_center("README.md").is_some()
                    });
                    browser.edit_tags(readme_path.clone());
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.has_role(Role::Dialog)
                    });
                    std::thread::sleep(Duration::from_millis(80));
                    ctx.snap_primary("12-dialog-tag-editor")?;
                    send_key(
                        ctx.harness,
                        KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                        Modifiers::NONE,
                        ctx.ts,
                    );

                    browser.select_all();
                    pump_frames(ctx.harness);
                    browser.delete(true);
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.has_role(Role::Dialog)
                    });
                    ctx.snap_primary("12b-dialog-destructive-confirm")?;
                    browser.cancel_prompt();
                    browser.update_selection(|state| {
                        let view = state.tab_mut().navigation.view_mut();
                        view.selection.clear();
                        view.anchor = None;
                        view.cursor = None;
                    });
                    pump_frames(ctx.harness);

                    // 13: Details panel + embedded Terminal dock (Light & Dark).
                    ctx.semantic.ingest_from(ctx.harness);
                    if let Some(pos) = ctx.semantic.find_entry_center("preview.png") {
                        send_click(ctx.harness, pos, PointerButton::Primary, ctx.ts);
                        park_pointer(ctx.harness, ctx.ts);
                    }
                    browser.0.details.set(true);
                    browser.toggle_terminal();
                    for _ in 0..8 {
                        std::thread::sleep(Duration::from_millis(40));
                        pump_frames(ctx.harness);
                    }
                    ctx.snap_primary("13-details-and-terminal-dock")?;
                    browser.0.details.set(false);
                    pump_frames(ctx.harness);
                    ctx.semantic.ingest_from(ctx.harness);
                    if let Some(pos) = ctx.semantic.find_entry_center("README.md") {
                        send_click(ctx.harness, pos, PointerButton::Primary, ctx.ts);
                        park_pointer(ctx.harness, ctx.ts);
                    }
                    apply_bamboo_mode(&browser, ModePreference::Dark);
                    browser.0.details.set(true);
                    ctx.snap_primary("13b-details-and-terminal-dark")?;
                    browser.0.details.set(false);
                    browser.toggle_terminal();
                    apply_bamboo_mode(&browser, ModePreference::Light);
                    pump_frames(ctx.harness);

                    // 14: Standalone Properties window (460x620 secondary surface).
                    let props_path = preview_png.clone();
                    let props_scope = browser.scope();
                    let props_surface = open_secondary_and_wait(ctx.harness, || {
                        browser.0.owner.with(|| {
                            Properties::open(props_path, props_scope);
                        });
                    })?;
                    std::thread::sleep(Duration::from_millis(160));
                    ctx.snap_surface("14-window-properties", &props_surface)?;
                    close_secondary(ctx.harness, &props_surface);

                    // 15: Permissions dialog.
                    let readme_entry = Entry::inspect(&readme_path)
                        .map_err(|err| anyhow::anyhow!("inspect README.md: {err}"))?;
                    browser.edit_permissions(readme_entry);
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.has_role(Role::Dialog)
                    });
                    ctx.snap_primary("15-dialog-permissions")?;
                    send_key(
                        ctx.harness,
                        KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                        Modifiers::NONE,
                        ctx.ts,
                    );

                    // 16: Settings Center — render every available Settings category page.
                    browser.show_settings();
                    pump_frames(ctx.harness);
                    let settings_shots: [&str; 6] = [
                        "16a-settings-general",
                        "16b-settings-appearance",
                        "16c-settings-files",
                        "16d-settings-terminal",
                        "16e-settings-extensions",
                        "16f-settings-performance",
                    ];
                    for (idx, shot_name) in settings_shots.into_iter().enumerate() {
                        if idx > 0 {
                            browser.0.settings.cycle_page(false);
                        }
                        std::thread::sleep(Duration::from_millis(60));
                        ctx.snap_primary(shot_name)?;
                        if idx == 0 {
                            ctx.snap_primary("16-settings-pages")?;
                        }
                        if idx == 1 {
                            apply_bamboo_mode(&browser, ModePreference::Dark);
                            ctx.snap_primary("16b-settings-appearance-dark")?;
                            apply_bamboo_mode(&browser, ModePreference::Light);
                        }
                    }
                    ctx.semantic.ingest_from(ctx.harness);
                    if let Some(unlock_btn) =
                        ctx.semantic.find_button_center("I understand, enable editing")
                    {
                        send_click(ctx.harness, unlock_btn, PointerButton::Primary, ctx.ts);
                        ctx.snap_primary("16g-settings-performance-unlocked")?;
                    }
                    browser.show_files();
                    pump_frames(ctx.harness);

                    // 17: FileChooser Open & Save windows at 1280x800.
                    let open_req = chooser::Request {
                        app_id: "io.github.GanDesktop.GdsFiles".into(),
                        title: "Open Document".into(),
                        parent_window: String::new(),
                        modal: true,
                        accept_label: None,
                        mode: chooser::Mode::Open {
                            directory: false,
                            multiple: true,
                        },
                        current_folder: None,
                        current_file: None,
                        filters: vec![
                            chooser::Filter {
                                label: "Images & Markdown".into(),
                                patterns: vec![
                                    chooser::Pattern::Glob("*.png".into()),
                                    chooser::Pattern::Glob("*.md".into()),
                                ],
                            },
                            chooser::Filter {
                                label: "All Files".into(),
                                patterns: vec![chooser::Pattern::Glob("*".into())],
                            },
                        ],
                        current_filter: None,
                        choices: Vec::new(),
                    };
                    let open_opts = pick_files::Options::from_request(open_req)
                        .context("build Open picker options")?;
                    let picker_docs = docs.clone();
                    let picker_home = home.clone();
                    let picker_volumes = browser.0.volumes.clone();
                    let open_picker_surface = open_secondary_and_wait(ctx.harness, || {
                        browser.0.owner.with(|| {
                            if let Some(windows) = try_use_windows() {
                                let opts = workspace_options("Open Document", None)
                                    .with_size(GALLERY_WIDTH, GALLERY_HEIGHT);
                                let launch_slot = RefCell::new(Some(LaunchOptions {
                                    places: Ok(Places::default()),
                                    start: picker_docs,
                                    home: picker_home,
                                    picker: Some(open_opts),
                                }));
                                windows.open(opts, move || {
                                    let _ = Gds::current().appearance().install_window_sheet();
                                    let Some(launch) = launch_slot.borrow_mut().take() else {
                                        return AnyView::new(view! { box() });
                                    };
                                    let picker_browser =
                                        Browser::new(launch, picker_volumes.clone(), Rc::new(|_| {}));
                                    AnyView::new(picker_browser.view())
                                });
                            }
                        });
                    })?;
                    std::thread::sleep(Duration::from_millis(120));
                    ctx.snap_surface("17a-picker-open", &open_picker_surface)?;
                    close_secondary(ctx.harness, &open_picker_surface);

                    let save_req = chooser::Request {
                        app_id: "io.github.GanDesktop.GdsFiles".into(),
                        title: "Save Export".into(),
                        parent_window: String::new(),
                        modal: true,
                        accept_label: None,
                        mode: chooser::Mode::Save {
                            name: "quarterly-report.md".into(),
                        },
                        current_folder: None,
                        current_file: None,
                        filters: vec![
                            chooser::Filter {
                                label: "Markdown (*.md)".into(),
                                patterns: vec![chooser::Pattern::Glob("*.md".into())],
                            },
                            chooser::Filter {
                                label: "All Files".into(),
                                patterns: vec![chooser::Pattern::Glob("*".into())],
                            },
                        ],
                        current_filter: None,
                        choices: Vec::new(),
                    };
                    let save_opts = pick_files::Options::from_request(save_req)
                        .context("build Save picker options")?;
                    let save_docs = docs.clone();
                    let save_home = home.clone();
                    let save_volumes = browser.0.volumes.clone();
                    let save_picker_browser: Rc<RefCell<Option<Browser>>> =
                        Rc::new(RefCell::new(None));
                    let save_picker_slot = Rc::clone(&save_picker_browser);
                    let save_picker_surface = open_secondary_and_wait(ctx.harness, || {
                        browser.0.owner.with(|| {
                            if let Some(windows) = try_use_windows() {
                                let opts = workspace_options("Save Export", None)
                                    .with_size(GALLERY_WIDTH, GALLERY_HEIGHT);
                                let launch_slot = RefCell::new(Some(LaunchOptions {
                                    places: Ok(Places::default()),
                                    start: save_docs,
                                    home: save_home,
                                    picker: Some(save_opts),
                                }));
                                let picker_out = Rc::clone(&save_picker_slot);
                                windows.open(opts, move || {
                                    let _ = Gds::current().appearance().install_window_sheet();
                                    let Some(launch) = launch_slot.borrow_mut().take() else {
                                        return AnyView::new(view! { box() });
                                    };
                                    let picker_browser =
                                        Browser::new(launch, save_volumes.clone(), Rc::new(|_| {}));
                                    *picker_out.borrow_mut() = Some(picker_browser.clone());
                                    AnyView::new(picker_browser.view())
                                });
                            }
                        });
                    })?;
                    std::thread::sleep(Duration::from_millis(120));
                    ctx.snap_surface("17-picker-open-and-save", &save_picker_surface)?;
                    if let Some(ref picker_b) = *save_picker_browser.borrow()
                        && let Ok(remote_addr) =
                            Address::parse("sftp://atlas@files.gandesktop.org:22/srv/shared")
                    {
                        picker_b.update(|state| {
                            state.places.set_remote_favorite(remote_addr.clone(), true);
                        });
                        picker_b.open(Location::Network);
                        pump_frames(ctx.harness);
                        picker_b.open(Location::Remote(remote_addr));
                        pump_frames(ctx.harness);
                        ctx.snap_surface("18b-picker-remote-local-notice", &save_picker_surface)?;
                    }
                    close_secondary(ctx.harness, &save_picker_surface);

                    // 18: Remote Directory Picker modal dialog (`DirectoryPickerDialog`).
                    let remote_dir_picker_slot: Rc<RefCell<Option<DirectoryPicker>>> =
                        Rc::new(RefCell::new(None));
                    let remote_dir_picker_out = Rc::clone(&remote_dir_picker_slot);
                    let remote_picker_surface = open_secondary_and_wait(ctx.harness, || {
                        browser.0.owner.with(|| {
                            if let Some(windows) = try_use_windows() {
                                let opts = workspace_options("Remote Directory Picker", None)
                                    .with_size(GALLERY_WIDTH, GALLERY_HEIGHT);
                                let picker_slot = Rc::clone(&remote_dir_picker_out);
                                windows.open(opts, move || {
                                    let _ = Gds::current().appearance().install_window_sheet();
                                    let loader: gds::ui::picker::directory::Loader =
                                        Rc::new(|req| {
                                            let path = req
                                                .path
                                                .unwrap_or_else(|| "/srv/shared/projects".into());
                                            Box::pin(async move {
                                                Ok(DirPage {
                                                    location: DirLocation {
                                                        path: path.clone(),
                                                        label: "projects".into(),
                                                    },
                                                    parent: Some("/srv/shared".into()),
                                                    ancestors: vec![
                                                        DirLocation {
                                                            path: "/".into(),
                                                            label: "/".into(),
                                                        },
                                                        DirLocation {
                                                            path: "/srv".into(),
                                                            label: "srv".into(),
                                                        },
                                                        DirLocation {
                                                            path: "/srv/shared".into(),
                                                            label: "shared".into(),
                                                        },
                                                        DirLocation {
                                                            path: path.clone(),
                                                            label: "projects".into(),
                                                        },
                                                    ],
                                                    entries: vec![
                                                        DirEntry {
                                                            location: DirLocation {
                                                                path: format!("{path}/assets"),
                                                                label: "assets".into(),
                                                            },
                                                            hidden: false,
                                                        },
                                                        DirEntry {
                                                            location: DirLocation {
                                                                path: format!("{path}/backups"),
                                                                label: "backups".into(),
                                                            },
                                                            hidden: false,
                                                        },
                                                        DirEntry {
                                                            location: DirLocation {
                                                                path: format!("{path}/docs"),
                                                                label: "docs".into(),
                                                            },
                                                            hidden: false,
                                                        },
                                                        DirEntry {
                                                            location: DirLocation {
                                                                path: format!("{path}/releases"),
                                                                label: "releases".into(),
                                                            },
                                                            hidden: false,
                                                        },
                                                    ],
                                                    next_cursor: None,
                                                })
                                            })
                                        });
                                    let dir_picker =
                                        DirectoryPicker::with_path(loader, "/srv/shared/projects");
                                    *picker_slot.borrow_mut() = Some(dir_picker.clone());
                                    let open_sig = RwSignal::new_local(true);
                                    view! {
                                        Frame(without_sidebar = true) {
                                            TitleBar(title = "sftp://files.gandesktop.org/srv/shared")
                                            box(
                                                style:flex = "1 1 0%",
                                                style:background-color = "var(--gds-content-bg, var(--gds-content))"
                                            ) {
                                                DirectoryPickerDialog(
                                                    open = open_sig,
                                                    picker = dir_picker,
                                                    on_select = UnsyncCallback::new(|_| {}),
                                                )
                                            }
                                        }
                                    }
                                });
                            }
                        });
                    })?;
                    if let Some(ref picker) = *remote_dir_picker_slot.borrow() {
                        let _ = wait_until(ctx.harness, Duration::from_secs(2), |_| {
                            !picker.rows().get_untracked().is_empty()
                        });
                        picker.select_row(0);
                        pump_frames(ctx.harness);
                    }
                    ctx.snap_surface(
                        "18-dialog-remote-directory-picker",
                        &remote_picker_surface,
                    )?;
                    close_secondary(ctx.harness, &remote_picker_surface);

                    // 19: Application menu, file context menu, column config dialog, status popover.
                    browser.open(Location::from(&docs));
                    let _ = wait_until(ctx.harness, Duration::from_secs(2), |h| {
                        ctx.semantic.ingest_from(h);
                        ctx.semantic.find_entry_center("README.md").is_some()
                    });
                    if let Some(pos) = ctx.semantic.find_entry_center("README.md") {
                        send_click(ctx.harness, pos, PointerButton::Primary, ctx.ts);
                        park_pointer(ctx.harness, ctx.ts);
                    }
                    send_key(
                        ctx.harness,
                        KeyEvent::character("c"),
                        Modifiers::CONTROL,
                        ctx.ts,
                    );
                    Clipboard::current().refresh();
                    send_key(
                        ctx.harness,
                        KeyEvent::character("X"),
                        Modifiers::ALT | Modifiers::SHIFT,
                        ctx.ts,
                    );
                    send_click(
                        ctx.harness,
                        Point::new(CssPx(980.0), CssPx(6.0)),
                        PointerButton::Primary,
                        ctx.ts,
                    );
                    park_pointer(ctx.harness, ctx.ts);

                    browser.show_app_menu();
                    ctx.snap_primary("19a-menu-application")?;
                    browser.0.menus.close();
                    pump_frames(ctx.harness);

                    browser.show_context_menu();
                    ctx.snap_primary("19-menus-and-status-popovers")?;
                    browser.0.menus.close();
                    pump_frames(ctx.harness);

                    browser.configure_columns();
                    ctx.snap_primary("19c-dialog-column-config")?;
                    browser.0.menus.close();
                    pump_frames(ctx.harness);

                    browser
                        .0
                        .menus
                        .surface
                        .set(MenuSurface::Status(StatusKind::Selection));
                    ctx.snap_primary("19d-popover-status-selection")?;
                    browser.0.menus.close();
                    pump_frames(ctx.harness);

                    let _ = browser.begin_background_task("files", "Copying 12 items");
                    browser.set_background_task_detail("files", Some("preview.png".into()));
                    let _ = browser.begin_background_task(
                        "remote-task-1",
                        "Uploading to files.gandesktop.org",
                    );
                    browser.set_background_task_detail(
                        "remote-task-1",
                        Some("4 · 18.4 MB · release-notes.txt".into()),
                    );
                    browser
                        .0
                        .menus
                        .surface
                        .set(MenuSurface::Status(StatusKind::Tasks));
                    ctx.snap_primary("19e-popover-status-tasks")?;
                    browser.0.menus.close();
                    let _ = browser.finish_background_task("files");
                    let _ = browser.finish_background_task("remote-task-1");
                    pump_frames(ctx.harness);

                    // 20: Overlays — About dialog, Help page, Keytips + Toast notifications.
                    browser.show_about();
                    ctx.snap_primary("20a-dialog-about")?;
                    browser.cancel_prompt();
                    pump_frames(ctx.harness);

                    browser.show_help();
                    ctx.snap_primary("20b-page-help")?;
                    browser.show_files();
                    pump_frames(ctx.harness);

                    browser.0.owner.with(|| {
                        gds::ui::feedback::notifications::success("Copied 2 items to Documents");
                        gds::ui::feedback::notifications::info("Indexed 4 workspace tags");
                    });
                    pump_frames(ctx.harness);
                    let alt_down_ts = Timestamp::from_origin(Duration::from_millis(ctx.ts.get()));
                    ctx.ts.set(ctx.ts.get() + 200);
                    ctx.harness.deliver_to_first(SurfaceEvent::Key {
                        state: KeyState::Pressed,
                        event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
                        modifiers: Modifiers::ALT,
                        timestamp: alt_down_ts,
                    });
                    pump_frames(ctx.harness);
                    ctx.snap_primary("20-overlays-about-help-keytips-notifications")?;
                    ctx.harness.deliver_to_first(SurfaceEvent::Key {
                        state: KeyState::Released,
                        event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ctx.ts.get())),
                    });
                    send_key(
                        ctx.harness,
                        KeyEvent::character("X"),
                        Modifiers::ALT | Modifiers::SHIFT,
                        ctx.ts,
                    );
                    let ctrl_down_ts = Timestamp::from_origin(Duration::from_millis(ctx.ts.get() + 40));
                    ctx.ts.set(ctx.ts.get() + 240);
                    ctx.harness.deliver_to_first(SurfaceEvent::Key {
                        state: KeyState::Pressed,
                        event: KeyEvent::named(
                            NamedKey::Control,
                            PhysicalKey::Code(KeyCode::ControlLeft),
                        ),
                        modifiers: Modifiers::CONTROL,
                        timestamp: ctrl_down_ts,
                    });
                    pump_frames(ctx.harness);
                    ctx.snap_primary("20c-overlays-ctrl-keytips")?;
                    ctx.harness.deliver_to_first(SurfaceEvent::Key {
                        state: KeyState::Released,
                        event: KeyEvent::named(
                            NamedKey::Control,
                            PhysicalKey::Code(KeyCode::ControlLeft),
                        ),
                        modifiers: Modifiers::NONE,
                        timestamp: Timestamp::from_origin(Duration::from_millis(ctx.ts.get())),
                    });
                    send_key(
                        ctx.harness,
                        KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
                        Modifiers::NONE,
                        ctx.ts,
                    );

                    // 21: Secondary Logs & Tasks diagnostic window (`gds::ui::logs::open`).
                    let logs_surface = open_secondary_and_wait(ctx.harness, || {
                        browser.0.owner.with(|| {
                            gds::ui::logs::open();
                        });
                    })?;
                    let logs_id = zgui::platform::Surface::id(logs_surface.as_ref());
                    std::thread::sleep(Duration::from_millis(100));
                    pump_frames(ctx.harness);
                    // Select the first log row (y = 148..170) so the detail pane mounts.
                    send_click_on(
                        ctx.harness,
                        logs_id,
                        Point::new(CssPx(320.0), CssPx(160.0)),
                        PointerButton::Primary,
                        ctx.ts,
                    );
                    ctx.snap_surface("21-window-logs-and-tasks", &logs_surface)?;

                    // Switch to the Tasks tab (bounds [69..113, 53..92]) and capture live TaskManagerView.
                    send_click_on(
                        ctx.harness,
                        logs_id,
                        Point::new(CssPx(90.0), CssPx(72.0)),
                        PointerButton::Primary,
                        ctx.ts,
                    );
                    ctx.snap_surface("21b-window-tasks-tab", &logs_surface)?;
                    close_secondary(ctx.harness, &logs_surface);

                    let primary_guard = ctx.registry.primary.borrow();
                    let backend = if primary_guard.gpu_backed {
                        "wgpu+vello"
                    } else {
                        "cpu-fallback"
                    };
                    let mut summary = format!(
                        "PASS gallery\nrenderer={backend}\ncount={}\nartifacts={}\n",
                        ctx.captured.len(),
                        artifacts.display()
                    );
                    for item in ctx.captured.iter() {
                        let _ = writeln!(summary, "shot={item}");
                    }
                    fs::write(artifacts.join("gallery-passed.txt"), summary)?;
                    Ok(())
                })();

                if let Err(err) = result {
                    *scenario_error_out.borrow_mut() = Some(err);
                }
                harness.shut_down();
                Ok(())
            })
            .map_err(|error| anyhow::anyhow!("zgui gallery harness failed: {error}"))?;

        if let Some(err) = scenario_error.borrow_mut().take() {
            return Err(err);
        }
        Ok(())
    }
}
