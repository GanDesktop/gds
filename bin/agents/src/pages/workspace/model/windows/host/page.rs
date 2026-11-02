use crate::shared::i18n::tr;
use gpui_kit::{AnyElement, App, Entity, IntoElement};

pub(super) enum Content {
    Help,
    Settings(Entity<crate::widgets::settings::Settings>),
    Catalog(Entity<crate::widgets::settings::CatalogContent>),
    History(Entity<crate::widgets::history::Browser>),
    About(Entity<gds::ui::about::About>),
}

pub(super) type Page = gds::ui::page::stack::Entry<Content>;

impl Content {
    pub fn sidebar_width(&self) -> Option<f32> {
        match self {
            Self::History(_) => Some(crate::widgets::history::SIDEBAR_WIDTH),
            Self::Settings(_) => Some(gds::appearance::tokens::SIDEBAR_WIDTH),
            Self::Help | Self::Catalog(_) | Self::About(_) => None,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Help => tr!("app-help"),
            Self::Settings(_) => tr!("settings-title"),
            Self::Catalog(_) => tr!("agent-install-action"),
            Self::History(_) => tr!("agent-history-title"),
            Self::About(_) => tr!("app-about"),
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Settings(_) => "settings",
            Self::Catalog(_) => "agent-installation-page",
            Self::History(_) => "agent-history-window",
            Self::About(_) => "about-window",
        }
    }

    pub fn element(&self, cx: &App) -> AnyElement {
        match self {
            Self::Help => crate::widgets::help::render(cx),
            Self::Settings(view) => view.clone().into_any_element(),
            Self::Catalog(view) => view.clone().into_any_element(),
            Self::History(view) => view.clone().into_any_element(),
            Self::About(view) => view.clone().into_any_element(),
        }
    }
}
