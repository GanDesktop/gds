use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::{Arc, OnceLock},
};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage};

use super::{EDGE, Error};

const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
const SVG_PREFIX_BYTES: usize = 4 * 1024;

/// # Errors
/// Reports unreadable, unsupported or oversized images.
pub(super) fn load(path: &Path, max_source_bytes: u64) -> Result<RgbaImage, Error> {
    let max_source_bytes = max_source_bytes.max(1);
    let file = File::open(path)?;
    if file.metadata()?.len() > max_source_bytes {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(max_source_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_source_bytes {
        return Err(Error::TooLarge);
    }
    load_bytes(&bytes, Some(path), max_source_bytes)
}

/// # Errors
/// Reports unsupported, malformed or oversized embedded images.
pub(super) fn load_bytes(
    bytes: &[u8],
    path: Option<&Path>,
    max_source_bytes: u64,
) -> Result<RgbaImage, Error> {
    let max_source_bytes = max_source_bytes.max(1);
    if bytes.len() as u64 > max_source_bytes {
        return Err(Error::TooLarge);
    }
    if is_svg(bytes) {
        return render_svg(bytes);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if reader.format().is_none()
        && let Some(path) = path
    {
        reader.set_format(ImageFormat::from_path(path)?);
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    // Decode in the source pixel format; RGBA conversion happens after resizing.
    if decoder.total_bytes() > MAX_DECODE_BYTES {
        return Err(Error::TooLarge);
    }
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?.thumbnail(EDGE, EDGE);
    image.apply_orientation(orientation);
    Ok(image.to_rgba8())
}

/// # Errors
/// Reports malformed SVGs or an unavailable raster allocation.
fn render_svg(bytes: &[u8]) -> Result<RgbaImage, Error> {
    use resvg::{tiny_skia, usvg};
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        Arc::new(fonts)
    });
    let options = usvg::Options {
        fontdb: fonts.clone(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(bytes, &options)?;
    let dimensions = tree.size();
    let source_width = dimensions.width();
    let source_height = dimensions.height();
    if !source_width.is_finite()
        || !source_height.is_finite()
        || source_width <= 0.
        || source_height <= 0.
    {
        return Err(Error::Svg(usvg::Error::InvalidSize));
    }
    let scale = (EDGE as f32 / source_width.max(source_height)).min(1.);
    let scaled_width = source_width * scale;
    let scaled_height = source_height * scale;
    if !scaled_width.is_finite()
        || !scaled_height.is_finite()
        || scaled_width <= 0.
        || scaled_height <= 0.
    {
        return Err(Error::Svg(usvg::Error::InvalidSize));
    }
    let width = scaled_width.ceil() as u32;
    let height = scaled_height.ceil() as u32;
    if width == 0 || height == 0 {
        return Err(Error::Svg(usvg::Error::InvalidSize));
    }
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(Error::TooLarge)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    Ok(RgbaImage::from_raw(width, height, pixels)
        .expect("pixmap dimensions match its pixel buffer"))
}

fn is_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(SVG_PREFIX_BYTES)]);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.starts_with("<svg")
        || ((head.starts_with("<?xml")
            || head.starts_with("<!DOCTYPE")
            || head.starts_with("<!--"))
            && head.contains("<svg"))
}
