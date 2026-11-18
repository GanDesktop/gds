//! Settings destinations and their pure editor models.
mod performance;
use gds::ui::Icon;
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
    pub fn label_id(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Appearance => "settings-appearance",
            Self::Files => "settings-files",
            Self::Terminal => "settings-terminal",
            Self::Extensions => "settings-extensions",
            Self::Performance => "settings-performance",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            Self::General => Icon::Settings,
            Self::Appearance => Icon::Palette,
            Self::Files => Icon::Folder,
            Self::Terminal => Icon::Terminal,
            Self::Extensions => Icon::Plug,
            Self::Performance => Icon::Gauge,
        }
    }
    pub(super) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|page| *page == self)
            .expect("every settings page is a destination")
    }
}
