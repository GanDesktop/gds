//! Shared presentation and state adapters for native settings controls.

use gds::ui::field::select as select_field;
use gds::ui::settings::NumberControl as NumericControl;
use gds_uim::{component, el};
use gpui_kit::{
    assets::IconName,
    component::{
        IndexPath, Sizable,
        button::ButtonVariants,
        select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState},
    },
    *,
};

use crate::entities::preferences::Preferences;
use crate::widgets::settings::ui::View;
use gds::ui::button::Button;
use gds::ui::settings::{
    SettingActions, SettingControl, SettingDescription, SettingLabel, SettingReset, SettingRow,
};
use gds::{
    appearance::theme::{BuiltinTheme, ModePreference, Source},
    i18n::text,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Source,
    Builtin,
    Mode,
    ContentOpacity,
    SidebarOpacity,
    ShowHidden,
    AutoFocusSearch,
    SidebarVolumes,
    Animations,
    RoundedCorners,
}

impl Field {
    pub fn label_id(self) -> &'static str {
        match self {
            Self::RoundedCorners => "appearance-rounded-corners",
            Self::Animations => "appearance-animations",
            Self::Source => "theme-source",
            Self::Builtin => "theme-builtin-selection",
            Self::Mode => "theme-mode",
            Self::ContentOpacity => "theme-opacity",
            Self::SidebarOpacity => "sidebar-opacity",
            Self::ShowHidden => "show-hidden",
            Self::AutoFocusSearch => "auto-focus-search",
            Self::SidebarVolumes => "sidebar-show-volumes",
        }
    }

    pub fn is_modified(self, preferences: &Preferences) -> bool {
        let defaults = Preferences::default();
        match self {
            Self::RoundedCorners => preferences.appearance.rounded_corners,
            Self::Animations => !preferences.animations_enabled.unwrap_or(true),
            Self::Source => preferences.appearance.source != defaults.appearance.source,
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
        }
    }

    pub fn reset(self, preferences: &mut Preferences) -> bool {
        if !self.is_modified(preferences) {
            return false;
        }
        let defaults = Preferences::default();
        match self {
            Self::RoundedCorners => preferences.appearance.rounded_corners = false,
            Self::Animations => preferences.animations_enabled = defaults.animations_enabled,
            Self::Source => preferences.appearance.source = defaults.appearance.source,
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
}

impl<T> Choice<T> {
    fn new(value: T, title: SharedString, search_terms: &'static str) -> Self {
        Self {
            value,
            title,
            search_terms,
        }
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

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self.search_terms.to_lowercase().contains(&query)
    }
}

impl Choice<Source> {
    pub fn catalog(selected: &Source, cx: &App) -> Vec<Self> {
        let mut choices = vec![
            Self::new(Source::System, text("theme-system", cx), "system 系统"),
            Self::new(Source::Builtin, text("theme-builtin", cx), "builtin 内置"),
        ];
        match gds::appearance::theme::provider::custom::catalog() {
            Ok(paths) => choices.extend(paths.into_iter().map(|path| {
                Self::new(
                    Source::Custom(path.clone()),
                    path.display().to_string().into(),
                    "custom 自定义",
                )
            })),
            Err(error) => tracing::warn!(%error, "cannot list user themes"),
        }
        if let Source::Custom(path) = selected
            && !choices.iter().any(|choice| &choice.value == selected)
        {
            choices.push(Self::new(
                selected.clone(),
                path.display().to_string().into(),
                "custom 自定义",
            ));
        }
        choices
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
                text("theme-mode-system", cx),
                "system automatic 系统",
            ),
            Self::new(ModePreference::Light, text("theme-light", cx), "light 浅色"),
            Self::new(ModePreference::Dark, text("theme-dark", cx), "dark 深色"),
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
    pub(super) fn setting_row(
        &self,
        field: Field,
        control: impl IntoElement,
        cx: &mut Context<View>,
    ) -> SettingRow {
        let reset = if field.is_modified(&self.preferences) {
            let label = text("settings-reset-default", cx);
            let on_reset = cx.listener(View::on_reset(field));
            Some(el! {
                $("settings-reset", field as usize).Button::new {
                    ghost; small; icon: IconName::RotateCcw;
                    accessibility_label: ~label;
                    tooltip: label;
                    on_click: on_reset;
                }
            })
        } else {
            None
        };
        el! {
            .SettingRow::default [
                .SettingLabel::default [$text(field.label_id(), cx)],
                .SettingActions::default [
                    .SettingReset::default [..reset],
                    .SettingControl::default [control],
                ],
            ]
        }
    }
}

#[component]
#[derive(Default, IntoElement)]
pub(super) struct SettingNote {
    #[attr(into)]
    message: SharedString,
}

impl RenderOnce for SettingNote {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        el! { .SettingDescription::default [$self.message] }
    }
}
