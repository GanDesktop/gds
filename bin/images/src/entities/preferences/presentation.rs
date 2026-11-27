//! Validate presentation changes before committing the application snapshot.

use super::Preferences;
use gds::{
    app::Gds,
    appearance::{theme, typography},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Theme(#[from] theme::Error),
    #[error(transparent)]
    Typography(#[from] typography::Error),
}

pub(super) struct Prepared {
    theme: Option<theme::Prepared>,
    font: Option<typography::Preferences>,
    animations_enabled: Option<bool>,
}

impl Prepared {
    pub(super) fn new(preferences: &Preferences, previous: &Preferences) -> Result<Self, Error> {
        let theme = (preferences.appearance != previous.appearance)
            .then(|| {
                preferences
                    .appearance
                    .prepare(Gds::current().appearance().theme().mode_untracked())
            })
            .transpose()?;
        let font = (preferences.font != previous.font)
            .then(|| {
                preferences.font.validate()?;
                Ok::<_, typography::Error>(preferences.font.clone())
            })
            .transpose()?;
        let animations_enabled = (preferences.animations_enabled != previous.animations_enabled)
            .then(|| preferences.animations_enabled.unwrap_or(true));
        Ok(Self {
            theme,
            font,
            animations_enabled,
        })
    }

    pub(super) fn appearance_changed(&self) -> bool {
        self.theme.is_some()
    }

    pub(super) fn install(self, _preferences: &Preferences) {
        let gds = Gds::current();
        if let Some(theme) = self.theme {
            gds.appearance()
                .theme()
                .install(theme)
                .expect("prepared theme has valid opacity");
        }
        if let Some(font) = self.font {
            gds.appearance()
                .typography()
                .apply(&font)
                .expect("prepared font has valid size");
        }
        if let Some(animations_enabled) = self.animations_enabled {
            gds.appearance().motion().set_enabled(animations_enabled);
        }
    }
}

pub(crate) fn normalize(preferences: &mut Preferences) {
    preferences.default_jpeg_quality = preferences.default_jpeg_quality.clamp(10, 100);
    let default_appearance = theme::Appearance::default();
    if !preferences.appearance.content_opacity.is_finite()
        || !(0.0..=1.0).contains(&preferences.appearance.content_opacity)
    {
        preferences.appearance.content_opacity = default_appearance.content_opacity;
    }
    if !preferences.appearance.sidebar_opacity.is_finite()
        || !(0.0..=1.0).contains(&preferences.appearance.sidebar_opacity)
    {
        preferences.appearance.sidebar_opacity = default_appearance.sidebar_opacity;
    }
    if !preferences.font.size.is_finite()
        || !(typography::MIN_SIZE..=typography::MAX_SIZE).contains(&preferences.font.size)
    {
        preferences.font.size = typography::DEFAULT_SIZE;
    }
}
