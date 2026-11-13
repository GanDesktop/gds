//! Validate every fallible presentation change before touching application state.

use super::Preferences;
use crate::shared::i18n;
use gds::app::AppExt as _;
use gds::appearance::{theme, typography};
use gpui_kit::App;

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
    pub(super) fn new(
        preferences: &Preferences,
        previous: &Preferences,
        cx: &App,
    ) -> Result<Self, Error> {
        let theme = (preferences.appearance != previous.appearance)
            .then(|| preferences.appearance.prepare(cx.window_appearance()))
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

    pub(super) fn install(self, preferences: &Preferences, cx: &mut App) {
        if let Some(theme) = self.theme {
            cx.gds_mut()
                .appearance()
                .theme()
                .install(theme)
                .expect("prepared theme has valid opacity");
        }
        if let Some(font) = self.font {
            cx.gds_mut()
                .appearance()
                .typography()
                .apply(&font)
                .expect("prepared font has a valid size");
        }
        if let Some(language) = self.language {
            cx.gds_mut().i18n().install_catalog(language);
        }
        cx.gds_mut().ui().time().apply(preferences.time_format);
        cx.gds_mut()
            .appearance()
            .motion()
            .set_enabled(preferences.animations_enabled.unwrap_or(true));
    }
}
