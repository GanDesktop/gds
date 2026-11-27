//! Anchored menus, context menu triggers, keyboard Escape handling, and the menu overlay host.

use std::rc::Rc;

use gds::app::Gds;
use gds::i18n;
use gds::ui::button::{ApplicationMenuButtonProps, IconButtonProps};
use gds::ui::feedback::dialog::{DialogDecision, DialogSize, ModalDialogProps};
use gds::ui::menu::{AnchoredMenuProps, MenuEntry, render_entries};
use gds::ui::{IconName, Shortcut};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, UnsyncCallback, enter_non_reactive_zone, on_cleanup_local};
use zgui::view;
use zgui::view::{EventCx, Scoped, events};
use zgui::vocab::{Key, KeyEvent, Modifiers, NamedKey, PointerButton};
use zgui_ui::menu::{MenuContentProps, MenuContext};
use zgui_ui::overlay::OverlayState;
use zgui_ui_primitives::{Align, Binding, Placement, Side};

use super::super::inspection::Inspection;
use super::actions::{show_about, show_help};
use super::columns::{apply_columns, columns_dialog};
use super::entries::context_popup_entries;
use super::pages::page_entries;
use crate::{
    entities::{file::ViewMode, location::Location},
    pages::workspace::{
        operations::{Lifecycle, LocalOps},
        projection::{Geometry, UiState, geometry::RectF},
        state::{
            context::Context,
            menus::{DirectoryTarget, SelectionTarget, Surface},
        },
    },
    shared::ui::actions::Command,
};

fn anchored_menu_surface(
    anchor: NodeRef,
    open: Signal<bool, LocalStorage>,
    on_close: UnsyncCallback<()>,
    placement: Placement,
    build_entries: Rc<dyn Fn() -> Vec<MenuEntry>>,
) -> AnyView {
    AnyView::new(view! {
        AnchoredMenu(
            anchor = anchor,
            placement = placement,
            class = "gds-files-menu-content",
            open = Binding::controlled(open, move |next: bool| {
                if !next {
                    on_close.run(());
                }
            }),
        ) {
            {render_entries(build_entries())}
        }
    })
}

fn toggle_page_menu(ctx: &Context) {
    ctx.0.menus.surface.update(|surface| {
        *surface = if matches!(surface, Surface::Page) {
            Surface::Closed
        } else {
            Surface::Page
        };
    });
}

fn is_context_menu_shortcut(ev: &EventCx<'_, events::KeyDown>) -> bool {
    if ev.repeat {
        return false;
    }
    let mods = ev.modifiers.held();
    (ev.key == Key::Named(NamedKey::ContextMenu) && mods.is_empty())
        || (ev.key == Key::Named(NamedKey::F10) && mods == Modifiers::SHIFT)
}

fn selection_menu_position(
    ctx: &Context,
    index: usize,
    grid: bool,
    scale: f32,
    port: RectF,
) -> (f32, f32) {
    let geometry = Geometry(ctx);
    let tile = if grid {
        geometry.grid_geometry_untracked().tile(index, scale)
    } else {
        let row = geometry.list_row_height_untracked();
        RectF::new(0.0, row * index as f32, 0.0, row)
    };
    let scroll = ctx.0.list.scroll_position().offset;
    let sx = scroll.x.0 / scale;
    let sy = scroll.y.0 / scale;
    let x = port.x + (tile.x - sx + 11.0).clamp(0.0, port.width);
    let y = port.y + ((tile.y + tile.height) - sy).clamp(0.0, port.height);
    (x, y)
}

fn install_menu_validity_effect(ctx: &Context) {
    let origin = ctx.clone();
    let validity_fx = RenderEffect::new(move |_| {
        let valid = match origin.0.menus.surface.get() {
            Surface::Selection { target, .. } => origin.read(|state| target.matches_state(state)),
            Surface::Directory { target, .. } => origin.read(|state| target.matches_state(state)),
            Surface::Trash { tab, .. } => origin.read(|state| {
                state.tab().id == tab
                    && matches!(state.tab().navigation.location(), Location::Trash)
            }),
            Surface::Tab { id, .. } => {
                origin.read(|state| state.tabs.iter().any(|tab| tab.id == id))
            }
            _ => true,
        };
        if !valid {
            origin.0.menus.close();
        }
    });
    on_cleanup_local(move || drop(validity_fx));
}

fn columns_modal_dialog(ctx: &Context) -> AnyView {
    let columns_open = Signal::derive_local({
        let ctx = ctx.clone();
        move || matches!(ctx.0.menus.surface.get(), Surface::Columns { .. })
    });
    let close_columns = ctx.clone();
    let decision_columns = ctx.clone();
    let content_columns = ctx.clone();

    AnyView::new(view! {
        ModalDialog(
            open = Binding::controlled(
                columns_open,
                move |next: bool| {
                    if !next {
                        close_columns.0.menus.close();
                    }
                }
            ),
            size = DialogSize::Md,
            close_on_confirm = false,
            on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                match decision {
                    DialogDecision::Confirm => {
                        if let Surface::Columns {
                            location,
                            layout,
                            scope,
                        } = decision_columns.0.menus.surface.get_untracked()
                        {
                            apply_columns(
                                &decision_columns,
                                location,
                                Some(layout.get_untracked()),
                                scope.get_untracked(),
                            );
                        }
                    }
                    DialogDecision::Cancel | DialogDecision::Dismiss => {
                        decision_columns.0.menus.close();
                    }
                }
            }),
            class = "gds-files-columns-dialog",
        ) {
            {{
                let c = content_columns.clone();
                move || columns_dialog(&c)
            }}
        }
    })
}

fn context_popup(ctx: &Context) -> AnyView {
    let surface = ctx.0.menus.surface.get();
    let Some((x, y, entries)) = ({
        let _z = enter_non_reactive_zone();
        context_popup_entries(ctx, surface)
    }) else {
        return AnyView::new(view! { box(class = "gds-files-hidden") });
    };

    let close = ctx.clone();
    AnyView::new(Scoped::new(move || {
        let anchor = NodeRef::new();
        let close = close.clone();
        let overlay = OverlayState::new(
            Binding::controlled(Signal::stored_local(true), move |next: bool| {
                if !next {
                    close.0.menus.close();
                }
            }),
            true,
            None,
        )
        .anchored_to(anchor)
        .provide();
        MenuContext::new(overlay).provide();

        view! {
            box(class = "gds-files-context-portal-anchor") {
                box(
                    node_ref = anchor,
                    class = "gds-files-context-point",
                    style:left = format!("{x:.1}px"),
                    style:top = format!("{y:.1}px"),
                    a11y:hidden = true
                )
                MenuContent(
                    state = overlay,
                    placement = Placement::new(Side::Bottom, Align::Start),
                    offset = 0.0,
                    class = "gds-menu-content gds-files-menu-content"
                ) {
                    {render_entries(entries.clone())}
                }
            }
        }
    }))
}

pub(super) fn menu_key(ctx: &Context, event: &KeyEvent) -> bool {
    if !ctx.0.menus.is_open() {
        return false;
    }
    if event.key == Key::Named(NamedKey::Escape) {
        ctx.0.menus.close();
        return true;
    }
    false
}

fn app_entry(
    ctx: &Context,
    label: String,
    icon: IconName,
    shortcut: Option<Shortcut>,
    disabled: bool,
    action: impl Fn(&Context) + 'static,
) -> MenuEntry {
    let weak = ctx.downgrade();
    MenuEntry::Item {
        label,
        icon: Some(icon),
        shortcut,
        disabled,
        destructive: false,
        on_select: UnsyncCallback::new(move |()| {
            if let Some(ctx) = weak.upgrade() {
                ctx.0.owner.with(|| action(&ctx));
            }
        }),
    }
}

fn app_entries(ctx: &Context) -> Vec<MenuEntry> {
    let in_picker = ctx.read_untracked(|state| state.picker.is_some());
    let lifecycle = Lifecycle(ctx);
    let undo_enabled = lifecycle.history_available(false);
    let redo_enabled = lifecycle.history_available(true);
    let mut entries = vec![
        app_entry(
            ctx,
            lifecycle.history_label(false),
            IconName::Undo2,
            Some(Command::Undo.shortcut()),
            !undo_enabled,
            |c| LocalOps(c).replay(false),
        ),
        app_entry(
            ctx,
            lifecycle.history_label(true),
            IconName::Redo2,
            Some(Command::Redo.shortcut()),
            !redo_enabled,
            |c| LocalOps(c).replay(true),
        ),
        MenuEntry::Separator,
        app_entry(
            ctx,
            i18n::text_untracked(Command::NewWindow.label_id()),
            IconName::Plus,
            Some(Command::NewWindow.shortcut()),
            in_picker,
            |c| {
                let path = c.read_untracked(|state| {
                    state
                        .tab()
                        .navigation
                        .path()
                        .unwrap_or(&state.home)
                        .to_owned()
                });
                (c.0.new_window)(path);
            },
        ),
        app_entry(
            ctx,
            i18n::text_untracked(Command::Settings.label_id()),
            IconName::Settings,
            Some(Command::Settings.shortcut()),
            false,
            |c| UiState(c).show_settings(),
        ),
        app_entry(
            ctx,
            i18n::text_untracked(Command::Help.label_id()),
            IconName::BookOpen,
            Some(Command::Help.shortcut()),
            false,
            show_help,
        ),
    ];
    if Gds::current().ui().logs().is_enabled() {
        entries.push(MenuEntry::Item {
            label: i18n::text_untracked("logs-title"),
            icon: Some(IconName::List),
            shortcut: Some(Shortcut::new(
                Key::character("l"),
                Modifiers::CONTROL | Modifiers::SHIFT,
            )),
            disabled: false,
            destructive: false,
            on_select: UnsyncCallback::new(|()| {
                gds::ui::logs::open();
            }),
        });
    }
    entries.push(app_entry(
        ctx,
        i18n::text_untracked("about"),
        IconName::Info,
        None,
        false,
        show_about,
    ));
    entries
}

pub(super) fn app_menu(ctx: &Context) -> AnyView {
    let anchor = NodeRef::new();
    let click_ctx = ctx.clone();
    let entries_ctx = ctx.clone();
    let close_ctx = ctx.clone();
    let open = Signal::derive_local({
        let ctx = ctx.clone();
        move || matches!(ctx.0.menus.surface.get(), Surface::Application)
    });
    let menu_content = anchored_menu_surface(
        anchor,
        open,
        UnsyncCallback::new(move |()| close_ctx.0.menus.close()),
        Placement::new(Side::Bottom, Align::Start),
        Rc::new(move || app_entries(&entries_ctx)),
    );
    AnyView::new(view! {
        box(class = "gds-files-menu-anchor") {
            ApplicationMenuButton(
                node_ref = anchor,
                label = Signal::derive_local(|| i18n::text("app-menu")),
                application = gds::ui::about::Application::try_cargo("gds-files"),
                shortcut = Some(Command::Menu.shortcut()),
                active = open,
                on:click = move |_| show_app_menu(&click_ctx)
            ) {}
            {menu_content}
        }
    })
}

pub(super) fn show_app_menu(ctx: &Context) {
    ctx.0.menus.surface.update(|surface| {
        *surface = if matches!(surface, Surface::Application) {
            Surface::Closed
        } else {
            Surface::Application
        };
    });
}

pub(super) fn page_menu(ctx: &Context) -> AnyView {
    let anchor = NodeRef::new();
    let click_ctx = ctx.clone();
    let entries_ctx = ctx.clone();
    let close_ctx = ctx.clone();
    let open = Signal::derive_local({
        let ctx = ctx.clone();
        move || matches!(ctx.0.menus.surface.get(), Surface::Page)
    });
    let menu_content = anchored_menu_surface(
        anchor,
        open,
        UnsyncCallback::new(move |()| close_ctx.0.menus.close()),
        Placement::new(Side::Bottom, Align::End),
        Rc::new(move || page_entries(&entries_ctx)),
    );
    AnyView::new(view! {
        box(class = "gds-files-menu-anchor") {
            IconButton(
                node_ref = anchor,
                icon = IconName::Ellipsis,
                label = Signal::derive_local(|| i18n::text("page-tools")),
                active = open,
                on:click = move |_| toggle_page_menu(&click_ctx)
            ) {}
            {menu_content}
        }
    })
}

pub(super) fn show_context_menu(ctx: &Context) {
    let Some(bounds) = ctx.0.list.window_bounds() else {
        return;
    };
    let scale = ctx.0.list.scale().max(1.0);
    let port = RectF::new(
        bounds.origin.x.0 / scale,
        bounds.origin.y.0 / scale,
        (bounds.size.width.0 / scale).max(1.0),
        (bounds.size.height.0 / scale).max(1.0),
    );

    let (target, grid) = ctx.read_untracked(|state| {
        let tab = state.tab();
        let target = tab
            .navigation
            .view()
            .cursor
            .as_ref()
            .filter(|key| tab.navigation.view().selection.contains(key))
            .and_then(|key| tab.rows.iter().position(|row| row.key() == *key))
            .or_else(|| {
                tab.rows
                    .iter()
                    .position(|row| tab.navigation.view().selection.contains(&row.key()))
            })
            .map(|index| (index, tab.rows[index].key()));
        (target, tab.mode == ViewMode::Grid)
    });

    if let Some((index, target)) = target {
        let (x, y) = selection_menu_position(ctx, index, grid, scale, port);
        ctx.0.menus.surface.set(Surface::Selection {
            target: ctx.read_untracked(|state| SelectionTarget::from_state(state, target)),
            x,
            y,
        });
    } else if ctx
        .read_untracked(|state| matches!(state.tab().navigation.location(), Location::Trash))
    {
        let tab = ctx.read_untracked(|state| state.tab().id);
        ctx.0.menus.surface.set(Surface::Trash {
            tab,
            x: port.x + 11.0,
            y: port.y + 11.0,
        });
    } else if let Some(target) = ctx.read_untracked(DirectoryTarget::from_state) {
        ctx.0.menus.surface.set(Surface::Directory {
            target,
            x: port.x + 11.0,
            y: port.y + 11.0,
        });
    }
}

pub(super) fn row_menu(ctx: &Context, target_view: AnyView, index: usize) -> AnyView {
    let Some(row) = ctx.read_untracked(|state| state.tab().rows.get(index).cloned()) else {
        return target_view;
    };
    let clicked = row.key();
    let open = ctx.clone();
    AnyView::new(view! {
        box(
            class = "gds-files-menu-wrap",
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                if ev.button == Some(PointerButton::Secondary) {
                    ev.stop_propagation();
                    let current_index = open.read_untracked(|state| {
                        state.tab().rows.iter().position(|row| row.key() == clicked)
                    });
                    if let Some(current_index) = current_index {
                        LocalOps(&open).select_context(current_index);
                        open.0.menus.surface.set(Surface::Selection {
                            target: open.read_untracked(|state| {
                                SelectionTarget::from_state(state, clicked.clone())
                            }),
                            x: ev.position.x.0,
                            y: ev.position.y.0,
                        });
                    }
                }
            }
        ) {
            {target_view}
        }
    })
}

pub(super) fn context_menu_region(
    ctx: &Context,
    full_width: bool,
    target_view: AnyView,
    build: impl Fn(f32, f32) -> Surface + 'static,
) -> AnyView {
    let host_ref = NodeRef::new();
    let build = Rc::new(build);
    let open_ptr = ctx.clone();
    let build_ptr = Rc::clone(&build);
    let open_ctx = ctx.clone();
    let build_ctx = Rc::clone(&build);
    let open_key = ctx.clone();
    AnyView::new(view! {
        box(
            node_ref = host_ref,
            class = "gds-files-menu-wrap",
            style:flex-direction = full_width.then(|| "column".to_owned()),
            style:width = full_width.then(|| "100%".to_owned()),
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                if ev.button == Some(PointerButton::Secondary) {
                    ev.prevent_default();
                    ev.stop_propagation();
                    open_ptr
                        .0
                        .menus
                        .surface
                        .set(build_ptr(ev.position.x.0, ev.position.y.0));
                }
            },
            on:context_menu = move |ev: &mut EventCx<'_, events::ContextMenu>| {
                ev.prevent_default();
                ev.stop_propagation();
                open_ctx
                    .0
                    .menus
                    .surface
                    .set(build_ctx(ev.position.x.0, ev.position.y.0));
            },
            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                if is_context_menu_shortcut(ev)
                    && let Some(bounds) = host_ref.window_bounds()
                {
                    let scale = host_ref.scale().max(0.01);
                    let x = (bounds.origin.x.0 + bounds.size.width.0 * 0.5) / scale;
                    let y = (bounds.origin.y.0 + bounds.size.height.0 * 0.5) / scale;
                    ev.prevent_default();
                    ev.stop_propagation();
                    open_key.0.menus.surface.set(build(x, y));
                }
            }
        ) {
            {target_view}
        }
    })
}

pub(super) fn directory_menu(ctx: &Context, target_view: AnyView) -> AnyView {
    let open = ctx.clone();
    let trigger = move |x: f32, y: f32| {
        if let Some(tab) = open.read_untracked(|state| {
            matches!(state.tab().navigation.location(), Location::Trash).then_some(state.tab().id)
        }) {
            open.0.menus.surface.set(Surface::Trash { tab, x, y });
        } else if let Some(target) = open.read_untracked(DirectoryTarget::from_state) {
            open.0
                .menus
                .surface
                .set(Surface::Directory { target, x, y });
        }
    };
    let trigger_menu = trigger.clone();
    AnyView::new(view! {
        box(
            class = "gds-files-directory-surface",
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                if ev.button == Some(PointerButton::Secondary) {
                    ev.stop_propagation();
                    trigger(ev.position.x.0, ev.position.y.0);
                }
            },
            on:context_menu = move |ev: &mut EventCx<'_, events::ContextMenu>| {
                ev.prevent_default();
                ev.stop_propagation();
                trigger_menu(ev.position.x.0, ev.position.y.0);
            }
        ) {
            {target_view}
        }
    })
}

pub(super) fn tab_menu(ctx: &Context, target_view: AnyView, id: u64) -> AnyView {
    let open = ctx.clone();
    AnyView::new(view! {
        box(
            class = "gds-files-menu-wrap gds-files-sidebar-row-wrap",
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                if ev.button == Some(PointerButton::Secondary) {
                    ev.stop_propagation();
                    open.0.menus.surface.set(Surface::Tab {
                        id,
                        x: ev.position.x.0,
                        y: ev.position.y.0,
                    });
                }
            }
        ) {
            {target_view}
        }
    })
}

pub(super) fn menu_overlay(ctx: &Context) -> AnyView {
    install_menu_validity_effect(ctx);
    let popup_ctx = ctx.clone();
    let columns_dialog_view = columns_modal_dialog(ctx);
    let item_editor_view = Inspection(ctx).item_editor_dialog();

    AnyView::new(view! {
        box(class = "gds-files-menu-overlay") {
            {move || context_popup(&popup_ctx)}
            {columns_dialog_view}
            {item_editor_view}
        }
    })
}
