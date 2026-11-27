//! Files chrome keeps address navigation, folder filtering, and sidebar regions distinct.

use zgui::prelude::*;
use zgui::view::current_host;

use crate::pages::workspace::state::context::Context;

mod address;
mod filter;
mod sidebar;
mod toolbar;

#[cfg(test)]
pub(in crate::pages::workspace) mod tests;

#[cfg(test)]
use crate::pages::workspace::state::ui_flags::CollapsibleSection;
#[cfg(test)]
use address::Breadcrumb;
#[cfg(test)]
use sidebar::{
    CollapsibleMeta, TabCycleTargets, allocate_collapsible_heights, compute_tab_cycle_targets,
    default_accordion_section, sidebar_accordion_threshold,
};

pub(in crate::pages::workspace) use toolbar::subpage_toolbar_title;

/// Layer 6 accessor for window toolbar, address bar, filter inputs, and sidebar.
pub(in crate::pages::workspace) struct Chrome<'a>(pub &'a Context);

pub(in crate::pages::workspace) fn install_chrome_stylesheets() {
    gds::ui::styles::install_ui_stylesheets();
    if current_host().is_some() {
        toolbar::install_stylesheet();
        address::install_stylesheet();
        filter::install_stylesheet();
        sidebar::install_stylesheet();
    }
}

fn track_language() {
    if let Some(gds) = gds::app::Gds::try_current() {
        let _ = gds.i18n().language();
    }
}

fn node_contains_focus(node: NodeRef) -> bool {
    current_host().is_some_and(|host| {
        host.focused()
            .get_untracked()
            .is_some_and(|active| node.get_untracked() == Some(active) || node.contains(active))
    })
}
