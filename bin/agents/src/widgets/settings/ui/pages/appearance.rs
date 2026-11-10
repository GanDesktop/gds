//! Appearance owns its form and persistence; the settings shell owns navigation.

use crate::shared::i18n::tr;
use gds_uim::el;
mod controls;
mod form;
mod sessions;
#[cfg(all(test, feature = "gui-tests"))]
mod tests;
use crate::shared::lib::appearance::{self as theme, preferences::Preferences};
use form::{DraftError, Form};
use gpui_kit::{
    App, Context, Render, ScrollHandle, Subscription, Task, TestSupportExt, Window,
    component::{input::*, select::*, *},
    div,
    prelude::*,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::widgets::settings) enum Group {
    Theme,
    Fonts,
    Layout,
}
impl Group {
    pub const ALL: [Self; 3] = [Self::Theme, Self::Fonts, Self::Layout];
    pub fn title(self) -> &'static str {
        match self {
            Self::Theme => tr!("appearance-theme"),
            Self::Fonts => tr!("appearance-fonts"),
            Self::Layout => tr!("appearance-layout"),
        }
    }
    pub fn help(self) -> &'static str {
        match self {
            Self::Theme => tr!("appearance-theme-help"),
            Self::Fonts => tr!("appearance-fonts-help"),
            Self::Layout => tr!("appearance-layout-help"),
        }
    }
    /// Navigation entry in the settings sidebar.
    pub fn id(self) -> &'static str {
        match self {
            Self::Theme => "settings-theme",
            Self::Fonts => "settings-fonts",
            Self::Layout => "settings-layout",
        }
    }
    /// Card that holds this group's rows in the page body.
    fn card_id(self) -> &'static str {
        match self {
            Self::Theme => "appearance-theme-card",
            Self::Fonts => "appearance-fonts-card",
            Self::Layout => "appearance-layout-card",
        }
    }
    pub(in crate::widgets::settings) fn index(self) -> usize {
        match self {
            Self::Theme => 0,
            Self::Fonts => 1,
            Self::Layout => 2,
        }
    }
}

#[derive(Clone, Copy)]
enum Field {
    Animations,
    RoundedCorners,
    Language,
    Source,
    Builtin,
    Custom,
    Mode,
    Background,
    UiFont,
    MonoFont,
    Fallbacks,
    UiSize,
    ContentSize,
    LineHeight,
    Opacity,
    SidebarOpacity,
    ComposerEnter,
}
impl Field {
    const ALL: [Self; 16] = [
        Self::Language,
        Self::Animations,
        Self::RoundedCorners,
        Self::Source,
        Self::Builtin,
        Self::Custom,
        Self::Mode,
        Self::Background,
        Self::Opacity,
        Self::SidebarOpacity,
        Self::UiFont,
        Self::UiSize,
        Self::ContentSize,
        Self::MonoFont,
        Self::Fallbacks,
        Self::LineHeight,
    ];
    fn id(self) -> &'static str {
        match self {
            Self::RoundedCorners => "appearance-rounded-corners",
            Self::Animations => "appearance-animations",
            Self::Language => "appearance-language",
            Self::Source => "appearance-source",
            Self::Builtin => "appearance-builtin-theme",
            Self::Custom => "appearance-custom-theme",
            Self::Mode => "appearance-mode",
            Self::Background => "appearance-background",
            Self::UiFont => "appearance-ui-font",
            Self::MonoFont => "appearance-mono-font",
            Self::Fallbacks => "appearance-fallbacks",
            Self::UiSize => "appearance-ui-size",
            Self::ContentSize => "appearance-content-size",
            Self::LineHeight => "appearance-line-height",
            Self::Opacity => "appearance-opacity",
            Self::SidebarOpacity => "appearance-sidebar-opacity",
            Self::ComposerEnter => "appearance-composer-enter",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::RoundedCorners => tr!("appearance-rounded-corners"),
            Self::Animations => tr!("appearance-animations"),
            Self::Language => tr!("appearance-language"),
            Self::Source => tr!("appearance-source"),
            Self::Builtin => tr!("appearance-builtin-theme"),
            Self::Custom => tr!("appearance-custom-theme"),
            Self::Mode => tr!("appearance-mode"),
            Self::Background => tr!("appearance-background"),
            Self::UiFont => tr!("appearance-ui-font"),
            Self::MonoFont => tr!("appearance-mono-font"),
            Self::Fallbacks => tr!("appearance-fallbacks"),
            Self::UiSize => tr!("appearance-ui-size"),
            Self::ContentSize => tr!("appearance-content-size"),
            Self::LineHeight => tr!("appearance-line-height"),
            Self::Opacity => tr!("appearance-opacity"),
            Self::SidebarOpacity => tr!("appearance-sidebar-opacity"),
            Self::ComposerEnter => tr!("appearance-enter"),
        }
    }
    fn help(self) -> &'static str {
        match self {
            Self::RoundedCorners => tr!("appearance-rounded-corners-help"),
            Self::Animations => tr!("appearance-animations-help"),
            Self::Language => tr!("appearance-language-help"),
            Self::Source => tr!("appearance-source-help"),
            Self::Builtin => tr!("appearance-builtin-theme-help"),
            Self::Custom => tr!("appearance-custom-theme-help"),
            Self::Mode => tr!("appearance-mode-help"),
            Self::Background => tr!("appearance-background-help"),
            Self::UiFont => tr!("appearance-ui-font-help"),
            Self::MonoFont => tr!("appearance-mono-font-help"),
            Self::Fallbacks => tr!("appearance-fallbacks-help"),
            Self::UiSize => tr!("appearance-size-help"),
            Self::ContentSize => tr!("appearance-content-size-help"),
            Self::LineHeight => tr!("appearance-line-height-help"),
            Self::Opacity => tr!("appearance-opacity-help"),
            Self::SidebarOpacity => tr!("appearance-sidebar-opacity-help"),
            Self::ComposerEnter => {
                tr!("appearance-enter-help")
            }
        }
    }
    fn group(self) -> Group {
        match self {
            Self::Animations | Self::Language | Self::RoundedCorners => Group::Theme,
            Self::Source
            | Self::Custom
            | Self::Builtin
            | Self::Mode
            | Self::Background
            | Self::Opacity
            | Self::SidebarOpacity => Group::Theme,
            Self::UiFont | Self::MonoFont | Self::Fallbacks | Self::UiSize | Self::ContentSize => {
                Group::Fonts
            }
            Self::LineHeight | Self::ComposerEnter => Group::Layout,
        }
    }
    fn matches(self, query: &str) -> bool {
        if matches!(self, Self::UiFont) && Self::UiSize.matches(query) {
            return true;
        }
        format!(
            "{} {} {} {}",
            tr!("settings-appearance"),
            self.group().title(),
            self.label(),
            self.help()
        )
        .to_lowercase()
        .contains(query)
    }
}

pub(in crate::widgets::settings) struct AppearancePage {
    editor: theme::Editor,
    document: Option<theme::Document>,
    form: Form,
    pub(in crate::widgets::settings) query: String,
    /// Section the sidebar asked to reveal; consumed by the next render.
    pub(in crate::widgets::settings) scroll_to: Option<usize>,
    scroll: ScrollHandle,
    pub(in crate::widgets::settings) message: String,
    pending: Option<Task<()>>,
    reload: bool,
    subscriptions: Vec<Subscription>,
    _appearance: Subscription,
}

impl AppearancePage {
    pub(in crate::widgets::settings) fn refresh_themes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.form.refresh_themes(window, cx);
    }

    pub(in crate::widgets::settings) fn new(
        editor: theme::Editor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (document, message) = match editor.load() {
            Ok(document) => (Some(document), String::new()),
            Err(error) => (
                None,
                tr!("settings-load-error", error = format!("{:#}", error)),
            ),
        };
        let preferences = document
            .as_ref()
            .map(|d| d.preferences.clone())
            .unwrap_or_default();
        let form = Form::new(&preferences, window, cx);
        let mut page = Self {
            editor,
            document,
            form,
            query: String::new(),
            scroll_to: None,
            scroll: ScrollHandle::new(),
            message,
            pending: None,
            reload: false,
            subscriptions: Vec::new(),
            _appearance: cx.observe_global_in::<theme::Appearance>(window, |this, window, cx| {
                this.form.fallbacks.update(cx, |input, cx| {
                    input.set_placeholder(tr!("appearance-inherit-placeholder"), window, cx)
                });
                this.reload = true;
                cx.notify();
            }),
        };
        page.subscribe(window, cx);
        page
    }

    fn subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.subscriptions.clear();
        self.subscriptions.push(cx.subscribe_in(
            &self.form.fallbacks,
            window,
            |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    if this.document.is_none() {
                        cx.notify();
                        return;
                    }
                    this.message = this
                        .candidate(cx)
                        .err()
                        .map(|error| tr!("settings-apply-error", error = error.to_string()))
                        .unwrap_or_default();
                    this.pending = Some(cx.spawn_in(window, async move |view, cx| {
                        cx.background_executor()
                            .timer(super::super::SAVE_DELAY)
                            .await;
                        let _ = view.update_in(cx, |this, window, cx| this.save(window, cx));
                    }));
                    cx.notify();
                }
            },
        ));
        for field in Field::NUMBERS {
            let state = self.setting_number(field).control.state.clone();
            self.subscriptions.push(cx.subscribe_in(
                &state,
                window,
                move |this, _, event, window, cx| {
                    let gpui_kit::component::slider::SliderEvent::Change(value) = event else {
                        return;
                    };
                    this.apply_number(field, *value, window, cx);
                },
            ));
            self.subscriptions.push(cx.observe_in(
                &state,
                window,
                move |this, state, window, cx| {
                    let value = state.read(cx).value();
                    if !this
                        .setting_number(field)
                        .control
                        .notification_is_programmatic(value)
                    {
                        this.apply_number(field, value, window, cx);
                    }
                },
            ));
        }
        macro_rules! select {
            ($field:ident) => {
                self.subscriptions.push(cx.subscribe_in(
                    &self.form.$field,
                    window,
                    |this, _, event, window, cx| {
                        if matches!(event, SelectEvent::Confirm(_)) {
                            this.flush_pending(window, cx);
                        }
                    },
                ));
            };
        }
        self.subscriptions.push(cx.subscribe_in(
            &self.form.source,
            window,
            |this, _, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(_)) {
                    cx.notify();
                    if this.form.source.read(cx).selected_value()
                        == Some(&gds::appearance::theme::SourceKind::Custom)
                        && this.form.custom.read(cx).selected_value().is_none()
                    {
                        return;
                    }
                    this.flush_pending(window, cx);
                }
            },
        ));
        select!(custom);
        select!(builtin);
        select!(language);
        select!(mode);
        select!(background);
        select!(ui_font);
        select!(mono_font);
        select!(composer_enter);
        select!(default_agent);
    }

    /// # Errors
    /// Rejects malformed numbers and invalid appearance values without changing live state.
    fn candidate(&self, cx: &App) -> Result<Preferences, DraftError> {
        self.form.candidate(cx)
    }

    fn dirty(&self, cx: &App) -> bool {
        match (self.candidate(cx), &self.document) {
            (Ok(candidate), Some(document)) => {
                let mut current = document.preferences.clone();
                if !self.form.system_theme_available
                    && current.theme_source == theme::preferences::ThemeSource::Desktop
                {
                    current.theme_source = theme::preferences::ThemeSource::Builtin;
                }
                candidate != current
            }
            (_, None) => false,
            _ => true,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dirty(cx) {
            return;
        }
        self.message = match self.candidate(cx) {
            Err(error) => tr!("settings-apply-error", error = (error).to_string()),
            Ok(candidate) => match self.document.as_mut() {
                None => tr!("settings-fix-file").into(),
                Some(document) => match self.editor.save(document, candidate, window, cx) {
                    Ok(()) => tr!("settings-applied").into(),
                    Err(error)
                        if matches!(
                            error.downcast_ref::<crate::shared::config::Error>(),
                            Some(crate::shared::config::Error::Conflict { .. })
                        ) =>
                    {
                        tr!("settings-conflict").into()
                    }
                    Err(error) => {
                        tr!("settings-apply-error", error = format!("{:#}", error))
                    }
                },
            },
        };
        cx.notify();
    }

    pub(in crate::widgets::settings) fn flush_pending(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = None;
        self.save(window, cx);
    }

    fn replace_form(
        &mut self,
        preferences: &Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = None;
        self.form = Form::new(preferences, window, cx);
        self.subscribe(window, cx);
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reload = false;
        if self.dirty(cx) {
            return; // Keep invalid/in-progress edits and their original conflict baseline.
        }
        match self.editor.load() {
            Ok(document) => {
                if self
                    .document
                    .as_ref()
                    .is_none_or(|old| old.preferences != document.preferences)
                {
                    self.replace_form(&document.preferences, window, cx);
                    self.message.clear();
                }
                self.document = Some(document);
            }
            Err(error) => {
                self.message = tr!("settings-reload-error", error = format!("{:#}", error))
            }
        }
    }

    fn control(&self, field: Field, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        match field {
            Field::RoundedCorners => el! {
                ($(field.id())).crate::shared::ui::checkbox {
                    accessibility_label: $(field.label());
                    checked: $(self.form.rounded_corners);
                    disabled: $(self.document.is_none());
                    on_click: $(cx.listener(Self::on_rounded_corners_changed));
                }
            }
            .into_any_element(),
            Field::Animations => el! {
                ($(field.id())).crate::shared::ui::checkbox {
                    accessibility_label: $(field.label());
                    checked: $(self.form.animations_enabled.unwrap_or(true));
                    disabled: $(self.document.is_none());
                    on_click: $(cx.listener(Self::on_animations_changed));
                }
            }
            .into_any_element(),
            Field::Language => el! {
                .div {
                    # $(field.id());
                    test_support;
                    w_full;
                } [
                    ($(&self.form.language), cx).gds::ui::field::select {
                        small;
                        w_full;
                        cursor_pointer;
                    },
                ]
            }
            .into_any_element(),
            Field::Source => el! {
                ($(&self.form.source), cx).gds::ui::field::select {
                    small;
                    cursor_pointer;
                }
            }
            .into_any_element(),
            Field::Custom => el! {
                ($(&self.form.custom), cx).gds::ui::field::select {
                    small; cursor_pointer;
                    placeholder: $(tr!("appearance-custom-theme"));
                }
            }
            .into_any_element(),
            Field::Builtin => el! {
                ($(&self.form.builtin), cx).gds::ui::field::select {
                    small; cursor_pointer;
                }
            }
            .into_any_element(),
            Field::Mode => el! {
                ($(&self.form.mode), cx).gds::ui::field::select {
                    small;
                    cursor_pointer;
                }
            }
            .into_any_element(),
            Field::Background => el! {
                ($(&self.form.background), cx).gds::ui::field::select {
                    small;
                    cursor_pointer;
                }
            }
            .into_any_element(),
            Field::ComposerEnter => el! {
                .div {
                    # $(field.id());
                    test_support;
                    w_full;
                } [
                    ($(&self.form.composer_enter), cx).gds::ui::field::select {
                        small;
                        w_full;
                        cursor_pointer;
                    },
                ]
            }
            .into_any_element(),
            Field::UiFont => el! {
                ($(&self.form.ui_font), cx).gds::ui::field::select {
                    small;
                    cursor_pointer;
                    search_placeholder: tr!("appearance-search-font");
                }
            }
            .into_any_element(),
            Field::MonoFont => el! {
                ($(&self.form.mono_font), cx).gds::ui::field::select {
                    small;
                    cursor_pointer;
                    search_placeholder: tr!("appearance-search-font");
                }
            }
            .into_any_element(),
            Field::UiSize
            | Field::ContentSize
            | Field::LineHeight
            | Field::Opacity
            | Field::SidebarOpacity => {
                unreachable!("numeric fields use shared sliders")
            }
            _ => {
                let input = match field {
                    Field::Fallbacks => &self.form.fallbacks,
                    _ => unreachable!("select fields handled above"),
                }
                .clone();
                el! {
                    .div {
                        # $(field.id());
                        test_support;
                        w_full;
                        min_w_0;
                    } [
                        $(crate::shared::ui::input::placeholder(&input, gds::ui::field::input(&input, cx).small().disabled(self.document.is_none()))),
                    ]
                }
                .into_any_element()
            }
        }
    }
    fn group_card(
        &self,
        group: Group,
        fields: &[Field],
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let title = el! {
            .div { # $(group.id()); text_size: crate::shared::ui::HEADING;
                hoverable_tooltip: $(move |window, cx| crate::shared::ui::tooltip::view(group.help(), window, cx));
            } [$(group.title())]
        };
        el! {
            .div {
                # $(group.card_id());
                test_support;
                flex;
                flex_col;
                gap_2;
            } [
                title,
                .div {
                    border_t_1;
                    border_color: $(cx.theme().border);
                    flex;
                    flex_col;
                } [
                    ..$(fields.iter().map(|field| self.row(*field, cx))),
                ],
            ]
        }
        .into_any_element()
    }

    pub(in crate::widgets::settings) fn on_reset_all(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_form(
            &Preferences {
                confirm_session_delete: self.form.confirm_delete,
                auto_collapse_activity: self.form.auto_collapse_activity,
                composer_enter_behavior: self
                    .form
                    .composer_enter
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or_default(),
                default_agent: self
                    .form
                    .default_agent
                    .read(cx)
                    .selected_value()
                    .cloned()
                    .flatten(),
                ..Preferences::default()
            },
            window,
            cx,
        );
        self.save(window, cx);
    }

    /// Sections rendered for the current search, in page order.
    fn sections(&self, cx: &App) -> Vec<(Group, Vec<Field>)> {
        let query = self.query.trim().to_lowercase();
        Group::ALL
            .into_iter()
            .filter_map(|group| {
                let fields: Vec<_> = Field::ALL
                    .into_iter()
                    .filter(|field| field.group() == group)
                    .filter(|field| match field {
                        Field::Builtin => {
                            self.form.source.read(cx).selected_value()
                                == Some(&gds::appearance::theme::SourceKind::Builtin)
                        }
                        Field::Custom => {
                            self.form.source.read(cx).selected_value()
                                == Some(&gds::appearance::theme::SourceKind::Custom)
                        }
                        _ => true,
                    })
                    .filter(|field| query.is_empty() || field.matches(&query))
                    .collect();
                (!fields.is_empty()).then_some((group, fields))
            })
            .collect()
    }
}

impl Render for AppearancePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.reload {
            self.refresh(window, cx);
        }
        self.sync_numbers(window, cx);
        let sections = self.sections(cx);
        if let Some(index) = self.scroll_to.take() {
            self.scroll
                .scroll_to_top_of_item(index.min(sections.len().saturating_sub(1)));
        }
        let body = el! {
            .div {
                # "appearance-sections";
                flex_1;
                min_h_0;
                overflow_y_scroll;
                track_scroll: $(&self.scroll);
                p_6;
                flex;
                flex_col;
                gap_5;
            } [
                ..$(sections.is_empty().then(|| el! {.div {
                                # "settings-no-results";
                                test_support;
                            } [
                                tr!("settings-no-results"),
                ]})),
                ..$(sections.iter().map(|(group, fields)| self.group_card(*group, fields, cx))),
            ]
        };
        el! {
            .div {
                size_full;
                flex;
                flex_col;
            } [
                body,
            ]
        }
    }
}
