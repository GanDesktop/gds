//! Embedded MIME categories and short display names.
use super::ContentCategory;
use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

const CONFIG: &str = include_str!("assets/mime-types.toml");

static FORMATS: LazyLock<HashMap<String, Format>> = LazyLock::new(|| {
    let config: Config = toml::from_str(CONFIG).expect("invalid embedded MIME presentation config");
    config
        .formats
        .into_iter()
        .flat_map(|rule| {
            rule.mimes
                .into_iter()
                .map(move |mime| (mime, rule.format.clone()))
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
    #[serde(flatten)]
    format: Format,
}

#[derive(Clone, Deserialize)]
pub(super) struct Format {
    pub category: ContentCategory,
    pub name: Option<String>,
}

impl Format {
    pub fn for_mime(mime: &str) -> Option<&'static Self> {
        FORMATS.get(mime)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn embedded_rules_have_unique_mime_types() {
        let config: Config = toml::from_str(CONFIG).unwrap();
        let mut seen = HashSet::new();
        for rule in config.formats {
            for mime in rule.mimes {
                assert!(seen.insert(mime.clone()), "duplicate MIME rule: {mime}");
            }
        }
    }
}
