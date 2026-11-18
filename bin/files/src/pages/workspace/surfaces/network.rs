use crate::pages::workspace::{model::Contents, ui::Browser};
use crate::{entities::location::Location, features::connect_server::Intent};
use anti_webui::prelude::*;
use gds::{
    appearance::tokens::DataListStyle,
    i18n,
    service::remote::{Address, Protocol},
    ui::{
        Icon, IconButton,
        page::{PageActions, PageNotice},
    },
};
use gds_uim::el;
use std::path::PathBuf;

fn label(value: String, muted: bool) -> impl IntoView {
    el! { value.text.{
        ellipsis; w_full; min_w: 0.0_f32; whitespace_nowrap;
        if muted {
            text_size: 12.0_f32;
            dyn_style: style => $(style.text_color(theme().text_muted));
        } else {
            font_weight: FontWeight::MEDIUM;
        };
    } }
}

fn section(title: &'static str, rows: Vec<Div>) -> Div {
    el! { .div.{flex; flex_col; gap: 8.0_f32; w_full; mb: 24.0_f32}.[
        .div.{text_size: 12.0_f32; font_weight: FontWeight::SEMIBOLD;
            dyn_style: style => $(style.text_color(theme().text_muted));
        }.[ $(i18n::text(title)).text ],
        ..rows,
    ] }
}

fn card() -> Div {
    el! { .div.{flex; items_center; gap: 12.0_f32; p: 12.0_f32; min_w: 0.0_f32;
        rounded: DataListStyle::ROW_RADIUS; border: 1.0_f32;
        dyn_style: style => $(style.border_color(theme().border));
    } }
}

impl Browser {
    pub(in crate::pages::workspace) fn network_content(&self) -> Div {
        let browser = self.clone();
        el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32}.[
            $(move || browser.network_groups()).dyn_view,
        ] }
    }

    fn network_groups(&self) -> Div {
        let (saved, mounts, error, loading, picker) = self.read(|state| {
            let picker = state.picker.is_some();
            let saved = state
                .places
                .favorites
                .iter()
                .filter_map(|location| match location {
                    Location::Remote(address) if !picker => Some(address.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let (mounts, error) = match &state.tab().contents {
                Contents::Network { mounts, error } => (mounts.clone(), error.clone()),
                _ => (Vec::new(), None),
            };
            (
                saved,
                mounts,
                error,
                state.tab().navigation.is_loading(),
                picker,
            )
        });
        let pending = self.0.connector.pending();
        let connected = self
            .0
            .connector
            .connections()
            .into_iter()
            .filter(|address| {
                !picker
                    && !saved
                        .iter()
                        .any(|saved| saved.connection_key() == address.connection_key())
            })
            .collect::<Vec<_>>();
        let mut sections = Vec::new();
        for (title, addresses, is_saved) in [
            ("remote-saved-locations", saved, true),
            ("remote-connected-servers", connected, false),
        ] {
            if !addresses.is_empty() {
                let rows = addresses
                    .into_iter()
                    .map(|address| {
                        let connecting = pending.as_ref().is_some_and(|(pending, _)| {
                            pending.connection_key() == address.connection_key()
                        });
                        self.remote_location_card(address, is_saved, connecting)
                    })
                    .collect();
                sections.push(section(title, rows));
            }
        }
        if !mounts.is_empty() {
            sections.push(section(
                "remote-mounted-folders",
                mounts
                    .into_iter()
                    .map(|path| self.mounted_location_card(path))
                    .collect(),
            ));
        }
        let empty = sections.is_empty() && !loading && pending.is_none();
        let connector = self.0.connector.clone();
        el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32}.[
            if let Some((address, intent)) = pending [
                .PageNotice::default.[
                    .div.{flex; items_center; gap: 12.0_f32; py: 8.0_f32}.[
                        .spinner.{size: 16.0_f32},
                        .div.{flex; flex_col; gap: 4.0_f32; flex_1; min_w: 0.0_f32}.[
                            $(i18n::text(if intent == Intent::Add { "remote-adding" } else { "remote-connecting" })).text,
                            $(label(address.to_string(), true)),
                        ],
                        "cancel-network-connection".IconButton::new.{
                            icon: Icon::Close;
                            label_fn: () => $(i18n::text("operation-cancel"));
                            on_click: move _ => $(connector.cancel());
                        },
                    ],
                ],
            ],
            if loading [
                .PageNotice::default.[
                    .div.{flex; items_center; gap: 8.0_f32; text_size: 12.0_f32}.[
                        .spinner.{size: 16.0_f32},
                        $(i18n::text("network-loading")).text,
                    ],
                ],
            ],
            if let Some(error) = error [
                .PageNotice::default.[
                    error.text.{text_size: 12.0_f32; dyn_style: style => $(style.text_color(theme().danger))},
                ],
            ],
            if empty [
                .div.{flex; flex_col; flex_1; items_center; justify_center; gap: 12.0_f32; px: 24.0_f32;
                    dyn_style: style => $(style.text_color(theme().text_muted));
                }.[ $(Icon::Network.view(32.0_f32)), $(i18n::text("remote-network-empty")).text ],
            ] else [
                .div.{flex; flex_col; flex_1; min_h: 0.0_f32; overflow_y_scroll; px: 24.0_f32; py: 8.0_f32}.[
                    ..sections,
                ],
            ],
        ] }
    }

    fn remote_location_card(&self, address: Address, saved: bool, connecting: bool) -> Div {
        let connected = self.0.connector.session(&address).is_some();
        let user = address.username();
        let server = if user.is_empty() {
            address.host().to_owned()
        } else {
            format!("{user}@{}", address.host())
        };
        let protocol = match address.protocol() {
            Protocol::Sftp => "SFTP",
            Protocol::Smb => "SMB",
        };
        let detail = format!("{protocol} · {server}");
        let status = i18n::text(if connecting {
            "remote-connecting"
        } else if connected {
            "remote-connected"
        } else {
            "remote-disconnected"
        });
        let name = address.name().to_owned();
        let path = address.path().to_owned();
        let open = self.clone();
        let target = address.clone();
        let activate = move |event: &PointerEvent| {
            if connecting {
                return;
            }
            let location = Location::Remote(target.clone());
            if event.modifiers.ctrl || event.modifiers.logo {
                open.new_tab(Some(location));
            } else {
                open.open(location);
            }
        };
        let body = activate.clone();
        let favorite = self.clone();
        el! { .card.[
            $(Icon::Network.view(20.0_f32)),
            .div.{flex; flex_col; gap: 4.0_f32; flex_1; min_w: 0.0_f32;
                cursor: CursorIcon::Pointer; on_click: body;
            }.[ $(label(name, false)), $(label(detail, true)), $(label(path, true)) ],
            .div.{flex; items_center; gap: 8.0_f32; min_w: 0.0_f32; text_size: 12.0_f32;
                dyn_style: style => $(style.text_color(theme().text_muted));
            }.[ if connecting [.spinner.{size: 16.0_f32}], status.text.{ellipsis; whitespace_nowrap} ],
            .PageActions::default.[
                "open-network-location".IconButton::new.{
                    icon: $(if connected { Icon::FolderOpen } else { Icon::Plug });
                    label_fn: move () => $(i18n::text(if connected { "remote-open" } else { "remote-connect-action" }));
                    disabled: connecting; on_click: activate;
                },
                "save-network-location".IconButton::new.{
                    icon: $(if saved { Icon::Trash } else { Icon::Star });
                    label_fn: move () => $(i18n::text(if saved { "remote-remove-saved" } else { "remote-save-location" }));
                    on_click: move _ => ${
                        favorite.update(|state| state.places.set_remote_favorite(address.clone(), !saved));
                        favorite.save_places();
                        favorite.refresh_location(&Location::Favorites);
                    };
                },
            ],
        ] }
    }

    fn mounted_location_card(&self, path: PathBuf) -> Div {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let display = path.display().to_string();
        let browser = self.clone();
        el! { .card.[
            $(Icon::Folder.view(20.0_f32)),
            .div.{flex; flex_col; gap: 4.0_f32; flex_1; min_w: 0.0_f32}.[
                $(label(name, false)),
                $(label(display, true)),
            ],
            "open-network-mount".IconButton::new.{
                icon: Icon::FolderOpen;
                label_fn: () => $(i18n::text("remote-open"));
                on_click: move _ => $(browser.open(Location::from(path.as_path())));
            },
        ] }
    }
}
