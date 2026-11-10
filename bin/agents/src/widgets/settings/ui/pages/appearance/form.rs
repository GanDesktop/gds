use super::AppearancePage;
use crate::shared::i18n::tr;
use crate::shared::lib::appearance::{
    self as theme,
    preferences::{
        Background, ComposerEnterBehavior, Language, Preferences, ThemeMode, ThemeSource,
    },
};
use gds::appearance::theme::{BuiltinTheme, SourceKind, provider::custom};
use gds::ui::settings::{FONT_SIZE_RANGE, NumberControl, NumberRange, PERCENTAGE_RANGE};
use gpui_kit::{
    App, AppContext, Context, Entity, Window,
    component::{IndexPath, input::*, select::*},
};

pub(super) struct Form {
    pub animations_enabled: Option<bool>,
    pub rounded_corners: bool,
    pub confirm_delete: Option<bool>,
    pub auto_collapse_activity: Option<bool>,
    pub default_agent: Entity<SelectState<Vec<Choice<Option<String>>>>>,
    pub default_agent_choices: Vec<Option<String>>,
    pub language: Entity<SelectState<SearchableVec<Choice<Language>>>>,
    pub ui_font: Entity<SelectState<SearchableVec<FontChoice>>>,
    pub mono_font: Entity<SelectState<SearchableVec<FontChoice>>>,
    pub fallbacks: Entity<InputState>,
    pub ui_size: Numeric,
    pub content_size: Numeric,
    pub line_height: Numeric,
    pub opacity: Numeric,
    pub sidebar_opacity: Numeric,
    pub source: Entity<SelectState<Vec<SourceChoice>>>,
    pub system_theme_available: bool,
    pub custom: Entity<SelectState<Vec<CustomChoice>>>,
    pub builtin: Entity<SelectState<Vec<Choice<BuiltinTheme>>>>,
    pub mode: Entity<SelectState<Vec<Choice<ThemeMode>>>>,
    pub background: Entity<SelectState<Vec<Choice<Background>>>>,
    pub composer_enter: Entity<SelectState<Vec<Choice<ComposerEnterBehavior>>>>,
}

pub(super) struct Numeric {
    pub control: NumberControl,
    pub value: Option<f32>,
}

impl Numeric {
    fn new(
        value: Option<f32>,
        default: f32,
        range: NumberRange,
        scale: f32,
        cx: &mut Context<AppearancePage>,
    ) -> Self {
        Self {
            control: NumberControl::new(value.unwrap_or(default) * scale, range, cx),
            value,
        }
    }
}

#[derive(Clone)]
pub(super) struct Choice<T>(pub T);

impl SelectItem for Choice<Option<String>> {
    type Value = Option<String>;
    fn title(&self) -> gpui_kit::SharedString {
        self.0
            .clone()
            .unwrap_or_else(|| tr!("settings-last-agent").into())
            .into()
    }
    fn value(&self) -> &Self::Value {
        &self.0
    }
}

impl SelectItem for Choice<Language> {
    type Value = Language;
    fn title(&self) -> gpui_kit::SharedString {
        match self.0 {
            Language::Chinese => "简体中文",
            Language::English => "English",
        }
        .into()
    }
    fn value(&self) -> &Language {
        &self.0
    }
}

#[derive(Clone)]
pub(super) struct SourceChoice {
    value: SourceKind,
    disabled: bool,
}

impl SelectItem for SourceChoice {
    type Value = SourceKind;
    fn title(&self) -> gpui_kit::SharedString {
        match self.value {
            SourceKind::Custom => tr!("appearance-custom"),
            SourceKind::System => tr!("appearance-system"),
            SourceKind::Builtin => tr!("appearance-builtin"),
        }
        .into()
    }
    fn value(&self) -> &SourceKind {
        &self.value
    }
    fn disabled(&self) -> bool {
        self.disabled
    }
}

pub(super) fn source_kind(source: &ThemeSource) -> SourceKind {
    match source {
        ThemeSource::Custom(_) => SourceKind::Custom,
        ThemeSource::Desktop => SourceKind::System,
        ThemeSource::Builtin => SourceKind::Builtin,
    }
}

#[derive(Clone)]
pub(super) struct CustomChoice(custom::Entry);
impl SelectItem for CustomChoice {
    type Value = std::path::PathBuf;
    fn title(&self) -> gpui_kit::SharedString {
        self.0
            .metadata
            .display_name(crate::shared::i18n::language().locale())
            .to_owned()
            .into()
    }
    fn value(&self) -> &Self::Value {
        &self.0.path
    }
}

impl SelectItem for Choice<BuiltinTheme> {
    type Value = BuiltinTheme;
    fn title(&self) -> gpui_kit::SharedString {
        crate::shared::i18n::text(self.0.label_id()).into()
    }
    fn value(&self) -> &BuiltinTheme {
        &self.0
    }
}

impl SelectItem for Choice<ThemeMode> {
    type Value = ThemeMode;
    fn title(&self) -> gpui_kit::SharedString {
        match self.0 {
            ThemeMode::Desktop => tr!("appearance-desktop"),
            ThemeMode::System => tr!("appearance-system"),
            ThemeMode::Light => tr!("appearance-light"),
            ThemeMode::Dark => tr!("appearance-dark"),
        }
        .into()
    }
    fn value(&self) -> &ThemeMode {
        &self.0
    }
}

impl SelectItem for Choice<Background> {
    type Value = Background;
    fn title(&self) -> gpui_kit::SharedString {
        match self.0 {
            Background::Auto => tr!("appearance-auto"),
            Background::Opaque => tr!("appearance-opaque"),
            Background::Transparent => tr!("appearance-transparent"),
            Background::Blurred => tr!("appearance-blurred"),
        }
        .into()
    }
    fn value(&self) -> &Background {
        &self.0
    }
}

impl SelectItem for Choice<ComposerEnterBehavior> {
    type Value = ComposerEnterBehavior;
    fn title(&self) -> gpui_kit::SharedString {
        match self.0 {
            ComposerEnterBehavior::Send => tr!("composer-enter-send"),
            ComposerEnterBehavior::Newline => tr!("composer-enter-newline"),
        }
        .into()
    }
    fn value(&self) -> &ComposerEnterBehavior {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum DraftError {
    #[error("{}", validation_message(.0))]
    Invalid(#[from] theme::preferences::ValidationError),
}

fn validation_message(error: &theme::preferences::ValidationError) -> String {
    use theme::preferences::ValidationError;
    let label = |field| match field {
        "ui_font_size" => tr!("appearance-ui-size"),
        "content_font_size" => tr!("appearance-content-size"),
        "content_line_height" => tr!("appearance-line-height"),
        "content_opacity" => tr!("appearance-opacity"),
        "sidebar_opacity" => tr!("appearance-sidebar-opacity"),
        "ui_font_family" => tr!("appearance-ui-font"),
        "mono_font_family" => tr!("appearance-mono-font"),
        "font_fallbacks" => tr!("appearance-fallbacks"),
        _ => tr!("settings-appearance"),
    };
    match error {
        ValidationError::Positive(field) => {
            tr!("appearance-positive", field = label(field))
        }
        ValidationError::Opacity(field) => {
            tr!("appearance-opacity-range", field = label(field))
        }
        ValidationError::Font(field) => {
            tr!("appearance-font-required", field = label(field))
        }
        ValidationError::ThemeFile => tr!("appearance-path-required").into(),
    }
}

impl Form {
    pub fn new(p: &Preferences, window: &mut Window, cx: &mut Context<AppearancePage>) -> Self {
        let opacity = gds::appearance::tokens::SurfaceOpacity::default();
        let system_theme_available =
            gds::appearance::theme::Source::system_available(cx.window_appearance());
        let sources = vec![SourceKind::System, SourceKind::Builtin, SourceKind::Custom];
        let mut selected_source = source_kind(&p.theme_source);
        if !system_theme_available && selected_source == SourceKind::System {
            selected_source = SourceKind::Builtin;
        }
        let default_agent_choices: Vec<_> = std::iter::once(None)
            .chain(p.default_agent.clone().map(Some))
            .collect();
        let default_agent = choice(
            default_agent_choices.clone(),
            p.default_agent.clone(),
            window,
            cx,
        );
        let mut input = |value: String| {
            cx.new(|cx| {
                let mut input =
                    InputState::new(window, cx).placeholder(tr!("appearance-inherit-placeholder"));
                input.set_value(value, window, cx);
                input
            })
        };
        Self {
            animations_enabled: p.animations_enabled,
            rounded_corners: p.rounded_corners,
            confirm_delete: p.confirm_session_delete,
            auto_collapse_activity: p.auto_collapse_activity,
            default_agent,
            default_agent_choices,
            fallbacks: input(
                p.font_fallbacks
                    .as_ref()
                    .map(|fonts| {
                        if fonts.is_empty() {
                            "[]".into()
                        } else {
                            fonts.join(", ")
                        }
                    })
                    .unwrap_or_default(),
            ),
            ui_size: Numeric::new(
                p.ui_font_size,
                gds::appearance::typography::DEFAULT_SIZE,
                FONT_SIZE_RANGE,
                1.,
                cx,
            ),
            content_size: Numeric::new(
                p.content_font_size,
                gds::appearance::typography::DEFAULT_SIZE * theme::preferences::CONTENT_FONT_SCALE,
                FONT_SIZE_RANGE,
                1.,
                cx,
            ),
            line_height: Numeric::new(
                p.content_line_height,
                theme::preferences::DEFAULT_LINE_HEIGHT,
                NumberRange {
                    min: 1.,
                    max: 3.,
                    step: 0.1,
                },
                1.,
                cx,
            ),
            opacity: Numeric::new(
                p.content_opacity,
                opacity.content,
                PERCENTAGE_RANGE,
                100.,
                cx,
            ),
            sidebar_opacity: Numeric::new(
                p.sidebar_opacity,
                opacity.sidebar,
                PERCENTAGE_RANGE,
                100.,
                cx,
            ),
            language: language_choice(
                vec![Language::Chinese, Language::English],
                p.language,
                window,
                cx,
            ),
            ui_font: font_choice(p.ui_font_family.clone(), window, cx),
            mono_font: font_choice(p.mono_font_family.clone(), window, cx),
            source: {
                let selected = sources
                    .iter()
                    .position(|source| *source == selected_source)
                    .map(IndexPath::new);
                let items = sources
                    .into_iter()
                    .map(|value| SourceChoice {
                        disabled: value == SourceKind::System && !system_theme_available,
                        value,
                    })
                    .collect::<Vec<_>>();
                cx.new(|cx| SelectState::new(items, selected, window, cx))
            },
            system_theme_available,
            custom: {
                let items = custom_themes();
                let selected = match &p.theme_source {
                    ThemeSource::Custom(path) => items
                        .iter()
                        .position(|item| item.value() == path)
                        .map(IndexPath::new),
                    _ => None,
                };
                cx.new(|cx| SelectState::new(items, selected, window, cx))
            },
            builtin: choice(BuiltinTheme::ALL.to_vec(), p.builtin_theme, window, cx),
            mode: choice(
                vec![
                    ThemeMode::Desktop,
                    ThemeMode::System,
                    ThemeMode::Light,
                    ThemeMode::Dark,
                ],
                p.theme_mode,
                window,
                cx,
            ),
            background: choice(
                vec![
                    Background::Auto,
                    Background::Opaque,
                    Background::Transparent,
                    Background::Blurred,
                ],
                p.background,
                window,
                cx,
            ),
            composer_enter: choice(
                vec![ComposerEnterBehavior::Send, ComposerEnterBehavior::Newline],
                p.composer_enter_behavior,
                window,
                cx,
            ),
        }
    }

    /// # Errors
    /// Rejects malformed numeric input and invalid preference values.
    pub fn candidate(&self, cx: &App) -> Result<Preferences, DraftError> {
        let optional = |input: &Entity<InputState>| {
            let value = input.read(cx).value().trim().to_owned();
            (!value.is_empty()).then_some(value)
        };
        let p = Preferences {
            animations_enabled: self.animations_enabled,
            rounded_corners: self.rounded_corners,
            confirm_session_delete: self.confirm_delete,
            auto_collapse_activity: self.auto_collapse_activity,
            default_agent: self
                .default_agent
                .read(cx)
                .selected_value()
                .cloned()
                .flatten(),
            language: self
                .language
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            ui_font_family: self
                .ui_font
                .read(cx)
                .selected_value()
                .and_then(FontChoice::name),
            mono_font_family: self
                .mono_font
                .read(cx)
                .selected_value()
                .and_then(FontChoice::name),
            font_fallbacks: optional(&self.fallbacks).map(|v| {
                if v == "[]" {
                    vec![]
                } else {
                    v.split(',').map(|v| v.trim().to_owned()).collect()
                }
            }),
            ui_font_size: self.ui_size.value,
            content_font_size: self.content_size.value,
            content_line_height: self.line_height.value,
            content_opacity: self.opacity.value,
            sidebar_opacity: self.sidebar_opacity.value,
            theme_file: None,
            theme_source: match self
                .source
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default()
            {
                SourceKind::System => ThemeSource::Desktop,
                SourceKind::Builtin => ThemeSource::Builtin,
                SourceKind::Custom => ThemeSource::Custom(
                    self.custom
                        .read(cx)
                        .selected_value()
                        .cloned()
                        .ok_or(theme::preferences::ValidationError::ThemeFile)?,
                ),
            },
            builtin_theme: self
                .builtin
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            theme_mode: self
                .mode
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            background: self
                .background
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            composer_enter_behavior: self
                .composer_enter
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
        };
        p.validate()?;
        Ok(p)
    }
}

fn language_choice(
    items: Vec<Language>,
    selected: Language,
    window: &mut Window,
    cx: &mut Context<AppearancePage>,
) -> Entity<SelectState<SearchableVec<Choice<Language>>>> {
    let index = items
        .iter()
        .position(|item| *item == selected)
        .map(IndexPath::new);
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(items.into_iter().map(Choice).collect::<Vec<_>>()),
            index,
            window,
            cx,
        )
        .searchable(true)
    })
}

fn choice<T: Clone + PartialEq + 'static>(
    items: Vec<T>,
    selected: T,
    window: &mut Window,
    cx: &mut Context<AppearancePage>,
) -> Entity<SelectState<Vec<Choice<T>>>>
where
    Choice<T>: SelectItem<Value = T>,
{
    let index = items
        .iter()
        .position(|item| *item == selected)
        .map(IndexPath::new);
    cx.new(|cx| SelectState::new(items.into_iter().map(Choice).collect(), index, window, cx))
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum FontChoice {
    Default,
    Named(String),
}

impl FontChoice {
    fn name(&self) -> Option<String> {
        match self {
            Self::Default => None,
            Self::Named(name) => Some(name.clone()),
        }
    }
}

impl SelectItem for FontChoice {
    type Value = Self;
    fn title(&self) -> gpui_kit::SharedString {
        match self {
            Self::Default => tr!("appearance-font-default").into(),
            Self::Named(name) => name.clone().into(),
        }
    }
    fn value(&self) -> &Self {
        self
    }
}

fn font_choice(
    selected: Option<String>,
    window: &mut Window,
    cx: &mut Context<AppearancePage>,
) -> Entity<SelectState<SearchableVec<FontChoice>>> {
    let mut names = cx.text_system().all_font_names();
    if let Some(name) = &selected {
        names.push(name.clone());
    }
    names.sort();
    names.dedup();
    let items = std::iter::once(FontChoice::Default)
        .chain(names.into_iter().map(FontChoice::Named))
        .collect::<Vec<_>>();
    let selected = selected
        .map(FontChoice::Named)
        .unwrap_or(FontChoice::Default);
    let index = items
        .iter()
        .position(|item| *item == selected)
        .map(IndexPath::new);
    cx.new(|cx| SelectState::new(SearchableVec::new(items), index, window, cx).searchable(true))
}

fn custom_themes() -> Vec<CustomChoice> {
    match custom::catalog() {
        Ok(entries) => entries.into_iter().map(CustomChoice).collect(),
        Err(error) => {
            tracing::warn!(%error, "cannot list user themes");
            Vec::new()
        }
    }
}

impl Form {
    pub(super) fn refresh_themes(&self, window: &mut Window, cx: &mut Context<AppearancePage>) {
        let selected = self.custom.read(cx).selected_value().cloned();
        let items = custom_themes();
        self.custom.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            if let Some(selected) = selected {
                state.set_selected_value(&selected, window, cx);
            }
        });
    }
}
