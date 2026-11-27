use crate::appearance::tokens::{SIDEBAR_ICON_SIZE, TOOLTIP_DELAY};
use crate::ui::{
    IconName, IconProps, Shortcut, ShortcutBadgeProps,
    frame::WindowDragAreaProps,
    keytips::{self, HintPlacement},
    navigation::NavItemProps,
    sidebar::{SideBarContentProps, SideBarProps, SidebarPresentation},
    tooltip::TooltipTextProps,
};
use zgui::vocab::{Key, Modifiers, NamedKey};
use zgui::{css, prelude::*, view::install_stylesheet, vocab::Role};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipTriggerProps};

const CSS: &str = css!(
    r#"
    .gds-settings-nav-corner-anchor {
        position: absolute;
        inset: 0;
        pointer-events: none;
    }
    .gds-settings-sidebar {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0;
        gap: 4px;
        padding: var(--gds-sidebar-padding, 10px);
        box-sizing: border-box;
    }

    .gds-settings-sidebar > .zui-tooltip__trigger {
        display: flex;
        width: 100%;
        min-width: 0;
    }

    .gds-settings-content {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        padding: 24px;
        box-sizing: border-box;
        outline: none;
        box-shadow: none;
    }

    .gds-settings-content:focus,
    .gds-settings-content:focus-visible {
        outline: none;
        box-shadow: none;
    }

    .gds-settings-heading {
        font-size: var(--gds-text-lg, 20px);
        font-weight: 600;
        margin-bottom: 20px;
    }

    .gds-settings-col {
        display: flex;
        flex-direction: column;
        width: 100%;
        max-width: 880px;
        min-width: 0;
    }

    .gds-subpage-toolbar-title {
        display: flex;
        flex: 1 1 0%;
        align-items: center;
        min-width: 0px;
        height: 100%;
        padding: 0px 8px;
        box-sizing: border-box;
    }

    .gds-subpage-toolbar-title-text {
        font-size: var(--gds-text-sm, 14px);
        font-weight: 400;
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-settings-footer { font-size:var(--gds-text-xs, 12px); color:var(--gds-muted-foreground); }
"#
);

#[component]
pub fn SubpageToolbarTitle(#[prop(into)] label: Signal<String, LocalStorage>) -> impl IntoView {
    install_stylesheet("gds-settings-layout", CSS);
    view! { WindowDragArea(class="gds-subpage-toolbar-title") {
        text(class="gds-subpage-toolbar-title-text") {{move || label.get()}}
    }}
}

#[component]
pub fn SettingsPage(
    #[prop(into)] heading: Signal<String, LocalStorage>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet("gds-settings-layout", CSS);
    let focus = node_ref.unwrap_or_default();
    let focus_on_mount = zgui::reactive::RenderEffect::new(move |prev: Option<bool>| {
        let mounted = focus.get().is_some();
        if mounted && prev != Some(true) {
            focus.focus();
        }
        mounted
    });
    zgui::reactive::on_cleanup_local(move || drop(focus_on_mount));

    view! { scroll(node_ref=focus, class="gds-settings-content", tabindex=Focus::Sequential, {..attrs}) {
        text(class="gds-settings-heading", a11y:role=Role::Heading, a11y:label=move||heading.get()) {{move||heading.get()}}
        {children.into_view_once()}
    }}
}

#[component]
pub fn SettingsColumn(children: Children) -> impl IntoView {
    install_stylesheet("gds-settings-layout", CSS);
    view! { column(class="gds-settings-col") { {children.into_view_once()} } }
}

#[component]
pub fn SettingsNavigation(
    #[prop(into)] presentation: Signal<SidebarPresentation, LocalStorage>,
    #[prop(into)] label: String,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet("gds-settings-layout", CSS);
    view! { SideBar(presentation=presentation,label=label) { SideBarContent {
        column(class="gds-settings-sidebar", {..attrs}) { {children.into_view_once()} }
    }}}
}

#[component]
pub fn SettingsFooter(#[prop(into)] label: Signal<String, LocalStorage>) -> impl IntoView {
    install_stylesheet("gds-settings-layout", CSS);
    view! { text(class="gds-settings-footer") {{move||label.get()}} }
}

fn cycle_shortcut(previous: bool) -> Shortcut {
    let mods = if previous {
        Modifiers::CONTROL | Modifiers::SHIFT
    } else {
        Modifiers::CONTROL
    };
    Shortcut::new(Key::Named(NamedKey::Tab), mods)
}

pub fn adjacent_page(current: usize, count: usize, previous: bool) -> usize {
    (current + if previous { count - 1 } else { 1 }) % count
}

pub fn category_shortcut(current: usize, destination: usize, count: usize) -> Option<Shortcut> {
    if current == destination {
        return None;
    }
    [false, true]
        .into_iter()
        .find(|previous| adjacent_page(current, count, *previous) == destination)
        .map(cycle_shortcut)
}

pub fn handle_page_cycle(
    event: &mut zgui::view::EventCx<'_, zgui::view::events::KeyDown>,
    cycle: impl FnOnce(bool),
) {
    if event.repeat || keytips::has_active_modal_scope() {
        return;
    }
    let held = event.modifiers.held();
    if held.control() && !held.alt() && !held.meta() && event.key == Key::Named(NamedKey::Tab) {
        cycle(held.shift());
        event.prevent_default();
        event.stop_propagation();
    }
}

#[component]
pub fn SettingsCategory(
    #[prop(into)] label: Signal<String, LocalStorage>,
    icon: IconName,
    #[prop(into)] selected: Signal<bool, LocalStorage>,
    #[prop(into)] shortcut: Signal<Option<Shortcut>, LocalStorage>,
    on_click: UnsyncCallback<()>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    let item_ref = node_ref.unwrap_or_default();
    let corner_ref = NodeRef::new();
    let presentation = crate::ui::sidebar::SidebarContext::current()
        .map(|context| context.presentation)
        .unwrap_or_else(|| Signal::stored_local(SidebarPresentation::default()));
    for previous in [false, true] {
        let key = cycle_shortcut(previous);
        let enabled =
            Memo::new(move |_| shortcut.get().as_ref() == Some(&cycle_shortcut(previous)));
        keytips::stage_shortcut(
            item_ref,
            key.clone(),
            HintPlacement::Trailing,
            Signal::derive_local(move || enabled.get() && !presentation.get().collapsed()),
        );
        keytips::stage_shortcut(
            corner_ref,
            key,
            HintPlacement::Corner,
            Signal::derive_local(move || enabled.get() && presentation.get().collapsed()),
        );
    }
    let icon_view = AnyView::new(view! {
        Icon(name = icon, size = SIDEBAR_ICON_SIZE)
    });
    let label_view = AnyView::new(view! {
        {move || label.get()}
    });
    let tooltip_label = label;

    let nav_row = view! {
        NavItem(
            node_ref = item_ref,
            selected = selected,
            presentation = presentation,
            icon = icon_view,
            label = label_view,
            tooltip_enabled = false,
            {..attrs},
            a11y:label = move || label.get(),
            on_click = on_click,
        ) {
            box(
                node_ref = corner_ref,
                class = "gds-settings-nav-corner-anchor",
                a11y:hidden = true,
            ) {}
        }
    };

    AnyView::new(view! {
        Tooltip(delay = TOOLTIP_DELAY) {
            TooltipTrigger {{nav_row}}
            TooltipContent(class = "gds-tooltip-content", arrow = false) {
                row(class = "gds-shortcut-hint") {
                    box(class = "gds-shortcut-hint__label") {
                        TooltipText(text = tooltip_label)
                    }
                    {move || {
                        shortcut.get().map(|shortcut| {
                            view! {
                                box(class = "gds-shortcut-hint__keys") {
                                    ShortcutBadge(shortcut = shortcut)
                                }
                            }
                            .into_view()
                        })
                    }}
                }
            }
        }
    })
}
