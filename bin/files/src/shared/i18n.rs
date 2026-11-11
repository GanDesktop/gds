//! Files owns its application messages; GDS supplies Fluent and base messages.

pub use gds::i18n::{Error, Language, text};
use gpui_kit::App;

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

pub fn install(language: Language, cx: &mut App) -> Result<(), Error> {
    prepare(language)?.install(cx);
    Ok(())
}

#[cfg(all(test, feature = "gui-tests"))]
#[gpui_kit::test]
fn application_and_shared_catalogs_have_no_conflicting_keys(cx: &mut gpui_kit::TestAppContext) {
    for language in [Language::English, Language::Chinese] {
        cx.update(|cx| {
            install(language, cx).expect("Files and shared messages must compose");
            assert_ne!(text("logs-tab-tasks", cx).as_ref(), "logs-tab-tasks");
        });
    }
}
