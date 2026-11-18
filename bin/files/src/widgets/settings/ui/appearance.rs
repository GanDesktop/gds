use super::{Preferences, View};
use anti_webui::prelude::*;
use gds::{
    app::{Gds, appearance::Theme},
    appearance::theme::{self, Source, SourceKind},
    i18n,
    ui::{
        Icon, IconButton,
        field::{SearchOption, search_choice},
        settings::{
            SettingsActions, SettingsControl, SettingsDescription, SettingsLabel, SettingsReset,
            SettingsRow,
        },
    },
};
use gds_uim::el;
use std::path::PathBuf;

type Entries = Vec<theme::provider::custom::Entry>;

#[derive(Clone)]
pub(super) struct Themes {
    system_available: Signal<Option<bool>>,
    catalogue: Signal<Result<Entries, String>>,
    custom_pending: Signal<bool>,
}

impl Themes {
    pub(super) fn new() -> Self {
        Self {
            system_available: signal(None),
            catalogue: signal(Ok(Vec::new())),
            custom_pending: signal(false),
        }
    }

    pub(super) fn refresh(&self) {
        let available = self.system_available;
        let catalogue = self.catalogue;
        let (task, complete) = Task::pending();
        let scope = Gds::current().tasks().owner_scope("Available themes");
        scope
            .spawn_blocking_named("Discover themes", || {
                (
                    Source::system_available(Theme::platform_mode()),
                    theme::provider::custom::catalog(),
                )
            })
            .on_complete(&scope, move |result| complete.complete(result));
        task.then(move |result| match result {
            Ok((system, entries)) => batch(|| {
                available.set_if_changed(Some(system));
                catalogue.set_if_changed(entries.map_err(|error| error.to_string()));
            }),
            Err(error) => catalogue.set_if_changed(Err(error.to_string())),
        });
    }

    pub(super) fn normalize_source(&self, source: &mut Source) {
        if self.system_available.get_untracked() == Some(false) && *source == Source::System {
            *source = Source::Builtin;
        }
    }
}

impl View {
    fn theme_choice<T: Clone + PartialEq + 'static>(
        label: &'static str,
        reset: SettingsReset,
        value: Binding<T>,
        options: Vec<SearchOption<T>>,
    ) -> SettingsRow {
        el! { .SettingsRow::default.[
            .SettingsLabel::default.[(move () => label.i18n::text).text],
            .SettingsActions::default.[reset, .SettingsControl::default.[
                $(search_choice(value, options, i18n::text(label), i18n::text("theme-search"))),
            ]],
        ] }
    }

    fn theme_source(&self) -> Div {
        let preferences = self.preferences;
        let themes = self.themes.clone();
        let available = themes.system_available.get();
        let pending = themes.custom_pending;
        let options = [
            (SourceKind::System, "theme-system", "system 系统"),
            (SourceKind::Builtin, "theme-builtin", "builtin 内置"),
            (SourceKind::Custom, "theme-custom", "custom 自定义"),
        ]
        .into_iter()
        .map(|(kind, label, keywords)| {
            SearchOption::new(kind, i18n::text(label))
                .keywords(keywords)
                .disabled(kind == SourceKind::System && available != Some(true))
        })
        .collect();
        let value = Binding::new(
            move || {
                if pending.get() {
                    SourceKind::Custom
                } else {
                    preferences.with(|p| match &p.appearance.source {
                        Source::System if available != Some(true) => SourceKind::Builtin,
                        source => SourceKind::from(source),
                    })
                }
            },
            {
                let view = self.clone();
                move |kind| {
                    pending.set(kind == SourceKind::Custom);
                    match kind {
                        SourceKind::System => view.apply(|p| p.appearance.source = Source::System),
                        SourceKind::Builtin => {
                            view.apply(|p| p.appearance.source = Source::Builtin)
                        }
                        SourceKind::Custom => {}
                    }
                }
            },
        );
        let view = self.clone();
        let kind = memo(move || value.get());
        let reset = self.clone();
        let reset = el! { .SettingsReset::default.[
            (move () => $(pending.get() || preferences.with(|p| p.appearance.source != Source::default())), move () => ${
                let reset = reset.clone();
                el! { "theme-source-reset".IconButton::new.{
                    icon: Icon::Reset; label_fn: () => "form-reset".i18n::text;
                    on_click: move _ => ${
                        pending.set(false);
                        reset.apply(|p| p.appearance.source = Source::default());
                        reset.page_focus.focus();
                    };
                } }
            }).show,
        ] };
        el! { .div.{flex; flex_col}.[
            $(Self::theme_choice("theme-source", reset, value, options)),
            (move () => ${
                match kind.get() {
                    SourceKind::System => div(),
                    SourceKind::Builtin => {
                        let options = theme::BuiltinTheme::ALL.into_iter()
                            .map(|entry| SearchOption::new(entry, i18n::text(entry.label_id())).keywords(entry.label_id())).collect();
                        div().child(view.palette_choice("theme-builtin-selection", |p| p.appearance.builtin, |p,v| p.appearance.builtin = v, options))
                    }
                    SourceKind::Custom => view.custom_choice(),
                }
            }).dyn_view,
        ] }
    }

    fn custom_choice(&self) -> Div {
        let preferences = self.preferences;
        let view = self.clone();
        let value = Binding::new(
            move || {
                preferences.with(|p| match &p.appearance.source {
                    Source::Custom(path) => path.clone(),
                    _ => PathBuf::new(),
                })
            },
            move |path| {
                view.themes.custom_pending.set(false);
                view.apply(|p| p.appearance.source = Source::Custom(path));
            },
        );
        let locale = Gds::current().i18n().language();
        let mut options = self.themes.catalogue.with(|catalogue| {
            catalogue.as_ref().ok().map_or_else(Vec::new, |entries| {
                entries
                    .iter()
                    .map(|entry| {
                        SearchOption::new(
                            entry.path.clone(),
                            entry.metadata.display_name(locale.locale()).to_owned(),
                        )
                    })
                    .collect()
            })
        });
        let selected = value.get_untracked();
        if !selected.as_os_str().is_empty() && !options.iter().any(|item| item.value == selected) {
            options.push(SearchOption::new(
                selected.clone(),
                selected.display().to_string(),
            ));
        }
        let error = self
            .themes
            .catalogue
            .with(|entries| entries.as_ref().err().cloned());
        el! { .div.[
            $(Self::theme_choice("theme-custom-selection", SettingsReset::default(), value, options)),
            if let Some(error) = error [.SettingsDescription::default.[error.text]],
        ] }
    }

    fn palette_choice<T: Clone + PartialEq + 'static>(
        &self,
        label: &'static str,
        get: fn(&Preferences) -> T,
        set: fn(&mut Preferences, T),
        options: Vec<SearchOption<T>>,
    ) -> SettingsRow {
        let preferences = self.preferences;
        let view = self.clone();
        let value = Binding::new(
            move || preferences.with(get),
            move |value| view.apply(|preferences| set(preferences, value)),
        );
        Self::theme_choice(label, self.reset(label, get, set), value, options)
    }

    pub(super) fn appearance(&self) -> Div {
        let source = self.clone();
        let mode = self.clone();
        let font = self.subform(|p| p.font.clone(), |p, v| p.font = v);
        el! { .div.{ flex; flex_col }.[
            (move() => &source.{theme_source}).dyn_view,
            (move() => ${
                let options = [
                    (theme::ModePreference::System, "theme-mode-system", "system automatic 系统"),
                    (theme::ModePreference::Light, "theme-light", "light 浅色"),
                    (theme::ModePreference::Dark, "theme-dark", "dark 深色"),
                ].into_iter().map(|(mode, label, keywords)| SearchOption::new(mode, i18n::text(label)).keywords(keywords)).collect();
                mode.palette_choice("theme-mode", |p| p.appearance.mode, |p,v| p.appearance.mode = v, options)
            }).dyn_view,
            $(self.toggle("appearance-rounded-corners", "", |p|p.appearance.rounded_corners, |p,v|p.appearance.rounded_corners=v)),
            $(self.toggle("appearance-animations", "", |p|p.animations_enabled.unwrap_or(true), |p,v|p.animations_enabled=Some(v))),
            .SettingsDescription::default.{dyn_style:s=>$(s.text_size(Gds::current().appearance().typography().scaled(14.0)))}.[
                (() => "appearance-animations-help".i18n::text).text,
            ],
            $(self.opacity("theme-opacity", |p|p.appearance.content_opacity, |p,v|p.appearance.content_opacity=v)),
            $(self.opacity("sidebar-opacity", |p|p.appearance.sidebar_opacity, |p,v|p.appearance.sidebar_opacity=v)),
            font,
            (() => "font-sample".i18n::text).text.{
                py: 16.0_f32; ellipsis; border_b: 1.0_f32;
                dyn_style: s => $(s.text_size(Gds::current().appearance().typography().scaled(18.0))
                    .text_color(theme().text_muted).border_color(theme().border));
            },
        ] }
    }
}
