use gds_uim::el;
use gpui_kit::{component::switch::Switch, *};

use crate::widgets::settings::ui::{View, controls::Field};
use gds::i18n::text;

impl View {
    pub(super) fn render_files(&mut self, cx: &mut Context<Self>) -> Div {
        let on_hidden_changed = cx.listener(Self::on_hidden_changed);
        let toggle = el! {
            "settings-show-hidden".Switch::new {
                accessibility_label: $(text("show-hidden", cx));
                checked: $self.preferences.files.show_hidden;
                on_change: on_hidden_changed;
            }
        };
        let row = self.setting_row(Field::ShowHidden, toggle, cx);
        let on_volumes_changed = cx.listener(Self::on_sidebar_volumes_changed);
        let volumes_toggle = el! {
            "settings-sidebar-volumes".Switch::new {
                accessibility_label: $(text("sidebar-show-volumes", cx));
                checked: $self.preferences.files.sidebar.show_volumes;
                on_change: on_volumes_changed;
            }
        };
        let volumes_row = self.setting_row(Field::SidebarVolumes, volumes_toggle, cx);
        let on_auto_focus_search_changed = cx.listener(Self::on_auto_focus_search_changed);
        let search_toggle = el! {
            "settings-auto-focus-search".Switch::new {
                accessibility_label: $(text("auto-focus-search", cx));
                tooltip: $(text("auto-focus-search", cx));
                checked: $self.preferences.files.auto_focus_search;
                on_change: on_auto_focus_search_changed;
            }
        };
        let search_row = self.setting_row(Field::AutoFocusSearch, search_toggle, cx);
        el! { .div [row, volumes_row, search_row] }
    }
}
