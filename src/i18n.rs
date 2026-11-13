//! Application messages use Fluent; component translations share the same locale.

mod translate;

pub use crate::t;
pub use fluent_bundle::FluentArgs;

use fluent_bundle::{FluentResource, concurrent::FluentBundle};
use fluent_syntax::ast::Entry;
use gpui_kit::{App, Global, SharedString};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};
use thiserror::Error;
use unic_langid::LanguageIdentifier;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
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
    labels: HashMap<String, SharedString>,
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
        // Disable BiDi isolating marks before pre-formatting cached labels so
        // zero-argument lookups match runtime formatting.
        bundle.set_use_isolating(false);
        let sources = [core, time, terminal, logs, about]
            .into_iter()
            .chain(application.iter().copied());
        let mut ids = Vec::new();
        for source in sources {
            let resource = FluentResource::try_new(source.to_owned())
                .map_err(|(_, errors)| Error::Resource(format!("{errors:?}")))?;
            ids.extend(resource.entries().filter_map(|entry| match entry {
                Entry::Message(message) => Some(message.id.name.to_owned()),
                _ => None,
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
                catalog.labels.insert(id, value.into());
            }
        }
        Ok(catalog)
    }

    pub fn text(&self, id: &str) -> Option<&str> {
        self.labels.get(id).map(AsRef::<str>::as_ref)
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

#[cfg_attr(not(feature = "app"), allow(dead_code))]
struct Messages(Catalog);
impl Global for Messages {}

/// Install the suite's base messages and component translations.
///
/// # Errors
/// Returns an error when bundled Fluent cannot be parsed or registered.
#[cfg(feature = "app")]
pub(crate) fn install(language: Language, cx: &mut App) -> Result<(), Error> {
    install_resources(language, &[], cx)
}

/// Install messages owned by the current application alongside the base catalog.
///
/// # Errors
/// Returns an error for invalid Fluent or duplicate message IDs.
#[cfg(feature = "app")]
pub(crate) fn install_resources(
    language: Language,
    application: &[&str],
    cx: &mut App,
) -> Result<(), Error> {
    Catalog::new(language, application)?.install(cx);
    Ok(())
}

impl Catalog {
    /// Install a fully parsed catalog without introducing another fallible step.
    #[cfg(feature = "app")]
    pub(crate) fn install(self, cx: &mut App) {
        gpui_kit::component::set_locale(self.language.locale());
        cx.set_global(Messages(self));
        cx.refresh_windows();
    }
}

static REPORTED_MISSING_MESSAGES: OnceLock<Mutex<HashSet<(Language, String)>>> = OnceLock::new();

fn report_missing_with(
    reported: &Mutex<HashSet<(Language, String)>>,
    language: Language,
    id: &str,
    emit: impl FnOnce(Language, &str),
) {
    let is_new = reported
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert((language, id.to_owned()));
    if is_new {
        emit(language, id);
    }
}

fn report_missing_message(language: Language, id: &str) {
    let reported = REPORTED_MISSING_MESSAGES.get_or_init(|| Mutex::new(HashSet::new()));
    report_missing_with(reported, language, id, |language, id| {
        #[cfg(feature = "app")]
        tracing::warn!(
            id,
            locale = language.locale(),
            "missing or invalid Fluent message"
        );
        #[cfg(not(feature = "app"))]
        eprintln!(
            "Missing or invalid Fluent message ({}): {id}",
            language.locale()
        );
    });
}

pub fn text(id: &str, cx: &App) -> SharedString {
    cx.global::<Messages>()
        .0
        .labels
        .get(id)
        .cloned()
        .unwrap_or_else(|| format(id, None, cx))
}

/// Fix the deferred callback's input type before UIM checks its arguments.
#[doc(hidden)]
pub fn deferred(callback: impl FnOnce(&App) -> SharedString) -> impl FnOnce(&App) -> SharedString {
    callback
}

#[cfg(feature = "app")]
pub(crate) fn language(cx: &App) -> Language {
    cx.global::<Messages>().0.language
}

pub fn format(id: &str, arguments: Option<&FluentArgs<'_>>, cx: &App) -> SharedString {
    let catalog = &cx.global::<Messages>().0;
    catalog
        .format(id, arguments.unwrap_or(&FluentArgs::new()))
        .unwrap_or_else(|| {
            report_missing_message(catalog.language, id);
            id.to_owned()
        })
        .into()
}

pub fn count(id: &str, count: usize, cx: &App) -> SharedString {
    let mut arguments = FluentArgs::new();
    arguments.set("count", count as i64);
    format(id, Some(&arguments), cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn ast_extraction_handles_comments_multiline_and_varying_whitespace() {
        let catalog = Catalog::new(
            Language::English,
            &[r#"
# Line comment
## Group comment
### Resource comment
-test-brand = GanDesktop

test-tight-equals=Tight
test-wide-equals   =   Wide
test-multiline =
    First line
    Second line
test-interpolated-static = Hello { -test-brand }!
test-parameterized = Value: { $count }
test-attr-only =
    .title = Only attribute
"#],
        )
        .expect("catalog should parse");

        assert_eq!(catalog.text("test-tight-equals"), Some("Tight"));
        assert_eq!(catalog.text("test-wide-equals"), Some("Wide"));
        assert_eq!(
            catalog.text("test-multiline"),
            Some("First line\nSecond line")
        );
        let static_label = catalog
            .text("test-interpolated-static")
            .expect("interpolated static message should be cached");
        assert_eq!(static_label, "Hello GanDesktop!");
        assert!(!static_label.contains(['\u{2068}', '\u{2069}']));
        assert_eq!(catalog.text("-test-brand"), None);
        assert_eq!(catalog.text("test-parameterized"), None);
        assert_eq!(catalog.text("test-attr-only"), None);
    }

    #[test]
    fn missing_key_warning_is_reported_once_per_language_and_id() {
        let reported = Mutex::new(HashSet::new());
        let emitted = AtomicUsize::new(0);

        for _ in 0..4 {
            report_missing_with(&reported, Language::English, "missing-key", |_, _| {
                emitted.fetch_add(1, Ordering::Relaxed);
            });
        }
        assert_eq!(emitted.load(Ordering::Relaxed), 1);

        // A different key or language emits once more and then deduplicates.
        report_missing_with(&reported, Language::English, "other-missing-key", |_, _| {
            emitted.fetch_add(1, Ordering::Relaxed);
        });
        report_missing_with(&reported, Language::English, "other-missing-key", |_, _| {
            emitted.fetch_add(1, Ordering::Relaxed);
        });
        report_missing_with(&reported, Language::Chinese, "missing-key", |_, _| {
            emitted.fetch_add(1, Ordering::Relaxed);
        });
        report_missing_with(&reported, Language::Chinese, "missing-key", |_, _| {
            emitted.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(emitted.load(Ordering::Relaxed), 3);
    }

    #[cfg(all(feature = "app", feature = "gui-tests"))]
    #[gpui_kit::test]
    fn text_hits_cached_labels_without_isolating_marks(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            install_resources(
                Language::English,
                &[
                    "-test-term = GanDesktop\ntest-cached-greeting = Welcome to { -test-term }!\ntest-param-msg = Count: { $count }",
                ],
                cx,
            )
            .expect("install test resources");

            let greeting = text("test-cached-greeting", cx);
            assert_eq!(greeting.as_ref(), "Welcome to GanDesktop!");
            assert_eq!(
                cx.global::<Messages>().0.text("test-cached-greeting"),
                Some("Welcome to GanDesktop!")
            );
            assert!(!greeting.contains(['\u{2068}', '\u{2069}']));
            // Fast-path `text()` serves cached `labels` directly without invoking `FluentBundle`.
            cx.global_mut::<Messages>()
                .0
                .labels
                .insert("test-cache-only".to_owned(), SharedString::from("from-cache"));
            assert_eq!(text("test-cache-only", cx).as_ref(), "from-cache");

            // Runtime count formatting also strips isolating marks.
            let counted = count("test-param-msg", 3, cx);
            assert_eq!(counted.as_ref(), "Count: 3");
            assert!(!counted.contains(['\u{2068}', '\u{2069}']));

            // Missing keys and missing-arg lookups fall back to the key and record once.
            assert_eq!(text("test-missing-key", cx).as_ref(), "test-missing-key");
            assert_eq!(text("test-missing-key", cx).as_ref(), "test-missing-key");
            assert_eq!(text("test-param-msg", cx).as_ref(), "test-param-msg");
            let reported = REPORTED_MISSING_MESSAGES
                .get()
                .expect("missing lookups should record reported keys")
                .lock()
                .unwrap();
            assert!(reported.contains(&(Language::English, "test-missing-key".to_owned())));
            assert!(reported.contains(&(Language::English, "test-param-msg".to_owned())));
        });
    }
}
