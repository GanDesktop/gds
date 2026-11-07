//! Agents preferences mapped onto the suite's appearance and typography services.
use anyhow::{Result, ensure};
use gds::appearance::{theme, typography};
use gpui_kit::WindowBackgroundAppearance;

pub(crate) use crate::shared::config::appearance::{
    Appearance as Preferences, Background, ComposerEnterBehavior, Language, ThemeMode, ThemeSource,
    ValidationError,
};

#[cfg(test)]
mod tests;

pub(crate) const DEFAULT_LINE_HEIGHT: f32 = 1.6;
pub(crate) const CONTENT_FONT_SCALE: f32 = gds::appearance::tokens::UI_TEXT_SCALE;

pub(super) struct Effective {
    pub preferences: Preferences,
    pub font: typography::Preferences,
    pub content_font_size: f32,
    pub content_line_height: f32,
    pub background: WindowBackgroundAppearance,
}

pub(super) fn appearance(preferences: &Preferences) -> theme::Appearance {
    let opacity = gds::appearance::tokens::SurfaceOpacity::default();
    theme::Appearance {
        source: match &preferences.theme_source {
            ThemeSource::Custom(path) => theme::Source::Custom(path.clone()),
            ThemeSource::Desktop => theme::Source::System,
            ThemeSource::Builtin => theme::Source::Builtin,
        },
        builtin: preferences.builtin_theme,
        mode: match preferences.theme_mode {
            ThemeMode::Desktop | ThemeMode::System => theme::ModePreference::System,
            ThemeMode::Light => theme::ModePreference::Light,
            ThemeMode::Dark => theme::ModePreference::Dark,
        },
        content_opacity: preferences.content_opacity.unwrap_or(opacity.content),
        sidebar_opacity: preferences.sidebar_opacity.unwrap_or(opacity.sidebar),
        rounded_corners: preferences.rounded_corners,
    }
}

/// # Errors
/// Reports invalid dimensions, font sizes outside the suite range or invalid preferences.
pub(super) fn resolve(preferences: &Preferences) -> Result<Effective> {
    preferences.validate()?;
    let size = preferences.ui_font_size.unwrap_or(typography::DEFAULT_SIZE);
    ensure!(
        (typography::MIN_SIZE..=typography::MAX_SIZE).contains(&size),
        "UI font size must be between {} and {} px",
        typography::MIN_SIZE,
        typography::MAX_SIZE,
    );
    Ok(Effective {
        preferences: preferences.clone(),
        font: typography::Preferences {
            family: preferences.ui_font_family.clone(),
            size,
        },
        content_font_size: preferences
            .content_font_size
            .unwrap_or(size * CONTENT_FONT_SCALE),
        content_line_height: preferences
            .content_line_height
            .unwrap_or(DEFAULT_LINE_HEIGHT),
        background: match preferences.background {
            Background::Auto => gds::appearance::tokens::WINDOW_BACKGROUND,
            Background::Transparent => WindowBackgroundAppearance::Transparent,
            Background::Opaque => WindowBackgroundAppearance::Opaque,
            Background::Blurred => WindowBackgroundAppearance::Blurred,
        },
    })
}
