//! Shared, user-facing transport descriptions, independent of saved names and notes.

use crate::shared::i18n::tr;
use bas::service::Target;
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme, Icon, menu::PopupMenuItem},
    prelude::*,
    *,
};

#[cfg(test)]
mod tests;

pub(crate) fn label(target: &Target) -> &'static str {
    match target {
        Target::Local => tr!("connections-local"),
        Target::Tcp(_) => "TCP",
        Target::Mtls(_) => "mTLS",
        Target::Unix(_) => "Unix socket",
        Target::Pipe(_) => tr!("connections-pipe"),
    }
}

pub(crate) fn icon(target: &Target) -> Icon {
    match target {
        Target::Local => Icon::new(IconName::Monitor),
        Target::Tcp(_) => Icon::new(IconName::Server),
        Target::Mtls(_) => secure_icon(),
        Target::Unix(_) | Target::Pipe(_) => Icon::new(IconName::Cable),
    }
}

pub(crate) fn secure_icon() -> Icon {
    Icon::new(IconName::Server).path(crate::shared::ui::assets::SECURE_CONNECTION)
}

pub(crate) fn endpoint(target: &Target) -> String {
    match target {
        Target::Local => local_endpoint(target.resolved()),
        Target::Tcp(address) | Target::Mtls(address) => address.display_compact(),
        Target::Unix(path) => path.display().to_string(),
        Target::Pipe(name) => name.clone(),
    }
}

fn local_endpoint(resolved: std::io::Result<Target>) -> String {
    match resolved {
        Ok(Target::Unix(path)) => path.display().to_string(),
        Ok(Target::Pipe(name)) => name,
        _ => tr!("connections-local-endpoint").into(),
    }
}

pub(crate) fn summary(target: &Target) -> String {
    format!("{} · {}", label(target), endpoint(target))
}

pub(crate) fn tooltip(target: &Target) -> String {
    match target {
        Target::Tcp(_) => format!("{} · {}", summary(target), tr!("connections-tcp-security")),
        Target::Mtls(_) => format!("{} · {}", summary(target), tr!("connections-mtls-security")),
        _ => summary(target),
    }
}

pub(crate) fn menu_item(profile: &crate::shared::config::ConnectionProfile) -> PopupMenuItem {
    let name = profile.name.clone();
    let address = endpoint(&profile.target);
    let hint = format!("{name} · {}", tooltip(&profile.target));
    let id = format!("connection-menu-{}", profile.id);
    PopupMenuItem::element(move |_, cx| {
        let hint = hint.clone();
        el! {
            .div {
                # $(id.clone());
                test_support;
                role: Role::Label;
                aria_label: $(hint.clone());
                flex_1;
                flex;
                flex_col;
                gap_1;
                py_1;
                min_w_0;
                w_0;
                overflow_hidden;
                hoverable_tooltip: $(move |window, cx| crate::shared::ui::tooltip::view(hint.clone(), window, cx));
            } [
                .div {
                    # format!("{id}-name");
                    test_support;
                    role: Role::Label;
                    aria_label: $(name.clone());
                    w_full;
                    min_w_0;
                    truncate;
                    text_size: crate::shared::ui::SMALL;
                } [$(name.clone())],
                .div {
                    # format!("{id}-endpoint");
                    test_support;
                    role: Role::Label;
                    aria_label: $(address.clone());
                    w_full;
                    min_w_0;
                    truncate;
                    text_size: crate::shared::ui::SMALL;
                    text_color: $(cx.theme().muted_foreground);
                } [
                    $(address.clone()),
                ],
            ]
        }
    })
    .icon(icon(&profile.target).self_start().mt(rems(0.5)))
}
