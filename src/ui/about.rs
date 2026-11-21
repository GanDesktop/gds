//! Shared About content, independent of its dialog, page, or window host.

mod metadata;
#[cfg(test)]
mod tests;

pub use metadata::{APPLICATIONS, Application, Credit};

use std::cell::Cell;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal, UnsyncCallback, on_cleanup_local};
use zgui::view::{Either, ListenerOptions, PropValue, events, install_stylesheet};
use zgui::vocab::{Key, Modifiers, NamedKey, Role};
use zgui::{component, css, view};
use zgui_ui::ButtonSize;
use zgui_ui::overlay::OverlayState;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::prelude::{DismissReason, DismissableLayerProps};

use crate::app::Gds;
use crate::ui::button::IconButtonProps;
use crate::ui::dialog::{DialogAnchor, ModalDialogProps};
use crate::ui::icon::{IconName, IconProps};
use crate::ui::shortcut::Shortcut;
use crate::ui::tooltip::HintTooltipProps;

pub const WIDTH: f32 = 420.;
pub const HEIGHT: f32 = 408.;

const COPIED_RESET_DURATION: Duration = Duration::from_millis(1800);
const ABOUT_SHEET: &str = "gds-ui-about";

const ABOUT_CSS: &str = css!(
    r#"
    .gds-about {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        padding: 0px;
        gap: 0px;
        outline: none;
        color: var(--gds-foreground);
        font-family: var(--gds-font-sans);
        font-size: var(--gds-font-size-small, 14px);
    }

    .gds-about__overview-scroll {
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow-y: auto;
        overflow-x: hidden;
    }

    .gds-about__overview {
        display: flex;
        flex-direction: column;
        width: 100%;
        max-width: 480px;
        margin: 0px auto;
        min-width: 0px;
        padding: 22px 20px 20px 20px;
        box-sizing: border-box;
        gap: 12px;
    }

    .gds-about__hero {
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: 6px;
        padding-bottom: 8px;
    }

    .gds-about__desktop-icon {
        width: 76px;
        height: 76px;
        flex-shrink: 0;
        object-fit: contain;
    }

    .gds-about__emblem {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 64px;
        height: 64px;
        margin-bottom: 4px;
        flex-shrink: 0;
        border-radius: 16px;
        background-color: var(--gds-primary-container);
        color: var(--gds-on-primary-container);
        border: 1px solid color-mix(in srgb, var(--gds-primary) 26%, var(--gds-border));
        box-shadow:
            0 6px 16px -6px color-mix(in srgb, var(--gds-shadow) 28%, transparent),
            inset 0 1px 0 0 color-mix(in srgb, var(--gds-on-primary-container) 12%, transparent);
    }

    .gds-about__emblem-svg {
        width: 34px;
        height: 34px;
        flex-shrink: 0;
        color: inherit;
    }

    .gds-about__title {
        font-size: calc(20px * var(--gds-font-scale, 1));
        font-weight: 600;
        line-height: 1.25;
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 100%;
    }

    .gds-about__group {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
        border-radius: 8px;
        border: 1px solid var(--gds-border);
        background-color: var(--gds-control);
        color: var(--gds-control-foreground);
        overflow: hidden;
    }

    .gds-about__group > *:not(:first-child) {
        border-top: 1px solid var(--gds-border);
    }

    .gds-about__info-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        min-height: 42px;
        padding: 5px 6px 5px 14px;
        gap: 12px;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-about__info-label {
        flex-shrink: 0;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-font-size-small, 14px);
        line-height: 1.35;
    }

    .gds-about__info-value-wrap {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: flex-end;
        flex: 1 1 0%;
        min-width: 0px;
        gap: 4px;
    }

    .gds-about__mono-value {
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        text-align: right;
        font-family: var(--gds-font-mono);
        font-size: calc(13px * var(--gds-font-scale, 1));
        font-variant-numeric: tabular-nums;
        line-height: 1.35;
        color: var(--gds-control-foreground);
        user-select: text;
    }

    .zui-button.gds-icon-button.gds-about__copy-btn {
        color: var(--gds-muted-foreground);
    }

    .zui-button.gds-icon-button.gds-about__copy-btn:hover {
        color: var(--gds-control-foreground);
    }

    .zui-button.gds-icon-button.gds-about__copy-btn[data-copied="true"],
    .zui-button.gds-icon-button.gds-about__copy-btn[data-copied="true"]:hover {
        color: var(--gds-success);
    }

    .gds-about__links-grid {
        display: flex;
        flex-direction: row;
        align-items: stretch;
        width: 100%;
        min-width: 0px;
        gap: 8px;
    }

    .gds-about__link-card {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0%;
        min-width: 0px;
        min-height: 40px;
        padding: 0px 14px;
        gap: 8px;
        border-radius: 8px;
        border: 1px solid var(--gds-border);
        background-color: var(--gds-control);
        color: var(--gds-control-foreground);
        cursor: pointer;
        box-sizing: border-box;
        transition: background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
    }

    .gds-about__link-card:hover {
        background-color: var(--gds-control-hover);
    }

    .gds-about__link-card:active {
        background-color: var(--gds-control-pressed);
    }

    .gds-about__link-card:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-about__link-card:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-about__nav-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        width: 100%;
        min-width: 0px;
        min-height: 42px;
        padding: 0px 13px 0px 14px;
        gap: 10px;
        background-color: transparent;
        color: var(--gds-control-foreground);
        cursor: pointer;
        box-sizing: border-box;
        transition: background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
    }

    .gds-about__nav-row:hover {
        background-color: var(--gds-control-hover);
    }

    .gds-about__nav-row:active {
        background-color: var(--gds-control-pressed);
    }

    .gds-about__nav-row:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-about__nav-row:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-about__row-icon {
        flex-shrink: 0;
        color: var(--gds-muted-foreground);
    }

    .gds-about__row-label {
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-font-size-small, 14px);
        font-weight: 500;
        line-height: 1.35;
        color: var(--gds-control-foreground);
    }

    .gds-about__row-meta {
        flex-shrink: 0;
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
        white-space: nowrap;
    }

    .gds-about__row-trailing {
        flex-shrink: 0;
        color: var(--gds-muted-foreground);
        transition: color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
    }

    .gds-about__link-card:hover .gds-about__row-trailing,
    .gds-about__nav-row:hover .gds-about__row-trailing,
    .gds-about__credit-row:hover .gds-about__row-trailing {
        color: var(--gds-control-foreground);
    }

    .gds-about__details-wrap {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-about__subheader {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        height: var(--gds-toolbar-height, 52px);
        padding: 0px 48px 0px 10px;
        gap: 8px;
        min-width: 0px;
        border-bottom: 1px solid var(--gds-border);
        box-sizing: border-box;
    }

    .gds-about__subtitle {
        flex: 1 1 0%;
        min-width: 0px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-font-size-md, 16px);
        font-weight: 600;
        line-height: 1.25;
        color: var(--gds-foreground);
    }

    .gds-about__details {
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow-y: auto;
        overflow-x: hidden;
    }

    .gds-about__details-inner {
        display: flex;
        flex-direction: column;
        width: 100%;
        max-width: 480px;
        margin: 0px auto;
        min-width: 0px;
        padding: 16px 20px 20px 20px;
        gap: 16px;
        box-sizing: border-box;
    }

    .gds-about__details-inner--license {
        padding: 16px 8px 20px 20px;
    }

    .gds-about__section {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0px;
        gap: 6px;
    }

    .gds-about__section-label {
        padding-left: 4px;
        font-size: var(--gds-font-size-xs, 12px);
        font-weight: 500;
        line-height: 1.3;
        color: var(--gds-muted-foreground);
    }

    .gds-about__author-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        min-height: 40px;
        padding: 8px 14px;
        gap: 10px;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-about__author-name {
        flex: 1 1 0%;
        min-width: 0px;
        font-size: var(--gds-font-size-small, 14px);
        color: var(--gds-control-foreground);
        overflow-wrap: anywhere;
        user-select: text;
    }

    .gds-about__credit-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        width: 100%;
        min-width: 0px;
        min-height: 46px;
        padding: 9px 14px;
        gap: 10px;
        background-color: transparent;
        color: var(--gds-control-foreground);
        cursor: pointer;
        box-sizing: border-box;
        transition: background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
    }

    .gds-about__credit-row:hover {
        background-color: var(--gds-control-hover);
    }

    .gds-about__credit-row:active {
        background-color: var(--gds-control-pressed);
    }

    .gds-about__credit-row:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-about__credit-row:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-about__credit-static {
        display: flex;
        flex-direction: column;
        justify-content: center;
        min-height: 40px;
        padding: 8px 14px;
        min-width: 0px;
        box-sizing: border-box;
    }

    .gds-about__credit-copy {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        min-width: 0px;
        gap: 2px;
    }

    .gds-about__credit-name {
        font-size: var(--gds-font-size-small, 14px);
        font-weight: 500;
        line-height: 1.3;
        color: var(--gds-control-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-about__credit-url {
        font-size: var(--gds-font-size-xs, 12px);
        line-height: 1.3;
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-about__license-header {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        padding: 10px 14px;
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-about__license-id {
        font-size: var(--gds-font-size-small, 14px);
        font-weight: 600;
        color: var(--gds-control-foreground);
    }

    .gds-about__license-body {
        padding: 14px;
        min-width: 0px;
    }

    .gds-about__license-text {
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        font-size: var(--gds-font-size-xs, 12px);
        line-height: 1.55;
        color: var(--gds-muted-foreground);
        user-select: text;
    }

    .zui-dialog.gds-dialog-content.gds-about-dialog,
    .zui-dialog.gds-about-dialog,
    .zui-dialog.gds-files-prompt-dialog[data-about="true"] {
        width: 420px;
        height: 408px;
        min-width: 0px;
        max-width: calc(100vw - 32px);
        max-height: calc(100vh - 32px);
        padding: 0px;
        gap: 0px;
        overflow: hidden;
    }

    .zui-dialog.gds-dialog-content.gds-about-dialog .zui-dialog__dismiss,
    .zui-dialog.gds-about-dialog .zui-dialog__dismiss,
    .zui-dialog.gds-files-prompt-dialog[data-about="true"] .zui-dialog__dismiss {
        top: 10px;
        right: 10px;
        width: 32px;
        height: 32px;
        border-radius: 6px;
        color: var(--gds-muted-foreground);
        opacity: 1;
        cursor: pointer;
        transition:
            background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out),
            color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
    }

    .zui-dialog.gds-dialog-content.gds-about-dialog .zui-dialog__dismiss:hover,
    .zui-dialog.gds-about-dialog .zui-dialog__dismiss:hover,
    .zui-dialog.gds-files-prompt-dialog[data-about="true"] .zui-dialog__dismiss:hover {
        background-color: var(--gds-hover);
        color: var(--gds-foreground);
    }

    .zui-dialog.gds-dialog-content.gds-about-dialog .zui-dialog__dismiss:active,
    .zui-dialog.gds-about-dialog .zui-dialog__dismiss:active,
    .zui-dialog.gds-files-prompt-dialog[data-about="true"] .zui-dialog__dismiss:active {
        background-color: var(--gds-pressed);
        color: var(--gds-foreground);
    }
    "#
);

fn install_about_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    install_stylesheet(ABOUT_SHEET, ABOUT_CSS);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Section {
    #[default]
    Overview,
    Credits,
    License,
}

pub(crate) fn localized(key: &'static str, fallback: &'static str) -> String {
    Gds::try_current().map_or_else(|| fallback.to_owned(), |gds| gds.i18n().text(key))
}

pub(crate) fn is_http_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("https://") || trimmed.starts_with("http://")
}

pub(crate) fn display_url(url: &str) -> String {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    without_scheme.trim_end_matches('/').to_owned()
}

fn is_activate_key(ev: &EventCx<'_, events::KeyDown>) -> bool {
    !ev.repeat
        && ev.modifiers.held().is_empty()
        && (ev.key == Key::Named(NamedKey::Enter) || ev.key == Key::Character(" ".into()))
}

fn spawn_url_command(url: &str) {
    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", "start", "", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    if let Err(err) = result {
        tracing::warn!(%err, url, "failed to launch external URL from About view");
    }
}

fn open_external_url(url: &str) {
    let trimmed = url.trim();
    if !is_http_url(trimmed) {
        return;
    }
    let url_owned = trimmed.to_owned();
    if let Some(gds) = Gds::try_current()
        && gds.tasks().scope().runtime_handle().is_some()
    {
        gds.tasks().spawn_ui(
            "about.open_url",
            async move { spawn_url_command(&url_owned) },
            |_| {},
        );
    } else {
        spawn_url_command(&url_owned);
    }
}

fn copy_text(
    text: &'static str,
    copied: RwSignal<bool, LocalStorage>,
    generation: &Rc<Cell<u64>>,
    alive: &Rc<Cell<bool>>,
) {
    if let Some(clipboard) = try_use_clipboard() {
        clipboard.set_text(ClipboardKind::Standard, text);
    }
    let next_gen = generation.get().wrapping_add(1);
    generation.set(next_gen);
    copied.set(true);

    if let Some(gds) = Gds::try_current()
        && gds.tasks().scope().runtime_handle().is_some()
    {
        let generation = Rc::clone(generation);
        let alive = Rc::clone(alive);
        gds.tasks().spawn_ui(
            "about.copy_reset",
            async move {
                tokio::time::sleep(COPIED_RESET_DURATION).await;
            },
            move |_| {
                if alive.get() && generation.get() == next_gen {
                    copied.set(false);
                }
            },
        );
    }
}

#[component]
fn BrandHero(application: &'static Application) -> impl IntoView {
    let desktop_icon = application.desktop_icon_image();
    let artwork = application.artwork_svg();
    view! {
        column(class = "gds-about__hero") {
            {match desktop_icon {
                Some(bytes) => Either::Left(
                    view! {
                        image(
                            class = "gds-about__desktop-icon",
                            src = {Some(bytes.url())},
                            a11y:hidden = true,
                        )
                    }
                    .into_view(),
                ),
                None => Either::Right(
                    view! {
                        box(class = "gds-about__emblem") {
                            {match artwork {
                                Some(markup) => Either::Left(
                                    view! {
                                        vector(
                                            class = "gds-about__emblem-svg",
                                            prop:svg = PropValue::from(markup),
                                        )
                                    }
                                    .into_view(),
                                ),
                                None => Either::Right(
                                    view! {
                                        Icon(name = IconName::Info, size = 32.0)
                                    }
                                    .into_view(),
                                ),
                            }}
                        }
                    }
                    .into_view(),
                ),
            }}
            text(
                class = "gds-about__title",
                a11y:role = Role::Heading,
                a11y:level = 1usize,
            ) {{application.name}}
        }
    }
}

#[component]
fn IdentityRow(
    id: &'static str,
    text_id: &'static str,
    label_key: &'static str,
    label_fallback: &'static str,
    value: &'static str,
) -> impl IntoView {
    let copied = RwSignal::new_local(false);
    let generation = Rc::new(Cell::new(0u64));
    let alive = Rc::new(Cell::new(true));
    {
        let alive = Rc::clone(&alive);
        on_cleanup_local(move || alive.set(false));
    }

    let icon = Signal::derive_local(move || {
        if copied.get() {
            IconName::Check
        } else {
            IconName::Copy
        }
    });
    let field_label = Signal::derive_local(move || localized(label_key, label_fallback));
    let accessible = Signal::derive_local(move || {
        let label = localized(label_key, label_fallback);
        format!("{label}: {value}")
    });
    let copy_tip = Signal::derive_local(move || {
        if copied.get() {
            localized("gds-about-copied", "Copied")
        } else {
            localized("gds-about-copy", "Copy")
        }
    });

    view! {
        row(class = "gds-about__info-row") {
            text(class = "gds-about__info-label") {{move || field_label.get()}}
            row(class = "gds-about__info-value-wrap") {
                text(class = "gds-about__mono-value", attr:data-testid = text_id) {{value}}
                HintTooltip(text = copy_tip) {
                    IconButton(
                        icon = icon,
                        label = accessible,
                        size = ButtonSize::IconSm,
                        tooltip = false,
                        class = "gds-about__copy-btn",
                        attr:data-copied = move || copied.get().then(|| "true".to_owned()),
                        attr:data-testid = id,
                        on:click = move |_| copy_text(value, copied, &generation, &alive),
                    )
                }
            }
        }
    }
}

#[component]
fn ExternalLinkTile(
    id: &'static str,
    href: &'static str,
    icon: IconName,
    #[prop(into)] label: Signal<String, LocalStorage>,
) -> impl IntoView {
    view! {
        control(
            class = "gds-about__link-card",
            tabindex = Focus::Sequential,
            a11y:role = Role::Link,
            a11y:label = move || label.get(),
            attr:data-testid = id,
            on:click = move |_| open_external_url(href),
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                if is_activate_key(ev) {
                    open_external_url(href);
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            },
        ) {
            Icon(name = icon, size = 15.0, class = "gds-about__row-icon")
            text(class = "gds-about__row-label") {{move || label.get()}}
            Icon(name = IconName::ArrowUpRight, size = 14.0, class = "gds-about__row-trailing")
        }
    }
}

#[component]
fn SectionNavRow(
    id: &'static str,
    icon: IconName,
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(optional)] meta: Option<&'static str>,
    on_open: UnsyncCallback<()>,
) -> impl IntoView {
    view! {
        control(
            class = "gds-about__nav-row",
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = move || match meta {
                Some(detail) if !detail.is_empty() => format!("{}: {detail}", label.get()),
                _ => label.get(),
            },
            attr:data-testid = id,
            on:click = move |_| on_open.run(()),
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                if is_activate_key(ev) {
                    on_open.run(());
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            },
        ) {
            Icon(name = icon, size = 16.0, class = "gds-about__row-icon")
            text(class = "gds-about__row-label") {{move || label.get()}}
            {meta.filter(|detail| !detail.is_empty()).map(|detail| {
                view! {
                    text(class = "gds-about__row-meta") {{detail}}
                }
                .into_view()
            })}
            Icon(name = IconName::ChevronRight, size = 15.0, class = "gds-about__row-trailing")
        }
    }
}

#[component]
fn OverviewSection(
    application: &'static Application,
    section: RwSignal<Section, LocalStorage>,
    root_ref: NodeRef,
) -> impl IntoView {
    let has_website = is_http_url(application.website);
    let has_feedback = is_http_url(application.feedback);
    let has_links = has_website || has_feedback;
    let has_credits = !application.authors.is_empty() || !application.credits.is_empty();
    let has_license = !application.license.is_empty() || !application.license_text.is_empty();
    let has_details = has_credits || has_license;

    let open_credits = UnsyncCallback::new(move |()| {
        section.set(Section::Credits);
        root_ref.focus();
    });
    let open_license = UnsyncCallback::new(move |()| {
        section.set(Section::License);
        root_ref.focus();
    });

    view! {
        scroll(class = "gds-about__overview-scroll") {
            column(class = "gds-about__overview", attr:data-testid = "about-overview") {
                BrandHero(application = application)

                column(class = "gds-about__group") {
                    IdentityRow(
                        id = "about-version",
                        text_id = "about-version-text",
                        label_key = "gds-about-version",
                        label_fallback = "Version",
                        value = application.version,
                    )
                    IdentityRow(
                        id = "about-build-tag",
                        text_id = "about-build-tag-text",
                        label_key = "gds-about-build-tag",
                        label_fallback = "Build tag",
                        value = application.build_tag,
                    )
                }

                {has_links.then(|| {
                    view! {
                        row(class = "gds-about__links-grid") {
                            {has_website.then(|| {
                                view! {
                                    ExternalLinkTile(
                                        id = "about-website",
                                        href = application.website,
                                        icon = IconName::Globe,
                                        label = Signal::derive_local(|| {
                                            localized("gds-about-website", "Website")
                                        }),
                                    )
                                }
                                .into_view()
                            })}
                            {has_feedback.then(|| {
                                view! {
                                    ExternalLinkTile(
                                        id = "about-feedback",
                                        href = application.feedback,
                                        icon = IconName::MessageSquare,
                                        label = Signal::derive_local(|| {
                                            localized("gds-about-feedback", "Feedback")
                                        }),
                                    )
                                }
                                .into_view()
                            })}
                        }
                    }
                    .into_view()
                })}

                {has_details.then(|| {
                    view! {
                        column(class = "gds-about__group") {
                            {has_credits.then(|| {
                                view! {
                                    SectionNavRow(
                                        id = "about-credits",
                                        icon = IconName::Users,
                                        label = Signal::derive_local(|| {
                                            localized("gds-about-credits", "Credits")
                                        }),
                                        on_open = open_credits,
                                    )
                                }
                                .into_view()
                            })}
                            {has_license.then(|| {
                                view! {
                                    SectionNavRow(
                                        id = "about-license",
                                        icon = IconName::Scale,
                                        label = Signal::derive_local(|| {
                                            localized("gds-about-license", "License")
                                        }),
                                        meta = application.license,
                                        on_open = open_license,
                                    )
                                }
                                .into_view()
                            })}
                        }
                    }
                    .into_view()
                })}
            }
        }
    }
}

#[component]
fn CreditLinkRow(credit: &'static Credit) -> impl IntoView {
    let href = credit.website;
    let subtitle = display_url(href);
    view! {
        control(
            class = "gds-about__credit-row",
            tabindex = Focus::Sequential,
            a11y:role = Role::Link,
            a11y:label = credit.name,
            attr:data-testid = credit.name,
            on:click = move |_| open_external_url(href),
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                if is_activate_key(ev) {
                    open_external_url(href);
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            },
        ) {
            column(class = "gds-about__credit-copy") {
                text(class = "gds-about__credit-name") {{credit.name}}
                text(class = "gds-about__credit-url") {{subtitle}}
            }
            Icon(name = IconName::ArrowUpRight, size = 14.0, class = "gds-about__row-trailing")
        }
    }
}

#[component]
fn CreditsBody(application: &'static Application) -> impl IntoView {
    let has_authors = !application.authors.is_empty();
    let has_credits = !application.credits.is_empty();
    view! {
        column(class = "gds-about__details-inner") {
            {has_authors.then(|| {
                view! {
                    column(class = "gds-about__section") {
                        text(class = "gds-about__section-label") {
                            {move || localized("gds-about-authors", "Developers")}
                        }
                        column(class = "gds-about__group") {
                            for author in move || application.authors.iter().copied(), key = |author| *author {
                                row(class = "gds-about__author-row") {
                                    Icon(name = IconName::User, size = 15.0, class = "gds-about__row-icon")
                                    text(class = "gds-about__author-name") {{author}}
                                }
                            }
                        }
                    }
                }
                .into_view()
            })}

            {has_credits.then(|| {
                view! {
                    column(class = "gds-about__section") {
                        text(class = "gds-about__section-label") {
                            {move || localized("gds-about-projects", "Open-source projects used")}
                        }
                        column(class = "gds-about__group") {
                            for credit in move || application.credits.iter(), key = |credit| credit.name {
                                {if is_http_url(credit.website) {
                                    Either::Left(
                                        view! {
                                            CreditLinkRow(credit = credit)
                                        }
                                        .into_view(),
                                    )
                                } else {
                                    Either::Right(
                                        view! {
                                            column(class = "gds-about__credit-static") {
                                                text(class = "gds-about__credit-name") {{credit.name}}
                                            }
                                        }
                                        .into_view(),
                                    )
                                }}
                            }
                        }
                    }
                }
                .into_view()
            })}
        }
    }
}

#[component]
fn LicenseBody(application: &'static Application) -> impl IntoView {
    let license_body = if application.license_text.is_empty() {
        application.license
    } else {
        application.license_text
    };
    view! {
        column(class = "gds-about__details-inner gds-about__details-inner--license") {
            column(class = "gds-about__group") {
                row(class = "gds-about__license-header") {
                    Icon(name = IconName::Scale, size = 15.0, class = "gds-about__row-icon")
                    text(class = "gds-about__license-id") {{application.license}}
                }
                box(class = "gds-about__license-body") {
                    text(
                        class = "gds-about__license-text",
                        attr:data-testid = "about-license-text",
                    ) {
                        {license_body}
                    }
                }
            }
        }
    }
}

#[component]
fn DetailsSection(
    application: &'static Application,
    which: Section,
    section: RwSignal<Section, LocalStorage>,
    root_ref: NodeRef,
) -> impl IntoView {
    let (title_key, title_fallback) = match which {
        Section::Credits => ("gds-about-credits", "Credits"),
        Section::License => ("gds-about-license", "License"),
        Section::Overview => ("gds-about-credits", "Credits"),
    };
    let back_shortcut = Shortcut::new(Key::Named(NamedKey::ArrowLeft), Modifiers::ALT);
    let go_back = move || {
        section.set(Section::Overview);
        root_ref.focus();
    };
    let host_overlay = OverlayState::current();
    let exclude_surface = host_overlay.map_or(root_ref, |overlay| overlay.content());
    let on_dismiss = UnsyncCallback::new(move |reason: DismissReason| match reason {
        DismissReason::EscapeKey => go_back(),
        DismissReason::OutsidePress => {
            if let Some(overlay) = host_overlay {
                overlay.close();
            }
        }
        _ => go_back(),
    });

    view! {
        DismissableLayer(
            layer = OverlayLayer::Modal,
            class = "gds-about__details-wrap",
            exclude = exclude_surface,
            dismiss_on_outside_press = host_overlay.is_some(),
            dismiss_on_escape = true,
            on_dismiss = on_dismiss,
        ) {
            row(class = "gds-about__subheader") {
                IconButton(
                    icon = IconName::ArrowLeft,
                    label = Signal::derive_local(|| localized("gds-about-back", "Back")),
                    shortcut = back_shortcut,
                    attr:data-testid = "about-back",
                    on:click = move |_| go_back(),
                )
                text(
                    class = "gds-about__subtitle",
                    a11y:role = Role::Heading,
                    a11y:level = 2usize,
                ) {{move || localized(title_key, title_fallback)}}
            }
            scroll(class = "gds-about__details", attr:data-testid = "about-details") {
                {match which {
                    Section::Credits => {
                        Either::Left(view! { CreditsBody(application = application) }.into_view())
                    }
                    Section::License => Either::Right(Either::Left(
                        view! { LicenseBody(application = application) }.into_view(),
                    )),
                    Section::Overview => Either::Right(Either::Right(())),
                }}
            }
        }
    }
}

pub(crate) fn is_back_key(key: &Key, modifiers: Modifiers) -> bool {
    let held = modifiers.held();
    (held.is_empty() && *key == Key::Named(NamedKey::Escape))
        || (held == Modifiers::ALT && *key == Key::Named(NamedKey::ArrowLeft))
}

#[cfg(test)]
pub(crate) mod test_probe {
    use super::*;
    use std::cell::RefCell;

    pub(crate) struct AboutProbe {
        pub section: RwSignal<Section, LocalStorage>,
        pub root_ref: NodeRef,
    }

    thread_local! {
        pub(crate) static PROBE: RefCell<Option<AboutProbe>> = const { RefCell::new(None) };
    }
}

/// Shared application About view displaying brand identity, copyable version/build rows,
/// external Website/Feedback links, Credits, and embedded Apache-2.0 License subpages.
#[component]
pub fn About(
    application: &'static Application,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_about_stylesheets();
    let section = RwSignal::new_local(Section::Overview);
    let root_ref = NodeRef::new();

    #[cfg(test)]
    test_probe::PROBE.with(|slot| {
        *slot.borrow_mut() = Some(test_probe::AboutProbe { section, root_ref });
    });

    let capture_back = Attrs::new().listener(
        events::KEY_DOWN,
        ListenerOptions::CAPTURE,
        move |ev: &mut EventCx<'_, events::KeyDown>| {
            if !ev.repeat
                && section.get_untracked() != Section::Overview
                && is_back_key(&ev.key, ev.modifiers)
            {
                section.set(Section::Overview);
                root_ref.focus();
                ev.prevent_default();
                ev.stop_propagation();
            }
        },
    );

    view! {
        column(
            class = "gds-about",
            class = class,
            node_ref = root_ref,
            tabindex = Focus::Programmatic,
            attr:data-binary = application.binary,
            attr:data-testid = "about-content",
            {..capture_back},
            {..attrs}
        ) {
            {move || match section.get() {
                Section::Overview => Either::Left(
                    view! {
                        OverviewSection(
                            application = application,
                            section = section,
                            root_ref = root_ref,
                        )
                    }
                    .into_view(),
                ),
                which @ (Section::Credits | Section::License) => Either::Right(
                    view! {
                        DetailsSection(
                            application = application,
                            which = which,
                            section = section,
                            root_ref = root_ref,
                        )
                    }
                    .into_view(),
                ),
            }}
        }
    }
}

pub use About as AboutView;
pub use AboutProps as AboutViewProps;

/// Modal dialog wrapper hosting [`About`] with `DialogScope` isolation and `Alt+C` / `Esc` dismissal.
#[component]
pub fn AboutDialog(
    #[prop(into, optional)] open: Binding<bool>,
    application: &'static Application,
    #[prop(optional)] anchor: Option<DialogAnchor>,
    #[prop(optional)] on_close: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    install_about_stylesheets();
    let on_decision = on_close.map(|cb| UnsyncCallback::new(move |_| cb.run(())));

    view! {
        ModalDialog(
            open = open,
            disabled = true,
            anchor = anchor,
            on_decision = on_decision,
            class = "gds-about-dialog",
        ) {
            About(application = application)
        }
    }
}

/// Convenience helper returning an [`AnyView`] modal About dialog controlled by `open`.
#[must_use]
pub fn open_about_dialog(
    open: impl Into<Binding<bool>>,
    application: &'static Application,
) -> AnyView {
    let open = open.into();
    AnyView::new(view! {
        AboutDialog(open = open, application = application)
    })
}
