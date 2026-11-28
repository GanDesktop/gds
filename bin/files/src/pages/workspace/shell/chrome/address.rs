//! Editable address bar and scrollable breadcrumb trail.

use std::cell::Cell;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use gds::i18n;
use gds::ui::button::{ButtonSize, IconButtonProps};
use gds::ui::{IconName, IconProps};
use zgui::geom::{DevicePx, Point};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, on_cleanup_local};
use zgui::view::{
    Scoped, ScrollBehavior, ScrollTarget, events, install_stylesheet as install_sheet,
};
use zgui::vocab::{Key, Modifiers, NamedKey, Role, ScrollDelta};
use zgui::{css, view};
use zgui_ui::InputProps;
use zgui_ui_primitives::Binding;

use super::{Chrome, install_chrome_stylesheets, track_language};
use crate::entities::location::{Collection, Location, scheme::TagAddress};
use crate::pages::workspace::{
    navigation::Navigator,
    projection::{Rows, UiState},
    state::{context::local_memo, model::AddressPresentation},
};
use crate::shared::ui::actions::Command;

const ADDRESS_SHEET: &str = "gds-files-chrome-address";

const ADDRESS_CSS: &str = css!(
    r#"
    .gds-files-address-bar.gds-files-address-bar {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 96px;
        height: var(--gds-icon-button-size, 32px);
        margin: 0px 6px;
        border-radius: var(--gds-radius-control, 6px);
        background: transparent;
        background-color: transparent;
        border: 1px solid var(--gds-border);
        box-sizing: border-box;
        overflow: hidden;
        transition: border-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-address-bar.gds-files-address-bar[data-editing="true"] {
        border-color: var(--gds-primary);
    }

    .gds-files-address-bar .zui-field.gds-files-address-input {
        flex: 1 1 0%;
        width: auto;
        min-width: 0px;
        height: 100%;
        padding: 0px 8px;
        border: 0px solid transparent;
        border-radius: 0px;
        background: transparent;
        background-color: transparent;
        color: var(--gds-foreground);
        font-family: var(--gds-font-sans);
        font-size: var(--gds-text-sm, 14px);
        box-shadow: none;
        outline: none;
    }

    .gds-files-address-bar .zui-field.gds-files-address-input:focus,
    .gds-files-address-bar .zui-field.gds-files-address-input:focus-visible {
        border-color: transparent;
        box-shadow: none;
        outline: none;
    }

    .gds-files-address-loading.gds-files-address-loading {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 0px;
        height: 100%;
        padding: 0px 6px;
    }

    .gds-files-address-loading-text.gds-files-address-loading-text {
        width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--gds-foreground);
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-files-address-read.gds-files-address-read {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 0px;
        height: 100%;
        padding-right: 2px;
    }

    .zui-button.gds-icon-button.gds-files-address-edit {
        width: 26px;
        height: 26px;
        min-width: 26px;
        min-height: 26px;
        padding: 0px;
        flex-shrink: 0;
    }

    .gds-files-breadcrumbs.gds-files-breadcrumbs {
        flex: 1 1 0%;
        min-width: 0px;
        height: 100%;
        overflow-x: hidden;
        overflow-y: hidden;
    }

    .gds-files-breadcrumbs-track.gds-files-breadcrumbs-track {
        display: flex;
        align-items: center;
        height: 100%;
        gap: 2px;
        padding: 0px 4px;
    }

    .gds-files-crumb.gds-files-crumb {
        display: inline-flex;
        flex: none;
        align-items: center;
        gap: 4px;
        height: 24px;
        padding: 0px 4px;
        border-radius: 4px;
        cursor: pointer;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-text-sm, 14px);
        user-select: none;
    }

    .gds-files-crumb.gds-files-crumb[data-current="true"] {
        color: var(--gds-foreground);
        font-weight: 500;
    }

    .gds-files-crumb.gds-files-crumb:hover {
        background-color: var(--gds-hover);
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"] {
        height: 22px;
        padding: 0px 7px;
        border-radius: 5px;
        background-color: color-mix(in srgb, var(--gds-primary) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--gds-primary) 24%, transparent);
        color: var(--gds-primary);
        font-size: var(--gds-text-xs, 12px);
        font-weight: 500;
        box-sizing: border-box;
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"][data-interactive="true"]:hover {
        background-color: color-mix(in srgb, var(--gds-primary) 18%, transparent);
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"][data-interactive="false"] {
        cursor: default;
    }

    .gds-files-crumb.gds-files-crumb[data-scheme="true"][data-interactive="false"]:hover {
        background-color: color-mix(in srgb, var(--gds-primary) 12%, transparent);
    }

    .gds-files-crumb-icon {
        flex: none;
        color: currentColor;
    }

    .gds-files-crumb.gds-files-crumb:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-files-crumb.gds-files-crumb:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-crumb-sep {
        flex: none;
        color: color-mix(in srgb, var(--gds-muted-foreground) 70%, transparent);
    }
    "#
);

pub(super) fn install_stylesheet() {
    install_sheet(ADDRESS_SHEET, ADDRESS_CSS);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Breadcrumb {
    pub label: String,
    pub location: Option<Location>,
    pub icon: Option<IconName>,
    pub scheme: bool,
    pub current: bool,
    pub separated: bool,
}

impl Breadcrumb {
    pub(super) fn for_location(location: &Location, path: Option<&Path>) -> Vec<Self> {
        if let Some(path) = path {
            let components = path.components().collect::<Vec<_>>();
            let last_visible = components
                .iter()
                .rposition(|part| !matches!(part, Component::CurDir));
            let mut target = PathBuf::new();
            let mut segments = Vec::new();
            for (index, part) in components.iter().enumerate() {
                target.push(part.as_os_str());
                if matches!(part, Component::CurDir) {
                    continue;
                }
                let current = last_visible == Some(index);
                let name = part.as_os_str().to_string_lossy();
                let label = match part {
                    Component::RootDir | Component::Prefix(_) => name.into_owned(),
                    _ if current => name.into_owned(),
                    _ => format!("{name}{}", std::path::MAIN_SEPARATOR),
                };
                segments.push(Self {
                    label,
                    location: Some(Location::from(target.as_path())),
                    icon: None,
                    scheme: false,
                    current,
                    separated: false,
                });
            }
            if segments.is_empty() {
                segments.push(Self {
                    label: path.display().to_string(),
                    location: Some(Location::from(path)),
                    icon: None,
                    scheme: false,
                    current: true,
                    separated: false,
                });
            }
            return segments;
        }
        match location {
            Location::Tags(address) => Self::for_tags(address),
            Location::Remote(address) => Self::for_remote(address),
            Location::Recent | Location::Network | Location::Trash => vec![Self {
                label: location.label(),
                location: Some(location.clone()),
                icon: Some(location.icon()),
                scheme: true,
                current: true,
                separated: false,
            }],
            Location::Favorites | Location::Volumes => vec![
                Self::scheme_prefix(i18n::text_untracked("scheme-location"), IconName::Bookmark),
                Self {
                    label: location.label(),
                    location: Some(location.clone()),
                    icon: None,
                    scheme: false,
                    current: true,
                    separated: true,
                },
            ],
            Location::Volume { identity, relative } => {
                let mut segments = vec![
                    Self::scheme_prefix(
                        i18n::text_untracked("scheme-location"),
                        IconName::Bookmark,
                    ),
                    Self {
                        label: i18n::text_untracked(Collection::Volumes.label_id()),
                        location: Some(Location::Volumes),
                        icon: None,
                        scheme: false,
                        current: relative.is_empty(),
                        separated: true,
                    },
                ];
                if !relative.is_empty() {
                    let parts: Vec<&std::ffi::OsStr> = Path::new(relative)
                        .components()
                        .filter_map(|part| match part {
                            Component::Normal(seg) => Some(seg),
                            _ => None,
                        })
                        .collect();
                    let total = parts.len();
                    let mut acc = PathBuf::new();
                    for (idx, part) in parts.into_iter().enumerate() {
                        acc.push(part);
                        segments.push(Self {
                            label: part.to_string_lossy().into_owned(),
                            location: Some(Location::Volume {
                                identity: identity.clone(),
                                relative: acc.as_os_str().to_owned(),
                            }),
                            icon: None,
                            scheme: false,
                            current: idx + 1 == total,
                            separated: true,
                        });
                    }
                }
                segments
            }
            Location::Directory(_) => vec![Self {
                label: location.display_address(),
                location: Some(location.clone()),
                icon: None,
                scheme: false,
                current: true,
                separated: false,
            }],
        }
    }

    fn scheme_prefix(label: String, icon: IconName) -> Self {
        Self {
            label,
            location: None,
            icon: Some(icon),
            scheme: true,
            current: false,
            separated: false,
        }
    }

    fn for_tags(address: &TagAddress) -> Vec<Self> {
        let mut chain = vec![address.clone()];
        let mut cursor = address.clone();
        while let Some(parent) = cursor.parent() {
            chain.push(parent.clone());
            cursor = parent;
        }
        chain.reverse();
        let total = chain.len();
        chain
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let is_root = index == 0;
                let loc = Location::Tags(item);
                Self {
                    label: if is_root {
                        i18n::text_untracked("scheme-tags")
                    } else {
                        loc.label()
                    },
                    location: Some(loc),
                    icon: is_root.then_some(IconName::Tag),
                    scheme: is_root,
                    current: index + 1 == total,
                    separated: index > 0,
                }
            })
            .collect()
    }

    fn for_remote(address: &gds::service::remote::Address) -> Vec<Self> {
        let (scheme_key, scheme_icon) = match address.protocol() {
            gds::service::remote::Protocol::Sftp => ("scheme-sftp", IconName::Server),
            gds::service::remote::Protocol::Smb => ("scheme-smb", IconName::Network),
        };
        let mut chain = vec![address.clone()];
        let mut cursor = address.clone();
        while let Some(parent) = cursor.parent() {
            chain.push(parent.clone());
            cursor = parent;
        }
        chain.reverse();
        let total = chain.len();
        let mut segments = Vec::with_capacity(total + 1);
        segments.push(Self::scheme_prefix(
            i18n::text_untracked(scheme_key),
            scheme_icon,
        ));
        for (index, item) in chain.into_iter().enumerate() {
            segments.push(Self {
                label: item.name(),
                location: Some(Location::Remote(item)),
                icon: None,
                scheme: false,
                current: index + 1 == total,
                separated: true,
            });
        }
        segments
    }
}

fn pending_destination_label(location: &Location, path: Option<&Path>) -> String {
    match location {
        Location::Remote(address) => address.to_string(),
        Location::Directory(dir) => Path::new(dir).display().to_string(),
        _ => path.map_or_else(|| location.label(), |p| p.display().to_string()),
    }
}

impl Chrome<'_> {
    pub(super) fn address_bar(&self) -> AnyView {
        install_chrome_stylesheets();
        let editing = self.0.0.address_presentation;
        let is_editing = Signal::derive_local(move || {
            matches!(editing.get(), AddressPresentation::Editing { .. })
        });
        let destination = local_memo({
            let ctx = self.0.clone();
            move || {
                ctx.read(|state| {
                    let navigation = &state.tab().navigation;
                    if let Some(pending) = navigation.pending_location() {
                        let resolved = navigation
                            .pending_path()
                            .map(Path::to_owned)
                            .or_else(|| Rows(&ctx).resolve_path(pending));
                        (pending.clone(), resolved, true)
                    } else {
                        (
                            navigation.location().clone(),
                            navigation.path().map(Path::to_owned),
                            false,
                        )
                    }
                })
            }
        });

        let edit_click = self.0.clone();
        let blur = self.0.clone();
        let submit = self.0.clone();
        let keys = self.0.clone();
        let crumbs_ctx = self.0.clone();
        let edit_btn = self.0.clone();
        let composing = Rc::new(Cell::new(false));
        let composing_start = Rc::clone(&composing);
        let composing_preedit = Rc::clone(&composing);
        let composing_commit = Rc::clone(&composing);
        let composing_end = Rc::clone(&composing);
        let address_sig = self.0.0.address;
        let address_node = self.0.0.address_node;

        AnyView::new(view! {
            row(
                class = "gds-files-address-bar",
                attr:data-editing = move || is_editing.get().then(|| "true".to_owned()),
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    if matches!(editing.get_untracked(), AddressPresentation::Editing { .. }) {
                        ev.stop_propagation();
                    }
                },
                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                    if !matches!(editing.get_untracked(), AddressPresentation::Editing { .. }) {
                        ev.stop_propagation();
                        UiState(&edit_click).start_address_edit();
                    }
                }
            ) {
                {{move || {
                    if is_editing.get() {
                        let composing_key = Rc::clone(&composing);
                        let submit_key = submit.clone();
                        let cancel_key = keys.clone();
                        let blur_out = blur.clone();
                        let cs = Rc::clone(&composing_start);
                        let cp = Rc::clone(&composing_preedit);
                        let cc = Rc::clone(&composing_commit);
                        let ce = Rc::clone(&composing_end);
                        AnyView::new(Scoped::new(move || {
                            let focus_effect =
                                RenderEffect::new(move |mounted_before: Option<bool>| {
                                    let mounted = address_node.get().is_some();
                                    if mounted && mounted_before != Some(true) {
                                        let current = address_sig.get_untracked();
                                        address_node.set_value(&current);
                                        address_node.focus();
                                        address_node.select_all();
                                    }
                                    mounted
                                });
                            on_cleanup_local(move || drop(focus_effect));
                            view! {
                                Input(
                                    node_ref = address_node,
                                    value = Binding::from(address_sig),
                                    placeholder = i18n::text("path-placeholder"),
                                    label = i18n::text("place-location"),
                                    class = "gds-files-address-input",
                                    on:ime_start = move |_| cs.set(true),
                                    on:ime_preedit = move |ev: &mut EventCx<'_, events::ImePreedit>| cp.set(ev.is_composing()),
                                    on:ime_commit = move |_| cc.set(false),
                                    on:ime_end = move |_| ce.set(false),
                                    on:focus_out = move |_| UiState(&blur_out).finish_address_edit(),
                                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                        if composing_key.get() {
                                            return;
                                        }
                                        if ev.key == Key::Named(NamedKey::Enter) && ev.modifiers == Modifiers::NONE {
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                            Navigator(&submit_key).open_address();
                                        } else if ev.key == Key::Named(NamedKey::Escape) && ev.modifiers == Modifiers::NONE {
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                            UiState(&cancel_key).finish_address_edit();
                                            cancel_key.focus_content_after_mount();
                                        }
                                    }
                                )
                            }
                        }))
                    } else {
                        track_language();
                        let (location, path, is_pending) = destination.get();
                        let c_click = edit_btn.clone();
                        let body = if is_pending {
                            let label = pending_destination_label(&location, path.as_deref());
                            let a11y_label = label.clone();
                            let edit_pending = edit_btn.clone();
                            let edit_key = edit_btn.clone();
                            AnyView::new(view! {
                                row(class = "gds-files-address-loading") {
                                    box(
                                        class = "gds-files-crumb",
                                        attr:data-current = "true",
                                        tabindex = Focus::Sequential,
                                        a11y:role = Role::Button,
                                        a11y:label = a11y_label,
                                        on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                            ev.stop_propagation();
                                            UiState(&edit_pending).start_address_edit();
                                        },
                                        on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                            if ev.modifiers == Modifiers::NONE
                                                && (ev.key == Key::Named(NamedKey::Enter)
                                                    || ev.key == Key::Named(NamedKey::Space)
                                                    || ev.key == Key::character(" "))
                                            {
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                                UiState(&edit_key).start_address_edit();
                                            }
                                        }
                                    ) {
                                        text(class = "gds-files-address-loading-text") {{label}}
                                    }
                                }
                            })
                        } else {
                            Chrome(&crumbs_ctx).breadcrumbs(&location, path.as_deref())
                        };
                        AnyView::new(view! {
                            row(class = "gds-files-address-read") {
                                {body}
                                IconButton(
                                    icon = IconName::Pencil,
                                    label = Signal::derive_local(|| i18n::text("path-edit")),
                                    size = ButtonSize::IconSm,
                                    shortcut = Some(Command::EditLocation.shortcut()),
                                    class = "gds-files-address-edit",
                                    on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                        ev.stop_propagation();
                                        UiState(&c_click).start_address_edit();
                                    }
                                )
                            }
                        })
                    }
                }}}
            }
        })
    }

    fn breadcrumbs(&self, location: &Location, path: Option<&Path>) -> AnyView {
        let ctx = self.0.clone();
        let segments = Breadcrumb::for_location(location, path);
        AnyView::new(Scoped::new(move || {
            let scroller = NodeRef::new();
            let size_obs = scroller.observe_border_size();
            let scroll_fx = RenderEffect::new(move |_| {
                let _ = size_obs.get();
                scroller.scroll_to(
                    ScrollTarget::Offset(Point::new(DevicePx(f32::MAX), DevicePx(0.0))),
                    ScrollBehavior::Instant,
                );
            });
            on_cleanup_local(move || drop(scroll_fx));

            let crumbs = segments
                .into_iter()
                .flat_map(|crumb| {
                    let mut items = Vec::with_capacity(2);
                    if crumb.separated {
                        items.push(AnyView::new(view! {
                            Icon(
                                name = IconName::ChevronRight,
                                size = 12.0,
                                class = "gds-files-crumb-sep",
                                a11y:hidden = true
                            )
                        }));
                    }
                    let is_current = crumb.current;
                    let is_scheme = crumb.scheme;
                    let icon_view = crumb.icon.map(|icon_name| {
                        AnyView::new(view! {
                            Icon(
                                name = icon_name,
                                size = 13.0,
                                class = "gds-files-crumb-icon",
                                a11y:hidden = true
                            )
                        })
                    });
                    let label = crumb.label;
                    if let Some(target_loc) = crumb.location {
                        let click_ctx = ctx.clone();
                        let key_ctx = ctx.clone();
                        let key_loc = target_loc.clone();
                        let a11y_label = label.clone();
                        let crumb_ref = NodeRef::new();
                        items.push(AnyView::new(view! {
                            box(
                                node_ref = crumb_ref,
                                class = "gds-files-crumb",
                                attr:data-scheme = is_scheme.then(|| "true".to_owned()),
                                attr:data-interactive = is_scheme.then(|| "true".to_owned()),
                                attr:data-current = is_current.then(|| "true".to_owned()),
                                tabindex = Focus::Sequential,
                                a11y:role = Role::Button,
                                a11y:label = a11y_label,
                                on:focus_in = move |_| {
                                    let (Some(port), Some(item)) =
                                        (scroller.window_bounds(), crumb_ref.window_bounds())
                                    else {
                                        return;
                                    };
                                    let port_left = port.origin.x.0;
                                    let port_right = port_left + port.size.width.0;
                                    let item_left = item.origin.x.0;
                                    let item_right = item_left + item.size.width.0;
                                    let dx = if item_left < port_left {
                                        item_left - port_left
                                    } else if item_right > port_right {
                                        item_right - port_right
                                    } else {
                                        0.0
                                    };
                                    if dx.abs() > 0.5 {
                                        scroller.scroll_to(
                                            ScrollTarget::By(Point::new(DevicePx(dx), DevicePx(0.0))),
                                            ScrollBehavior::Instant,
                                        );
                                    }
                                },
                                on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                    ev.stop_propagation();
                                    if is_current {
                                        UiState(&click_ctx).start_address_edit();
                                    } else if ev.modifiers.primary() {
                                        Navigator(&click_ctx).new_tab(Some(target_loc.clone()));
                                    } else {
                                        Navigator(&click_ctx).open(target_loc.clone());
                                        click_ctx.focus_content_after_mount();
                                    }
                                },
                                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                    let is_activate = ev.key == Key::Named(NamedKey::Enter)
                                        || ev.key == Key::Named(NamedKey::Space)
                                        || ev.key == Key::character(" ");
                                    if !is_activate {
                                        return;
                                    }
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                    if is_current {
                                        UiState(&key_ctx).start_address_edit();
                                    } else if ev.modifiers.primary() {
                                        Navigator(&key_ctx).new_tab(Some(key_loc.clone()));
                                    } else {
                                        Navigator(&key_ctx).open(key_loc.clone());
                                        key_ctx.focus_content_after_mount();
                                    }
                                }
                            ) {
                                {icon_view}
                                text(class = "gds-files-crumb-text") {{label}}
                            }
                        }));
                    } else {
                        items.push(AnyView::new(view! {
                            box(
                                class = "gds-files-crumb",
                                attr:data-scheme = is_scheme.then(|| "true".to_owned()),
                                attr:data-interactive = is_scheme.then(|| "false".to_owned()),
                            ) {
                                {icon_view}
                                text(class = "gds-files-crumb-text") {{label}}
                            }
                        }));
                    }
                    items
                })
                .collect::<Vec<_>>();

            let handle = request_frame(move |_| {
                scroller.scroll_to(
                    ScrollTarget::Offset(Point::new(DevicePx(f32::MAX), DevicePx(0.0))),
                    ScrollBehavior::Instant,
                );
            });
            on_cleanup_local(move || drop(handle));

            view! {
                scroll(
                    node_ref = scroller,
                    class = "gds-files-breadcrumbs",
                    on:wheel = move |ev: &mut EventCx<'_, events::Wheel>| {
                        let scale = scroller.scale().max(1.0);
                        let dx = match ev.delta {
                            ScrollDelta::Lines { x, y } => {
                                let lines = if x.abs() > 0.001 { x } else { y };
                                lines * 28.0 * scale
                            }
                            ScrollDelta::Pixels(pixels) => {
                                if pixels.width.0.abs() > 0.001 {
                                    pixels.width.0
                                } else {
                                    pixels.height.0
                                }
                            }
                            _ => 0.0,
                        };
                        if dx.abs() > 0.001 {
                            scroller.scroll_to(
                                ScrollTarget::By(Point::new(DevicePx(dx), DevicePx(0.0))),
                                ScrollBehavior::Instant,
                            );
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    },
                ) {
                    row(class = "gds-files-breadcrumbs-track") {
                        {crumbs}
                    }
                }
            }
        }))
    }
}
