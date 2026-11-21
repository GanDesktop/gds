//! Navigation list items, pinned badges, and hover/corner action slots.

mod action;
mod actions;
mod content;
mod details;
mod header;
mod icon;
mod label;

pub use action::{
    CornerAction, CornerActionProps, CornerButton, CornerButtonProps, HoverAction,
    HoverActionProps, corner_action, corner_button, hover_action,
};
pub use actions::{NavItemActions, NavItemActionsProps};
pub use content::{NavItemContent, NavItemContentProps};
pub use details::{NavItemDetails, NavItemDetailsProps};
pub use header::{NavItemHeader, NavItemHeaderProps};
pub use icon::{PinnedIcon, PinnedIconProps, WithPin, WithPinProps, pinned_icon, with_pin};
pub use label::{NavItemLabel, NavItemLabelProps};

use std::cell::Cell;
use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback, provide_local_context, use_local_context};
use zgui::view::{AttrName, CustomPropertyName};
use zgui::vocab::PointerButton;
use zgui::{component, css, view};

use crate::appearance::tokens::{
    COLLAPSED_SIDEBAR_WIDTH, ICON_BUTTON_SIZE, SIDEBAR_ICON_SIZE, SIDEBAR_PADDING,
};
use crate::ui::actions::Command;
use crate::ui::shortcut::{Shortcut, ShortcutHintProps};
use crate::ui::sidebar::{SidebarContext, SidebarPresentation};

const ITEM_GAP: f32 = 8.0;

#[derive(Clone, Copy)]
pub(crate) struct NavItemContext {
    pub(crate) trailing_reserve: f32,
}

impl NavItemContext {
    pub(crate) fn current() -> Option<Self> {
        zgui::reactive::is_ui_thread()
            .then(use_local_context::<Self>)
            .flatten()
    }
}

const NAVIGATION_SHEET: &str = "gds-ui-navigation";

const NAVIGATION_CSS: &str = css!(
    ".gds-nav-item {
        position: relative;
        flex-shrink: 0;
        width: 100%;
        max-width: 100%;
        min-width: 0px;
        min-height: var(--gds-sidebar-item-height, 38px);
        padding-left: var(--gds-nav-item-pad-left, 8px);
        padding-right: var(--gds-nav-item-pad-right, 8px);
        gap: var(--gds-nav-item-gap, 8px);
        box-sizing: border-box;
        overflow: hidden;
        border: none;
        border-radius: var(--gds-radius-control, 6px);
        user-select: none;
        --gds-nav-label-ink: var(--gds-foreground);
        transition:
            background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-sidebar .gds-nav-item {
        --gds-nav-label-ink: var(--gds-sidebar-foreground);
    }
    .gds-nav-item[data-selected=\"true\"] {
        --gds-nav-label-ink: var(--gds-selection-foreground);
    }
    .gds-nav-item[data-compact=\"true\"] {
        min-height: 32px;
    }
    .gds-nav-item[data-disabled=\"true\"] {
        opacity: 0.5;
        pointer-events: none;
        cursor: default;
    }
    .gds-nav-item:active {
        background-color: var(--gds-pressed);
    }
    .gds-nav-item[data-selected=\"true\"]:hover,
    .gds-nav-item[data-selected=\"true\"]:active {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
    }
    .gds-nav-item[data-drop-target=\"true\"] {
        background-color: var(--gds-drag-target);
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }
    .gds-nav-item:focus:not(:focus-visible) {
        outline: none;
    }
    .gds-nav-item:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }
    .gds-nav-item__content {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        overflow: hidden;
        padding: 4px 0px;
        gap: 2px;
        box-sizing: border-box;
    }
    .gds-nav-item__header {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 0 0 auto;
        min-width: 0px;
        height: 18px;
        min-height: 18px;
        overflow: hidden;
        gap: var(--gds-nav-item-gap, 8px);
        padding-right: calc(var(--gds-nav-header-reserve, 0px) * var(--gds-sidebar-progress, 1));
        box-sizing: border-box;
    }
    .gds-nav-item__label {
        display: block;
        flex: 0 0 auto;
        min-width: 0px;
        height: 18px;
        min-height: 18px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-font-size-small, 14px);
        line-height: 18px;
        color: color-mix(
            in srgb,
            var(--gds-nav-label-ink, var(--gds-foreground)) var(--gds-sidebar-label-pct, 100%),
            transparent
        );
    }
    .gds-nav-item > .gds-nav-item__label,
    .gds-nav-item__header > .gds-nav-item__label {
        flex: 1 1 0px;
    }
    .gds-nav-item__details {
        display: block;
        flex: 0 0 auto;
        min-width: 0px;
        height: 16px;
        min-height: 16px;
        line-height: 16px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-font-size-xs, 12px);
        color: color-mix(
            in srgb,
            var(--gds-muted-foreground) var(--gds-sidebar-label-pct, 100%),
            transparent
        );
        box-sizing: border-box;
    }
    .gds-nav-item[data-selected=\"true\"] .gds-nav-item__details {
        color: color-mix(
            in srgb,
            var(--gds-selection-foreground) var(--gds-sidebar-selected-details-pct, 85%),
            transparent
        );
    }
    .gds-nav-item__actions {
        position: absolute;
        right: 0px;
        top: 3px;
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        gap: 2px;
        z-index: 1;
        opacity: var(--gds-sidebar-actions-opacity, 1);
        pointer-events: auto;
        visibility: visible;
    }
    .gds-sidebar:not([data-actions-visible=\"true\"]) .gds-nav-item__actions,
    .gds-nav-item[data-actions-visible=\"false\"] .gds-nav-item__actions {
        pointer-events: none;
        visibility: hidden;
    }
    .gds-hover-slot__status {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 100%;
        transition: opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-hover-slot__action {
        opacity: 0;
        pointer-events: none;
        transition: opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-hover-slot[data-has-action=\"true\"]:hover .gds-hover-slot__action,
    .gds-hover-slot[data-has-action=\"true\"][data-revealed=\"true\"] .gds-hover-slot__action,
    .gds-nav-item:not([data-collapsed=\"true\"]):hover .gds-hover-slot[data-has-action=\"true\"] .gds-hover-slot__action,
    .gds-nav-item:not([data-collapsed=\"true\"]):focus-within .gds-hover-slot[data-has-action=\"true\"] .gds-hover-slot__action {
        opacity: 1;
        pointer-events: auto;
    }
    .gds-hover-slot:not([data-has-action=\"true\"]):hover .gds-hover-slot__status,
    .gds-hover-slot:not([data-has-action=\"true\"])[data-revealed=\"true\"] .gds-hover-slot__status,
    .gds-nav-item:not([data-collapsed=\"true\"]):hover .gds-hover-slot:not([data-has-action=\"true\"]) .gds-hover-slot__status {
        opacity: 1;
    }
    .gds-sidebar[data-collapsed=\"true\"] .gds-nav-item .gds-hover-slot,
    .gds-sidebar[data-collapsed=\"true\"] .gds-nav-item .gds-hover-slot .gds-hover-slot__action,
    .gds-nav-item[data-collapsed=\"true\"] .gds-hover-slot,
    .gds-nav-item[data-collapsed=\"true\"] .gds-hover-slot .gds-hover-slot__action {
        opacity: 0;
        pointer-events: none;
    }
    .gds-corner-action {
        position: absolute;
        top: 1px;
        right: 1px;
        opacity: 0;
        pointer-events: none;
        visibility: hidden;
        z-index: 2;
        transition: opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-sidebar[data-collapsed=\"true\"] .gds-corner-action,
    .gds-nav-item[data-collapsed=\"true\"] .gds-corner-action {
        visibility: visible;
    }
    .gds-sidebar[data-collapsed=\"true\"] .gds-corner-action[data-revealed=\"true\"],
    .gds-sidebar[data-collapsed=\"true\"] .gds-nav-item:hover .gds-corner-action,
    .gds-sidebar[data-collapsed=\"true\"] .gds-nav-item:focus-within .gds-corner-action,
    .gds-nav-item[data-collapsed=\"true\"] .gds-corner-action[data-revealed=\"true\"],
    .gds-nav-item[data-collapsed=\"true\"]:hover .gds-corner-action,
    .gds-nav-item[data-collapsed=\"true\"]:focus-within .gds-corner-action {
        opacity: 1;
        pointer-events: auto;
        visibility: visible;
    }
    .gds-sidebar[data-expanded=\"true\"] .gds-corner-action,
    .gds-nav-item[data-expanded=\"true\"] .gds-corner-action {
        opacity: 0;
        pointer-events: none;
        visibility: hidden;
    }
    .gds-corner-button {
        width: 16px;
        height: 16px;
        min-width: 16px;
        min-height: 16px;
        padding: 0px;
    }
    .gds-corner-button .gds-icon {
        width: 11px;
        height: 11px;
    }"
);

pub(super) fn install_navigation_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(NAVIGATION_SHEET, NAVIGATION_CSS);
}

/// Interactive borderless navigation pill supporting continuous `SidebarPresentation` width
/// interpolation (`x = 29px` dead-center collapsed rail alignment and stable `38px`/`44px` row
/// heights), trailing overlay actions, shortcuts, tooltips, and middle-click handling.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn NavItem(
    #[prop(into, default = Signal::stored_local(false))] selected: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] drop_target: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] disabled: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] compact: Signal<bool, LocalStorage>,
    #[prop(into, optional)] presentation: Option<Signal<SidebarPresentation, LocalStorage>>,
    #[prop(optional)] icon: Option<AnyView>,
    #[prop(optional)] label: Option<AnyView>,
    #[prop(optional)] details: Option<AnyView>,
    #[prop(optional)] trailing: Option<AnyView>,
    #[prop(optional)] command: Option<Command>,
    #[prop(optional)] shortcut: Option<Shortcut>,
    #[prop(optional)] reserve_trailing_keytip: Option<f32>,
    #[prop(into, optional)] tooltip: Option<Signal<String, LocalStorage>>,
    #[prop(default = true)] tooltip_enabled: bool,
    #[prop(optional)] on_click: Option<UnsyncCallback<()>>,
    #[prop(optional)] on_middle_click: Option<UnsyncCallback<()>>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    install_navigation_stylesheets();

    let sidebar_ctx = SidebarContext::current();
    let in_sidebar = sidebar_ctx.is_some();
    let effective_presentation = presentation
        .or_else(|| sidebar_ctx.map(|ctx| ctx.presentation))
        .unwrap_or_else(|| Signal::stored_local(SidebarPresentation::default()));

    let has_trailing = trailing.is_some();
    let trailing_reserve = (if has_trailing { ICON_BUTTON_SIZE } else { 0.0 })
        + reserve_trailing_keytip.unwrap_or(0.0);

    provide_local_context(NavItemContext { trailing_reserve });

    let collapsed_icon_inset =
        (COLLAPSED_SIDEBAR_WIDTH - 2.0 * SIDEBAR_PADDING - SIDEBAR_ICON_SIZE) * 0.5;
    let standalone_attrs = if in_sidebar {
        Attrs::new()
    } else {
        Attrs::new()
            .custom_property(CustomPropertyName::new("gds-sidebar-progress"), move || {
                Some(format!("{:.4}", effective_presentation.get().progress()))
            })
            .custom_property(
                CustomPropertyName::new("gds-sidebar-label-opacity"),
                move || {
                    Some(format!(
                        "{:.3}",
                        effective_presentation.get().label_opacity()
                    ))
                },
            )
            .custom_property(
                CustomPropertyName::new("gds-sidebar-label-pct"),
                move || {
                    Some(format!(
                        "{:.2}%",
                        effective_presentation.get().label_opacity() * 100.0
                    ))
                },
            )
            .custom_property(
                CustomPropertyName::new("gds-sidebar-selected-details-pct"),
                move || {
                    Some(format!(
                        "{:.2}%",
                        effective_presentation.get().label_opacity() * 85.0
                    ))
                },
            )
            .custom_property(
                CustomPropertyName::new("gds-sidebar-actions-opacity"),
                move || {
                    Some(format!(
                        "{:.3}",
                        effective_presentation.get().actions_opacity()
                    ))
                },
            )
            .custom_property(
                CustomPropertyName::new("gds-nav-item-pad-left"),
                move || {
                    let inset = effective_presentation
                        .get()
                        .interpolate(ITEM_GAP, collapsed_icon_inset);
                    Some(format!("{inset:.2}px"))
                },
            )
            .custom_property(
                CustomPropertyName::new("gds-nav-item-pad-right"),
                move || {
                    let pad = effective_presentation.get().interpolate(ITEM_GAP, 0.0);
                    Some(format!("{pad:.2}px"))
                },
            )
            .custom_property(CustomPropertyName::new("gds-nav-item-gap"), move || {
                let gap = effective_presentation.get().interpolate(ITEM_GAP, 0.0);
                Some(format!("{gap:.2}px"))
            })
            .attribute(AttrName::new("data-collapsed"), move || {
                effective_presentation
                    .get()
                    .is_collapsed()
                    .then(|| "true".to_owned())
            })
            .attribute(AttrName::new("data-expanded"), move || {
                (!effective_presentation.get().is_collapsed()).then(|| "true".to_owned())
            })
            .attribute(AttrName::new("data-actions-visible"), move || {
                Some(
                    if effective_presentation.get().actions_visible() {
                        "true"
                    } else {
                        "false"
                    }
                    .to_owned(),
                )
            })
    };
    let resolved_ref = node_ref.unwrap_or_default();

    let middle_armed = Rc::new(Cell::new(false));
    let middle_down = Rc::clone(&middle_armed);
    let middle_up = Rc::clone(&middle_armed);
    let middle_leave = Rc::clone(&middle_armed);

    let structured_body = if icon.is_some() || label.is_some() || details.is_some() {
        let label_view = label.map(|lbl| AnyView::new(view! { NavItemLabel {{lbl}} }));
        let details_view = details.map(|det| AnyView::new(view! { NavItemDetails {{det}} }));
        Some(AnyView::new(view! {
            NavItemContent {
                NavItemHeader {
                    {icon}
                    {label_view}
                }
                {details_view}
            }
        }))
    } else {
        None
    };

    let trailing_overlay = trailing.map(|trail| {
        AnyView::new(view! {
            NavItemActions {{trail}}
        })
    });

    let row_view = AnyView::new(view! {
        row(
            class = "gds-nav-item",
            node_ref = resolved_ref,
            tabindex = Focus::Sequential,
            {..standalone_attrs},
            attr:data-selected = move || selected.get().then(|| "true".to_owned()),
            attr:data-drop-target = move || drop_target.get().then(|| "true".to_owned()),
            attr:data-disabled = move || disabled.get().then(|| "true".to_owned()),
            attr:data-compact = move || compact.get().then(|| "true".to_owned()),
            a11y:role = Role::Button,
            a11y:selected = move || selected.get(),
            a11y:disabled = move || disabled.get(),
            on:pointer_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerDown>| {
                if on_middle_click.is_some()
                    && !disabled.get_untracked()
                    && ev.button == Some(PointerButton::Middle)
                {
                    middle_down.set(true);
                    ev.stop_propagation();
                }
            },
            on:pointer_up = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::PointerUp>| {
                if middle_up.replace(false)
                    && !disabled.get_untracked()
                    && ev.button == Some(PointerButton::Middle)
                    && let Some(ref cb) = on_middle_click
                {
                    ev.stop_propagation();
                    cb.run(());
                }
            },
            on:pointer_leave = move |_| {
                middle_leave.set(false);
            },
            on:click = move |_| {
                if !disabled.get_untracked()
                    && let Some(ref cb) = on_click
                {
                    cb.run(());
                }
            },
            class = class,
            {..attrs}
        ) {
            {structured_body}
            {children.map(Children::into_view_once)}
            {trailing_overlay}
        }
    });

    let resolved_shortcut = shortcut.or_else(|| command.map(Shortcut::command));
    if tooltip_enabled && let Some(tip_sig) = tooltip {
        AnyView::new(view! {
            ShortcutHint(label = tip_sig, shortcut = resolved_shortcut) {
                {row_view}
            }
        })
    } else {
        row_view
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use zgui::app::Fonts;
    use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
    use zgui::geom::{CssPx, Device, DevicePx, Point, Rect, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{RwSignal, UnsyncCallback, flush};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::view;
    use zgui::vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Role, Timestamp};
    use zgui_platform_headless::Harness;

    use super::{
        NavItemActionsProps, NavItemContentProps, NavItemDetailsProps, NavItemHeaderProps,
        NavItemLabelProps, NavItemProps, corner_action, corner_button, hover_action, pinned_icon,
    };
    use crate::ui::button::IconButtonProps;
    use crate::ui::icon::{IconName, IconProps};
    use crate::ui::sidebar::{
        SideBarProps, SideBarSectionContentProps, SideBarSectionProps, SidebarPresentation,
    };

    struct NoopSink;

    impl TextureSink for NoopSink {
        fn create_texture(
            &mut self,
            _: TextureId,
            _: zgui::geom::Size<i32, Device>,
            _: TextureFormat,
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn write_texture(
            &mut self,
            _: TextureId,
            _: Rect<i32, Device>,
            _: TextureFormat,
            _: &[u8],
        ) -> Result<(), SinkError> {
            Ok(())
        }

        fn destroy_texture(&mut self, _: TextureId) {}
    }

    struct NoopRenderer {
        target: Option<RenderTarget>,
        sink: NoopSink,
    }

    impl Renderer for NoopRenderer {
        fn capabilities(&self) -> RenderCapabilities {
            RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<RenderTarget> {
            self.target
        }

        fn draw(&mut self, _: &zgui::scene::Scene, _: &zgui::bits::DamageSet) -> FrameOutcome {
            FrameOutcome::Presented(FrameStats::default())
        }

        fn register_external(&mut self, _: ExternalTexture) -> TextureHandle {
            TextureHandle(1)
        }

        fn release_external(&mut self, _: TextureHandle) {}

        fn memory(&self) -> MemoryReport {
            MemoryReport::ZERO
        }

        fn texture_sink(&mut self) -> &mut dyn TextureSink {
            &mut self.sink
        }
    }

    fn noop_renderer(
        _: &Arc<dyn Surface>,
        target: RenderTarget,
    ) -> Result<Box<dyn Renderer>, AppError> {
        Ok(Box::new(NoopRenderer {
            target: Some(target),
            sink: NoopSink,
        }))
    }

    #[test]
    fn nav_item_projects_selection_middle_click_and_corner_button_stops_click_propagation() {
        let row_clicks = Rc::new(Cell::new(0usize));
        let middle_clicks = Rc::new(Cell::new(0usize));
        let corner_clicks = Rc::new(Cell::new(0usize));
        let seen_a11y: Arc<Mutex<Option<(Role, Option<bool>)>>> = Arc::new(Mutex::new(None));

        let row_clicks_ref = Rc::clone(&row_clicks);
        let middle_clicks_ref = Rc::clone(&middle_clicks);
        let corner_clicks_ref = Rc::clone(&corner_clicks);
        let seen_a11y_ref = Arc::clone(&seen_a11y);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(240.0),
                DevicePx(120.0),
            )));
            harness.settle(32);

            let surface = harness
                .platform()
                .offscreens()
                .first()
                .cloned()
                .expect("surface");
            for update in surface.a11y_log() {
                for (_, node) in update.nodes {
                    if node.role() == Role::Button && node.label() == Some("Inbox") {
                        *seen_a11y_ref.lock().expect("lock") =
                            Some((node.role(), node.is_selected()));
                    }
                }
            }

            // Click the revealed corner close button at top-right of the 38x38 collapsed item.
            let corner_pos = Point::new(CssPx(30.0), CssPx(9.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(corner_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(corner_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            // Middle-click the row body -> invokes on_middle_click without firing primary on:click.
            let center_pos = Point::new(CssPx(12.0), CssPx(26.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(center_pos).with_button(PointerButton::Middle),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(center_pos).with_button(PointerButton::Middle),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(240.0, 120.0)
            .run_on(driver, move || {
                let row_clicks = Rc::clone(&row_clicks_ref);
                let middle_clicks = Rc::clone(&middle_clicks_ref);
                let corner_clicks = Rc::clone(&corner_clicks_ref);
                let revealed = RwSignal::new_local(true);
                let close_btn = corner_button(
                    IconName::X,
                    "Close tab",
                    UnsyncCallback::new(move |()| corner_clicks.set(corner_clicks.get() + 1)),
                );
                view! {
                    column(style:width = "38px") {
                        NavItem(
                            selected = true,
                            drop_target = true,
                            presentation = SidebarPresentation::from_collapsed(true),
                            a11y:label = "Inbox",
                            on_middle_click = UnsyncCallback::new(move |()| {
                                middle_clicks.set(middle_clicks.get() + 1);
                            }),
                            on:click = move |_| row_clicks.set(row_clicks.get() + 1)
                        ) {
                            NavItemContent {
                                NavItemHeader {
                                    {pinned_icon(IconName::Folder, true)}
                                    NavItemLabel { "Inbox" }
                                }
                            }
                            NavItemActions {
                                {hover_action(view! { text { "2" } }, None)}
                            }
                            {corner_action(close_btn, revealed.into())}
                        }
                    }
                }
            })
            .expect("nav item runs headlessly");

        assert_eq!(
            *seen_a11y.lock().expect("lock"),
            Some((Role::Button, Some(true)))
        );
        assert_eq!(corner_clicks.get(), 1);
        assert_eq!(middle_clicks.get(), 1);
        assert_eq!(row_clicks.get(), 0);
    }

    #[test]
    fn collapsed_rail_preserves_44px_two_line_height_and_centers_icon_at_x_29() {
        let row_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
        let icon_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
        let pres_slot: Arc<
            Mutex<Option<RwSignal<SidebarPresentation, zgui::reactive::LocalStorage>>>,
        > = Arc::new(Mutex::new(None));

        let row_driver = Arc::clone(&row_slot);
        let icon_driver = Arc::clone(&icon_slot);
        let pres_driver = Arc::clone(&pres_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(300.0),
                DevicePx(200.0),
            )));
            harness.settle(32);

            let row_node = row_driver.lock().expect("lock").expect("row ref");
            let icon_node = icon_driver.lock().expect("lock").expect("icon ref");
            let expanded_bounds = row_node.window_bounds().expect("expanded bounds");
            let expanded_h = expanded_bounds.size.height.0 / row_node.scale().max(1.0);
            assert!(
                (expanded_h - 44.0).abs() < 0.5,
                "2-line NavItem must lay out at 44px when expanded, got {expanded_h}"
            );

            // Switch to collapsed 58px presentation -> row height MUST remain 44px and icon center x == 29px!
            if let Some(sig) = *pres_driver.lock().expect("lock") {
                sig.set(SidebarPresentation::from_collapsed(true));
                flush();
            }
            harness.settle(32);

            let collapsed_bounds = row_node.window_bounds().expect("collapsed bounds");
            let collapsed_h = collapsed_bounds.size.height.0 / row_node.scale().max(1.0);
            let icon_bounds = icon_node.window_bounds().expect("icon bounds");
            let collapsed_cx = (icon_bounds.origin.x.0 + icon_bounds.size.width.0 * 0.5)
                / icon_node.scale().max(1.0);
            assert!(
                (collapsed_h - 44.0).abs() < 0.5,
                "2-line NavItem height MUST stay 44px when collapsed (never jump to 38px), got {collapsed_h}"
            );
            assert!(
                (collapsed_cx - 29.0).abs() < 0.5,
                "18px icon inside 10px-padded 58px rail must sit dead-center at x=29, got {collapsed_cx}"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(300.0, 200.0)
            .run_on(driver, move || {
                let pres = RwSignal::new_local(SidebarPresentation::default());
                *pres_slot.lock().expect("lock") = Some(pres);

                let row_ref = NodeRef::new();
                let icon_ref = NodeRef::new();
                *row_slot.lock().expect("lock") = Some(row_ref);
                *icon_slot.lock().expect("lock") = Some(icon_ref);

                view! {
                    SideBar(presentation = Signal::from(pres)) {
                        SideBarSection(style:padding = "0 10px") {
                            SideBarSectionContent {
                                NavItem(node_ref = row_ref, presentation = Signal::from(pres)) {
                                    box(
                                        node_ref = icon_ref,
                                        style:width = "18px",
                                        style:height = "18px",
                                        style:flex-shrink = "0"
                                    ) {
                                        Icon(name = IconName::Folder, size = 18.0)
                                    }
                                    NavItemContent {
                                        NavItemLabel { "Documents" }
                                        NavItemDetails { "/home/atlas/Documents" }
                                    }
                                }
                            }
                        }
                    }
                }
            })
            .expect("collapsed geometry test runs");
    }

    #[test]
    fn corner_action_is_suppressed_when_expanded_and_reveals_on_collapsed_hover() {
        let row_clicks = Rc::new(Cell::new(0usize));
        let expanded_clicks = Rc::new(Cell::new(0usize));
        let corner_clicks = Rc::new(Cell::new(0usize));
        let pres_slot: Arc<
            Mutex<Option<RwSignal<SidebarPresentation, zgui::reactive::LocalStorage>>>,
        > = Arc::new(Mutex::new(None));
        let revealed_slot: Arc<Mutex<Option<RwSignal<bool, zgui::reactive::LocalStorage>>>> =
            Arc::new(Mutex::new(None));

        let row_clicks_driver = Rc::clone(&row_clicks);
        let expanded_clicks_driver = Rc::clone(&expanded_clicks);
        let corner_clicks_driver = Rc::clone(&corner_clicks);
        let row_clicks_view = Rc::clone(&row_clicks);
        let expanded_clicks_view = Rc::clone(&expanded_clicks);
        let corner_clicks_view = Rc::clone(&corner_clicks);
        let pres_driver = Arc::clone(&pres_slot);
        let revealed_driver = Arc::clone(&revealed_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(260.0),
                DevicePx(120.0),
            )));
            harness.settle(32);

            // 1. Expanded (240px SideBar -> 220px NavItem at x=10..230, y=0..38):
            // Hovering the row (and even forcing revealed=true on CornerAction) must NEVER
            // expose or hit-test CornerButton; clicking the shared top-right action overlap
            // at (221, 9) must hit the expanded HoverAction IconButton instead.
            let expanded_action_pos = Point::new(CssPx(221.0), CssPx(9.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(expanded_action_pos),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(expanded_action_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(expanded_action_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);
            assert_eq!(
                corner_clicks_driver.get(),
                0,
                "expanded NavItem hover must never activate CornerAction"
            );
            assert_eq!(
                expanded_clicks_driver.get(),
                1,
                "expanded NavItem hover must activate trailing HoverAction"
            );
            assert_eq!(row_clicks_driver.get(), 0);

            // 2. Collapse to 58px rail (NavItem at x=10..48, y=0..38) with revealed=false:
            // Hovering the collapsed row must reveal CornerAction at (39, 9) while keeping
            // trailing HoverAction hidden.
            if let Some(rev) = *revealed_driver.lock().expect("lock") {
                rev.set(false);
            }
            if let Some(pres) = *pres_driver.lock().expect("lock") {
                pres.set(SidebarPresentation::from_collapsed(true));
                flush();
            }
            harness.settle(32);

            let collapsed_corner_pos = Point::new(CssPx(39.0), CssPx(9.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Moved,
                event: PointerEvent::mouse(collapsed_corner_pos),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(collapsed_corner_pos)
                    .with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(collapsed_corner_pos)
                    .with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(260.0, 120.0)
            .run_on(driver, move || {
                let row_clicks = Rc::clone(&row_clicks_view);
                let expanded_clicks = Rc::clone(&expanded_clicks_view);
                let corner_clicks = Rc::clone(&corner_clicks_view);

                let pres = RwSignal::new_local(SidebarPresentation::from_width(240.0, 240.0, 58.0));
                let corner_revealed = RwSignal::new_local(true);
                *pres_slot.lock().expect("lock") = Some(pres);
                *revealed_slot.lock().expect("lock") = Some(corner_revealed);

                let expanded_close = AnyView::new(view! {
                    IconButton(
                        icon = IconName::X,
                        label = "Close expanded",
                        on:click = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                            ev.stop_propagation();
                            expanded_clicks.set(expanded_clicks.get() + 1);
                        }
                    )
                });
                let collapsed_close = corner_button(
                    IconName::X,
                    "Close collapsed",
                    UnsyncCallback::new(move |()| corner_clicks.set(corner_clicks.get() + 1)),
                );

                view! {
                    SideBar(presentation = Signal::from(pres)) {
                        SideBarSection(style:padding = "0 10px") {
                            SideBarSectionContent {
                                NavItem(
                                    presentation = Signal::from(pres),
                                    a11y:label = "folder_a",
                                    on:click = move |_| row_clicks.set(row_clicks.get() + 1)
                                ) {
                                    {pinned_icon(IconName::Folder, false)}
                                    NavItemContent {
                                        NavItemHeader {
                                            NavItemLabel { "folder_a" }
                                        }
                                    }
                                    NavItemActions {
                                        {hover_action(view! { box() }, Some(expanded_close))}
                                    }
                                    {corner_action(collapsed_close, corner_revealed.into())}
                                }
                            }
                        }
                    }
                }
            })
            .expect("expanded vs collapsed corner action test runs");

        assert_eq!(expanded_clicks.get(), 1);
        assert_eq!(corner_clicks.get(), 1);
        assert_eq!(row_clicks.get(), 0);
    }

    #[test]
    fn nav_items_never_squish_vertically_inside_collapsing_parent_section() {
        let refs_slot: Arc<Mutex<Vec<NodeRef>>> = Arc::new(Mutex::new(Vec::new()));
        let height_slot: Arc<Mutex<Option<RwSignal<f32, zgui::reactive::LocalStorage>>>> =
            Arc::new(Mutex::new(None));

        let refs_driver = Arc::clone(&refs_slot);
        let height_driver = Arc::clone(&height_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(260.0),
                DevicePx(260.0),
            )));
            harness.settle(32);

            // Collapse the section container mid-flight to 60px and then 14px (far smaller than
            // 5 * 38px = 190px): every NavItem must retain its full 38px height (`flex-shrink: 0`).
            for collapsed_h in [206.0_f32, 60.0, 14.0] {
                if let Some(sig) = *height_driver.lock().expect("lock") {
                    sig.set(collapsed_h);
                    flush();
                }
                harness.settle(16);

                let refs = refs_driver.lock().expect("lock").clone();
                assert_eq!(refs.len(), 5);
                for (idx, row_ref) in refs.iter().enumerate() {
                    let bounds = row_ref.window_bounds().expect("nav item bounds");
                    let h = bounds.size.height.0 / row_ref.scale().max(1.0);
                    assert!(
                        (h - 38.0).abs() <= 0.5,
                        "NavItem {idx} vertically squished to {h}px at container height {collapsed_h}px"
                    );
                }
            }

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_size(260.0, 260.0)
            .run_on(driver, move || {
                let section_h = RwSignal::new_local(206.0_f32);
                *height_slot.lock().expect("lock") = Some(section_h);

                let r0 = NodeRef::new();
                let r1 = NodeRef::new();
                let r2 = NodeRef::new();
                let r3 = NodeRef::new();
                let r4 = NodeRef::new();
                *refs_slot.lock().expect("lock") = vec![r0, r1, r2, r3, r4];

                view! {
                    SideBar {
                        column(
                            style:overflow = "hidden",
                            style:gap = "4px",
                            style:height = move || Some(format!("{:.1}px", section_h.get())),
                        ) {
                            NavItem(node_ref = r0, style:height = "38px", label = AnyView::new(view! { text { "Recent" } }))
                            NavItem(node_ref = r1, style:height = "38px", label = AnyView::new(view! { text { "Starred" } }))
                            NavItem(node_ref = r2, style:height = "38px", label = AnyView::new(view! { text { "Home" } }))
                            NavItem(node_ref = r3, style:height = "38px", label = AnyView::new(view! { text { "Network" } }))
                            NavItem(node_ref = r4, style:height = "38px", label = AnyView::new(view! { text { "Trash" } }))
                        }
                    }
                }
            })
            .expect("collapsing section nav item test runs");
    }
}
