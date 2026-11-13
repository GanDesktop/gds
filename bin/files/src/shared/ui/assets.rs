use std::borrow::Cow;

use gpui_kit::{AssetSource, SharedString};

const APP_ICON: &str = "assets/files/icon-solid.svg";
const APP_ICON_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/files/icon-solid.svg"
));

pub(crate) struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON {
            return Ok(Some(Cow::Borrowed(APP_ICON_BYTES)));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut assets = gpui_kit::assets::AllAssets.list(path)?;
        if APP_ICON.starts_with(path) {
            assets.push(APP_ICON.into());
        }
        assets.sort();
        assets.dedup();
        Ok(assets)
    }
}
