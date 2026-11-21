//! Validate presentation changes before committing the application snapshot.

use super::Preferences;
use crate::shared::i18n;
use gds::{
    app::Gds,
    appearance::{theme, typography},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Theme(#[from] theme::Error),
    #[error(transparent)]
    Language(#[from] i18n::Error),
    #[error(transparent)]
    Typography(#[from] typography::Error),
}

pub(super) struct Prepared {
    theme: Option<theme::Prepared>,
    font: Option<typography::Preferences>,
    language: Option<gds::i18n::Catalog>,
    time_format: Option<gds::ui::time::Format>,
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
        let language = (preferences.language != previous.language)
            .then(|| i18n::prepare(preferences.language.unwrap_or_else(i18n::Language::detect)))
            .transpose()?;
        let time_format =
            (preferences.time_format != previous.time_format).then_some(preferences.time_format);
        let animations_enabled = (preferences.animations_enabled != previous.animations_enabled)
            .then(|| preferences.animations_enabled.unwrap_or(true));
        Ok(Self {
            theme,
            font,
            language,
            time_format,
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
        if let Some(language) = self.language {
            gds.i18n().install_catalog(language);
        }
        if let Some(time_format) = self.time_format {
            gds::ui::time::apply(time_format);
        }
        if let Some(animations_enabled) = self.animations_enabled {
            gds.appearance().motion().set_enabled(animations_enabled);
        }
    }
}
