//! Settings destinations and their pure editor models.

pub mod performance;

use gds::ui::IconName;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Appearance,
    Files,
    Terminal,
    Extensions,
    Performance,
}

impl Page {
    pub const ALL: [Self; 6] = [
        Self::General,
        Self::Appearance,
        Self::Files,
        Self::Terminal,
        Self::Extensions,
        Self::Performance,
    ];

    #[must_use]
    pub const fn label_id(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Appearance => "settings-appearance",
            Self::Files => "settings-files",
            Self::Terminal => "settings-terminal",
            Self::Extensions => "settings-extensions",
            Self::Performance => "settings-performance",
        }
    }

    #[must_use]
    pub const fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Palette,
            Self::Files => IconName::Folder,
            Self::Terminal => IconName::Terminal,
            Self::Extensions => IconName::Plug,
            Self::Performance => IconName::Gauge,
        }
    }

    #[must_use]
    pub(super) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|page| *page == self)
            .expect("every settings page is a destination")
    }
}
