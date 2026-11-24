//! Reactive settings pages editing the persisted application schema.

mod appearance;
mod pages;
mod tags;

use std::rc::Rc;

use gds::appearance::tokens::{
    COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_ICON_SIZE, SIDEBAR_WIDTH, TOOLTIP_DELAY,
};
use gds::i18n;
use gds::ui::field::FieldLabelProps;
use gds::ui::keytips::{self, HintPlacement};
use gds::ui::navigation::NavItemProps;
use gds::ui::settings::form::{Changed, Editor, Model};
use gds::ui::settings::{
    NumberControlProps, PERCENTAGE_RANGE, SettingActionsProps, SettingControlProps,
    SettingDescriptionProps, SettingLabelProps, SettingResetProps, SettingRowProps,
};
use gds::ui::sidebar::{SideBarContentProps, SideBarProps, SidebarPresentation};
use gds::ui::tooltip::{HintTooltipProps, TooltipTextProps};
use gds::ui::{
    FrameContext, IconButtonProps, IconName, IconProps, Shortcut, ShortcutBadgeProps,
    ToggleSwitchProps, window::WidthTier,
};
use serde::{Serialize, de::DeserializeOwned};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, ReadSignal, RwSignal, UnsyncCallback};
use zgui::view::{ShowProps, install_stylesheet};
use zgui::vocab::{Key, NamedKey};
use zgui::{component, css, view};
use zgui_ui::{TooltipContentProps, TooltipProps, TooltipTriggerProps};
use zgui_ui_primitives::Binding;

use crate::entities::preferences::Preferences;
use crate::shared::ui::actions::Command;
pub use pages::Page;

const SETTINGS_SHEET: &str = "gds-files-settings";
const SETTINGS_CSS: &str = css!(
    r#"
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

    .gds-settings-nav-corner-anchor {
        position: absolute;
        inset: 0;
        pointer-events: none;
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

    .gds-settings-label-trigger {
        display: inline-flex;
        align-self: flex-start;
        max-width: 100%;
    }

    .gds-settings-footer {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }
    "#
);

#[derive(Clone)]
pub struct View {
    preferences: ReadSignal<Preferences, LocalStorage>,
    on_apply: Rc<dyn Fn(Preferences)>,
    page: RwSignal<Page, LocalStorage>,
    themes: appearance::Themes,
    tags: tags::Probe,
    nav_refs: [NodeRef; Page::ALL.len()],
    page_focus: NodeRef,
    performance_unlocked: RwSignal<bool, LocalStorage>,
}

impl View {
    pub fn new(
        preferences: ReadSignal<Preferences, LocalStorage>,
        on_apply: impl Fn(Preferences) + 'static,
    ) -> Self {
        Self {
            preferences,
            on_apply: Rc::new(on_apply),
            page: RwSignal::new_local(Page::General),
            themes: appearance::Themes::new(&preferences.get_untracked()),
            tags: tags::Probe::new(preferences),
            nav_refs: std::array::from_fn(|_| NodeRef::new()),
            page_focus: NodeRef::new(),
            performance_unlocked: RwSignal::new_local(false),
        }
    }

    fn apply(&self, change: impl FnOnce(&mut Preferences)) {
        let previous = self.preferences.get_untracked();
        let mut next = previous.clone();
        change(&mut next);
        self.themes.normalize_source(&mut next.appearance.source);
        if next == previous {
            return;
        }
        let language_changed = next.language != previous.language;
        (self.on_apply)(next);
        if language_changed {
            self.page_focus.focus();
        }
    }

    fn subform<T>(
        &self,
        get: fn(&Preferences) -> T,
        set: impl Fn(&mut Preferences, T) + Clone + 'static,
    ) -> Editor<T>
    where
        T: Model + Serialize + DeserializeOwned,
    {
        let preferences = self.preferences;
        let view = self.clone();
        let set_binding = set.clone();
        let value = Binding::controlled(
            Signal::derive_local(move || preferences.with(get)),
            move |next| view.apply(|preferences| set_binding(preferences, next)),
        );
        let apply_view = self.clone();
        Editor::new(value)
            .reset_focus(self.page_focus)
            .on_change(move |changed: Changed<T>| {
                apply_view.apply(|preferences| set(preferences, changed.value));
            })
    }

    pub(super) fn reset_button(
        &self,
        modified: Signal<bool, LocalStorage>,
        testid: Option<&'static str>,
        on_reset: impl Fn() + Clone + 'static,
    ) -> AnyView {
        let focus = self.page_focus;
        AnyView::new(view! {
            SettingReset {
                {move || {
                    let on_reset = on_reset.clone();
                    modified.get().then(|| {
                        let attrs = match testid {
                            Some(id) => Attrs::new().attribute(zgui::view::AttrName::new("data-testid"), id),
                            None => Attrs::new(),
                        };
                        view! {
                            IconButton(
                                icon = IconName::RotateCcw,
                                label = Signal::derive_local(|| i18n::text("settings-reset-default")),
                                on:click = move |_| {
                                    on_reset();
                                    focus.focus();
                                },
                                {..attrs}
                            )
                        }
                        .into_view()
                    })
                }}
            }
        })
    }

    pub(super) fn reset<T: PartialEq + 'static>(
        &self,
        _label: &'static str,
        get: fn(&Preferences) -> T,
        set: fn(&mut Preferences, T),
    ) -> AnyView {
        let preferences = self.preferences;
        let view = self.clone();
        let modified =
            Signal::derive_local(move || preferences.with(get) != get(&Preferences::default()));
        self.reset_button(modified, None, move || {
            view.apply(|preferences| set(preferences, get(&Preferences::default())));
        })
    }

    pub(super) fn toggle_with_reset(
        &self,
        label: &'static str,
        help: &'static str,
        get: fn(&Preferences) -> bool,
        set: fn(&mut Preferences, bool),
        reset_fn: fn(&mut Preferences),
    ) -> AnyView {
        let preferences = self.preferences;
        let view = self.clone();
        let reset_view = self.clone();
        let title_ref = NodeRef::new();
        let value = Binding::controlled(
            Signal::derive_local(move || preferences.with(get)),
            move |checked| view.apply(|preferences| set(preferences, checked)),
        );
        let modified =
            Signal::derive_local(move || preferences.with(get) != get(&Preferences::default()));
        let reset = self.reset_button(modified, None, move || {
            reset_view.apply(reset_fn);
        });
        let title = if help.is_empty() {
            AnyView::new(view! {
                FieldLabel(node_ref = title_ref) {{move || i18n::text(label)}}
            })
        } else {
            AnyView::new(view! {
                box(class = "gds-settings-label-trigger") {
                    HintTooltip(text = Signal::derive_local(move || i18n::text(help))) {
                        FieldLabel(node_ref = title_ref) {{move || i18n::text(label)}}
                    }
                }
            })
        };
        AnyView::new(view! {
            SettingRow {
                SettingLabel {
                    {title}
                    {(!help.is_empty()).then(|| view! {
                        SettingDescription {{move || i18n::text(help)}}
                    }.into_view())}
                }
                SettingActions {
                    {reset}
                    SettingControl {
                        ToggleSwitch(
                            checked = value,
                            labelled_by = Some(title_ref),
                        )
                    }
                }
            }
        })
    }

    pub(super) fn toggle(
        &self,
        label: &'static str,
        help: &'static str,
        get: fn(&Preferences) -> bool,
        set: fn(&mut Preferences, bool),
    ) -> AnyView {
        let preferences = self.preferences;
        let view = self.clone();
        let reset_view = self.clone();
        let title_ref = NodeRef::new();
        let value = Binding::controlled(
            Signal::derive_local(move || preferences.with(get)),
            move |checked| view.apply(|preferences| set(preferences, checked)),
        );
        let modified =
            Signal::derive_local(move || preferences.with(get) != get(&Preferences::default()));
        let reset = self.reset_button(modified, None, move || {
            reset_view.apply(|preferences| set(preferences, get(&Preferences::default())));
        });
        let title = if help.is_empty() {
            AnyView::new(view! {
                FieldLabel(node_ref = title_ref) {{move || i18n::text(label)}}
            })
        } else {
            AnyView::new(view! {
                box(class = "gds-settings-label-trigger") {
                    HintTooltip(text = Signal::derive_local(move || i18n::text(help))) {
                        FieldLabel(node_ref = title_ref) {{move || i18n::text(label)}}
                    }
                }
            })
        };
        AnyView::new(view! {
            SettingRow {
                SettingLabel {
                    {title}
                    {(!help.is_empty()).then(|| view! {
                        SettingDescription {{move || i18n::text(help)}}
                    }.into_view())}
                }
                SettingActions {
                    {reset}
                    SettingControl {
                        ToggleSwitch(
                            checked = value,
                            labelled_by = Some(title_ref),
                        )
                    }
                }
            }
        })
    }

    pub(super) fn opacity(
        &self,
        label: &'static str,
        get: fn(&Preferences) -> f32,
        set: fn(&mut Preferences, f32),
    ) -> AnyView {
        let preferences = self.preferences;
        let view = self.clone();
        let reset_view = self.clone();
        let title_ref = NodeRef::new();
        let pct = Signal::derive_local(move || {
            f64::from(PERCENTAGE_RANGE.quantize(preferences.with(get) * 100.0))
        });
        let binding = Binding::controlled(pct, move |next| {
            let quantized = PERCENTAGE_RANGE.quantize(next as f32);
            let current = preferences.with_untracked(get) * 100.0;
            if !PERCENTAGE_RANGE.equivalent(current, quantized) {
                view.apply(|p| set(p, quantized / 100.0));
            }
        });
        let modified = Signal::derive_local(move || {
            !opacity_matches_default(preferences.with(get), get(&Preferences::default()))
        });
        let reset = self.reset_button(modified, None, move || {
            reset_view.apply(|p| set(p, get(&Preferences::default())));
        });
        AnyView::new(view! {
            SettingRow {
                SettingLabel {
                    FieldLabel(node_ref = title_ref) {{move || i18n::text(label)}}
                }
                SettingActions {
                    {reset}
                    SettingControl {
                        row(class = "gds-setting-number-row") {
                            NumberControl(
                                value = binding,
                                range = PERCENTAGE_RANGE,
                                label = i18n::text_untracked(label),
                                labelled_by = Some(title_ref),
                            )
                            text(class = "gds-setting-number-value") {
                                {move || format!("{:.0}%", pct.get())}
                            }
                        }
                    }
                }
            }
        })
    }

    fn contents(&self) -> AnyView {
        match self.page.get() {
            Page::General => {
                let view = self.clone();
                let preferences = self.preferences;
                let editor = Editor::new(Binding::controlled(
                    Signal::derive_local(move || preferences.get()),
                    move |next| view.apply(|preferences| *preferences = next),
                ))
                .reset_focus(self.page_focus)
                .view();
                AnyView::new(view! {
                    column(class = "gds-settings-col") { {editor} }
                })
            }
            Page::Appearance => self.appearance(),
            Page::Files => AnyView::new(view! {
                column(class = "gds-settings-col") {
                    {self.toggle(
                        "sidebar-show-quick-access",
                        "",
                        |p| p.files.sidebar.show_quick_access,
                        |p, v| p.files.sidebar.show_quick_access = v,
                    )}
                    {self.toggle(
                        "sidebar-show-volumes",
                        "",
                        |p| p.files.sidebar.show_volumes,
                        |p, v| p.files.sidebar.show_volumes = v,
                    )}
                    {self.toggle(
                        "auto-focus-search",
                        "auto-focus-search-help",
                        |p| p.files.auto_focus_search,
                        |p, v| p.files.auto_focus_search = v,
                    )}
                }
            }),
            Page::Terminal => {
                let preferences = self.preferences;
                let view = self.clone();
                AnyView::new(view! {
                    column(class = "gds-settings-col") {
                        {self.toggle_with_reset(
                            "files-terminal-enabled",
                            "",
                            Preferences::terminal_enabled,
                            |p, v| p.terminal_enabled = Some(v),
                            |p| p.terminal_enabled = Preferences::default().terminal_enabled,
                        )}
                        Show(when = move || preferences.with(Preferences::terminal_enabled)) {
                            {{
                                let terminal = view
                                    .subform(|p| p.terminal.clone(), |p, v| p.terminal = v)
                                    .view();
                                view! {
                                    box { {terminal} }
                                }
                            }}
                        }
                    }
                })
            }
            Page::Extensions => self.extensions(),
            Page::Performance => self.performance(),
        }
    }

    pub(super) fn adjacent(page: Page, backwards: bool) -> Page {
        let offset = if backwards { Page::ALL.len() - 1 } else { 1 };
        Page::ALL[(page.index() + offset) % Page::ALL.len()]
    }

    pub(super) fn navigation_shortcut(current: Page, destination: Page) -> Option<Shortcut> {
        if destination == Self::adjacent(current, false) {
            Some(Command::NextTab.shortcut())
        } else if destination == Self::adjacent(current, true) {
            Some(Command::PreviousTab.shortcut())
        } else {
            None
        }
    }

    pub fn cycle_page(&self, backwards: bool) {
        if keytips::has_active_modal_scope() {
            return;
        }
        self.page
            .set(Self::adjacent(self.page.get_untracked(), backwards));
        self.page_focus.focus();
    }

    fn handle_cycle_shortcut(
        &self,
        event: &mut zgui::view::EventCx<'_, zgui::view::events::KeyDown>,
    ) {
        if event.repeat || keytips::has_active_modal_scope() {
            return;
        }
        let held = event.modifiers.held();
        if held.control() && !held.alt() && !held.meta() && event.key == Key::Named(NamedKey::Tab) {
            self.cycle_page(held.shift());
            event.prevent_default();
            event.stop_propagation();
        }
    }

    fn navigation_item(
        &self,
        destination: Page,
        pres_sig: Signal<SidebarPresentation, LocalStorage>,
    ) -> AnyView {
        let page = self.page;
        let selected = Signal::derive_local(move || page.get() == destination);
        let item_ref = self.nav_refs[destination.index()];
        let corner_ref = NodeRef::new();

        let next_expanded = Signal::derive_local(move || {
            !pres_sig.get().collapsed() && destination == Self::adjacent(page.get(), false)
        });
        let prev_expanded = Signal::derive_local(move || {
            !pres_sig.get().collapsed() && destination == Self::adjacent(page.get(), true)
        });
        let next_collapsed = Signal::derive_local(move || {
            pres_sig.get().collapsed() && destination == Self::adjacent(page.get(), false)
        });
        let prev_collapsed = Signal::derive_local(move || {
            pres_sig.get().collapsed() && destination == Self::adjacent(page.get(), true)
        });

        keytips::stage_shortcut(
            item_ref,
            Command::NextTab.shortcut(),
            HintPlacement::Trailing,
            next_expanded,
        );
        keytips::stage_shortcut(
            item_ref,
            Command::PreviousTab.shortcut(),
            HintPlacement::Trailing,
            prev_expanded,
        );
        keytips::stage_shortcut(
            corner_ref,
            Command::NextTab.shortcut(),
            HintPlacement::Corner,
            next_collapsed,
        );
        keytips::stage_shortcut(
            corner_ref,
            Command::PreviousTab.shortcut(),
            HintPlacement::Corner,
            prev_collapsed,
        );

        let icon_view = AnyView::new(view! {
            Icon(name = destination.icon(), size = SIDEBAR_ICON_SIZE)
        });
        let label_view = AnyView::new(view! {
            {move || i18n::text(destination.label_id())}
        });
        let tooltip_label = Signal::derive_local(move || i18n::text(destination.label_id()));

        let nav_row = view! {
            NavItem(
                node_ref = item_ref,
                selected = selected,
                presentation = pres_sig,
                icon = icon_view,
                label = label_view,
                tooltip_enabled = false,
                attr:data-testid = format!("settings-page-{}", destination.index()),
                a11y:label = move || i18n::text(destination.label_id()),
                on_click = UnsyncCallback::new(move |()| {
                    if page.get_untracked() != destination {
                        page.set(destination);
                    }
                }),
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
                            Self::navigation_shortcut(page.get(), destination).map(|shortcut| {
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

    /// Uses the host frame's synchronized sidebar width without painting an extra surface.
    pub fn sidebar(&self, presentation: SidebarPresentation) -> AnyView {
        let view = self.clone();
        AnyView::new(view! {
            SettingsSidebar(view = view, presentation = presentation)
        })
    }

    pub fn content(&self) -> AnyView {
        install_stylesheet(SETTINGS_SHEET, SETTINGS_CSS);
        self.themes.refresh();
        let page = self.page;
        let focus = self.page_focus;
        let contents = self.clone();
        let navigation = self.clone();
        let focus_on_mount = zgui::reactive::RenderEffect::new(move |prev: Option<bool>| {
            let mounted = focus.get().is_some();
            if mounted && prev != Some(true) {
                focus.focus();
            }
            mounted
        });
        zgui::reactive::on_cleanup_local(move || drop(focus_on_mount));

        AnyView::new(view! {
            scroll(
                class = "gds-settings-content",
                node_ref = focus,
                tabindex = {Focus::Sequential},
                on:key_down = move |event| navigation.handle_cycle_shortcut(event),
            ) {
                text(class = "gds-settings-heading") {{move || i18n::text(page.get().label_id())}}
                {move || contents.contents()}
            }
        })
    }

    pub fn footer_text(&self) -> AnyView {
        install_stylesheet(SETTINGS_SHEET, SETTINGS_CSS);
        AnyView::new(view! {
            text(class = "gds-settings-footer") {{move || i18n::text("settings-auto-apply")}}
        })
    }
}

#[component]
fn SettingsSidebar(view: View, presentation: SidebarPresentation) -> impl IntoView {
    install_stylesheet(SETTINGS_SHEET, SETTINGS_CSS);
    let frame_sidebar_width = FrameContext::current().and_then(|ctx| ctx.sidebar_width);
    let initial_frame_width = frame_sidebar_width.map(|sig| sig.get_untracked());
    let window = zgui::prelude::try_use_window();
    let pres_sig = Signal::derive_local(move || {
        if window.as_ref().is_some_and(|w| {
            let width = w.size().get().width.0;
            width > 0.0 && WidthTier::from_width(width) == WidthTier::Micro
        }) {
            return SidebarPresentation::from_width(
                COLLAPSED_SIDEBAR_WIDTH,
                SIDEBAR_WIDTH,
                COLLAPSED_SIDEBAR_WIDTH,
            );
        }
        if let (Some(width_sig), Some(initial_w)) = (frame_sidebar_width, initial_frame_width) {
            let live_w = width_sig.get();
            if (initial_w - presentation.width()).abs() < 0.5 || (live_w - initial_w).abs() >= 0.5 {
                return SidebarPresentation::from_width(
                    live_w,
                    SIDEBAR_WIDTH,
                    COLLAPSED_SIDEBAR_WIDTH,
                );
            }
        }
        presentation
    });

    let navigation = view.clone();
    let items = Page::ALL
        .into_iter()
        .map(|destination| view.navigation_item(destination, pres_sig))
        .collect::<Vec<_>>();

    view! {
        SideBar(
            presentation = pres_sig,
            label = i18n::text_untracked("settings"),
        ) {
            SideBarContent {
                column(
                    class = "gds-settings-sidebar",
                    on:key_down = move |event| navigation.handle_cycle_shortcut(event),
                ) {
                    {items}
                }
            }
        }
    }
}

pub(super) fn opacity_matches_default(current: f32, default: f32) -> bool {
    PERCENTAGE_RANGE.equivalent(current * 100.0, default * 100.0)
}

impl IntoView for View {
    type Output = AnyView;

    fn into_view(self) -> Self::Output {
        self.content()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gds::app::Gds;
    use gds::appearance::{theme, tokens};
    use gds::service::task_manager::Scope;
    use gds::ui::frame::{FrameProps, WorkspaceFrameProps};
    use gds::ui::keytips::KeytipHostProps;
    use gds::ui::sidebar::SidebarPresentation;
    use zgui::app::Fonts;
    use zgui::atlas::{SinkError, TextureFormat, TextureId, TextureSink};
    use zgui::geom::{CssPx, Device, DevicePx, Edges, Point, Rect, Size};
    use zgui::platform::{AppHandler, Decorations, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{RwSignal, flush};
    use zgui::render::{
        ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
        Renderer, TextureHandle,
    };
    use zgui::runtime::AppError;
    use zgui::view;
    use zgui::vocab::{
        Key, Modifiers, NamedKey, PointerAction, PointerButton, PointerEvent, Role, Timestamp,
    };
    use zgui_platform_headless::Harness;

    use super::{Page, Preferences, View, opacity_matches_default};

    struct NoopSink;

    impl TextureSink for NoopSink {
        fn create_texture(
            &mut self,
            _: TextureId,
            _: Size<i32, Device>,
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
    fn opacity_equivalence_and_page_cycling_match_gpui_contracts() {
        assert!(opacity_matches_default(0.85, 0.85));
        assert!(opacity_matches_default(0.853, 0.85));
        assert!(opacity_matches_default(0.996, 1.0));
        assert!(!opacity_matches_default(0.84, 0.85));

        assert_eq!(View::adjacent(Page::General, false), Page::Appearance);
        assert_eq!(View::adjacent(Page::General, true), Page::Performance);
        assert_eq!(View::adjacent(Page::Performance, false), Page::General);

        assert_eq!(
            View::navigation_shortcut(Page::General, Page::Appearance),
            Some(gds::ui::Shortcut::new(
                Key::Named(NamedKey::Tab),
                Modifiers::CONTROL
            ))
        );
        assert_eq!(
            View::navigation_shortcut(Page::General, Page::Performance),
            Some(gds::ui::Shortcut::new(
                Key::Named(NamedKey::Tab),
                Modifiers::CONTROL | Modifiers::SHIFT
            ))
        );
        assert_eq!(View::navigation_shortcut(Page::General, Page::Files), None);
    }

    #[test]
    fn settings_sidebar_fits_column_preserves_clicked_nav_focus_and_centers_collapsed_icons() {
        let content_focus_slot: Arc<Mutex<Option<NodeRef>>> = Arc::new(Mutex::new(None));
        let nav_refs_slot: Arc<Mutex<Option<[NodeRef; Page::ALL.len()]>>> =
            Arc::new(Mutex::new(None));
        let page_slot: Arc<Mutex<Option<RwSignal<Page, zgui::reactive::LocalStorage>>>> =
            Arc::new(Mutex::new(None));
        let width_slot: Arc<Mutex<Option<RwSignal<f32, zgui::reactive::LocalStorage>>>> =
            Arc::new(Mutex::new(None));
        let focused_slot: Arc<
            Mutex<Option<Signal<Option<zgui::view::NodeId>, zgui::reactive::LocalStorage>>>,
        > = Arc::new(Mutex::new(None));

        let content_driver = Arc::clone(&content_focus_slot);
        let nav_driver = Arc::clone(&nav_refs_slot);
        let page_driver = Arc::clone(&page_slot);
        let width_driver = Arc::clone(&width_slot);
        let focused_driver = Arc::clone(&focused_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            harness.settle(32);

            let surface = harness
                .platform()
                .offscreens()
                .first()
                .cloned()
                .expect("headless surface");
            harness
                .platform()
                .set_surface_tiled(surface.id(), Edges::ALL);
            harness.settle(16);

            // 1. Regression check for #94: `.gds-settings-sidebar` fits 224px and leaves >1000px for content!
            let content_node = content_driver.lock().expect("lock").expect("content ref");
            let content_bounds = content_node.window_bounds().expect("content bounds");
            let scale = content_node.scale().max(1.0);
            let content_x = content_bounds.origin.x.0 / scale;
            let content_w = content_bounds.size.width.0 / scale;
            assert!(
                (content_x - tokens::SIDEBAR_WIDTH).abs() < 2.0,
                "settings content must start at sidebar boundary (~224px), got x={content_x}"
            );
            assert!(
                content_w > 1000.0,
                "settings content must fill remaining 1280px width (>1000px), got w={content_w}"
            );

            // 2. Click the Appearance NavItem -> switches page WITHOUT yanking focus to `page_focus`!
            let appearance_pos = Point::new(CssPx(80.0), CssPx(72.0));
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Pressed,
                event: PointerEvent::mouse(appearance_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action: PointerAction::Released,
                event: PointerEvent::mouse(appearance_pos).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
            harness.settle(16);

            let current_page = page_driver
                .lock()
                .expect("lock")
                .expect("page sig")
                .get_untracked();
            assert_eq!(current_page, Page::Appearance);

            let focused_after_click = focused_driver
                .lock()
                .expect("lock")
                .expect("focused signal")
                .get_untracked();
            assert!(
                focused_after_click.is_some(),
                "clicked category NavItem must retain focus"
            );
            let nav_refs = nav_driver.lock().expect("lock").expect("nav refs");
            assert_eq!(
                focused_after_click,
                nav_refs[Page::Appearance.index()].get_untracked(),
                "clicked Appearance NavItem must retain direct focus (not yanked to page_focus)"
            );
            assert_ne!(
                focused_after_click,
                content_node.get_untracked(),
                "clicking a category NavItem must NOT yank focus onto page_focus"
            );

            let a11y_nodes: Vec<_> = surface
                .a11y_log()
                .into_iter()
                .flat_map(|update| update.nodes)
                .map(|(_, node)| {
                    (
                        node.role(),
                        node.label().map(str::to_owned),
                        node.bounds()
                            .map(|rect| (rect.x1 - rect.x0, rect.y1 - rect.y0)),
                    )
                })
                .collect();
            for expected_select in ["Theme source", "Built-in palette", "Color mode"] {
                let select_size = a11y_nodes
                    .iter()
                    .rev()
                    .find_map(|(role, label, size)| {
                        (*role == Role::ComboBox && label.as_deref() == Some(expected_select))
                            .then_some(*size)
                            .flatten()
                    })
                    .unwrap_or_else(|| {
                        panic!("{expected_select} must expose Role::ComboBox SelectTrigger bounds")
                    });
                let width_css = select_size.0 / f64::from(scale);
                let height_css = select_size.1 / f64::from(scale);
                assert!(
                    (width_css - f64::from(gds::ui::settings::CONTROL_WIDTH)).abs() < 1.0,
                    "{expected_select} SelectTrigger must fill CONTROL_WIDTH (260px), got {width_css}"
                );
                assert!(
                    (height_css - 36.0).abs() < 1.0,
                    "{expected_select} SelectTrigger must match 36px control height, got {height_css}"
                );
            }
            let switch_sizes: Vec<(f64, f64)> = a11y_nodes
                .iter()
                .filter_map(|(role, _, size)| (*role == Role::Switch).then_some(*size).flatten())
                .collect();
            assert!(
                switch_sizes.len() >= 2,
                "Appearance page must render Rounded corners and Interface animations ToggleSwitch controls"
            );
            for (sw_w, sw_h) in switch_sizes {
                let width_css = sw_w / f64::from(scale);
                let height_css = sw_h / f64::from(scale);
                assert!(
                    (width_css - 44.0).abs() < 1.0 && (height_css - 24.0).abs() < 1.0,
                    "Settings ToggleSwitch must lay out at 44x24px alongside 36px controls, got {width_css}x{height_css}"
                );
            }
            assert!(
                a11y_nodes
                    .iter()
                    .any(|(role, label, _)| *role == Role::Button
                        && label.as_deref() == Some("Reset to default")),
                "modified Theme source row must render Reset to default IconButton"
            );

            // 3. Collapse sidebar width to 56px -> content shifts cleanly to x=56px and category rail centers at x=28px!
            let width_sig = width_driver.lock().expect("lock").expect("width sig");
            width_sig.set(tokens::COLLAPSED_SIDEBAR_WIDTH);
            flush();
            harness.settle(24);

            let collapsed_content_bounds = content_node.window_bounds().expect("collapsed content");
            let collapsed_content_x = collapsed_content_bounds.origin.x.0 / scale;
            assert!(
                (collapsed_content_x - tokens::COLLAPSED_SIDEBAR_WIDTH).abs() < 2.0,
                "collapsed settings content must start at ~56px, got x={collapsed_content_x}"
            );

            let collapsed_nav_centers: Vec<f32> = nav_refs
                .into_iter()
                .map(|node| {
                    let b = node.window_bounds().expect("collapsed nav item bounds");
                    (b.origin.x.0 + b.size.width.0 * 0.5) / scale
                })
                .collect();
            assert_eq!(collapsed_nav_centers.len(), Page::ALL.len());
            assert!(
                collapsed_nav_centers
                    .iter()
                    .all(|cx| (cx - tokens::COLLAPSED_SIDEBAR_WIDTH * 0.5).abs() < 0.5)
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_decorations(Decorations::None)
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let gds = Gds::install(Scope::default());
                let catalog = crate::shared::i18n::prepare(gds::i18n::Language::English)
                    .expect("English catalog");
                gds.i18n().install_catalog(catalog);

                let mut initial = Preferences::default();
                initial.appearance.source = theme::Source::Builtin;
                let prefs = RwSignal::new_local(initial);
                let apply_prefs = prefs;
                let view = View::new(prefs.read_only(), move |next| apply_prefs.set(next));

                let sidebar_w = RwSignal::new_local(tokens::SIDEBAR_WIDTH);
                *width_slot.lock().expect("lock") = Some(sidebar_w);
                *content_focus_slot.lock().expect("lock") = Some(view.page_focus);
                *nav_refs_slot.lock().expect("lock") = Some(view.nav_refs);
                *page_slot.lock().expect("lock") = Some(view.page);
                *focused_slot.lock().expect("lock") = Some(zgui::view::focused_node());

                let sidebar_view = view.sidebar(SidebarPresentation::default());
                let content_view = view.content();

                view! {
                    KeytipHost {
                        Frame(sidebar_width = Signal::from(sidebar_w)) {
                            WorkspaceFrame(
                                sidebar = sidebar_view,
                                content = content_view,
                                paint_content_surface = true,
                            )
                        }
                    }
                }
            })
            .expect("headless settings layout test succeeds");
    }

    #[test]
    fn terminal_page_subform_stays_mounted_across_nested_preference_edits() {
        let prefs_slot: Arc<Mutex<Option<RwSignal<Preferences, zgui::reactive::LocalStorage>>>> =
            Arc::new(Mutex::new(None));
        let page_slot: Arc<Mutex<Option<RwSignal<Page, zgui::reactive::LocalStorage>>>> =
            Arc::new(Mutex::new(None));

        let prefs_driver = Arc::clone(&prefs_slot);
        let page_driver = Arc::clone(&page_slot);

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(1280.0),
                DevicePx(800.0),
            )));
            let surface = harness
                .platform()
                .offscreens()
                .first()
                .cloned()
                .expect("headless surface");
            surface.set_a11y_listener(true);
            harness.settle(24);

            let page = page_driver.lock().expect("lock").expect("page sig");
            page.set(Page::Terminal);
            flush();
            harness.settle(24);

            let collect_switch_ids = || -> Vec<zgui::vocab::NodeId> {
                let mut ids = Vec::new();
                for update in surface.a11y_log() {
                    for (id, node) in update.nodes {
                        if node.role() == Role::Switch && !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
                ids
            };

            let before_ids = collect_switch_ids();
            assert!(
                before_ids.len() >= 2,
                "Terminal page must mount enable toggle plus subform toggles, got {}",
                before_ids.len()
            );

            // Mutating a nested terminal boolean must NOT tear down & recreate subform Switch nodes!
            let prefs = prefs_driver.lock().expect("lock").expect("prefs sig");
            prefs.update(|p| p.terminal.login_shell = !p.terminal.login_shell);
            flush();
            harness.settle(16);

            let after_ids = collect_switch_ids();
            assert_eq!(
                before_ids, after_ids,
                "editing nested terminal preferences must preserve mounted subform ToggleSwitch nodes without remounting"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_renderer(Box::new(noop_renderer))
            .with_decorations(Decorations::None)
            .with_size(1280.0, 800.0)
            .run_on(driver, move || {
                let gds = Gds::install(Scope::default());
                let catalog = crate::shared::i18n::prepare(gds::i18n::Language::English)
                    .expect("English catalog");
                gds.i18n().install_catalog(catalog);

                let initial = Preferences {
                    terminal_enabled: Some(true),
                    ..Default::default()
                };
                let prefs = RwSignal::new_local(initial);
                let apply_prefs = prefs;
                let view = View::new(prefs.read_only(), move |next| apply_prefs.set(next));

                *prefs_slot.lock().expect("lock") = Some(prefs);
                *page_slot.lock().expect("lock") = Some(view.page);

                view.content()
            })
            .expect("terminal subform Show stability test succeeds");
    }
}
