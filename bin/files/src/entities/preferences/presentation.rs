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
}

impl Prepared {
    pub(super) fn new(preferences: &Preferences, previous: &Preferences) -> Result<Self, Error> {
        let theme = (preferences.appearance != previous.appearance)
            .then(|| {
                preferences
                    .appearance
                    .prepare(Gds::current().appearance().theme().mode())
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
        Ok(Self {
            theme,
            font,
            language,
        })
    }

    pub(super) fn install(self, preferences: &Preferences) {
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
        gds.ui().time().apply(preferences.time_format);
        gds.appearance()
            .motion()
            .set_enabled(preferences.animations_enabled.unwrap_or(true));
    }
}
