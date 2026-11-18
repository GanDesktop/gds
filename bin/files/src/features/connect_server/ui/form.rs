use super::super::model::{DEFAULT_SFTP_PORT, DEFAULT_SMB_PORT, Details, Error, Request};
use anti_webui::prelude::*;
use gds::{
    i18n,
    service::remote::{Address, Credentials, Protocol},
    ui::{Icon, IconButton, field::Field},
};
use gds_uim::el;
use std::rc::Rc;

#[derive(Clone)]
pub(super) enum Authentication {
    Agent,
    Password(Signal<String>),
    Key {
        path: Signal<String>,
        passphrase: Signal<String>,
    },
    Guest,
}

impl Authentication {
    fn label(&self, protocol: Protocol) -> &'static str {
        match self {
            Self::Agent => "remote-auth-agent",
            Self::Password(_) if protocol == Protocol::Smb => "remote-auth-user",
            Self::Password(_) => "remote-password",
            Self::Key { .. } => "remote-auth-key",
            Self::Guest => "remote-auth-guest",
        }
    }
    fn same_method(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Clone)]
struct Fields {
    protocol: Protocol,
    host: Signal<String>,
    host_node: NodeRef,
    port: Signal<String>,
    path: Signal<String>,
    user: Signal<String>,
    authentication: Signal<Authentication>,
    methods: Vec<Authentication>,
}

#[derive(Clone)]
pub(super) struct Form {
    owner: Owner,
    fields: Signal<Option<Fields>>,
}

impl Fields {
    fn new(protocol: Protocol, address: Option<&Address>) -> Self {
        let user = address.map(Address::username).unwrap_or_default();
        let user = if user.is_empty() && protocol == Protocol::Sftp {
            std::env::var("USER").unwrap_or_default()
        } else {
            user
        };
        let password = Authentication::Password(signal(String::new()));
        let (authentication, methods) = match protocol {
            Protocol::Sftp => (
                Authentication::Agent,
                vec![
                    Authentication::Agent,
                    password,
                    Authentication::Key {
                        path: signal(String::new()),
                        passphrase: signal(String::new()),
                    },
                ],
            ),
            Protocol::Smb => (
                if user.eq_ignore_ascii_case("guest") {
                    Authentication::Guest
                } else {
                    password.clone()
                },
                vec![password, Authentication::Guest],
            ),
        };
        Self {
            protocol,
            host_node: node_ref(),
            host: signal(
                address
                    .map(|address| address.host().to_owned())
                    .unwrap_or_default(),
            ),
            port: signal(
                address
                    .map(Address::port)
                    .unwrap_or(match protocol {
                        Protocol::Sftp => DEFAULT_SFTP_PORT,
                        Protocol::Smb => DEFAULT_SMB_PORT,
                    })
                    .to_string(),
            ),
            path: signal(address.map(Address::path).unwrap_or_else(|| {
                if protocol == Protocol::Sftp {
                    "/".into()
                } else {
                    String::new()
                }
            })),
            user: signal(user),
            authentication: signal(authentication),
            methods,
        }
    }

    fn request(&self) -> Result<Request, Error> {
        let authentication = self.authentication.get_untracked();
        let user = if matches!(authentication, Authentication::Guest) {
            "guest".to_owned()
        } else {
            self.user.get_untracked()
        };
        if user.is_empty() {
            return Err(Error::User);
        }
        let address = Details {
            protocol: self.protocol,
            host: self.host.get_untracked(),
            port: self.port.get_untracked(),
            user,
            path: self.path.get_untracked(),
        }
        .address()?;
        let credentials = match authentication {
            Authentication::Agent | Authentication::Guest => Credentials::default(),
            Authentication::Password(password) => Credentials {
                password: password.get_untracked(),
                ..Default::default()
            },
            Authentication::Key { path, passphrase } => {
                let path = std::path::PathBuf::from(path.get_untracked());
                if !path.is_absolute() {
                    return Err(Error::Key);
                }
                let passphrase = passphrase.get_untracked();
                Credentials {
                    private_key: Some(path),
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

    fn focus(&self) {
        let host = self.host_node;
        let value = self.host.get_untracked();
        request_frame(move |_| {
            host.focus();
            host.set_text(&value, (0, value.len()));
        });
    }

    fn field(label: &'static str, input: TextInput, submit: Rc<dyn Fn()>) -> Field {
        el! { .Field::default.{min_w: 0.0_f32}.[
            $(i18n::text(label)).text.{
                text_size: 12.0_f32;
                text_color: $(theme().text_muted);
            },
            input.{w_full; min_w: 0.0_f32; on_submit: move _ => .submit},
        ] }
    }

    fn view(&self, choose_protocol: impl Fn() + 'static, submit: Rc<dyn Fn()>) -> Div {
        let field = move |label, input| Self::field(label, input, submit.clone());
        let auth = self.authentication;
        let protocol = self.protocol;
        let methods = self.methods.iter().cloned().map(move |method| {
            let selected = method.clone();
            let access = method.clone();
            el! { $(i18n::text(method.label(protocol))).button.{
                on_click: move _ => $(auth.set(method.clone()));
                access: move a => $(a.selected(auth.with(|current| current.same_method(&access))));
                dyn_style: move style => ${
                    if auth.with(|current| current.same_method(&selected)) {
                        style.bg(theme().selected).text_color(
                            gds::app::Gds::current().appearance().theme().colors().selection_foreground,
                        )
                    } else { style }
                };
            } }
        }).collect::<Vec<_>>();
        let user = self.user;
        let (protocol_label, path_label, user_label) = match protocol {
            Protocol::Sftp => ("SFTP", "remote-folder", "remote-username"),
            Protocol::Smb => ("SMB", "remote-share-path", "remote-smb-user"),
        };
        let host = self.host_node;
        el! { .div.{flex; flex_col; gap: 12.0_f32; w_full; min_w: 0.0_f32}.[
            .div.{flex; items_center; gap: 8.0_f32}.[
                "change-protocol".IconButton::new.{
                    icon: Icon::Back;
                    label: $(i18n::text("remote-change-protocol"));
                    on_click: move _ => .choose_protocol;
                },
                protocol_label.text.{font_weight: FontWeight::SEMIBOLD},
            ],
            .div.{flex; gap: 12.0_f32; w_full; min_w: 0.0_f32}.[
                .div.{flex_1; min_w: 0.0_f32}.[
                    $(field("remote-host", el! { $(self.host).text_field.{
                        node_ref: host; placeholder: "server.example.com";
                    } })),
                ],
                .div.{w: 100.0_f32; flex_none}.[
                    $(field("remote-port", el! { $(self.port).text_field })),
                ],
            ],
            $(field(path_label, el! { $(self.path).text_field })),
            .div.{flex; flex_col; gap: 4.0_f32}.[
                $(i18n::text("remote-authentication")).text.{text_size: 12.0_f32; text_color: $(theme().text_muted)},
                .div.{flex; flex_wrap; gap: 6.0_f32}.[..methods],
            ],
            $(move || match auth.get() {
                Authentication::Agent => el! { .div.[
                    $(field(user_label, el! { user.text_field })),
                ] },
                Authentication::Password(password) => el! { .div.{flex; flex_col; gap: 12.0_f32}.[
                    $(field(user_label, el! { user.text_field })),
                    $(field("remote-password", el! { password.text_field.{purpose: ImePurpose::Password} })),
                ] },
                Authentication::Key {path, passphrase} => el! { .div.{flex; flex_col; gap: 12.0_f32}.[
                    $(field(user_label, el! { user.text_field })),
                    $(field("remote-auth-key", el! { path.text_field.{placeholder: "/home/user/.ssh/id_ed25519"} })),
                    $(field("remote-passphrase", el! { passphrase.text_field.{purpose: ImePurpose::Password} })),
                ] },
                Authentication::Guest => div(),
            }).dyn_view,
        ] }
    }
}

impl Form {
    pub fn new(address: Option<Address>) -> Self {
        let owner = Owner::current().expect("connection form belongs to a window");
        Self {
            owner,
            fields: signal(
                address
                    .as_ref()
                    .map(|address| Fields::new(address.protocol(), Some(address))),
            ),
        }
    }
    pub fn focus(&self) {
        if let Some(fields) = self.fields.get_untracked() {
            fields.focus();
        }
    }
    pub fn ready(&self) -> bool {
        self.fields.with(Option::is_some)
    }
    pub fn request(&self) -> Result<Request, Error> {
        self.fields
            .get_untracked()
            .ok_or(Error::Protocol)?
            .request()
    }
    pub fn view(&self, submit: impl Fn() + 'static) -> impl IntoView {
        let submit: Rc<dyn Fn()> = Rc::new(submit);
        let fields = self.fields;
        let owner = self.owner;
        dyn_view(move || match fields.get() {
            Some(current) => current.view(move || fields.set(None), submit.clone()),
            None => {
                let options = [
                    (Protocol::Sftp, "SFTP", "remote-sftp-description"),
                    (Protocol::Smb, "SMB", "remote-smb-description"),
                ]
                .into_iter()
                .map(|(protocol, label, description)| {
                    el! { "".quiet_button.{
                        gap: 0.0_f32;
                        w_full; h: Length::Auto; p: 12.0_f32; justify_start;
                        access: move a => $(a.label(label));
                        on_click: move _ => ${
                            owner.with(|| {
                                let current = Fields::new(protocol, None);
                                current.focus();
                                fields.set(Some(current));
                            });
                        };
                    }.[
                        .div.{flex; items_center; gap: 12.0_f32; w_full; min_w: 0.0_f32}.[
                            .div.{flex; flex_col; flex_1; min_w: 0.0_f32; gap: 4.0_f32; text_align: TextAlign::Start}.[
                                label.text.{font_weight: FontWeight::SEMIBOLD},
                                $(i18n::text(description)).text.{text_size: 12.0_f32; text_color: $(theme().text_muted)},
                            ],
                            $(Icon::ChevronRight.view(gds::appearance::tokens::ICON_SIZE)),
                        ],
                    ] }

                })
                .collect::<Vec<_>>();
                el! { .div.{flex; flex_col; gap: 8.0_f32; w_full; min_w: 0.0_f32}.[
                    $(i18n::text("remote-choose-protocol")).text.{text_color: $(theme().text_muted)},
                    ..options,
                ] }
            }
        })
    }
}
