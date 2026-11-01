//! Application messages use Fluent; component translations share the same locale.

mod translate;

pub use crate::t;
pub use fluent_bundle::FluentArgs;

use fluent_bundle::{FluentResource, concurrent::FluentBundle};
use gpui_kit::{App, Global, SharedString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use unic_langid::LanguageIdentifier;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[serde(alias = "en-US")]
    English,
    #[default]
    #[serde(alias = "zh-CN")]
    Chinese,
}

impl Language {
    pub fn detect() -> Self {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty());
        if locale.is_some_and(|value| value.starts_with("zh")) {
            Self::Chinese
        } else {
            Self::English
        }
    }

    pub fn locale(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::Chinese => "zh-CN",
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid bundled translation: {0}")]
    Resource(String),
    #[error("could not register translations: {0}")]
    Bundle(String),
}

/// A Fluent catalog composed of suite messages and the owning application's resources.
/// Static labels are cached for consumers that render outside a GPUI context.
pub struct Catalog {
    language: Language,
    bundle: FluentBundle<FluentResource>,
    labels: HashMap<String, String>,
}

impl Catalog {
    /// # Errors
    /// Returns an error for invalid Fluent or duplicate message IDs.
    pub fn new(language: Language, application: &[&str]) -> Result<Self, Error> {
        let core = match language {
            Language::English => include_str!("i18n/assets/en-US/core.ftl"),
            Language::Chinese => include_str!("i18n/assets/zh-CN/core.ftl"),
        };
        let time = match language {
            Language::English => include_str!("i18n/assets/en-US/time.ftl"),
            Language::Chinese => include_str!("i18n/assets/zh-CN/time.ftl"),
        };
        let terminal = match language {
            Language::English => include_str!("i18n/assets/en-US/terminal.ftl"),
            Language::Chinese => include_str!("i18n/assets/zh-CN/terminal.ftl"),
        };
        let logs = match language {
            Language::English => include_str!("i18n/assets/en-US/logs.ftl"),
            Language::Chinese => include_str!("i18n/assets/zh-CN/logs.ftl"),
        };
        let about = match language {
            Language::English => include_str!("i18n/assets/en-US/about.ftl"),
            Language::Chinese => include_str!("i18n/assets/zh-CN/about.ftl"),
        };
        let locale: LanguageIdentifier = language
            .locale()
            .parse()
            .expect("bundled locale identifiers are valid BCP 47");
        let mut bundle = FluentBundle::new_concurrent(vec![locale]);
        let sources = [core, time, terminal, logs, about]
            .into_iter()
            .chain(application.iter().copied());
        let mut ids = Vec::new();
        for source in sources {
            let resource = FluentResource::try_new(source.to_owned())
                .map_err(|(_, errors)| Error::Resource(format!("{errors:?}")))?;
            ids.extend(source.lines().filter_map(|line| {
                line.starts_with(char::is_alphabetic)
                    .then(|| line.split_once(" =").map(|(id, _)| id.to_owned()))
                    .flatten()
            }));
            bundle
                .add_resource(resource)
                .map_err(|errors| Error::Bundle(format!("{errors:?}")))?;
        }
        let mut catalog = Self {
            language,
            bundle,
            labels: HashMap::new(),
        };
        for id in ids {
            if let Some(value) = catalog.format(&id, &FluentArgs::new()) {
                catalog.labels.insert(id, value);
            }
        }
        Ok(catalog)
    }

    pub fn text(&self, id: &str) -> Option<&str> {
        self.labels.get(id).map(String::as_str)
    }

    pub fn format(&self, id: &str, arguments: &FluentArgs<'_>) -> Option<String> {
        let pattern = self.bundle.get_message(id)?.value()?;
        let mut errors = Vec::new();
        let text = self
            .bundle
            .format_pattern(pattern, Some(arguments), &mut errors);
        errors.is_empty().then(|| text.into_owned())
    }
}

struct Messages(Catalog);
impl Global for Messages {}

/// Install the suite's base messages and component translations.
///
/// # Errors
/// Returns an error when bundled Fluent cannot be parsed or registered.
pub fn install(language: Language, cx: &mut App) -> Result<(), Error> {
    install_resources(language, &[], cx)
}

/// Install messages owned by the current application alongside the base catalog.
///
/// # Errors
/// Returns an error for invalid Fluent or duplicate message IDs.
pub fn install_resources(
    language: Language,
    application: &[&str],
    cx: &mut App,
) -> Result<(), Error> {
    let mut catalog = Catalog::new(language, application)?;
    catalog.bundle.set_use_isolating(false);
    gpui_kit::component::set_locale(language.locale());
    cx.set_global(Messages(catalog));
    cx.refresh_windows();
    Ok(())
}

pub fn text(id: &str, cx: &App) -> SharedString {
    format(id, None, cx)
}

/// Fix the deferred callback's input type before UIM checks its arguments.
#[doc(hidden)]
pub fn deferred(callback: impl FnOnce(&App) -> SharedString) -> impl FnOnce(&App) -> SharedString {
    callback
}

pub fn language(cx: &App) -> Language {
    cx.global::<Messages>().0.language
}

pub fn format(id: &str, arguments: Option<&FluentArgs<'_>>, cx: &App) -> SharedString {
    cx.global::<Messages>()
        .0
        .format(id, arguments.unwrap_or(&FluentArgs::new()))
        .unwrap_or_else(|| {
            eprintln!("Missing or invalid Fluent message: {id}");
            id.to_owned()
        })
        .into()
}

pub fn count(id: &str, count: usize, cx: &App) -> SharedString {
    let mut arguments = FluentArgs::new();
    arguments.set("count", count as i64);
    format(id, Some(&arguments), cx)
}
