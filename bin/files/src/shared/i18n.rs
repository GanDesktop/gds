//! Files owns its application messages; GDS supplies Fluent and base messages.

pub use gds::i18n::{Error, Language, text};
pub(crate) use gds::t;

/// Build a Fluent catalog combining suite messages with Files translations.
///
/// # Errors
/// Returns an error if any bundled Fluent resource fails to parse or contains duplicate message IDs.
pub fn prepare(language: Language) -> Result<gds::i18n::Catalog, Error> {
    let sources = match language {
        Language::English => [
            include_str!("i18n/assets/en-US/app.ftl"),
            include_str!("i18n/assets/en-US/devices.ftl"),
            include_str!("i18n/assets/en-US/operations.ftl"),
            include_str!("i18n/assets/en-US/details.ftl"),
            include_str!("i18n/assets/en-US/font.ftl"),
            include_str!("i18n/assets/en-US/recycle.ftl"),
            include_str!("i18n/assets/en-US/terminal.ftl"),
            include_str!("i18n/assets/en-US/network.ftl"),
        ],
        Language::Chinese => [
            include_str!("i18n/assets/zh-CN/app.ftl"),
            include_str!("i18n/assets/zh-CN/devices.ftl"),
            include_str!("i18n/assets/zh-CN/operations.ftl"),
            include_str!("i18n/assets/zh-CN/details.ftl"),
            include_str!("i18n/assets/zh-CN/font.ftl"),
            include_str!("i18n/assets/zh-CN/recycle.ftl"),
            include_str!("i18n/assets/zh-CN/terminal.ftl"),
            include_str!("i18n/assets/zh-CN/network.ftl"),
        ],
    };
    gds::i18n::Catalog::new(language, &sources)
}

#[cfg(test)]
mod tests;
