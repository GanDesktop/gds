//! Shared presentation and state adapters for native settings controls.

use gds::ui::field::select as select_field;
use gds::ui::settings::NumberControl as NumericControl;
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, IndexPath, Sizable,
        select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState},
        switch::Switch,
    },
    *,
};

use crate::entities::preferences::Preferences;
use crate::widgets::settings::ui::View;
use gds::ui::button::Button;
use gds::ui::settings::{SettingActions, SettingControl, SettingLabel, SettingReset, SettingRow};
use gds::{
    app::AppExt as _,
    appearance::theme::{BuiltinTheme, ModePreference, SourceKind},
    i18n::{t, text},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Source,
    Builtin,
    Custom,
    Mode,
    ContentOpacity,
    SidebarOpacity,
    ShowHidden,
    AutoFocusSearch,
    SidebarVolumes,
    Animations,
    RoundedCorners,
    TerminalEnabled,
    TagsEnabled,
}

impl Field {
    pub const NUMBERS: [Self; 2] = [Self::ContentOpacity, Self::SidebarOpacity];

    pub fn label_id(self) -> &'static str {
        match self {
            Self::RoundedCorners => "appearance-rounded-corners",
            Self::Animations => "appearance-animations",
            Self::Source => "theme-source",
            Self::Builtin => "theme-builtin-selection",
            Self::Custom => "theme-custom-selection",
            Self::Mode => "theme-mode",
            Self::ContentOpacity => "theme-opacity",
            Self::SidebarOpacity => "sidebar-opacity",
            Self::ShowHidden => "show-hidden",
            Self::AutoFocusSearch => "auto-focus-search",
            Self::SidebarVolumes => "sidebar-show-volumes",
            Self::TerminalEnabled => "files-terminal-enabled",
            Self::TagsEnabled => "tags-enabled",
        }
    }

    pub fn help_id(self) -> Option<&'static str> {
        match self {
            Self::Animations => Some("appearance-animations-help"),
            _ => None,
        }
    }

    pub fn switch_id(self) -> &'static str {
        match self {
            Self::RoundedCorners => "settings-rounded-corners",
            Self::Animations => "settings-animations",
            Self::ShowHidden => "settings-show-hidden",
            Self::AutoFocusSearch => "settings-auto-focus-search",
            Self::SidebarVolumes => "settings-sidebar-volumes",
            Self::TerminalEnabled => "settings-terminal-enabled",
            Self::TagsEnabled => "tags-enabled",
            _ => unreachable!("only toggle fields have switch IDs"),
        }
    }

    pub fn bool(self, preferences: &Preferences) -> Option<bool> {
        match self {
            Self::RoundedCorners => Some(preferences.appearance.rounded_corners),
            Self::Animations => Some(preferences.animations_enabled.unwrap_or(true)),
            Self::ShowHidden => Some(preferences.files.show_hidden),
            Self::AutoFocusSearch => Some(preferences.files.auto_focus_search),
            Self::SidebarVolumes => Some(preferences.files.sidebar.show_volumes),
            Self::TerminalEnabled => Some(preferences.terminal_enabled()),
            Self::TagsEnabled => Some(preferences.tags.enabled),
            _ => None,
        }
    }

    pub fn set_bool(self, preferences: &mut Preferences, value: bool) -> bool {
        match self {
            Self::RoundedCorners => {
                Self::replace(&mut preferences.appearance.rounded_corners, value)
            }
            Self::Animations => Self::replace(&mut preferences.animations_enabled, Some(value)),
            Self::ShowHidden => Self::replace(&mut preferences.files.show_hidden, value),
            Self::AutoFocusSearch => Self::replace(&mut preferences.files.auto_focus_search, value),
            Self::SidebarVolumes => {
                Self::replace(&mut preferences.files.sidebar.show_volumes, value)
            }
            Self::TerminalEnabled => Self::replace(&mut preferences.terminal_enabled, Some(value)),
            Self::TagsEnabled => Self::replace(&mut preferences.tags.enabled, value),
            _ => unreachable!("only toggle fields set boolean values"),
        }
    }

    pub fn is_modified(self, preferences: &Preferences) -> bool {
        let defaults = Preferences::default();
        match self {
            Self::RoundedCorners => {
                preferences.appearance.rounded_corners != defaults.appearance.rounded_corners
            }
            Self::Animations => {
                preferences.animations_enabled.unwrap_or(true)
                    != defaults.animations_enabled.unwrap_or(true)
            }
            Self::Source => preferences.appearance.source != defaults.appearance.source,
            Self::Custom => false,
            Self::Builtin => preferences.appearance.builtin != defaults.appearance.builtin,
            Self::Mode => preferences.appearance.mode != defaults.appearance.mode,
            Self::ContentOpacity => !PERCENTAGE_RANGE.equivalent(
                preferences.appearance.content_opacity * 100.0,
                defaults.appearance.content_opacity * 100.0,
            ),
            Self::SidebarOpacity => !PERCENTAGE_RANGE.equivalent(
                preferences.appearance.sidebar_opacity * 100.0,
                defaults.appearance.sidebar_opacity * 100.0,
            ),
            Self::ShowHidden => preferences.files.show_hidden != defaults.files.show_hidden,
            Self::AutoFocusSearch => {
                preferences.files.auto_focus_search != defaults.files.auto_focus_search
            }
            Self::SidebarVolumes => {
                preferences.files.sidebar.show_volumes != defaults.files.sidebar.show_volumes
            }
            Self::TerminalEnabled => preferences.terminal_enabled() != defaults.terminal_enabled(),
            Self::TagsEnabled => preferences.tags.enabled != defaults.tags.enabled,
        }
    }

    pub fn reset(self, preferences: &mut Preferences) -> bool {
        if !self.is_modified(preferences) {
            return false;
        }
        let defaults = Preferences::default();
        match self {
            Self::RoundedCorners => {
                preferences.appearance.rounded_corners = defaults.appearance.rounded_corners
            }
            Self::Animations => preferences.animations_enabled = defaults.animations_enabled,
            Self::Source => preferences.appearance.source = defaults.appearance.source,
            Self::Custom => return false,
            Self::Builtin => preferences.appearance.builtin = defaults.appearance.builtin,
            Self::Mode => preferences.appearance.mode = defaults.appearance.mode,
            Self::ContentOpacity => {
                preferences.appearance.content_opacity = defaults.appearance.content_opacity
            }
            Self::SidebarOpacity => {
                preferences.appearance.sidebar_opacity = defaults.appearance.sidebar_opacity
            }
            Self::ShowHidden => preferences.files.show_hidden = defaults.files.show_hidden,
            Self::AutoFocusSearch => {
                preferences.files.auto_focus_search = defaults.files.auto_focus_search
            }
            Self::SidebarVolumes => {
                preferences.files.sidebar.show_volumes = defaults.files.sidebar.show_volumes
            }
            Self::TerminalEnabled => preferences.terminal_enabled = defaults.terminal_enabled,
            Self::TagsEnabled => preferences.tags.enabled = defaults.tags.enabled,
        }
        true
    }

    pub fn replace<T: PartialEq>(target: &mut T, value: T) -> bool {
        if *target == value {
            return false;
        }
        *target = value;
        true
    }

    pub fn number_range(self) -> Option<NumberRange> {
        match self {
            Self::ContentOpacity | Self::SidebarOpacity => Some(PERCENTAGE_RANGE),
            _ => None,
        }
    }

    pub fn number(self, preferences: &Preferences) -> Option<f32> {
        match self {
            Self::ContentOpacity => Some(preferences.appearance.content_opacity * 100.0),
            Self::SidebarOpacity => Some(preferences.appearance.sidebar_opacity * 100.0),
            _ => None,
        }
    }

    pub fn set_number(self, preferences: &mut Preferences, value: f32) -> bool {
        match self {
            Self::ContentOpacity => {
                Self::replace(&mut preferences.appearance.content_opacity, value / 100.0)
            }
            Self::SidebarOpacity => {
                Self::replace(&mut preferences.appearance.sidebar_opacity, value / 100.0)
            }
            _ => unreachable!("only numeric fields own numeric controls"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Choice<T> {
    value: T,
    title: SharedString,
    search_terms: &'static str,
    disabled: bool,
}

impl<T> Choice<T> {
    fn new(value: T, title: SharedString, search_terms: &'static str) -> Self {
        Self {
            value,
            title,
            search_terms,
            disabled: false,
        }
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl<T: Clone + PartialEq> SelectItem for Choice<T> {
    type Value = T;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &Self::Value {
        &self.value
    }

    fn disabled(&self) -> bool {
        self.disabled
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self.search_terms.to_lowercase().contains(&query)
    }
}

impl Choice<SourceKind> {
    pub fn catalog(system_available: bool, cx: &App) -> Vec<Self> {
        vec![
            Self::new(SourceKind::System, t!(@cx, "theme-system"), "system 系统")
                .disabled(!system_available),
            Self::new(
                SourceKind::Builtin,
                t!(@cx, "theme-builtin"),
                "builtin 内置",
            ),
            Self::new(SourceKind::Custom, t!(@cx, "theme-custom"), "custom 自定义"),
        ]
    }
}

impl Choice<std::path::PathBuf> {
    pub fn catalog(
        entries: &[gds::appearance::theme::provider::custom::Entry],
        cx: &App,
    ) -> Vec<Self> {
        let locale = cx.gds_ro().i18n().language().locale();
        entries
            .iter()
            .map(|entry| {
                Self::new(
                    entry.path.clone(),
                    entry.metadata.display_name(locale).to_owned().into(),
                    "",
                )
            })
            .collect()
    }
}

impl Choice<BuiltinTheme> {
    pub fn catalog(cx: &App) -> Vec<Self> {
        BuiltinTheme::ALL
            .into_iter()
            .map(|theme| Self::new(theme, text(theme.label_id(), cx), theme.label_id()))
            .collect()
    }
}

impl Choice<ModePreference> {
    pub fn catalog(cx: &App) -> Vec<Self> {
        vec![
            Self::new(
                ModePreference::System,
                t!(@cx, "theme-mode-system"),
                "system automatic 系统",
            ),
            Self::new(ModePreference::Light, t!(@cx, "theme-light"), "light 浅色"),
            Self::new(ModePreference::Dark, t!(@cx, "theme-dark"), "dark 深色"),
        ]
    }
}

pub(super) type ChoiceEvent<T> = SelectEvent<SearchableVec<Choice<T>>>;
pub(super) type ChoiceState<T> = SelectState<SearchableVec<Choice<T>>>;

pub(super) struct ChoiceControl<T: Clone + PartialEq + 'static> {
    pub state: Entity<ChoiceState<T>>,
    choices: Vec<Choice<T>>,
}

impl<T: Clone + PartialEq + 'static> ChoiceControl<T> {
    pub fn new(
        choices: Vec<Choice<T>>,
        value: &T,
        searchable: bool,
        window: &mut Window,
        cx: &mut Context<View>,
    ) -> Self {
        let selected = choices
            .iter()
            .position(|choice| &choice.value == value)
            .map(|row| IndexPath::default().row(row));
        let items = SearchableVec::new(choices.clone());
        let state =
            cx.new(|cx| SelectState::new(items, selected, window, cx).searchable(searchable));
        Self { state, choices }
    }

    /// Kit's programmatic setters notify but do not emit `SelectEvent::Confirm`.
    pub fn sync(
        &mut self,
        choices: Vec<Choice<T>>,
        value: &T,
        window: &mut Window,
        cx: &mut Context<View>,
    ) {
        if self.choices != choices {
            self.state.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(choices.clone()), window, cx)
            });
            self.choices = choices;
        }
        let expected = self
            .choices
            .iter()
            .find(|choice| &choice.value == value)
            .map(|choice| choice.value.clone());
        if self.state.read(cx).selected_value().cloned() == expected {
            return;
        }
        self.state.update(cx, |state, cx| match &expected {
            Some(value) => state.set_selected_value(value, window, cx),
            None => state.set_selected_index(None, window, cx),
        });
    }

    pub fn render(
        &self,
        field: Field,
        placeholder: SharedString,
        search_placeholder: SharedString,
        cx: &App,
    ) -> Select<SearchableVec<Choice<T>>> {
        el! {
            ($(&self.state), cx).select_field {
                # $("settings-select", field as usize);
                small; w_full;
                menu_width: CONTROL_WIDTH.px;
                accessibility_label: $(text(field.label_id(), cx));
                placeholder: placeholder;
                search_placeholder: search_placeholder;
            }
        }
    }
}

pub(super) use gds::ui::settings::{CONTROL_WIDTH, NumberRange, PERCENTAGE_RANGE};

pub(super) struct NumberControl(NumericControl);
impl std::ops::Deref for NumberControl {
    type Target = NumericControl;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl NumberControl {
    pub fn new(value: f32, range: NumberRange, cx: &mut Context<View>) -> Self {
        Self(NumericControl::new(value, range, cx))
    }
    pub fn render(
        &self,
        field: Field,
        value_label: SharedString,
        cx: &App,
    ) -> impl IntoElement + use<> {
        self.0.element(
            ("settings-number", field as usize).into(),
            text(field.label_id(), cx),
            value_label,
            false,
        )
    }
}

impl View {
    pub(super) fn toggle_row(
        &self,
        field: Field,
        disabled: bool,
        cx: &mut Context<View>,
    ) -> SettingRow {
        let checked = field
            .bool(&self.preferences)
            .expect("toggle rows use boolean fields");
        let label = text(field.label_id(), cx);
        let id = field.switch_id();
        let on_change = cx.listener(Self::on_toggle_change(field));
        let control = el! {
            id.Switch::new {
                small;
                checked: checked;
                disabled: disabled;
                accessibility_label: label;
                on_change: on_change;
            }
        };
        self.setting_row(field, control, cx)
    }

    pub(super) fn setting_row(
        &self,
        field: Field,
        control: impl IntoElement,
        cx: &mut Context<View>,
    ) -> SettingRow {
        let mut title = div()
            .id(("settings-title", field as usize))
            .child(text(field.label_id(), cx));
        if let Some(help_id) = field.help_id() {
            let help = text(help_id, cx);
            title = title.hoverable_tooltip(move |window, cx| {
                gds::ui::tooltip::view(help.clone(), window, cx)
            });
        }
        let reset = field.is_modified(&self.preferences).then(|| {
            let label = t!(@cx, "settings-reset-default");
            let on_reset = cx.listener(View::on_reset(field));
            Button::icon_button(("settings-reset", field as usize))
                .control_icon(IconName::RotateCcw)
                .accessibility_label(label.clone())
                .tooltip(label)
                .on_click(on_reset)
        });
        el! {
            .SettingRow::default [
                .SettingLabel::default [title],
                .SettingActions::default [
                    .SettingReset::default [..reset],
                    .SettingControl::default [control],
                ],
            ]
        }
    }
}
