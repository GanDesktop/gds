mod appearance;
mod files;
mod general;
mod terminal;

use gpui_kit::{Context, Div, assets::IconName};

use super::View;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Appearance,
    Terminal,
    Files,
}

impl Page {
    pub const ALL: [Self; 4] = [Self::General, Self::Appearance, Self::Files, Self::Terminal];

    pub fn label_id(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Appearance => "settings-appearance",
            Self::Terminal => "settings-terminal",
            Self::Files => "settings-files",
        }
    }

    pub(super) fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Palette,
            Self::Terminal => IconName::Terminal,
            Self::Files => IconName::Folder,
        }
    }

    /// Position in [`Page::ALL`], used for focus and wrap-around page cycling.
    pub(super) fn index(self) -> usize {
        match self {
            Self::General => 0,
            Self::Appearance => 1,
            Self::Terminal => 3,
            Self::Files => 2,
        }
    }

    pub(super) fn render(self, view: &mut View, cx: &mut Context<View>) -> Div {
        match self {
            Self::General => view.render_general(cx),
            Self::Appearance => view.render_appearance(cx),
            Self::Terminal => view.render_terminal(cx),
            Self::Files => view.render_files(cx),
        }
    }
}
