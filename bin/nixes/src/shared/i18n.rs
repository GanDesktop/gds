use gds::app::AppExt as _;
pub use gds::i18n::text;

pub fn install(cx: &mut gpui_kit::App) -> Result<(), gds::i18n::Error> {
    use gds::i18n::Language;
    let language = Language::detect();
    let source = match language {
        Language::English => include_str!("i18n/assets/en-US/app.ftl"),
        Language::Chinese => include_str!("i18n/assets/zh-CN/app.ftl"),
    };
    cx.gds().i18n().install_resources(language, &[source])
}
