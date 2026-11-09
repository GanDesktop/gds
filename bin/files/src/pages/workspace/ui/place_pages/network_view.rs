use super::*;
use gds::service::remote::{Address, Protocol};
use gds::ui::IconButton;
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{Icon, spinner::Spinner};

impl Browser {
    fn network_section(label: &'static str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
        el! { .div { flex; flex_col; gap_2; w_full; mb_6 } [
            .div { text_xs; font_semibold; text_color: $cx.theme().muted_foreground } [$i18n::text(label, cx)],
            ..rows,
        ] }.into_any_element()
    }

    fn open_remote_place(&mut self, address: Address, window: &mut Window, cx: &mut Context<Self>) {
        if window.modifiers().control || window.modifiers().platform {
            self.open_location_tab(Location::Remote(address), false, Some(window), cx);
        } else {
            self.navigate_location(self.tabs.active_id, Location::Remote(address), cx);
        }
    }

    fn remote_place_row(
        &self,
        group: &'static str,
        index: usize,
        address: &Address,
        saved: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let connected = self.remote.connector.read(cx).session(address).is_some();
        let connecting = self
            .remote
            .connector
            .read(cx)
            .pending()
            .is_some_and(|pending| pending.address.connection_key() == address.connection_key());
        let protocol = match address.protocol() {
            Protocol::Sftp => "SFTP",
            Protocol::Smb => "SMB",
        };
        let user = address.username();
        let server = if user.is_empty() {
            address.host().to_owned()
        } else {
            format!("{user}@{}", address.host())
        };
        let detail = format!("{protocol} · {server}");
        let body_address = address.clone();
        let open_address = address.clone();
        let save_address = address.clone();
        let on_open = cx.listener(move |this, _, window, cx| {
            this.open_remote_place(open_address.clone(), window, cx)
        });
        let open = el! { format!("{group}-open-{index}").IconButton::new {
            icon: $(if connected { IconName::FolderOpen } else { IconName::Plug });
            label: $i18n::text(if connected { "remote-open" } else { "remote-connect-action" }, cx);
            disabled: connecting; on_click: on_open;
        } };
        let on_save = cx.listener(move |this, _, _, cx| {
            this.locations
                .places
                .set_remote_favorite(save_address.clone(), !saved);
            this.save_places(cx);
            cx.notify();
        });
        let favorite = el! { format!("{group}-save-{index}").IconButton::new {
            icon: $(if saved { IconName::Trash } else { IconName::Star });
            label: $i18n::text(if saved { "remote-remove-saved" } else { "remote-save-location" }, cx);
            on_click: on_save;
        } };
        let status = i18n::text(
            if connecting {
                "remote-connecting"
            } else if connected {
                "remote-connected"
            } else {
                "remote-disconnected"
            },
            cx,
        );
        el! { .div { # $(group, index); flex; items_center; gap_3; p_3; rounded: DataListStyle::ROW_RADIUS.px; border_1; border_color: $cx.theme().border; min_w_0 } [
            (IconName::Network).Icon::new { size: 20.0.px; flex_shrink_0 },
            .div { # format!("{group}-body-{index}"); flex; flex_col; gap_1; flex_1; min_w_0; cursor_pointer;
                on_click: $(cx.listener(move |this, _, window, cx| this.open_remote_place(body_address.clone(), window, cx)));
            } [
                .div { font_medium; truncate } [$(address.name())],
                .div { text_xs; text_color: $cx.theme().muted_foreground; truncate } [detail],
                .div { text_xs; text_color: $cx.theme().muted_foreground; truncate } [$(address.path())],
            ],
            .div { flex; items_center; gap_2; flex_shrink_0; text_xs; text_color: $cx.theme().muted_foreground } [
                @if (connecting) .Spinner::new { small; }, @end,
                status,
            ],
            .PageActions::default [open, favorite],
        ] }.into_any_element()
    }

    pub(super) fn render_network_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let connector = self.remote.connector.read(cx);
        let saved = self
            .locations
            .places
            .favorites
            .iter()
            .filter_map(|location| match location {
                Location::Remote(address) if self.picker.is_none() => Some(address.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let connected = connector
            .connections()
            .into_iter()
            .filter(|address| {
                !saved
                    .iter()
                    .any(|s| s.connection_key() == address.connection_key())
            })
            .collect::<Vec<_>>();
        let connecting = connector.connecting;
        let pending = connector.pending().map(|p| (p.address.clone(), p.intent));
        let mut sections = Vec::new();
        if !saved.is_empty() {
            let rows = saved
                .iter()
                .enumerate()
                .map(|(index, a)| self.remote_place_row("saved-network", index, a, true, cx))
                .collect();
            sections.push(Self::network_section("remote-saved-locations", rows, cx));
        }
        if !connected.is_empty() {
            let rows = connected
                .iter()
                .enumerate()
                .map(|(index, a)| self.remote_place_row("connected-network", index, a, false, cx))
                .collect();
            sections.push(Self::network_section("remote-connected-servers", rows, cx));
        }
        if !self.collections.network.discovery.paths.is_empty() {
            let rows = self.collections.network.discovery.paths.iter().enumerate().map(|(index, path)| {
                let target = path.clone();
                el! { .div { # $("mounted-network", index); flex; items_center; gap_3; p_3; rounded: DataListStyle::ROW_RADIUS.px; border_1; border_color: $cx.theme().border; min_w_0 } [
                    (IconName::Folder).Icon::new { size: 20.0.px },
                    .div { flex; flex_col; gap_1; flex_1; min_w_0 } [
                        .div { font_medium; truncate } [$(path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned())],
                        .div { text_xs; text_color: $cx.theme().muted_foreground; truncate } [$(path.display().to_string())],
                    ],
                    format!("open-network-mount-{index}").IconButton::new { icon: IconName::FolderOpen; label: $i18n::text("remote-open", cx);
                        on_click: $(cx.listener(move |this, _, _, cx| this.navigate_tab(this.tabs.active_id, target.clone(), cx)));
                    },
                ] }.into_any_element()
            }).collect();
            sections.push(Self::network_section("remote-mounted-folders", rows, cx));
        }
        let empty = sections.is_empty();
        let on_add = cx.listener(|this, _, window, cx| this.add_network_location(window, cx));
        let add = el! { "add-network-location".IconButton::new {
            icon: IconName::Plus; label: $i18n::text("remote-add-location", cx);
            disabled: connecting; on_click: on_add;
        } };
        let on_refresh = cx.listener(|this, _, _, cx| this.refresh_network(cx));
        let refresh = el! { "refresh-network".IconButton::new {
            icon: IconName::RefreshCw; label: $i18n::text("refresh", cx);
            disabled: $self.collections.network.discovery.loading; on_click: on_refresh;
        } };
        el! { .ContentPage::default { # "network-page" } [
            .PageHeader::default { @if (self.window.layout.compact) hidden; @end } [
                .PageTitle::default [$i18n::text("network", cx)],
                .PageActions::default [@if (self.picker.is_none()) add, @end, refresh],
            ],
            @if (let Some((address, intent)) = pending)
                .PageNotice::default [
                    .div { flex; items_center; gap_3; py_2 } [
                        .Spinner::new { small; },
                        .div { flex; flex_col; gap_1; flex_1; min_w_0 } [
                            .div [$i18n::text(if intent == crate::features::connect_server::Intent::Add { "remote-adding" } else { "remote-connecting" }, cx)],
                            .div { text_xs; text_color: $cx.theme().muted_foreground; truncate } [$(address.to_string())],
                        ],
                        "cancel-network-connection".IconButton::new { icon: IconName::X; label: $i18n::text("operation-cancel", cx);
                            on_click: $(cx.listener(|this, _, _, cx| this.remote.connector.update(cx, |connector, cx| connector.cancel(cx))));
                        },
                    ],
                ],
            @end,
            @if (self.collections.network.discovery.loading)
                .PageNotice::default [.div { flex; items_center; gap_2; text_xs; text_color: $cx.theme().muted_foreground } [.Spinner::new { small; }, $i18n::text("network-loading", cx)]],
            @end,
            @if (let Some(error) = &self.collections.network.discovery.error)
                .PageNotice::default [.div { text_xs; text_color: $cx.theme().danger } [$(error.clone())]],
            @end,
            .PageContent::default [
                @if (empty && !connecting && !self.collections.network.discovery.loading)
                    .div { flex; flex_col; flex_1; items_center; justify_center; gap_3; text_color: $cx.theme().muted_foreground } [
                        (IconName::Network).Icon::new { size: 32.0.px },
                        .div [$i18n::text("remote-network-empty", cx)],
                    ],
                @else
                    .div { # "network-sections"; flex; flex_col; flex_1; min_h_0; overflow_y_scroll; px_6; py_2 } [..sections],
                @end,
            ],
        ] }.into_any_element()
    }
}
