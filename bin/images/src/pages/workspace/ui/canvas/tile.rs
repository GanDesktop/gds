//! A mounted tile keeps its texture until the replacement is uploaded on the same GPU queue.

use super::*;
use image::RgbaImage;
use tokio::sync::watch;
use zgui::surface::{GpuShare, SurfaceConfig, SurfaceEvent, wgpu};

struct Tile {
    surface: SurfaceHandle,
    pixels: watch::Sender<Arc<RgbaImage>>,
}

impl Tile {
    fn new(page: Page, image: &Arc<RgbaImage>) -> Self {
        let surface = SurfaceHandle::new(SurfaceConfig {
            premultiplied: false,
            ..Default::default()
        });
        let (device, mut devices) = watch::channel(None);
        surface.set_events(move |event| match event {
            SurfaceEvent::Attached { gpu, .. } | SurfaceEvent::DeviceLost { gpu } => {
                device.send_replace(Some(gpu));
            }
            SurfaceEvent::Detached => {
                device.send_replace(None);
            }
            _ => {}
        });
        let (pixels, mut images) = watch::channel(image.clone());
        let presenter = surface.clone();
        page.local(async move {
            let mut texture = None;
            let mut generation = None;
            loop {
                let update = tokio::select! {
                    result = devices.changed() => result,
                    result = images.changed() => result,
                };
                if update.is_err() {
                    break;
                }
                let gpu = devices.borrow_and_update().clone();
                let next_generation = gpu.as_ref().map(GpuShare::generation);
                if generation != next_generation {
                    texture = None;
                    generation = next_generation;
                }
                let image = images.borrow_and_update().clone();
                if let Some(gpu) = gpu {
                    upload(&presenter, &gpu, &image, &mut texture);
                }
            }
        });
        Self { surface, pixels }
    }
}

fn upload(
    surface: &SurfaceHandle,
    gpu: &GpuShare,
    image: &RgbaImage,
    slot: &mut Option<Arc<wgpu::Texture>>,
) {
    if image.width() == 0 || image.height() == 0 {
        return;
    }
    let size = wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    if slot.as_ref().is_some_and(|texture| texture.size() != size) {
        *slot = None;
    }
    let texture = slot.get_or_insert_with(|| {
        Arc::new(gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("GDS Images tile"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        }))
    });
    gpu.queue().write_texture(
        texture.as_image_copy(),
        image.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.width * 4),
            rows_per_image: None,
        },
        size,
    );
    surface.present(texture.clone());
}

#[derive(Default)]
pub(super) struct Tiles(Vec<Option<Tile>>);

impl Tiles {
    pub(super) fn resize(&mut self, count: usize) {
        self.0.resize_with(count, || None);
    }

    pub(super) fn hide(&mut self, index: usize) -> AnyView {
        self.0[index] = None;
        AnyView::new(())
    }

    pub(super) fn show(
        &mut self,
        page: Page,
        index: usize,
        image: &Arc<RgbaImage>,
        rect: Rect2,
    ) -> AnyView {
        let tile = self.0[index].get_or_insert_with(|| Tile::new(page, image));
        tile.pixels.send_if_modified(|current| {
            if Arc::ptr_eq(current, image) {
                return false;
            }
            *current = image.clone();
            true
        });
        AnyView::new(view! {
            surface(
                source = &tile.surface,
                class = "images-tile",
                a11y:hidden = true,
                style:left = px(rect.x),
                style:top = px(rect.y),
                style:width = px(rect.width),
                style:height = px(rect.height),
            )
        })
    }
}
