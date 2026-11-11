use super::super::model::{
    Authentication, DEFAULT_SFTP_PORT, DEFAULT_SMB_PORT, Details, Error, Request,
};
use crate::shared::i18n;
use gds::ui::field::input as input_field;
use gds::ui::{IconButton, button::Button};
use gds::{
    appearance::tokens::ICON_SIZE,
    service::remote::{Address, Credentials, Protocol},
    t,
};
use gds_uim::{component, el};
use gpui_kit::IntoElement;
use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::{
    component::{
        ActiveTheme, Icon,
        button::{Button as Choice, ButtonGroup, ButtonVariants},
        input::InputState,
    },
    *,
};

const PORT_FIELD_WIDTH: f32 = 100.0;

struct ProtocolOption {
    protocol: Protocol,
    label: &'static str,
    description_id: &'static str,
}

#[derive(Clone, Copy)]
struct AuthOption {
    method: Authentication,
    label_id: &'static str,
}

pub(super) struct Form {
    protocol: Option<Protocol>,
    authentication: Authentication,
    host: Entity<InputState>,
    port: Entity<InputState>,
    path: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    key: Entity<InputState>,
    passphrase: Entity<InputState>,
}

impl Form {
    pub(super) fn protocol(&self) -> Option<Protocol> {
        self.protocol
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut App) {
        if self.protocol.is_some() {
            self.host.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
    }

    pub(super) fn new(
        address: Option<Address>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let protocol = address.as_ref().map(Address::protocol);
        let host = address
            .as_ref()
            .map_or(String::new(), |a| a.host().to_owned());
        let mut user = address.as_ref().map_or(String::new(), Address::username);
        if protocol == Some(Protocol::Sftp) && user.is_empty() {
            user = std::env::var("USER").unwrap_or_default();
        }
        let authentication = if protocol == Some(Protocol::Smb) {
            if user.eq_ignore_ascii_case("guest") {
                Authentication::Guest
            } else {
                Authentication::Password
            }
        } else {
            Authentication::Agent
        };
        let path = address.as_ref().map_or(String::new(), Address::path);
        let port = address
            .as_ref()
            .map_or(String::new(), |a| a.port().to_string());
        Self {
            protocol,
            authentication,
            host: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(host)
                    .placeholder("server.example.com")
            }),
            port: cx.new(|cx| InputState::new(window, cx).default_value(port)),
            path: cx.new(|cx| InputState::new(window, cx).default_value(path)),
            user: cx.new(|cx| InputState::new(window, cx).default_value(user)),
            password: cx.new(|cx| InputState::new(window, cx).masked(true)),
            key: cx.new(|cx| InputState::new(window, cx).placeholder("/home/user/.ssh/id_ed25519")),
            passphrase: cx.new(|cx| InputState::new(window, cx).masked(true)),
        }
    }

    fn choose_protocol(&mut self, protocol: Protocol, window: &mut Window, cx: &mut Context<Self>) {
        self.protocol = Some(protocol);
        let port;
        let path;
        let user;
        if protocol == Protocol::Sftp {
            self.authentication = Authentication::Agent;
            port = DEFAULT_SFTP_PORT.to_string();
            path = "/";
            user = std::env::var("USER").unwrap_or_default();
        } else {
            self.authentication = Authentication::Password;
            port = DEFAULT_SMB_PORT.to_string();
            path = "";
            user = String::new();
        }
        self.port
            .update(cx, |input, cx| input.set_value(port, window, cx));
        self.path
            .update(cx, |input, cx| input.set_value(path, window, cx));
        self.user
            .update(cx, |input, cx| input.set_value(user, window, cx));
        self.password
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.passphrase
            .update(cx, |input, cx| input.set_value("", window, cx));
        let form = cx.entity();
        window.on_next_frame(move |window, cx| form.update(cx, |form, cx| form.focus(window, cx)));
        cx.notify();
        cx.refresh_windows();
    }

    /// # Errors
    /// Reports invalid connection fields or missing credentials for the selected method.
    pub(super) fn request(&self, cx: &App) -> Result<Request, Error> {
        let protocol = self.protocol.ok_or(Error::Protocol)?;
        let user = if self.authentication == Authentication::Guest {
            "guest".to_owned()
        } else {
            self.user.read(cx).value().to_string()
        };
        if user.is_empty() {
            return Err(Error::User);
        }
        let address = Details {
            protocol,
            host: self.host.read(cx).value().to_string(),
            port: self.port.read(cx).value().to_string(),
            user,
            path: self.path.read(cx).value().to_string(),
        }
        .address()?;
        let credentials = match self.authentication {
            Authentication::Agent | Authentication::Guest => Credentials::default(),
            Authentication::Password => Credentials {
                password: self.password.read(cx).value().to_string(),
                ..Default::default()
            },
            Authentication::Key => {
                let key = self.key.read(cx).value().to_string();
                if !std::path::Path::new(&key).is_absolute() {
                    return Err(Error::Key);
                }
                let passphrase = self.passphrase.read(cx).value().to_string();
                Credentials {
                    private_key: Some(key.into()),
                    key_passphrase: (!passphrase.is_empty()).then_some(passphrase),
                    ..Default::default()
                }
            }
        };
        Ok(Request {
            address,
            credentials,
        })
    }
}

#[component]
#[derive(Default, IntoElement)]
struct ConnectionField {
    #[attr(into)]
    label: SharedString,
    #[attr(option)]
    input: Option<Entity<InputState>>,
}

impl RenderOnce for ConnectionField {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! { .div { flex; flex_col; gap_1; min_w_0; flex_1; } [
            .div { text_xs; text_color: $(cx.theme().muted_foreground); } [$(self.label)],
            @if (let Some(input) = self.input) ($&input, cx).input_field, @end,
        ] }
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(protocol) = self.protocol else {
            let options = [
                ProtocolOption {
                    protocol: Protocol::Sftp,
                    label: "SFTP",
                    description_id: "remote-sftp-description",
                },
                ProtocolOption {
                    protocol: Protocol::Smb,
                    label: "SMB",
                    description_id: "remote-smb-description",
                },
            ]
            .into_iter()
            .enumerate()
            .map(|(index, option)| {
                let protocol = option.protocol;
                let label = option.label;
                let description = i18n::text(option.description_id, cx);
                let on_choose = cx.listener(move |this, _, window, cx| {
                    this.choose_protocol(protocol, window, cx)
                });
                el! {
                    format!("choose-protocol-{index}").Button::new {
                        ghost; accessibility_label: label; w_full; h_auto; p_3; justify_start;
                        on_click: on_choose;
                    } [
                        .div { flex; w_full; min_w_0; items_center; gap_3 } [
                            .div { flex; flex_col; flex_1; min_w_0; gap_1; text_left } [
                                .div { font_semibold } [label],
                                .div { text_xs; text_color: $cx.theme().muted_foreground } [description],
                            ],
                            IconName::ChevronRight.Icon::new { size: ICON_SIZE.px; flex_shrink_0 },
                        ],
                    ]
                }
                .into_any_element()
            })
            .collect::<Vec<_>>();
            return el! { .div { flex; flex_col; gap_2; w_full; min_w_0 } [
                .div { text_color: $cx.theme().muted_foreground } [$(t!(@cx, "remote-choose-protocol"))],
                ..options,
            ] }
            .into_any_element();
        };
        let methods = if protocol == Protocol::Sftp {
            vec![
                AuthOption {
                    method: Authentication::Agent,
                    label_id: "remote-auth-agent",
                },
                AuthOption {
                    method: Authentication::Password,
                    label_id: "remote-password",
                },
                AuthOption {
                    method: Authentication::Key,
                    label_id: "remote-auth-key",
                },
            ]
        } else {
            vec![
                AuthOption {
                    method: Authentication::Password,
                    label_id: "remote-auth-user",
                },
                AuthOption {
                    method: Authentication::Guest,
                    label_id: "remote-auth-guest",
                },
            ]
        };
        let values = methods.iter().map(|opt| opt.method).collect::<Vec<_>>();
        let choices = methods.into_iter().enumerate().map(|(index, opt)| {
            let label = i18n::text(opt.label_id, cx);
            let selected = self.authentication == opt.method;
            el! { format!("authentication-{index}").Choice::new {
                label: label; selected: selected;
            } }
        });
        let on_authenticate = cx.listener(move |this, indices: &Vec<usize>, _, cx| {
            if let Some(index) = indices.first() {
                this.authentication = values[*index];
                cx.notify();
            }
        });
        let authentication = el! { "remote-authentication".ButtonGroup::new {
            children: choices; on_click: on_authenticate;
        } };
        let on_change = cx.listener(|this, _, _, cx| {
            this.protocol = None;
            cx.notify();
            cx.refresh_windows();
        });
        let change = el! { "change-protocol".IconButton::new {
            icon: IconName::ArrowLeft; label: $(t!(@cx, "remote-change-protocol"));
            on_click: on_change;
        } };
        let show_user = self.authentication != Authentication::Guest;
        let protocol_label = if protocol == Protocol::Sftp {
            "SFTP"
        } else {
            "SMB"
        };
        let path_label = if protocol == Protocol::Sftp {
            t!(@cx, "remote-folder")
        } else {
            t!(@cx, "remote-share-path")
        };
        let user_label = if protocol == Protocol::Smb {
            t!(@cx, "remote-smb-user")
        } else {
            t!(@cx, "remote-username")
        };
        el! { .div { flex; flex_col; gap_3; w_full } [
            .div { flex; items_center; gap_2 } [change, .div { font_semibold } [protocol_label]],
            .div { flex; gap_3; w_full } [
                .ConnectionField::default { label: $(t!(@cx, "remote-host")); input: $(self.host.clone()); },
                .div { w: PORT_FIELD_WIDTH.px; flex_shrink_0 } [.ConnectionField::default { label: $(t!(@cx, "remote-port")); input: $(self.port.clone()); }],
            ],
            .ConnectionField::default { label: path_label; input: $(self.path.clone()); },
            .div { flex; flex_col; gap_1 } [.div { text_xs; text_color: $cx.theme().muted_foreground } [$(t!(@cx, "remote-authentication"))], authentication],
            @if (show_user) .ConnectionField::default { label: user_label; input: $(self.user.clone()); }, @end,
            @if (self.authentication == Authentication::Password) .ConnectionField::default { label: $(t!(@cx, "remote-password")); input: $(self.password.clone()); }, @end,
            @if (self.authentication == Authentication::Key)
                .ConnectionField::default { label: $(t!(@cx, "remote-auth-key")); input: $(self.key.clone()); },
                .ConnectionField::default { label: $(t!(@cx, "remote-passphrase")); input: $(self.passphrase.clone()); },
            @end,
        ] }
        .into_any_element()
    }
}
