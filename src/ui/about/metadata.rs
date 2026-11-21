//! Application identity embedded from Cargo metadata at build time.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use zgui_image::ImageBytes;

const DESKTOP_ICON_RASTER_PX: u32 = 256;

#[derive(Debug)]
pub struct Application {
    pub binary: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub version: &'static str,
    pub build_tag: &'static str,
    pub website: &'static str,
    pub feedback: &'static str,
    pub authors: &'static [&'static str],
    pub credits: &'static [Credit],
    pub license: &'static str,
    pub license_text: &'static str,
}

#[derive(Debug)]
pub struct Credit {
    pub name: &'static str,
    pub website: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/about.rs"));

fn rasterize_desktop_svg(svg: &str) -> Option<ImageBytes> {
    use resvg::{tiny_skia, usvg};

    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let max_edge = size.width().max(size.height());
    if !max_edge.is_finite() || max_edge <= 0.0 {
        return None;
    }
    let scale = DESKTOP_ICON_RASTER_PX as f32 / max_edge;
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for pixel in pixmap.pixels() {
        let px = pixel.demultiply();
        rgba.extend_from_slice(&[px.red(), px.green(), px.blue(), px.alpha()]);
    }
    let mut png_bytes = Vec::new();
    let encoder =
        PngEncoder::new_with_quality(&mut png_bytes, CompressionType::Fast, FilterType::Sub);
    encoder
        .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(ImageBytes::new(png_bytes))
}

impl Application {
    /// The entry for an application registered in Cargo metadata.
    ///
    /// # Panics
    /// Panics if `binary` is not a registered target of this package.
    pub fn cargo(binary: &str) -> &'static Self {
        Self::try_cargo(binary).expect("Cargo binary has registered application metadata")
    }

    /// Look up an application registered in Cargo metadata by binary target name.
    #[must_use]
    pub fn try_cargo(binary: &str) -> Option<&'static Self> {
        APPLICATIONS
            .iter()
            .find(|application| application.binary == binary)
    }

    /// Embedded monochrome solid SVG brand artwork corresponding to [`Application::icon`].
    #[must_use]
    pub const fn artwork_svg(&self) -> Option<&'static str> {
        match self.icon.as_bytes() {
            b"assets/files/icon-solid.svg" => {
                Some(include_str!("../../../assets/files/icon-solid.svg"))
            }
            b"assets/agents/icon-solid.svg" => {
                Some(include_str!("../../../assets/agents/icon-solid.svg"))
            }
            b"assets/images/icon-solid.svg" => {
                Some(include_str!("../../../assets/images/icon-solid.svg"))
            }
            _ => None,
        }
    }

    /// Embedded full-color desktop launcher SVG artwork corresponding to [`Application::icon`].
    #[must_use]
    pub const fn desktop_icon_svg(&self) -> Option<&'static str> {
        match self.icon.as_bytes() {
            b"assets/files/icon-solid.svg" => {
                Some(include_str!("../../../assets/files/icon-desktop.svg"))
            }
            b"assets/agents/icon-solid.svg" => {
                Some(include_str!("../../../assets/agents/icon-desktop.svg"))
            }
            b"assets/images/icon-solid.svg" => {
                Some(include_str!("../../../assets/images/icon-desktop.svg"))
            }
            _ => None,
        }
    }

    /// Rasterized, process-cached HiDPI PNG representation of [`Application::desktop_icon_svg`],
    /// preserving full SVG filters (Gaussian blur shading, clips, and gradients).
    #[must_use]
    pub fn desktop_icon_image(&self) -> Option<ImageBytes> {
        static CACHE: OnceLock<Mutex<HashMap<&'static str, Option<ImageBytes>>>> = OnceLock::new();
        let svg = self.desktop_icon_svg()?;
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .entry(self.binary)
            .or_insert_with(|| rasterize_desktop_svg(svg))
            .clone()
    }
}
