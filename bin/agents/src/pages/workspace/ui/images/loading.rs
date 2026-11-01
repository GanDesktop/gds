//! Bounded decoding for untrusted conversation images. Only a static raster frame is rendered.
use super::*;
use futures::AsyncReadExt as _;

pub(super) const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;
const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) async fn load(
    address: &str,
    client: Arc<dyn gpui_kit::http_client::HttpClient>,
    renderer: gpui_kit::SvgRenderer,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    let image = if address.starts_with("data:") {
        decode_data_uri(address)?
    } else {
        resolve(&Resource::Uri(address.to_owned().into()), None)?;
        let mut response = client
            .get(address, Default::default(), true)
            .await
            .map_err(ImageCacheError::from)?;
        if !response.status().is_success() {
            return Err(image_policy_error());
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        normalized(&bytes)?
    };
    // normalized() emits a single PNG frame; GPUI never sees animated/unbounded inputs.
    image.to_image_data(renderer).map_err(Into::into)
}

pub(super) fn normalized(bytes: &[u8]) -> Result<Image, ImageCacheError> {
    if bytes.len() > MAX_BYTES {
        return Err(image_policy_error());
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let raster = reader.decode()?;
    let mut encoded = std::io::Cursor::new(Vec::new());
    raster.write_to(&mut encoded, image::ImageFormat::Png)?;
    Ok(Image::from_bytes(ImageFormat::Png, encoded.into_inner()))
}
