//! Theme, typography and the shared visual language.
pub mod motion;
pub mod theme;
pub mod tokens;
pub mod typography;

/// Initialize Kit with the system fonts before it probes default typography.
pub fn init(cx: &mut gpui_kit::App) {
    #[cfg(target_os = "linux")]
    let seeded = {
        use gpui_kit::component::Theme;
        let seeded = !cx.has_global::<Theme>();
        if seeded {
            let mut theme = Theme::default();
            theme.font_family = typography::system_family();
            theme.mono_font_family = typography::system_mono_family();
            cx.set_global(theme);
        }
        seeded
    };
    gpui_kit::init(cx);
    motion::init(cx);
    #[cfg(target_os = "linux")]
    if seeded {
        use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
        // Kit assigns these defaults only when it creates the Theme itself.
        let registry = ThemeRegistry::global(cx);
        let light = registry.default_light_theme().clone();
        let dark = registry.default_dark_theme().clone();
        let theme = Theme::global_mut(cx);
        theme.light_theme = light;
        theme.dark_theme = dark;
        Theme::change(ThemeMode::Light, None, cx);
    }
}
