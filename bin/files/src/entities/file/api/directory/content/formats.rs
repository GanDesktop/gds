//! Minimal last-resort MIME category patches embedded at build time.
//!
//! Primary classification comes from `shared-mime-info` (`generic-icons`,
//! top-level MIME types, and `subclasses`) plus generated GitHub Linguist
//! source-code rules. This table only handles the few binary formats that
//! `shared-mime-info` leaves without a generic icon or inherited category.

use std::{collections::HashMap, sync::LazyLock};

use serde::Deserialize;

use super::ContentCategory;

const CONFIG: &str = include_str!("assets/mime-types.toml");

static PATCHES: LazyLock<HashMap<String, ContentCategory>> = LazyLock::new(|| {
    let config: Config = toml::from_str(CONFIG).expect("invalid embedded MIME patch config");
    config
        .patches
        .into_iter()
        .flat_map(|rule| {
            let category = rule.category;
            rule.mimes.into_iter().map(move |mime| (mime, category))
        })
        .collect()
});

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    patches: Vec<PatchRule>,
}

#[derive(Deserialize)]
struct PatchRule {
    mimes: Vec<String>,
    category: ContentCategory,
}

/// Look up a last-resort category patch for canonical `mime`.
pub(super) fn patch_category(mime: &str) -> Option<ContentCategory> {
    PATCHES.get(mime).copied()
}

#[cfg(test)]
mod tests;
