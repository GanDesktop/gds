use super::super::model::{Authentication, Details, Request};
use crate::shared::i18n;
use gds::ui::field::input as input_field;
use gds::ui::{IconButton, button::Button};
use gds::{
    appearance::tokens::ICON_SIZE,
    service::remote::{Address, Credentials, Protocol},
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
        self.authentication = if protocol == Protocol::Sftp {
            Authentication::Agent
        } else {
            Authentication::Password
        };
        let (port, path, user) = match protocol {
            Protocol::Sftp => ("22", "/", std::env::var("USER").unwrap_or_default()),
            Protocol::Smb => ("445", "", String::new()),
        };
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
    pub(super) fn request(&self, cx: &App) -> Result<Request, String> {
        let protocol = self
            .protocol
            .ok_or_else(|| i18n::text("remote-choose-protocol", cx).to_string())?;
        let user = if self.authentication == Authentication::Guest {
            "guest".to_owned()
        } else {
            self.user.read(cx).value().to_string()
        };
        if user.is_empty() {
            return Err(i18n::text("remote-missing-user", cx).to_string());
        }
        let address = Details {
            protocol,
            host: self.host.read(cx).value().to_string(),
            port: self.port.read(cx).value().to_string(),
            user,
            path: self.path.read(cx).value().to_string(),
        }
        .address()
        .map_err(|e| i18n::text(e.label(), cx).to_string())?;
        let credentials = match self.authentication {
            Authentication::Agent | Authentication::Guest => Credentials::default(),
            Authentication::Password => Credentials {
                password: self.password.read(cx).value().to_string(),
                ..Default::default()
            },
            Authentication::Key => {
                let key = self.key.read(cx).value().to_string();
                if !std::path::Path::new(&key).is_absolute() {
                    return Err(i18n::text("remote-missing-key", cx).to_string());
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
                (Protocol::Sftp, "SFTP", "remote-sftp-description"),
                (Protocol::Smb, "SMB", "remote-smb-description"),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (protocol, label, description))| {
                let on_choose = cx.listener(move |this, _, window, cx| {
                    this.choose_protocol(protocol, window, cx)
                });
                el! {
                    $("choose-protocol", index).Button::new {
                        ghost; accessibility_label: label; w_full; h_auto; p_3; justify_start;
                        on_click: on_choose;
                    } [
                        .div { flex; w_full; min_w_0; items_center; gap_3 } [
                            .div { flex; flex_col; flex_1; min_w_0; gap_1; text_left } [
                                .div { font_semibold } [label],
                                .div { text_xs; text_color: $cx.theme().muted_foreground } [$i18n::text(description, cx)],
                            ],
                            IconName::ChevronRight.Icon::new { size: ICON_SIZE.px; flex_shrink_0 },
                        ],
                    ]
                }.into_any_element()
            })
            .collect::<Vec<_>>();
            return el! { .div { flex; flex_col; gap_2; w_full; min_w_0 } [
                .div { text_color: $cx.theme().muted_foreground } [$i18n::text("remote-choose-protocol", cx)],
                ..options,
            ] }.into_any_element();
        };
        let methods = if protocol == Protocol::Sftp {
            vec![
                (Authentication::Agent, "remote-auth-agent"),
                (Authentication::Password, "remote-password"),
                (Authentication::Key, "remote-auth-key"),
            ]
        } else {
            vec![
                (Authentication::Password, "remote-auth-user"),
                (Authentication::Guest, "remote-auth-guest"),
            ]
        };
        let values = methods.iter().map(|(value, _)| *value).collect::<Vec<_>>();
        let choices = methods
            .into_iter()
            .enumerate()
            .map(|(index, (value, label))| {
                el! { $("authentication", index).Choice::new {
                    label: $i18n::text(label, cx); selected: $(self.authentication == value);
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
            icon: IconName::ArrowLeft; label: $i18n::text("remote-change-protocol", cx);
            on_click: on_change;
        } };
        let show_user = self.authentication != Authentication::Guest;
        el! { .div { flex; flex_col; gap_3; w_full } [
            .div { flex; items_center; gap_2 } [change, .div { font_semibold } [$(if protocol == Protocol::Sftp { "SFTP" } else { "SMB" })]],
            .div { flex; gap_3; w_full } [
                .ConnectionField::default { label: $(i18n::text("remote-host", cx)); input: $(self.host.clone()); },
                .div { w: 100.0.px; flex_shrink_0 } [.ConnectionField::default { label: $(i18n::text("remote-port", cx)); input: $(self.port.clone()); }],
            ],
            .ConnectionField::default { label: $(i18n::text(if protocol == Protocol::Sftp { "remote-folder" } else { "remote-share-path" }, cx)); input: $(self.path.clone()); },
            .div { flex; flex_col; gap_1 } [.div { text_xs; text_color: $cx.theme().muted_foreground } [$i18n::text("remote-authentication", cx)], authentication],
            @if (show_user) .ConnectionField::default { label: $(i18n::text(if protocol == Protocol::Smb { "remote-smb-user" } else { "remote-username" }, cx)); input: $(self.user.clone()); }, @end,
            @if (self.authentication == Authentication::Password) .ConnectionField::default { label: $(i18n::text("remote-password", cx)); input: $(self.password.clone()); }, @end,
            @if (self.authentication == Authentication::Key)
                .ConnectionField::default { label: $(i18n::text("remote-auth-key", cx)); input: $(self.key.clone()); },
                .ConnectionField::default { label: $(i18n::text("remote-passphrase", cx)); input: $(self.passphrase.clone()); },
            @end,
        ] }.into_any_element()
    }
}
