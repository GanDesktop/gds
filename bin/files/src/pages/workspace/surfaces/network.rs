//! Network location surface showing saved connections, active sessions, and mounted shares.

use std::collections::HashSet;
use std::path::PathBuf;

use gds::i18n;
use gds::service::remote::{Address, Protocol};
use gds::ui::page::{
    ContentPageProps, PageActionsProps, PageContentProps, PageHeaderProps, PageNoticeProps,
    PageTitleProps,
};
use gds::ui::{IconButtonProps, IconName, IconProps};
use zgui::prelude::*;
use zgui::view::{EventCx, current_host, events, install_stylesheet};
use zgui::vocab::{Key, Modifiers, NamedKey, Role};
use zgui::{css, view};
use zgui_ui::SpinnerProps;

use crate::entities::location::Location;
use crate::features::connect_server::Intent;
use crate::pages::workspace::{model::Contents, ui::Browser};
use crate::shared::ui::actions::Command;

const NETWORK_SHEET: &str = "gds-files-workspace-network";

const NETWORK_CSS: &str = css!(
    r#"
    .gds-files-network-scroll {
        flex: 1 1 0%;
        min-height: 0;
        overflow-y: auto;
        gap: 24px;
        padding: 8px 24px 16px 24px;
    }

    .gds-files-network-section {
        gap: 8px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-network-section__title {
        font-size: var(--gds-font-size-xs, 12px);
        font-weight: 600;
        color: var(--gds-muted-foreground);
    }

    .gds-files-network-list {
        gap: 8px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-network-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 12px;
        padding: 12px;
        border-radius: 6px;
        border: 1px solid var(--gds-border);
        min-width: 0;
    }

    .gds-files-network-row__icon {
        flex-shrink: 0;
        color: var(--gds-foreground);
    }

    .gds-files-network-row__body {
        display: flex;
        flex-direction: column;
        gap: 2px;
        flex: 1 1 0%;
        min-width: 0;
        cursor: pointer;
        border-radius: 4px;
        outline: none;
    }

    .gds-files-network-row__body:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: 2px;
    }

    .gds-files-network-row__title {
        font-size: var(--gds-font-size-sm, 14px);
        font-weight: 500;
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-network-row__detail {
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-network-row__status {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        flex-shrink: 0;
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-network-notice {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 12px;
        padding: 4px 0;
        width: 100%;
        min-width: 0;
    }

    .gds-files-network-notice__info {
        display: flex;
        flex-direction: column;
        gap: 2px;
        flex: 1 1 0%;
        min-width: 0;
    }

    .gds-files-network-notice__title {
        font-size: var(--gds-font-size-sm, 14px);
        color: var(--gds-foreground);
    }

    .gds-files-network-notice__detail {
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-network-notice__loading {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-files-network-notice__error {
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-error);
    }

    .gds-files-network-empty {
        flex: 1 1 0%;
        min-height: 220px;
        align-items: center;
        justify-content: center;
        gap: 12px;
        padding: 24px;
    }

    .gds-files-network-empty__icon {
        width: 32px;
        height: 32px;
        color: var(--gds-muted-foreground);
    }

    .gds-files-network-empty__text {
        font-size: var(--gds-font-size-sm, 14px);
        color: var(--gds-muted-foreground);
        text-align: center;
    }
    "#
);

fn install_network_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(NETWORK_SHEET, NETWORK_CSS);
    }
}

fn display_address_title(address: &Address) -> String {
    let name = address.name();
    if name.is_empty() || name == "/" {
        address.host().to_owned()
    } else {
        name
    }
}

fn address_server_detail(address: &Address) -> String {
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
    format!("{protocol} · {server}")
}

fn open_add_location_prompt(browser: Browser) {
    let target = browser.clone();
    browser
        .0
        .connector
        .prompt(None, Intent::Add, move |result| match result {
            Ok(address) => {
                target.update(|state| {
                    state.places.set_remote_favorite(address, true);
                });
                target.save_places();
                target.refresh_favorites_tabs();
                target.changed();
            }
            Err(message) => {
                if !message.is_empty() {
                    target.report_notice(message);
                }
            }
        });
}

fn open_remote_place(browser: &Browser, address: Address, modifiers: Modifiers) {
    let in_picker = browser.read_untracked(|state| state.picker.is_some());
    if modifiers.primary() && !in_picker {
        browser.new_tab(Some(Location::Remote(address)));
    } else {
        browser.open(Location::Remote(address));
    }
}

fn open_mounted_place(browser: &Browser, path: PathBuf, modifiers: Modifiers) {
    let in_picker = browser.read_untracked(|state| state.picker.is_some());
    let location = Location::from(path.as_path());
    if modifiers.primary() && !in_picker {
        browser.new_tab(Some(location));
    } else {
        browser.open(location);
    }
}

fn is_button_activation_key(key: &Key) -> bool {
    matches!(key, Key::Named(NamedKey::Enter | NamedKey::Space)) || *key == Key::character(" ")
}

fn remote_location_row(
    browser: Browser,
    address: Address,
    saved: bool,
    connected: bool,
    connecting: bool,
) -> AnyView {
    let title = display_address_title(&address);
    let a11y_title = title.clone();
    let detail = address_server_detail(&address);
    let path_label = address.path();
    let status = if connecting {
        i18n::text("remote-connecting")
    } else if connected {
        i18n::text("remote-connected")
    } else {
        i18n::text("remote-disconnected")
    };

    let body_addr = address.clone();
    let body_browser = browser.clone();
    let key_addr = address.clone();
    let key_browser = browser.clone();
    let open_addr = address.clone();
    let open_browser = browser.clone();
    let disconnect_addr = address.clone();
    let disconnect_browser = browser.clone();
    let fav_addr = address;
    let fav_browser = browser;

    AnyView::new(view! {
        row(class = "gds-files-network-row") {
            Icon(
                name = IconName::Network,
                size = 20.0,
                class = "gds-files-network-row__icon",
            )
            column(
                class = "gds-files-network-row__body",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = a11y_title,
                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                    open_remote_place(&body_browser, body_addr.clone(), ev.modifiers);
                },
                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                    if is_button_activation_key(&ev.key) {
                        ev.prevent_default();
                        ev.stop_propagation();
                        open_remote_place(&key_browser, key_addr.clone(), ev.modifiers);
                    }
                },
            ) {
                text(class = "gds-files-network-row__title") {{title}}
                text(class = "gds-files-network-row__detail") {{detail}}
                text(class = "gds-files-network-row__detail") {{path_label}}
            }
            row(class = "gds-files-network-row__status") {
                {{connecting.then(|| view! {
                    Spinner(label = status.clone())
                }.into_view())}}
                text {{status}}
            }
            PageActions {
                IconButton(
                    icon = if connected { IconName::FolderOpen } else { IconName::Plug },
                    label = i18n::text(if connected { "remote-open" } else { "remote-connect-action" }),
                    disabled = connecting,
                    on:click = move |ev: &mut EventCx<'_, events::Click>| {
                        open_remote_place(&open_browser, open_addr.clone(), ev.modifiers);
                    },
                ) {}
                {{connected.then(|| view! {
                    IconButton(
                        icon = IconName::Unplug,
                        label = i18n::text("remote-disconnect"),
                        on:click = move |_| {
                            disconnect_browser.disconnect_remote_address(disconnect_addr.clone());
                        },
                    )
                }.into_view())}}
                IconButton(
                    icon = if saved { IconName::Trash } else { IconName::Star },
                    label = i18n::text(if saved { "remote-remove-saved" } else { "remote-save-location" }),
                    on:click = move |_| {
                        fav_browser.update(|state| {
                            state.places.set_remote_favorite(fav_addr.clone(), !saved);
                        });
                        fav_browser.save_places();
                        fav_browser.refresh_favorites_tabs();
                        fav_browser.changed();
                    },
                ) {}
            }
        }
    })
}

fn mounted_location_row(browser: Browser, path: PathBuf) -> AnyView {
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string());
    let a11y_title = title.clone();
    let detail = path.display().to_string();
    let body_path = path.clone();
    let body_browser = browser.clone();
    let key_path = path.clone();
    let key_browser = browser.clone();
    let open_path = path;
    let open_browser = browser;

    AnyView::new(view! {
        row(class = "gds-files-network-row") {
            Icon(
                name = IconName::Folder,
                size = 20.0,
                class = "gds-files-network-row__icon",
            )
            column(
                class = "gds-files-network-row__body",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = a11y_title,
                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                    open_mounted_place(&body_browser, body_path.clone(), ev.modifiers);
                },
                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                    if is_button_activation_key(&ev.key) {
                        ev.prevent_default();
                        ev.stop_propagation();
                        open_mounted_place(&key_browser, key_path.clone(), ev.modifiers);
                    }
                },
            ) {
                text(class = "gds-files-network-row__title") {{title}}
                text(class = "gds-files-network-row__detail") {{detail}}
            }
            PageActions {
                IconButton(
                    icon = IconName::FolderOpen,
                    label = i18n::text("remote-open"),
                    on:click = move |ev: &mut EventCx<'_, events::Click>| {
                        open_mounted_place(&open_browser, open_path.clone(), ev.modifiers);
                    },
                )
            }
        }
    })
}

fn network_section(title_key: &'static str, rows: Vec<AnyView>) -> AnyView {
    AnyView::new(view! {
        column(class = "gds-files-network-section") {
            text(class = "gds-files-network-section__title") {{i18n::text(title_key)}}
            column(class = "gds-files-network-list") {
                {rows}
            }
        }
    })
}

impl Browser {
    pub(in crate::pages::workspace) fn network_content(&self) -> AnyView {
        install_network_stylesheet();
        self.track();

        let compact = self.compact();
        let (in_picker, saved, mounts, error, discovery_loading) = self.read(|state| {
            let in_picker = state.picker.is_some();
            let saved: Vec<Address> = if in_picker {
                Vec::new()
            } else {
                state
                    .places
                    .favorites
                    .iter()
                    .filter_map(|location| match location {
                        Location::Remote(address) => Some(address.clone()),
                        _ => None,
                    })
                    .collect()
            };
            let tab = state.tab();
            let (mounts, error, loading_contents) = match &tab.contents {
                Contents::Network { mounts, error } => (mounts.clone(), error.clone(), false),
                Contents::LoadingCollection => (Vec::new(), None, true),
                Contents::Unavailable(err) => (Vec::new(), Some(err.clone()), false),
                _ => (Vec::new(), None, false),
            };
            let discovery_loading =
                loading_contents || tab.work.is_some() || tab.navigation.is_loading();
            (in_picker, saved, mounts, error, discovery_loading)
        });

        let connecting = self.0.connector.is_connecting();
        let pending = self.0.connector.pending();
        let pending_key = pending.as_ref().map(|(addr, _)| addr.connection_key());
        let connections = if in_picker {
            Vec::new()
        } else {
            self.0.connector.connections()
        };

        let saved_keys: HashSet<String> = saved.iter().map(Address::connection_key).collect();
        let connected_extra: Vec<Address> = connections
            .into_iter()
            .filter(|addr| !saved_keys.contains(&addr.connection_key()))
            .collect();

        let pending_notice = pending.map(|(address, intent)| {
            let title = i18n::text(if intent == Intent::Add {
                "remote-adding"
            } else {
                "remote-connecting"
            });
            let detail = address.to_string();
            let cancel_browser = self.clone();
            AnyView::new(view! {
                PageNotice {
                    row(class = "gds-files-network-notice") {
                        Spinner(label = title.clone())
                        column(class = "gds-files-network-notice__info") {
                            text(class = "gds-files-network-notice__title") {{title}}
                            text(class = "gds-files-network-notice__detail") {{detail}}
                        }
                        IconButton(
                            icon = IconName::X,
                            label = i18n::text("operation-cancel"),
                            on:click = move |_| cancel_browser.0.connector.cancel(),
                        )
                    }
                }
            })
        });

        let loading_notice = discovery_loading.then(|| {
            let label = i18n::text("network-loading");
            AnyView::new(view! {
                PageNotice {
                    row(class = "gds-files-network-notice__loading") {
                        Spinner(label = label.clone())
                        text {{label}}
                    }
                }
            })
        });

        let error_notice = error.map(|message| {
            AnyView::new(view! {
                PageNotice {
                    text(class = "gds-files-network-notice__error") {{message}}
                }
            })
        });

        let mut sections = Vec::new();
        if !saved.is_empty() {
            let rows = saved
                .into_iter()
                .map(|address| {
                    let is_connected = self.0.connector.session(&address).is_some();
                    let is_connecting = pending_key.as_deref() == Some(&address.connection_key());
                    remote_location_row(self.clone(), address, true, is_connected, is_connecting)
                })
                .collect();
            sections.push(network_section("remote-saved-locations", rows));
        }

        if !connected_extra.is_empty() {
            let rows = connected_extra
                .into_iter()
                .map(|address| {
                    let is_connecting = pending_key.as_deref() == Some(&address.connection_key());
                    remote_location_row(self.clone(), address, false, true, is_connecting)
                })
                .collect();
            sections.push(network_section("remote-connected-servers", rows));
        }

        if !mounts.is_empty() {
            let rows = mounts
                .into_iter()
                .map(|path| mounted_location_row(self.clone(), path))
                .collect();
            sections.push(network_section("remote-mounted-folders", rows));
        }

        let empty = sections.is_empty();
        let add_header_browser = self.clone();
        let refresh_browser = self.clone();

        let header = (!compact).then(move || {
            let add_action = (!in_picker).then(move || {
                view! {
                    IconButton(
                        icon = IconName::Plus,
                        label = i18n::text("remote-add-location"),
                        disabled = connecting,
                        on:click = move |_| open_add_location_prompt(add_header_browser.clone()),
                    )
                }
                .into_view()
            });
            AnyView::new(view! {
                PageHeader {
                    PageTitle {{i18n::text("network")}}
                    PageActions {
                        {add_action}
                        IconButton(
                            icon = IconName::RefreshCw,
                            label = i18n::text("refresh"),
                            shortcut = Some(Command::Refresh.shortcut()),
                            disabled = discovery_loading,
                            on:click = move |_| refresh_browser.refresh(),
                        )
                    }
                }
            })
        });

        AnyView::new(view! {
            ContentPage {
                {header}
                {pending_notice}
                {loading_notice}
                {error_notice}
                PageContent {
                    {if empty && !connecting && !discovery_loading {
                        let empty_msg = if in_picker {
                            i18n::text("network-empty")
                        } else {
                            i18n::text("remote-network-empty")
                        };
                        AnyView::new(view! {
                            column(class = "gds-files-network-empty") {
                                Icon(
                                    name = IconName::Network,
                                    size = 32.0,
                                    class = "gds-files-network-empty__icon",
                                )
                                text(class = "gds-files-network-empty__text") {{empty_msg}}
                            }
                        })
                    } else {
                        AnyView::new(view! {
                            column(class = "gds-files-network-scroll") {
                                {sections}
                            }
                        })
                    }}
                }
            }
        })
    }
}
