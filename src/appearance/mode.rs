//! Core appearance mode and contrast enums shared across theme and portal integration.

use serde::{Deserialize, Serialize};
use zgui_ui_tokens::ColorScheme;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Light,
    #[default]
    Dark,
}

impl Mode {
    pub const fn color_scheme(self) -> ColorScheme {
        match self {
            Self::Light => ColorScheme::Light,
            Self::Dark => ColorScheme::Dark,
        }
    }

    pub(crate) const fn css_keyword(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Contrast {
    #[default]
    Normal,
    More,
}

impl Contrast {
    pub const fn is_more(self) -> bool {
        matches!(self, Self::More)
    }

    pub(crate) const fn css_keyword(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::More => "more",
        }
    }
}
