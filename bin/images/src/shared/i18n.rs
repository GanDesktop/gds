//! Application localization catalogs backed by GDS Fluent runtime.

#[cfg(test)]
mod tests;

pub use gds::i18n::Language;
#[cfg(test)]
pub use gds::i18n::{Catalog, FluentArgs};

pub(crate) fn resource_for(language: Language) -> &'static str {
    match language {
        Language::English => include_str!("i18n/assets/en-US/app.ftl"),
        Language::Chinese => include_str!("i18n/assets/zh-CN/app.ftl"),
    }
}
