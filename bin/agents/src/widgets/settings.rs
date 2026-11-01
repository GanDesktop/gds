//! Settings and Agent-management surfaces for embedded and standalone hosts.
mod ui;
pub(crate) use ui::pages::agents::Agents as AgentManager;
pub(crate) use ui::pages::agents::window::{Content as CatalogContent, content as catalog_content};
pub(crate) use ui::{Event, Settings, window};
