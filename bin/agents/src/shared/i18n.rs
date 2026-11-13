//! Embedded Fluent catalogs. Static labels borrow cached text; messages with
//! arguments are formatted by Fluent, including its plural and bidi handling.

use fluent_bundle::FluentArgs;
use gds::app::AppExt as _;
use gds::i18n::Catalog;
pub(crate) use gds::i18n::Language;
use std::sync::{
    LazyLock,
    atomic::{AtomicU8, Ordering},
};

const CHINESE: &str = include_str!("i18n/assets/zh-CN/app.ftl");
const ENGLISH: &str = include_str!("i18n/assets/en-US/app.ftl");
static LANGUAGE: AtomicU8 = AtomicU8::new(0);
static ZH: LazyLock<Catalog> = LazyLock::new(|| {
    Catalog::new(gds::i18n::Language::Chinese, &[CHINESE]).expect("embedded Chinese Fluent catalog")
});
static EN: LazyLock<Catalog> = LazyLock::new(|| {
    Catalog::new(gds::i18n::Language::English, &[ENGLISH]).expect("embedded English Fluent catalog")
});

pub(crate) fn install(language: Language, cx: &mut gpui_kit::App) {
    set_language(language);
    let source = match language {
        Language::Chinese => CHINESE,
        Language::English => ENGLISH,
    };
    cx.gds_mut()
        .i18n()
        .install_resources(language, &[source])
        .expect("embedded Agents Fluent catalog");
}

pub(crate) fn language() -> Language {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::English,
        _ => Language::Chinese,
    }
}

pub(crate) fn set_language(language: Language) {
    LANGUAGE.store(
        match language {
            Language::Chinese => 0,
            Language::English => 1,
        },
        Ordering::Relaxed,
    );
    // The published component library owns translations of its built-in editors
    // and selects. Agents's messages are supplied by the Fluent catalogs above.
    gpui_kit::component::set_locale(match language {
        Language::Chinese => "zh-CN",
        Language::English => "en",
    });
}

fn catalog(language: Language) -> &'static Catalog {
    match language {
        Language::Chinese => &ZH,
        Language::English => &EN,
    }
}

pub(crate) fn text(id: &'static str) -> &'static str {
    catalog(language())
        .text(id)
        .or_else(|| ZH.text(id))
        .unwrap_or_else(|| {
            tracing::error!(id, "missing interface label");
            id
        })
}

pub(crate) fn format(id: &'static str, args: FluentArgs<'_>) -> String {
    catalog(language())
        .format(id, &args)
        .or_else(|| ZH.format(id, &args))
        .unwrap_or_else(|| id.to_owned())
}

macro_rules! tr {
    ($id:literal) => { $crate::shared::i18n::text($id) };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = fluent_bundle::FluentArgs::new();
        $(args.set(stringify!($name), $value);)+
        $crate::shared::i18n::format($id, args)
    }};
}
pub(crate) use tr;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::visible_text;
