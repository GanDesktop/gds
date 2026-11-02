use super::*;
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme, button::ButtonVariants, spinner::Spinner};

impl Browser {
    pub(super) fn render_remote_unopened(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.tabs.active_id;
        let destination = match self.current_location() {
            Location::Remote(address) => address.to_string(),
            _ => String::new(),
        };
        el! { .div { # "remote-unopened"; flex; flex_col; flex_1; size_full; items_center; justify_center; gap_4; px_6 } [
            .div { font_medium } [$i18n::text("remote-not-open", cx)],
            .div { max_w_full; truncate; text_size: tokens::SMALL; text_color: $cx.theme().muted_foreground } [destination],
            .div { flex; gap_2 } [
                "retry-remote-location".ui::button::Button::new { ghost; icon: IconName::RefreshCw; label: $i18n::text("retry", cx);
                    on_click: $(cx.listener(move |this, _, _, cx| this.refresh_tab(tab, cx)));
                },
                "return-to-network".ui::button::Button::new { ghost; icon: IconName::ArrowLeft; label: $i18n::text("remote-back-network", cx);
                    on_click: $(cx.listener(move |this, _, _, cx| this.navigate_location(tab, Location::Network, cx)));
                },
            ],
        ] }.into_any_element()
    }

    pub(super) fn render_location_loading(
        &self,
        location: &Location,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let destination = match location {
            Location::Remote(address) => address.to_string(),
            Location::Directory(path) => Path::new(path).display().to_string(),
            _ => location.label(cx).to_string(),
        };
        let connecting = self
            .remote
            .connector
            .read(cx)
            .pending()
            .is_some_and(|p| p.tab == self.tabs.active_id);
        el! { .div { # "location-loading"; flex; flex_col; flex_1; size_full; items_center; justify_center; gap_4; px_6 } [
            .Spinner::new,
            .div { font_medium } [$i18n::text(if connecting { "remote-connecting" } else { "remote-loading-folder" }, cx)],
            .div { max_w_full; truncate; text_size: tokens::SMALL; text_color: $cx.theme().muted_foreground } [destination],
            "cancel-location-load".ui::button::Button::new { ghost; icon: IconName::X; label: $i18n::text("operation-cancel", cx);
                on_click: $(cx.listener(|this, _, _, cx| this.cancel_location_load(cx)));
            },
        ] }.into_any_element()
    }
}
