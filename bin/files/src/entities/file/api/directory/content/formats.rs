//! Embedded MIME categories and short display names.
use super::ContentCategory;
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

const CONFIG: &str = include_str!("assets/mime-types.toml");

static FORMATS: LazyLock<HashMap<String, Format>> = LazyLock::new(|| {
    let config: Config = toml::from_str(CONFIG).expect("invalid embedded MIME presentation config");
    config
        .formats
        .into_iter()
        .flat_map(|rule| {
            let name = rule.name.map(Arc::<str>::from);
            let category = rule.category;
            rule.mimes.into_iter().map(move |mime| {
                let interned_mime = Arc::<str>::from(mime.as_str());
                (
                    mime,
                    Format {
                        category,
                        name: name.clone(),
                        mime: interned_mime,
                    },
                )
            })
        })
        .collect()
});

#[derive(Deserialize)]
struct Config {
    formats: Vec<Rule>,
}

#[derive(Deserialize)]
struct Rule {
    mimes: Vec<String>,
    category: ContentCategory,
    name: Option<String>,
}

#[derive(Clone)]
pub(super) struct Format {
    pub category: ContentCategory,
    pub name: Option<Arc<str>>,
    pub mime: Arc<str>,
}

impl Format {
    pub fn for_mime(mime: &str) -> Option<&'static Self> {
        FORMATS.get(mime)
    }
}

#[cfg(test)]
mod tests;
