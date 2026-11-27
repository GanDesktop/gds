//! Root workspace frame, help subpage, content host, page action bar, and status summary.

mod grid;
mod list;
mod prompts;
#[cfg(test)]
pub(in crate::pages::workspace) mod tests;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gds::app::Gds;
use gds::appearance::motion::{WORKSPACE_DURATION_MS, ease_out_cubic, use_transition};
use gds::i18n;
use gds::ui::IconName;
use gds::ui::button::{ButtonSize, IconButtonProps};
use gds::ui::feedback::notifications::{NotificationHostProps, error as notify_error};
use gds::ui::frame::{FooterProps, FrameProps, ToolbarProps, WorkspaceFrameProps};
use gds::ui::interaction::{
    keytips::KeytipHostProps,
    region::{is_focus_visible, surface_with_shortcut, use_keyboard_modality},
};
use gds::ui::page::{
    ContentPageProps, PageActionsProps, PageContentProps, PageHeaderProps, PageTableAreaProps,
    PageTitleProps,
};
use gds::ui::window::{HeightTier, WindowControlsProps};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, Signal, enter_non_reactive_zone, on_cleanup_local};
use zgui::view::time::{FrameHandle, Timers};
use zgui::view::{Scoped, current_host, events, install_stylesheet};
use zgui::vocab::{Role, Timestamp};
use zgui::{css, view};
use zgui_ui::{ButtonProps, ButtonVariant, SpinnerProps};

use super::{
    chrome::{Chrome, subpage_toolbar_title},
    collections::{CollectionBodyState, Collections},
    keyboard::Keyboard,
};
use crate::{
    entities::{file::ViewMode, location::Location},
    pages::workspace::{
        navigation::Navigator,
        panes::{DragHost, Inspection, MenuHost, PickerView, StatusBar, TerminalDock},
        projection::{Geometry, Rows, UiState},
        state::{
            context::{Context, local_memo},
            menus::Surface,
            model::{AddressPresentation, Contents, Pane},
        },
    },
    shared::ui::actions::Command,
    widgets::help::FilesHelpProps,
};

const DETAILS_WIDTH: f32 = 280.0;
const TAB_ENTER_INITIAL_OPACITY: f32 = 0.88;
const WORKSPACE_SHEET: &str = "gds-files-workspace";

const WORKSPACE_CSS: &str = css!(
    r#"
    .gds-files-root {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-files-root:focus {
        outline: none;
    }

    .gds-files-frame {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
    }

    .gds-frame-content {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-workspace-outer {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-workspace-column {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-workspace-frame {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-settings-host {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-content-shell {
        display: flex;
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-title-text {
        min-width: 0px;
        width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-page-actions {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-page-action-strip {
        display: flex;
        align-items: center;
        gap: 4px;
    }

    .gds-files-column-controls {
        display: flex;
        align-items: center;
        gap: 4px;
    }

    .gds-files-content-focus,
    .gds-files-content-focus.gds-region-surface,
    .gds-files-content-focus.gds-region-surface[data-focus-visible="true"],
    .gds-files-content-focus.gds-region-surface[data-focus-visible="true"]:focus,
    .gds-files-content-focus.gds-region-surface[data-focus-visible="true"]:focus-visible {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
        outline: none;
    }

    .gds-files-details-host {
        display: flex;
        flex-direction: column;
        flex: none;
        height: 100%;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-details-pane {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 280px;
        min-width: 280px;
        min-height: 0px;
        overflow: hidden;
    }

    .gds-files-state-center {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        min-width: 0px;
        min-height: 0px;
        align-items: center;
        justify-content: center;
        gap: 12px;
        padding: 24px;
        text-align: center;
        color: var(--gds-muted-foreground);
    }

    .gds-files-state-center[data-focus-visible="true"] {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-menu-wrap {
        display: flex;
        min-width: 0px;
    }

    .gds-files-menu-anchor {
        position: relative;
        display: flex;
        align-items: center;
    }

    .gds-files-menu-item-row {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-menu-item-label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-menu-content,
    .gds-menu-content.gds-menu-content.gds-files-menu-content {
        min-width: 160px;
        max-width: 340px;
        max-height: min(calc(100vh - 18px), var(--zui-popper-available-height, calc(100vh - 18px)));
        overflow-y: auto;
    }

    .gds-files-context-portal-anchor {
        position: fixed;
        inset: 0px;
        pointer-events: none;
    }

    .gds-files-context-point {
        position: fixed;
        width: 1px;
        height: 1px;
        pointer-events: none;
    }

    .gds-files-hidden {
        display: none;
    }

    .gds-files-footer-scope {
        width: 100%;
        height: 100%;
    }

    .gds-files-footer-status {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0px;
    }

    .gds-files-footer-status-text {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        white-space: nowrap;
    }

    .gds-files-footer-controls {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
    }
    "#
);

fn install_workspace_stylesheets() {
    gds::ui::styles::install_ui_stylesheets();
    if current_host().is_some() {
        install_stylesheet(WORKSPACE_SHEET, WORKSPACE_CSS);
        list::install_list_stylesheet();
        grid::install_grid_stylesheet();
    }
}

/// Animates content opacity `0.88 -> 1.0` over `160ms` (`motion.md`) when switching tabs, while
/// keeping initial mount and reduced-motion switches at `1.0`.
fn use_tab_enter_opacity(active_tab_id: Signal<u64, LocalStorage>) -> Signal<f32, LocalStorage> {
    #[derive(Default)]
    struct TabFadeState {
        started_at: Cell<Option<Timestamp>>,
        last_frame_at: Cell<Option<Timestamp>>,
        frame_handle: RefCell<Option<FrameHandle>>,
    }

    impl TabFadeState {
        fn reset(&self) {
            self.started_at.set(None);
            self.last_frame_at.set(None);
            let old = self.frame_handle.borrow_mut().take();
            drop(old);
        }

        fn step(&self, at: Timestamp) -> (f32, bool) {
            let start = match self.started_at.get() {
                Some(start) => start,
                None => {
                    let base = self.last_frame_at.get().unwrap_or(at);
                    self.started_at.set(Some(base));
                    base
                }
            };
            self.last_frame_at.set(Some(at));
            let elapsed = (at - start).as_secs_f32();
            let duration = (WORKSPACE_DURATION_MS as f32) / 1000.0;
            let progress = (elapsed / duration).clamp(0.0, 1.0);
            if progress >= 1.0 {
                self.reset();
                (1.0, true)
            } else {
                let eased = ease_out_cubic(progress);
                let current = TAB_ENTER_INITIAL_OPACITY + (1.0 - TAB_ENTER_INITIAL_OPACITY) * eased;
                (current, false)
            }
        }

        fn schedule_frame(self: &Rc<Self>, timers: &Timers, opacity: RwSignal<f32, LocalStorage>) {
            let timers_next = timers.clone();
            let state_next = Rc::clone(self);
            let handle = timers.request_frame(move |at| {
                let reduced = Gds::try_current()
                    .is_some_and(|gds| gds.appearance().motion().reduced_untracked());
                if reduced {
                    state_next.reset();
                    opacity.set(1.0);
                    return;
                }
                let (value, done) = state_next.step(at);
                opacity.set(value);
                if !done {
                    state_next.schedule_frame(&timers_next, opacity);
                }
            });
            let previous = self.frame_handle.borrow_mut().replace(handle);
            drop(previous);
        }

        fn restart(self: &Rc<Self>, timers: Option<&Timers>, opacity: RwSignal<f32, LocalStorage>) {
            let reduced =
                Gds::try_current().is_some_and(|gds| gds.appearance().motion().reduced_untracked());
            self.reset();
            if reduced {
                opacity.set(1.0);
            } else if let Some(timers) = timers {
                opacity.set(TAB_ENTER_INITIAL_OPACITY);
                self.schedule_frame(timers, opacity);
            } else {
                opacity.set(1.0);
            }
        }
    }

    let opacity = RwSignal::new_local(1.0_f32);
    let timers = zgui::reactive::is_ui_thread()
        .then(Timers::current)
        .flatten();
    let state = Rc::new(TabFadeState::default());

    let watch_state = Rc::clone(&state);
    let tab_fx = RenderEffect::new(move |prev_tab: Option<u64>| {
        let current = active_tab_id.get();
        if prev_tab.is_none_or(|prev| prev == current) {
            return current;
        }
        watch_state.restart(timers.as_ref(), opacity);
        current
    });

    on_cleanup_local(move || {
        drop(tab_fx);
        state.reset();
    });

    opacity.into()
}

#[derive(Clone, PartialEq)]
enum FileBody {
    Loading(Location),
    Empty(&'static str),
    Network,
    Collection(CollectionBodyState),
    List,
    Grid,
}

pub(in crate::pages::workspace) struct WorkspaceView<'a>(pub &'a Context);

impl WorkspaceView<'_> {
    pub(super) fn view(&self) -> AnyView {
        let ctx = self.0.clone();
        AnyView::new(Scoped::new(move || WorkspaceView(&ctx).view_scoped()))
    }

    fn view_scoped(&self) -> impl IntoView + use<> {
        install_workspace_stylesheets();
        let ctx = self.0;

        let keytip_active = RwSignal::new_local(false);
        let hint_context = ctx.clone();
        let keytip_guard_fx = RenderEffect::new(move |_| {
            let menu_open = !matches!(hint_context.0.menus.surface.get(), Surface::Closed);
            if menu_open && keytip_active.get_untracked() {
                keytip_active.set(false);
            }
        });
        on_cleanup_local(move || drop(keytip_guard_fx));

        // `WindowHandle::size()` is already in `CssPx` and starts `(0.0, 0.0)` before Wayland
        // configure; ignore non-positive dimensions and do not divide by scale (#114).
        let width_sig = ctx.0.window_width;
        let height_sig = ctx.0.window_height;
        let apply_window_size = move |width: f32, height: f32| {
            if width.is_finite() && width > 0.0 {
                width_sig.set(width);
            }
            if height.is_finite() && height > 0.0 {
                height_sig.set(height);
            }
        };
        if let Some(window) = ctx.0.window.clone() {
            let size = window.size();
            let win_fx = RenderEffect::new(move |_| {
                let sz = size.get();
                apply_window_size(sz.width.0, sz.height.0);
            });
            on_cleanup_local(move || drop(win_fx));
        } else {
            let root = ctx.0.root_focus;
            let root_size = root.observe_border_size();
            let root_fx = RenderEffect::new(move |_| {
                if let Some(sz) = root_size.get() {
                    let s = root.scale().max(1.0);
                    apply_window_size(sz.width.0 / s, sz.height.0 / s);
                }
            });
            on_cleanup_local(move || drop(root_fx));
        }

        let help = local_memo({
            let ctx = ctx.clone();
            move || ctx.pane() == Pane::Help
        });

        let root_focus = ctx.0.root_focus;
        let notice_sig = ctx.0.notice;

        let keyboard = ctx.clone();
        let init_focus_ctx = ctx.clone();
        let mouse = ctx.clone();
        let main_ctx = ctx.clone();
        let prompt_ctx = ctx.clone();
        let connector_view = ctx.0.connector.view();
        let menu_overlay = MenuHost(ctx).menu_overlay();

        view! {
            KeytipHost(active = Some(keytip_active)) {
                NotificationHost {
                    {{
                        let notice_fx = RenderEffect::new(move |_| {
                            if let Some(message) = notice_sig.get() {
                                let _z = enter_non_reactive_zone();
                                notify_error(message);
                            }
                        });
                        on_cleanup_local(move || drop(notice_fx));

                        let prompt_dialog = prompts::prompt_dialogs(&prompt_ctx);

                        let shortcut_guard: Rc<RefCell<Option<zgui::view::WindowShortcut>>> =
                            Rc::new(RefCell::new(None));
                        let shortcut_guard_init = Rc::clone(&shortcut_guard);
                        let shortcut_guard_fx = Rc::clone(&shortcut_guard);
                        let shortcut_fx = RenderEffect::new(move |_| {
                            if root_focus.get().is_some() && shortcut_guard_fx.borrow().is_none() {
                                *shortcut_guard_fx.borrow_mut() = root_focus.window_shortcut();
                            }
                        });
                        on_cleanup_local(move || {
                            drop(shortcut_fx);
                            drop(shortcut_guard.borrow_mut().take());
                        });

                        let content_focus = init_focus_ctx.0.content_focus;
                        let init_focus_fx = RenderEffect::new(move |_| {
                            if content_focus.get().is_some()
                                && init_focus_ctx.pane_untracked() == Pane::Files
                                && matches!(
                                    init_focus_ctx.0.address_presentation.get_untracked(),
                                    AddressPresentation::Breadcrumbs
                                )
                                && !gds::ui::keytips::has_active_modal_scope()
                                && current_host()
                                    .is_some_and(|host| host.focused().get_untracked().is_none())
                            {
                                content_focus.focus();
                            }
                        });
                        on_cleanup_local(move || drop(init_focus_fx));

                        let sync_shortcut = Scoped::new(move || {
                            if shortcut_guard_init.borrow().is_none() {
                                *shortcut_guard_init.borrow_mut() = root_focus.window_shortcut();
                            }
                        });

                        view! {
                            column(
                                node_ref = root_focus,
                                class = "gds-files-root",
                                tabindex = Focus::Sequential,
                                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                    Keyboard(&keyboard).key_down(ev);
                                },
                                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                                    DragHost(&mouse).mouse_navigation(ev);
                                }
                            ) {
                                {{move || {
                                    if help.get() {
                                        WorkspaceView(&main_ctx).help_view()
                                    } else {
                                        WorkspaceView(&main_ctx).workspace_view()
                                    }
                                }}}
                                {connector_view}
                                {menu_overlay}
                                {prompt_dialog}
                            }
                            {sync_shortcut}
                        }
                    }}
                }
            }
        }
    }

    fn help_view(&self) -> AnyView {
        let root_ref = self.0.0.settings_node;
        let back_click = self.0.clone();

        let back_button = AnyView::new(view! {
            row(class = "gds-files-menu-controls") {
                IconButton(
                    icon = IconName::ArrowLeft,
                    label = Signal::derive_local(|| i18n::text("page-return")),
                    shortcut = Some(Command::Cancel.shortcut()),
                    on:click = move |_| back_click.back_page()
                )
            }
        });
        let title_controls = AnyView::new(view! {
            row(class = "gds-files-toolbar") {
                {subpage_toolbar_title("help")}
            }
        });
        let help_body = AnyView::new(view! {
            box(
                node_ref = root_ref,
                tabindex = Focus::Sequential,
                class = "gds-files-settings-host"
            ) {
                FilesHelp()
            }
        });
        let empty_status = AnyView::new(view! { box() });

        AnyView::new(view! {
            Frame(without_sidebar = true, class = "gds-files-frame") {
                Toolbar(
                    menu = back_button,
                    controls = title_controls,
                    window_controls = AnyView::new(view! { WindowControls() })
                ) {}
                WorkspaceFrame(
                    content = help_body,
                    paint_content_surface = true,
                    class = "gds-files-workspace-frame"
                ) {}
                Footer(status = empty_status) {}
            }
        })
    }

    fn workspace_view(&self) -> AnyView {
        let ctx = self.0.clone();
        AnyView::new(Scoped::new(move || {
            WorkspaceView(&ctx).workspace_view_scoped()
        }))
    }

    fn workspace_view_scoped(&self) -> AnyView {
        let ctx = self.0;
        let pane_memo = local_memo({
            let ctx = ctx.clone();
            move || ctx.pane()
        });
        let sidebar_width = ctx.sidebar_width();
        let sidebar_pres = ctx.sidebar_presentation();

        let workspace_col = NodeRef::new();
        let workspace_size = workspace_col.observe_border_size();
        let window_scale = ctx.0.window.as_ref().map(|window| window.scale());
        let workspace_width = Signal::derive_local(move || {
            let size = workspace_size.get()?;
            let scale = window_scale.map_or_else(|| workspace_col.scale(), |scale| scale.get());
            Some(size.width.0 / scale)
        });
        let resize_ctx = ctx.clone();
        let term_size_fx = RenderEffect::new(move |_| {
            if let Some(sz) = workspace_size.get() {
                let s = workspace_col.scale().max(1.0);
                let w = sz.width.0 / s;
                let h = sz.height.0 / s;
                if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
                    TerminalDock(&resize_ctx).terminal_workspace_resized(w, h);
                }
            }
        });
        on_cleanup_local(move || drop(term_size_fx));

        let picker_is_none = Signal::derive_local({
            let ctx = ctx.clone();
            move || ctx.read(|state| state.picker.is_none())
        });

        let sidebar_ctx = ctx.clone();
        let content_ctx = ctx.clone();
        let terminal_ctx = ctx.clone();
        let status_ctx = ctx.clone();
        let footer_ctrl_ctx = ctx.clone();

        let chrome = Chrome(ctx);
        let toolbar_left = chrome.toolbar_leading();
        let toolbar_controls = chrome.toolbar_controls();
        let sidebar_scope = AnyView::new(view! { box(class = "gds-files-footer-scope") });
        let footer_status = AnyView::new(view! {
            {{move || {
                if pane_memo.get() == Pane::Files {
                    let sb = status_ctx.clone();
                    if let Some(picker_name) = PickerView(&sb).render_picker_name() {
                        picker_name
                    } else {
                        let status_controls = StatusBar(&sb).status_controls();
                        let filter_ctx = sb.clone();
                        let show_micro_filter = Signal::derive_local(move || {
                            Geometry(&filter_ctx).height_tier() == HeightTier::Micro
                                && UiState(&filter_ctx).filter_available()
                        });
                        let micro_ctx = sb.clone();
                        AnyView::new(view! {
                            row(class = "gds-files-footer-status") {
                                text(class = "gds-files-footer-status-text") {
                                    {move || WorkspaceView(&sb).status()}
                                }
                                Show(when = move || show_micro_filter.get()) {
                                    {{Chrome(&micro_ctx).micro_filter(false)}}
                                }
                                {status_controls}
                            }
                        })
                    }
                } else {
                    status_ctx.0.settings.footer_text()
                }
            }}}
        });
        let footer_controls = AnyView::new(view! {
            Show(when = move || pane_memo.get() == Pane::Files) {
                {{
                    PickerView(&footer_ctrl_ctx)
                        .render_picker_controls()
                        .unwrap_or_else(|| WorkspaceView(&footer_ctrl_ctx).footer_controls())
                }}
            }
        });

        AnyView::new(view! {
            Frame(sidebar_width = sidebar_width, class = "gds-files-frame") {
                Toolbar(
                    left = toolbar_left,
                    controls = toolbar_controls,
                    window_controls = AnyView::new(view! { WindowControls() })
                ) {}
                {{move || match pane_memo.get() {
                    Pane::Settings => {
                        let sb = sidebar_ctx.clone();
                        let cb = content_ctx.clone();
                        let pres_initial = sidebar_pres.get_untracked();
                        let settings_sidebar = sb.0.settings.sidebar(pres_initial);
                        let settings_content = AnyView::new(view! {
                            box(
                                node_ref = cb.0.settings_node,
                                tabindex = Focus::Sequential,
                                class = "gds-files-settings-host"
                            ) {
                                {{cb.0.settings.content()}}
                            }
                        });
                        AnyView::new(view! {
                            WorkspaceFrame(
                                sidebar = settings_sidebar,
                                content = settings_content,
                                paint_content_surface = true,
                                class = "gds-files-workspace-frame"
                            ) {}
                        })
                    }
                    Pane::Files => {
                        let pres_initial = sidebar_pres.get_untracked();
                        let files_sidebar = Chrome(&sidebar_ctx).sidebar(pres_initial);
                        let files_content =
                            WorkspaceView(&content_ctx).file_content(workspace_width);
                        let drag_overlay = DragHost(&content_ctx).drag_overlay();
                        let term_ctx = terminal_ctx.clone();
                        AnyView::new(view! {
                            WorkspaceFrame(
                                paint_content_surface = false,
                                class = "gds-files-workspace-outer"
                            ) {
                                column(node_ref = workspace_col, class = "gds-files-workspace-column") {
                                    WorkspaceFrame(
                                        sidebar = files_sidebar,
                                        content = files_content,
                                        paint_content_surface = true,
                                        class = "gds-files-workspace-frame"
                                    ) {}
                                    Show(when = move || picker_is_none.get()) {
                                        {{TerminalDock(&term_ctx).terminal_view(pres_initial)}}
                                    }
                                    {drag_overlay}
                                }
                            }
                        })
                    }
                    Pane::Help => AnyView::new(view! { box(class = "gds-files-hidden") }),
                }}}
                Footer(
                    sidebar_status = sidebar_scope,
                    status = footer_status,
                    controls = footer_controls
                ) {}
            }
        })
    }

    fn footer_controls(&self) -> AnyView {
        let ctx = self.0;
        let terminal_sig = ctx.0.terminal;
        let click_ctx = ctx.clone();
        let terminal_toggled = Signal::derive_local(move || terminal_sig.get());
        let terminal_label = Signal::derive_local(move || {
            i18n::text(if terminal_sig.get() {
                "files-terminal-hide"
            } else {
                "files-terminal-show"
            })
        });
        let terminal_enabled = local_memo({
            let ctx = ctx.clone();
            move || ctx.0.preferences.state().with(|p| p.terminal_enabled())
        });
        let visible = Signal::derive_local({
            let ctx = ctx.clone();
            move || ctx.read(|state| state.picker.is_none()) && terminal_enabled.get()
        });

        AnyView::new(view! {
            row(class = "gds-files-footer-controls") {
                Show(when = move || visible.get()) {
                    {{
                        let clk_ctx = click_ctx.clone();
                        let term_btn_ref = NodeRef::new();
                        #[cfg(test)]
                        tests::record_footer_terminal_node(term_btn_ref);
                        view! {
                            IconButton(
                                node_ref = term_btn_ref,
                                icon = IconName::Terminal,
                                size = ButtonSize::IconXs,
                                label = terminal_label,
                                toggled = Some(terminal_toggled),
                                shortcut = Some(Command::Terminal.shortcut()),
                                a11y:role = Role::Button,
                                on:click = move |_| TerminalDock(&clk_ctx).toggle_terminal()
                            )
                        }
                    }}
                }
            }
        })
    }

    fn control(
        &self,
        icon: IconName,
        label: &'static str,
        action: fn(&Context),
        command: Option<Command>,
        active: Option<Signal<bool, LocalStorage>>,
    ) -> AnyView {
        let click_ctx = self.0.clone();
        let shortcut = command.map(Command::shortcut);
        let active_sig = active.unwrap_or_else(|| Signal::stored_local(false));

        AnyView::new(view! {
            IconButton(
                icon = icon,
                label = Signal::derive_local(move || i18n::text(label)),
                active = active_sig,
                shortcut = shortcut,
                on:click = move |_| action(&click_ctx)
            )
        })
    }

    fn page_title(&self) -> String {
        let rows = Rows(self.0);
        self.0.read(|state| {
            let location = state.tab().navigation.location();
            if *location == Location::from(state.home.as_path()) {
                return i18n::text("home");
            }
            if let Location::Volume { identity, relative } = location
                && relative.is_empty()
                && let Some(volume) = rows.volume_at_location(identity)
            {
                return volume.name;
            }
            rows.location_title(location)
        })
    }

    fn file_content(&self, workspace_width: Signal<Option<f32>, LocalStorage>) -> AnyView {
        let ctx = self.0.clone();
        AnyView::new(Scoped::new(move || {
            WorkspaceView(&ctx).file_content_scoped(workspace_width)
        }))
    }

    fn file_content_scoped(&self, workspace_width: Signal<Option<f32>, LocalStorage>) -> AnyView {
        let ctx = self.0;
        let content_ctx = ctx.clone();
        let details_ctx = ctx.clone();
        let search_text_ctx = ctx.clone();
        let search_ime_ctx = ctx.clone();

        let content_ref = ctx.0.content_focus;

        let region_attrs =
            surface_with_shortcut(content_ref, Some(Command::FocusContent.shortcut()));
        let keyboard_modality = use_keyboard_modality(content_ref);
        let visible_sig = ctx.0.list_focus_visible;
        {
            let sync_ctx = ctx.clone();
            let list_ref = ctx.0.list;
            let focus_sync_fx = RenderEffect::new(move |_| {
                let direct_ring = is_focus_visible(content_ref);
                let is_kbd = keyboard_modality.get();
                if let Some(host) = current_host() {
                    let active = host.focused().get();
                    let self_id = content_ref.get();
                    let direct = self_id.is_some() && active == self_id;
                    let inside =
                        active.is_some_and(|id| self_id == Some(id) || content_ref.contains(id));
                    sync_ctx.0.list_focused.set(inside);
                    let on_surface = direct
                        || active.is_some_and(|id| {
                            list_ref.get_untracked() == Some(id) || list_ref.contains(id)
                        });
                    if !on_surface {
                        sync_ctx.reset_typeahead();
                    }
                    if direct_ring {
                        if !visible_sig.get_untracked() {
                            visible_sig.set(true);
                        }
                    } else if (!direct || !is_kbd) && visible_sig.get_untracked() {
                        visible_sig.set(false);
                    }
                } else if visible_sig.get_untracked() != direct_ring {
                    visible_sig.set(direct_ring);
                }
            });
            on_cleanup_local(move || drop(focus_sync_fx));
        }

        let active_tab_id = local_memo({
            let ctx = ctx.clone();
            move || ctx.read_structure(|state| state.tab().id)
        });
        let tab_opacity = use_tab_enter_opacity(active_tab_id);

        let body = local_memo({
            let ctx = ctx.clone();
            move || WorkspaceView(&ctx).file_body()
        });
        let is_network = local_memo(move || matches!(body.get(), FileBody::Network));

        let details_goal = Signal::derive_local({
            let ctx = ctx.clone();
            move || {
                if Geometry(&ctx).details_permitted()
                    && ctx.0.details.get()
                    && UiState(&ctx).details_available_structure()
                {
                    1.0_f32
                } else {
                    0.0_f32
                }
            }
        });
        let details_progress = use_transition(details_goal, WORKSPACE_DURATION_MS);
        let details_mounted = Signal::derive_local({
            let ctx = ctx.clone();
            move || Geometry(&ctx).details_permitted() && details_progress.get() > 0.001
        });

        let details_width = Signal::derive_local(move || {
            if details_mounted.get() {
                DETAILS_WIDTH * details_progress.get()
            } else {
                0.0
            }
        });

        let content_width = ctx.0.content_width;
        let sidebar_width = ctx.sidebar_width();
        // Use the same pane widths as layout so column and grid bindings settle before layout.
        let size_fx = RenderEffect::new(move |_| {
            if let Some(total) = workspace_width
                .get()
                .filter(|width| width.is_finite() && *width > 0.0)
            {
                let width = (total - sidebar_width.get() - details_width.get()).max(1.0);
                if width != content_width.get_untracked() {
                    content_width.set(width);
                }
            }
        });
        on_cleanup_local(move || drop(size_fx));

        let float_check_ctx = ctx.clone();
        let show_floating_filter = Signal::derive_local(move || {
            PickerView(&float_check_ctx).is_save_picker()
                && Geometry(&float_check_ctx).height_tier() == HeightTier::Micro
                && UiState(&float_check_ctx).filter_available()
        });
        let float_filter_ctx = ctx.clone();

        AnyView::new(view! {
            row(class = "gds-files-content-shell") {
                box(
                    node_ref = content_ref,
                    class = "gds-files-content-focus",
                    tabindex = Focus::Sequential,
                    style:opacity = move || {
                        let alpha = tab_opacity.get();
                        (alpha < 0.999).then(|| format!("{alpha:.3}"))
                    },
                    {..region_attrs},
                    on:text = move |ev: &mut EventCx<'_, events::Text>| {
                        if Keyboard(&search_text_ctx)
                            .handle_content_text_input(ev.text.as_str(), ev.timestamp)
                        {
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    },
                    on:ime_commit = move |ev: &mut EventCx<'_, events::ImeCommit>| {
                        if let zgui::vocab::ImeEvent::Commit(committed) = &**ev
                            && Keyboard(&search_ime_ctx)
                                .handle_content_text_input(committed.as_str(), ev.timestamp)
                        {
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    }
                ) {
                    {{move || {
                        if is_network.get() {
                            // `collections/network.rs` renders its own `ContentPage` and `HeightTier::Normal` `PageHeader`.
                            let picker_footer = PickerView(&content_ctx).picker_footer();
                            AnyView::new(view! {
                                column(class = "gds-files-workspace-column") {
                                    {{Collections(&content_ctx).network_content()}}
                                    {picker_footer}
                                }
                            })
                        } else {
                            let hdr_ctx = content_ctx.clone();
                            let notice_ctx = content_ctx.clone();
                            let body_ctx = content_ctx.clone();
                            let height_ctx = content_ctx.clone();
                            let normal_height = Signal::derive_local(move || {
                                Geometry(&height_ctx).height_tier() == HeightTier::Normal
                            });
                            let picker_footer = PickerView(&content_ctx).picker_footer();
                            AnyView::new(view! {
                                ContentPage {
                                    Show(when = move || normal_height.get()) {
                                        {{
                                            let title_ctx = hdr_ctx.clone();
                                            let actions_ctx = hdr_ctx.clone();
                                            let filter_view = Chrome(&hdr_ctx).folder_filter();
                                            view! {
                                                PageHeader {
                                                    PageTitle {
                                                        text(class = "gds-files-title-text") {
                                                            {move || WorkspaceView(&title_ctx).page_title()}
                                                        }
                                                    }
                                                    PageActions(class = "gds-files-page-actions") {
                                                        {filter_view}
                                                        {move || WorkspaceView(&actions_ctx).page_actions()}
                                                    }
                                                }
                                            }
                                        }}
                                    }
                                    {move || Collections(&notice_ctx).collection_notice()}
                                    PageContent {
                                        {{move || {
                                            let current = body.get();
                                            let _z = enter_non_reactive_zone();
                                            WorkspaceView(&body_ctx).render_file_body(current)
                                        }}}
                                        {picker_footer}
                                    }
                                }
                            })
                        }
                    }}}
                    Show(when = move || show_floating_filter.get()) {
                        {{Chrome(&float_filter_ctx).micro_filter(true)}}
                    }
                }
                Show(when = move || details_mounted.get()) {
                    {{
                        let d_ctx = details_ctx.clone();
                        view! {
                            box(
                                class = "gds-files-details-host",
                                style:width = move || Some(format!("{:.1}px", details_width.get()))
                            ) {
                                box(class = "gds-files-details-pane") {
                                    {{Inspection(&d_ctx).details_view()}}
                                }
                            }
                        }
                    }}
                }
            }
        })
    }

    fn file_body(&self) -> FileBody {
        if let Some(location) = self.0.read(|state| {
            let tab = state.tab();
            tab.navigation.is_loading().then(|| {
                tab.navigation
                    .pending_location()
                    .unwrap_or_else(|| tab.navigation.location())
                    .clone()
            })
        }) {
            return FileBody::Loading(location);
        }
        if let Some(collection) = Collections(self.0).collection_body_state() {
            return FileBody::Collection(collection);
        }
        self.0.read(|state| {
            let tab = state.tab();
            match &tab.contents {
                Contents::Remote { .. } if tab.rows.is_empty() && tab.navigation.is_loading() => {
                    FileBody::Loading(tab.navigation.location().clone())
                }
                Contents::Network { .. } => FileBody::Network,
                _ => {
                    if tab.rows.is_empty() {
                        FileBody::Empty(if self.0.0.filter.with(String::is_empty) {
                            "empty-directory"
                        } else {
                            "empty-filter"
                        })
                    } else if tab.mode == ViewMode::Grid {
                        FileBody::Grid
                    } else {
                        FileBody::List
                    }
                }
            }
        })
    }

    fn render_file_body(&self, body: FileBody) -> AnyView {
        let drag = DragHost(self.0);
        match body {
            FileBody::Loading(location) => self.location_loading(&location),
            FileBody::Collection(state) => drag.background_drop_target(state.view(self.0)),
            FileBody::Network => drag.background_drop_target(Collections(self.0).network_content()),
            FileBody::Empty(message) => {
                let target = self.empty_view(message);
                MenuHost(self.0).directory_menu(drag.background_drop_target(target))
            }
            FileBody::List => list::list_view(self.0),
            FileBody::Grid => grid::grid_view(self.0),
        }
    }

    fn location_loading(&self, location: &Location) -> AnyView {
        let message = match location {
            Location::Volume { identity, .. } => {
                let label = Rows(self.0)
                    .volume_at_location(identity)
                    .map_or_else(|| location.label(), |volume| volume.name);
                gds::t!("loading-volume", name = label)
            }
            _ => i18n::text("loading"),
        };
        let cancel = self.0.clone();
        AnyView::new(view! {
            column(class = "gds-files-state-center") {
                Spinner()
                text {{message}}
                Button(
                    variant = ButtonVariant::Outline,
                    on:click = move |_| Navigator(&cancel).cancel_location_load(),
                ) {{i18n::text("operation-cancel")}}
            }
        })
    }

    fn empty_view(&self, message: &'static str) -> AnyView {
        let focus_visible = self.0.0.list_focus_visible;
        AnyView::new(view! {
            PageTableArea(class = "gds-files-table-area") {
                column(
                    class = "gds-files-state-center",
                    attr:data-focus-visible = move || focus_visible.get().then(|| "true".to_owned())
                ) {
                    text {{i18n::text(message)}}
                }
            }
        })
    }

    fn page_actions(&self) -> AnyView {
        if Geometry(self.0).actions_collapsed() {
            return MenuHost(self.0).page_menu();
        }
        if let Some(actions) = MenuHost(self.0).special_page_actions() {
            return actions;
        }
        let ctx = self.0.clone();
        let layout = local_memo({
            let ctx = self.0.clone();
            move || {
                let grid = ctx.read_structure(|state| state.tab().mode == ViewMode::Grid);
                let details_avail = Geometry(&ctx).details_permitted()
                    && UiState(&ctx).details_available_structure();
                (grid, details_avail)
            }
        });
        AnyView::new(view! {
            {{move || {
                let (grid, details_avail) = layout.get();
                let _z = enter_non_reactive_zone();
                let fav_ctx = ctx.clone();
                let favorite = Signal::derive_local(move || {
                    fav_ctx.read_structure(|state| {
                        state.places.is_favorite(state.tab().navigation.location())
                    })
                });
                let prefs = ctx.0.preferences.state();
                let hidden = Signal::derive_local(move || prefs.with(|p| p.files.show_hidden));
                let details: Signal<bool, LocalStorage> = ctx.0.details.into();
                let view_host = WorkspaceView(&ctx);
                let details_button = details_avail.then(|| {
                    view_host.control(
                        IconName::Info,
                        "details-toggle",
                        |c| Inspection(c).toggle_details(),
                        Some(Command::Details),
                        Some(details),
                    )
                });

                view! {
                    row(class = "gds-files-page-action-strip") {
                        {{view_host.control(
                            if grid { IconName::List } else { IconName::LayoutGrid },
                            "view-toggle",
                            |c| Navigator(c).toggle_view(),
                            Some(Command::ViewMode),
                            None,
                        )}}
                        {{view_host.control(
                            IconName::Star,
                            "favorite-toggle",
                            |c| Navigator(c).favorite(),
                            Some(Command::Favorite),
                            Some(favorite),
                        )}}
                        {{MenuHost(&ctx).column_menu()}}
                        {{view_host.control(
                            IconName::Eye,
                            "show-hidden",
                            |c| c.edit_preferences(|preferences| {
                                preferences.files.show_hidden = !preferences.files.show_hidden;
                            }),
                            Some(Command::Hidden),
                            Some(hidden),
                        )}}
                        {details_button}
                        {{view_host.control(
                            IconName::RefreshCw,
                            "refresh",
                            |c| Navigator(c).refresh(),
                            Some(Command::Refresh),
                            None,
                        )}}
                    }
                }
            }}}
        })
    }

    fn status(&self) -> String {
        let visible_volumes = self
            .0
            .0
            .volumes
            .snapshot()
            .volumes
            .iter()
            .filter(|volume| volume.visible)
            .count();
        let filter = self.0.0.filter.get();
        self.0.read(|state| {
            let tab = state.tab();
            if tab.navigation.is_loading() || matches!(tab.contents, Contents::LoadingCollection) {
                return i18n::text("loading");
            }
            if matches!(tab.navigation.location(), Location::Volumes)
                || matches!(tab.contents, Contents::Volumes)
            {
                let count = if matches!(tab.contents, Contents::Volumes) && !filter.is_empty() {
                    tab.rows.len()
                } else {
                    visible_volumes
                };
                return i18n::count("volume-count", count);
            }
            if !filter.is_empty() {
                let total = match &tab.contents {
                    Contents::Directory => tab.navigation.directory_entry_count(),
                    Contents::Places(items) => Some(items.len()),
                    Contents::Recent(entries) => Some(entries.len()),
                    Contents::Remote { listing, .. } => Some(listing.entries.len()),
                    Contents::Tags { entries, .. } if !tab.is_pure_tag_catalogue() => {
                        Some(entries.len())
                    }
                    _ => None,
                };
                if let Some(total) = total {
                    return gds::t!(
                        "filter-count",
                        visible = tab.rows.len() as i64,
                        total = total as i64
                    );
                }
            }
            i18n::count("item-count", tab.rows.len())
        })
    }
}
