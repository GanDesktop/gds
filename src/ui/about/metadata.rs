//! Application identity embedded from Cargo metadata at build time.

use crate::ui::icon::SvgAsset;
use anti_webui::prelude::Svg;

#[derive(Debug)]
pub struct Application {
    pub binary: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    artwork: SvgAsset,
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

impl Application {
    /// The entry for an application registered in Cargo metadata.
    ///
    /// # Panics
    /// Panics if `binary` is not a registered target of this package.
    pub fn cargo(binary: &str) -> &'static Self {
        APPLICATIONS
            .iter()
            .find(|application| application.binary == binary)
            .expect("Cargo binary has registered application metadata")
    }
}

impl Application {
    /// The embedded application mark keeps the surrounding native text color.
    pub fn artwork(&self) -> Svg {
        self.artwork.view()
    }
}
