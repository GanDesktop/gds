//! A native settings workspace and the preferences of the current application.

use crate::entities::preferences::{Preferences, PreferencesEditor};
use crate::shared::ui::actions::{Command, SETTINGS_CONTEXT};
use gds::appearance::typography::PreferencesEditor as FontEditor;
use gds::ui::terminal::PreferencesEditor as TerminalEditor;
mod controls;
mod pages;
mod tags;

pub use pages::Page;

use controls::{
    Choice, ChoiceControl, ChoiceEvent, ChoiceState, Field, NumberControl, PERCENTAGE_RANGE,
};
use gds_uim::el;
use gpui_kit::{
    component::{
        ActiveTheme, WindowExt,
        slider::{SliderEvent, SliderState, SliderValue},
    },
    *,
};

use gds::{
    appearance::{theme, tokens::SIDEBAR_PADDING, typography},
    i18n::text,
    ui::{Frame, NavItem, Shortcut, sidebar::SidebarPresentation},
};

#[derive(Clone)]
pub enum Event {
    /// An immediate change request; the application applies, persists, then reports the result.
    Apply(Preferences),
}

pub struct View {
    tags: tags::Controls,
    preferences: Preferences,
    sidebar: SidebarPresentation,
    page: Page,
    /// One focus target per page, so page cycling keeps keyboard navigation
    /// inside the visible page and blurs controls on the page being left.
    page_focus: [FocusHandle; Page::ALL.len()],
    general: Entity<PreferencesEditor>,
    source: ChoiceControl<theme::SourceKind>,
    system_theme_available: bool,
    custom: ChoiceControl<std::path::PathBuf>,
    themes: Vec<theme::provider::custom::Entry>,
    builtin: ChoiceControl<theme::BuiltinTheme>,
    mode: ChoiceControl<theme::ModePreference>,
    content_opacity: NumberControl,
    sidebar_opacity: NumberControl,
    font: Entity<FontEditor>,
    terminal: Entity<TerminalEditor>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Event> for View {}

impl View {
    fn source_kind(preferences: &Preferences, system_available: bool) -> theme::SourceKind {
        match &preferences.appearance.source {
            theme::Source::System if !system_available => theme::SourceKind::Builtin,
            source => theme::SourceKind::from(source),
        }
    }

    pub fn new(
        preferences: Preferences,
        sidebar: SidebarPresentation,
        tasks: gds::service::task_manager::Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let general = cx.new(|cx| PreferencesEditor::new(preferences.clone(), window, cx));
        let themes = theme::provider::custom::catalog().unwrap_or_else(|error| {
            tracing::warn!(%error, "cannot list user themes");
            Vec::new()
        });
        let custom_path = match &preferences.appearance.source {
            theme::Source::Custom(path) => path.clone(),
            _ => std::path::PathBuf::new(),
        };
        let custom = ChoiceControl::new(
            Choice::<std::path::PathBuf>::catalog(&themes, cx),
            &custom_path,
            true,
            window,
            cx,
        );
        let system_theme_available = theme::Source::system_available(cx.window_appearance());
        let source = ChoiceControl::new(
            Choice::<theme::SourceKind>::catalog(system_theme_available, cx),
            &Self::source_kind(&preferences, system_theme_available),
            false,
            window,
            cx,
        );
        let builtin = ChoiceControl::new(
            Choice::<theme::BuiltinTheme>::catalog(cx),
            &preferences.appearance.builtin,
            false,
            window,
            cx,
        );
        let mode = ChoiceControl::new(
            Choice::<theme::ModePreference>::catalog(cx),
            &preferences.appearance.mode,
            false,
            window,
            cx,
        );
        let content_opacity = NumberControl::new(
            preferences.appearance.content_opacity * 100.0,
            PERCENTAGE_RANGE,
            cx,
        );
        let sidebar_opacity = NumberControl::new(
            preferences.appearance.sidebar_opacity * 100.0,
            PERCENTAGE_RANGE,
            cx,
        );
        let font = cx.new(|cx| FontEditor::new(preferences.font.clone(), window, cx));
        let terminal = cx.new(|cx| TerminalEditor::new(preferences.terminal.clone(), window, cx));
        let tag_editor = cx.new(|cx| {
            crate::entities::tags::CommandsEditor::new(
                preferences.tags.commands.clone(),
                window,
                cx,
            )
        });
        let mut subscriptions = vec![
            cx.subscribe_in(&tag_editor, window, Self::on_tags_edited),
            cx.subscribe_in(&terminal, window, Self::on_terminal_edited),
            cx.subscribe_in(&general, window, Self::on_general_edited),
            cx.subscribe_in(&source.state, window, Self::on_source_confirm),
            cx.subscribe_in(&custom.state, window, Self::on_custom_confirm),
            cx.subscribe_in(&mode.state, window, Self::on_mode_confirm),
            cx.subscribe_in(&builtin.state, window, Self::on_builtin_confirm),
            cx.subscribe_in(&font, window, Self::on_font_edited),
        ];
        for (field, state) in [
            (Field::ContentOpacity, content_opacity.state.clone()),
            (Field::SidebarOpacity, sidebar_opacity.state.clone()),
        ] {
            subscriptions.push(cx.subscribe_in(&state, window, Self::on_slider_change(field)));
            // Kit accessibility increments notify the state without a Change event.
            subscriptions.push(cx.observe_in(&state, window, Self::on_slider_notify(field)));
        }
        let mut view = Self {
            tags: tags::Controls {
                editor: tag_editor,
                tasks,
                checked: None,
                error: None,
                pending: false,
                task: None,
            },
            preferences,
            sidebar,
            page: Page::General,
            page_focus: std::array::from_fn(|_| cx.focus_handle()),
            general,
            source,
            system_theme_available,
            custom,
            themes,
            builtin,
            mode,
            content_opacity,
            sidebar_opacity,
            font,
            terminal,
            _subscriptions: subscriptions,
        };
        view.probe_tags(cx);
        view
    }

    pub fn refresh_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.system_theme_available = theme::Source::system_available(cx.window_appearance());
        match theme::provider::custom::catalog() {
            Ok(themes) => self.themes = themes,
            Err(error) => tracing::warn!(%error, "cannot list user themes"),
        }
        self.sync_controls(window, cx);
        cx.notify();
    }

    pub fn set_sidebar_presentation(
        &mut self,
        sidebar: SidebarPresentation,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar != sidebar {
            self.sidebar = sidebar;
            cx.notify();
        }
    }

    /// Move to the adjacent settings page, wrapping at both ends of the sidebar.
    ///
    /// Only the visible page changes: file tabs and preference values are
    /// untouched. Focus moves into the new page, which also dismisses a menu or
    /// editor that belonged to the page being left. A modal dialog keeps both
    /// focus and the current page.
    pub fn cycle_page(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let count = Page::ALL.len();
        let current = self.page.index();
        let next = if backwards {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        self.select_page(Page::ALL[next], window, cx);
    }

    /// Open a specific settings page without changing the underlying workspace.
    pub fn select_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        self.page = page;
        self.page_focus[page.index()].focus(window, cx);
        cx.notify();
    }

    /// Update the controls after application or rollback, without submitting another request.
    pub fn set_preferences(
        &mut self,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tags_changed = self.preferences.tags.commands != preferences.tags.commands;
        self.preferences = preferences;
        self.sync_controls(window, cx);
        if tags_changed {
            self.probe_tags(cx);
        }
        cx.notify();
    }

    fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tags.editor.update(cx, |editor, cx| {
            editor.set_value(self.preferences.tags.commands.clone(), window, cx)
        });
        self.general.update(cx, |editor, cx| {
            editor.set_value(self.preferences.clone(), window, cx)
        });
        self.source.sync(
            Choice::<theme::SourceKind>::catalog(self.system_theme_available, cx),
            &Self::source_kind(&self.preferences, self.system_theme_available),
            window,
            cx,
        );
        let custom_path = match &self.preferences.appearance.source {
            theme::Source::Custom(path) => path.clone(),
            _ => self
                .custom
                .state
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default(),
        };
        self.custom.sync(
            Choice::<std::path::PathBuf>::catalog(&self.themes, cx),
            &custom_path,
            window,
            cx,
        );
        self.builtin.sync(
            Choice::<theme::BuiltinTheme>::catalog(cx),
            &self.preferences.appearance.builtin,
            window,
            cx,
        );
        self.mode.sync(
            Choice::<theme::ModePreference>::catalog(cx),
            &self.preferences.appearance.mode,
            window,
            cx,
        );
        self.content_opacity.sync(
            self.preferences.appearance.content_opacity * 100.0,
            window,
            cx,
        );
        self.sidebar_opacity.sync(
            self.preferences.appearance.sidebar_opacity * 100.0,
            window,
            cx,
        );
        self.font.update(cx, |editor, cx| {
            editor.set_value(self.preferences.font.clone(), window, cx)
        });
        self.terminal.update(cx, |editor, cx| {
            editor.set_value(self.preferences.terminal.clone(), window, cx)
        });
    }

    fn change(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Preferences) -> bool,
    ) {
        if !update(&mut self.preferences) {
            return;
        }
        if !self.system_theme_available
            && self.preferences.appearance.source == theme::Source::System
        {
            self.preferences.appearance.source = theme::Source::Builtin;
        }
        self.sync_controls(window, cx);
        cx.emit(Event::Apply(self.preferences.clone()));
        cx.notify();
    }

    fn on_general_edited(
        &mut self,
        _: &Entity<PreferencesEditor>,
        event: &gds::ui::settings::form::Changed<Preferences>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |preferences| {
            Field::replace(preferences, event.value.clone())
        });
    }

    fn on_terminal_edited(
        &mut self,
        _: &Entity<TerminalEditor>,
        event: &gds::ui::settings::form::Changed<gds::ui::terminal::Preferences>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |preferences| {
            Field::replace(&mut preferences.terminal, event.value.clone())
        });
    }

    fn on_font_edited(
        &mut self,
        _: &Entity<FontEditor>,
        event: &gds::ui::settings::form::Changed<typography::Preferences>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |preferences| {
            Field::replace(&mut preferences.font, event.value.clone())
        });
    }

    fn on_source_confirm(
        &mut self,
        _: &Entity<ChoiceState<theme::SourceKind>>,
        event: &ChoiceEvent<theme::SourceKind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ChoiceEvent::Confirm(Some(value)) = event {
            let source = match value {
                theme::SourceKind::System => theme::Source::System,
                theme::SourceKind::Builtin => theme::Source::Builtin,
                theme::SourceKind::Custom => {
                    cx.notify();
                    let Some(path) = self.custom.state.read(cx).selected_value().cloned() else {
                        return;
                    };
                    theme::Source::Custom(path)
                }
            };
            self.change(window, cx, |preferences| {
                Field::replace(&mut preferences.appearance.source, source)
            });
        }
    }

    fn on_custom_confirm(
        &mut self,
        _: &Entity<ChoiceState<std::path::PathBuf>>,
        event: &ChoiceEvent<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ChoiceEvent::Confirm(Some(path)) = event {
            self.change(window, cx, |preferences| {
                Field::replace(
                    &mut preferences.appearance.source,
                    theme::Source::Custom(path.clone()),
                )
            });
        }
    }

    fn on_builtin_confirm(
        &mut self,
        _: &Entity<ChoiceState<theme::BuiltinTheme>>,
        event: &ChoiceEvent<theme::BuiltinTheme>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ChoiceEvent::Confirm(Some(value)) = event {
            self.change(window, cx, |preferences| {
                Field::replace(&mut preferences.appearance.builtin, *value)
            });
        }
    }

    fn on_mode_confirm(
        &mut self,
        _: &Entity<ChoiceState<theme::ModePreference>>,
        event: &ChoiceEvent<theme::ModePreference>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ChoiceEvent::Confirm(Some(value)) = event {
            self.change(window, cx, |preferences| {
                Field::replace(&mut preferences.appearance.mode, *value)
            });
        }
    }

    fn on_slider_change(
        field: Field,
    ) -> impl Fn(&mut Self, &Entity<SliderState>, &SliderEvent, &mut Window, &mut Context<Self>)
    {
        move |view, _, event, window, cx| {
            if let SliderEvent::Change(value) = event {
                view.number_changed(field, *value, window, cx);
            }
        }
    }

    fn on_slider_notify(
        field: Field,
    ) -> impl Fn(&mut Self, Entity<SliderState>, &mut Window, &mut Context<Self>) {
        move |view, state, window, cx| {
            let value = state.read(cx).value();
            if !view
                .number_control(field)
                .notification_is_programmatic(value)
            {
                view.number_changed(field, value, window, cx);
            }
        }
    }

    fn on_rounded_corners_changed(
        &mut self,
        checked: &bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |preferences| {
            Field::replace(&mut preferences.appearance.rounded_corners, *checked)
        });
    }

    fn on_animations_changed(
        &mut self,
        checked: &bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |preferences| {
            Field::replace(&mut preferences.animations_enabled, Some(*checked))
        });
    }

    fn on_hidden_changed(&mut self, checked: &bool, window: &mut Window, cx: &mut Context<Self>) {
        self.change(window, cx, |preferences| {
            Field::replace(&mut preferences.files.show_hidden, *checked)
        });
    }

    fn on_auto_focus_search_changed(
        &mut self,
        checked: &bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |preferences| {
            Field::replace(&mut preferences.files.auto_focus_search, *checked)
        });
    }

    fn on_sidebar_volumes_changed(
        &mut self,
        checked: &bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |preferences| {
            Field::replace(&mut preferences.files.sidebar.show_volumes, *checked)
        });
    }

    fn on_reset(field: Field) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |view, _, window, cx| view.change(window, cx, |preferences| field.reset(preferences))
    }

    fn on_page(
        destination: Page,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |view, _, _, cx| {
            if view.page != destination {
                view.page = destination;
                cx.notify();
            }
        }
    }

    fn number_control(&self, field: Field) -> &NumberControl {
        match field {
            Field::ContentOpacity => &self.content_opacity,
            Field::SidebarOpacity => &self.sidebar_opacity,
            _ => unreachable!("only numeric fields own numeric controls"),
        }
    }

    fn number_changed(
        &mut self,
        field: Field,
        value: SliderValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SliderValue::Single(value) = value else {
            return;
        };
        let Some(range) = field.number_range() else {
            return;
        };
        let Some(current) = field.number(&self.preferences) else {
            return;
        };
        let value = range.quantize(value);
        if range.equivalent(current, value) {
            return;
        }
        self.change(window, cx, |preferences| {
            field.set_number(preferences, value)
        });
    }

    fn navigation_shortcut(&self, destination: Page) -> Option<Shortcut> {
        let count = Page::ALL.len();
        let current = self.page.index();
        let next = (current + 1) % count;
        let previous = (current + count - 1) % count;
        let destination = destination.index();
        if destination == next {
            Some(Shortcut::new(
                Command::NextTab.action(),
                Some(SETTINGS_CONTEXT),
            ))
        } else if destination == previous {
            Some(Shortcut::new(
                Command::PreviousTab.action(),
                Some(SETTINGS_CONTEXT),
            ))
        } else {
            None
        }
    }

    fn navigation_item(&self, index: usize, destination: Page, cx: &mut Context<Self>) -> NavItem {
        let id = ("settings-page", index);
        let icon = destination.icon();
        let label = text(destination.label_id(), cx);
        // Holding Alt reveals the real page-cycle binding on the adjacent row.
        let shortcut = self.navigation_shortcut(destination);
        let on_page = cx.listener(Self::on_page(destination));
        let item = el! {
            (id, icon, label).NavItem::new {
                presentation: $self.sidebar;
                selected: $(self.page == destination);
                on_click: on_page;
            }
        };
        match shortcut {
            Some(shortcut) => item.shortcut(shortcut),
            None => item,
        }
    }

    fn render_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let items: Vec<_> = Page::ALL
            .into_iter()
            .enumerate()
            .map(|(index, destination)| self.navigation_item(index, destination, cx))
            .collect();
        el! {
            .div { size_full; min_h_0; flex; flex_col; gap_1; p: SIDEBAR_PADDING.px } [..items]
        }
        .into_any_element()
    }

    fn render_page_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let page = self.page;
        let content = page.render(self, cx);
        let title = text(page.label_id(), cx);
        let page_focus = self.page_focus[page.index()].clone();
        el! {
            .div {
                # "settings-page-content";
                track_focus: $(&page_focus);
                size_full; min_w_0; overflow_y_scroll; p_6;
            } [
                .div { text_xl; mb_5 } [title],
                content,
            ]
        }
        .into_any_element()
    }
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let navigation = self.render_navigation(cx);
        let content = self.render_page_content(cx);
        let workspace = Frame::default()
            .sidebar_width(self.sidebar.width())
            .workspace(navigation, content, cx);
        el! {
            .div { size_full; min_h_0; flex; flex_col; text_color: $(cx.theme().foreground) } [workspace]
        }
    }
}
