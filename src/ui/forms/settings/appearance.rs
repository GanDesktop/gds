use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::ui::field::{FieldLabelProps, SearchChoiceFieldProps, SearchOption};
use crate::ui::settings::{
    SettingActionsProps, SettingControlProps, SettingLabelProps, SettingResetProps, SettingRowProps,
};
use crate::{
    app::{Gds, appearance::Theme},
    appearance::theme::{self, Source, SourceKind},
    i18n,
};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal, on_cleanup_local};
use zgui::view::install_stylesheet;
use zgui::{css, view};
use zgui_ui::{
    SelectContentProps, SelectItemProps, SelectProps, SelectTriggerProps, SelectValueProps,
};
use zgui_ui_primitives::Binding;

use super::{PERCENTAGE_RANGE, form::Editor, number_setting, reset_button, toggle_setting};
use crate::appearance::typography;

type Entries = Vec<theme::provider::custom::Entry>;

const APPEARANCE_SHEET: &str = "gds-settings-appearance";
const APPEARANCE_CSS: &str = css!(
    r#"
    .gds-settings-appearance {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0;
    }

    .gds-settings-appearance__sample {
        padding: 16px 0;
        border-bottom: 1px solid var(--gds-border);
        color: var(--gds-muted-foreground);
        font-size: calc(18px * var(--gds-font-scale, 1));
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    "#
);

fn system_option_disabled(system_available: Option<bool>) -> bool {
    matches!(system_available, Some(false))
}

fn effective_source_kind(
    source: &Source,
    custom_pending: bool,
    system_available: Option<bool>,
) -> SourceKind {
    if custom_pending {
        SourceKind::Custom
    } else {
        match source {
            Source::System if system_available == Some(false) => SourceKind::Builtin,
            other => SourceKind::from(other),
        }
    }
}

#[derive(Clone)]
pub struct Themes {
    system_available: RwSignal<Option<bool>, LocalStorage>,
    catalogue: RwSignal<Result<Entries, String>, LocalStorage>,
    custom_pending: RwSignal<bool, LocalStorage>,
    last_custom_path: RwSignal<Option<PathBuf>, LocalStorage>,
    scan_generation: Rc<Cell<u64>>,
}

impl Themes {
    pub fn new(source: &Source) -> Self {
        let initial_custom = match source {
            Source::Custom(path) if !path.as_os_str().is_empty() => Some(path.clone()),
            _ => None,
        };
        Self {
            system_available: RwSignal::new_local(None),
            catalogue: RwSignal::new_local(Ok(Vec::new())),
            custom_pending: RwSignal::new_local(false),
            last_custom_path: RwSignal::new_local(initial_custom),
            scan_generation: Rc::new(Cell::new(0)),
        }
    }

    pub fn refresh(&self) {
        let generation = self.scan_generation.get().wrapping_add(1);
        self.scan_generation.set(generation);
        let generation_check = Rc::clone(&self.scan_generation);
        let available = self.system_available;
        let catalogue = self.catalogue;
        let mode = Theme::platform_mode();
        let scope = Gds::current().tasks().owner_scope("Available themes");
        let work = scope.spawn_blocking_named("Discover themes", move || {
            (
                Source::system_available(mode),
                theme::provider::custom::catalog(),
            )
        });
        Gds::current()
            .tasks()
            .spawn_ui("Deliver theme catalog", work, move |result| {
                if generation_check.get() != generation {
                    return;
                }
                match result.and_then(std::convert::identity) {
                    Ok((system, entries)) => {
                        if available.with_untracked(|cur| *cur != Some(system)) {
                            available.set(Some(system));
                        }
                        let next = entries.map_err(|error| error.to_string());
                        if catalogue.with_untracked(|cur| *cur != next) {
                            catalogue.set(next);
                        }
                    }
                    Err(error) => {
                        let msg = error.to_string();
                        if catalogue.with_untracked(|cur| cur.as_ref().err() != Some(&msg)) {
                            catalogue.set(Err(msg));
                        }
                    }
                }
            });
    }

    pub fn normalize_source(&self, source: &mut Source) {
        if self.system_available.get_untracked() == Some(false) && *source == Source::System {
            *source = Source::Builtin;
        }
    }
}

#[derive(Clone)]
struct SelectOption {
    value: String,
    label_id: &'static str,
    disabled: bool,
}

fn select_option_view(option: SelectOption) -> AnyView {
    let label = i18n::text_untracked(option.label_id);
    let label_id = option.label_id;
    AnyView::new(view! {
        SelectItem(
            value = option.value,
            text = label,
            disabled = option.disabled,
        ) {
            {move || i18n::text(label_id)}
        }
    })
}

#[derive(Clone)]
pub struct AppearanceEditor {
    appearance: Binding<theme::Appearance>,
    font: Binding<typography::Preferences>,
    animations: Binding<Option<bool>>,
    themes: Themes,
    focus: NodeRef,
}

impl AppearanceEditor {
    pub fn new(
        appearance: Binding<theme::Appearance>,
        font: Binding<typography::Preferences>,
        animations: Binding<Option<bool>>,
        themes: Themes,
        focus: NodeRef,
    ) -> Self {
        Self {
            appearance,
            font,
            animations,
            themes,
            focus,
        }
    }

    fn field<T: Clone + PartialEq + 'static>(
        &self,
        get: fn(&theme::Appearance) -> T,
        set: fn(&mut theme::Appearance, T),
    ) -> Binding<T> {
        let prefs = self.appearance;
        Binding::controlled(
            Signal::derive_local(move || get(&prefs.get().unwrap_or_default())),
            move |value| {
                if let Some(mut current) = prefs.get_untracked() {
                    set(&mut current, value);
                    prefs.write(current);
                }
            },
        )
    }

    fn set_source(&self, source: Source) {
        if let Some(mut appearance) = self.appearance.get_untracked() {
            appearance.source = source;
            self.appearance.write(appearance);
        }
    }

    fn select_row(
        &self,
        label_id: &'static str,
        placeholder_id: &'static str,
        reset: AnyView,
        value: Signal<String, LocalStorage>,
        options: Signal<Vec<SelectOption>, LocalStorage>,
        on_pick: impl Fn(String) + 'static,
    ) -> AnyView {
        let title_ref = NodeRef::new();

        AnyView::new(view! {
            SettingRow {
                SettingLabel {
                    FieldLabel(node_ref = title_ref) {{move || i18n::text(label_id)}}
                }
                SettingActions {
                    {reset}
                    SettingControl {
                        Select(value = Binding::controlled(value, on_pick)) {
                            SelectTrigger(
                                label = i18n::text_untracked(label_id),
                                labelled_by = Some(title_ref),
                            ) {
                                SelectValue(placeholder = i18n::text_untracked(placeholder_id))
                            }
                            SelectContent {
                                for opt in move || {
                                    let _ = Gds::current().i18n().language();
                                    options.get()
                                }, key = |opt| (opt.value.clone(), i18n::text_untracked(opt.label_id), opt.disabled) {
                                    {select_option_view(opt)}
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    fn theme_source(&self) -> AnyView {
        let preferences = self.appearance;
        let themes = self.themes.clone();
        let available_sig = themes.system_available;
        let pending = themes.custom_pending;
        let last_custom = themes.last_custom_path;

        let sync_custom = RenderEffect::new(move |_| {
            if let Some(current) = preferences.get()
                && let Source::Custom(path) = &current.source
                && !path.as_os_str().is_empty()
            {
                if last_custom.with_untracked(|cur| cur.as_ref() != Some(path)) {
                    last_custom.set(Some(path.clone()));
                }
                if pending.get_untracked() {
                    pending.set(false);
                }
            }
        });
        on_cleanup_local(move || drop(sync_custom));

        // Resolve disabled state inside the `SelectContent` `for` loop so each `SelectItem` holds
        // a static stored bool rather than a derived signal read by `Listbox`'s event handlers.
        let options = Signal::derive_local(move || {
            vec![
                SelectOption {
                    value: source_kind_key(SourceKind::System).to_owned(),
                    label_id: "theme-system",
                    disabled: system_option_disabled(available_sig.get()),
                },
                SelectOption {
                    value: source_kind_key(SourceKind::Builtin).to_owned(),
                    label_id: "theme-builtin",
                    disabled: false,
                },
                SelectOption {
                    value: source_kind_key(SourceKind::Custom).to_owned(),
                    label_id: "theme-custom",
                    disabled: false,
                },
            ]
        });

        let active_kind = Signal::derive_local(move || {
            let source = preferences.get().unwrap_or_default().source;
            effective_source_kind(&source, pending.get(), available_sig.get())
        });
        let selected_kind =
            Signal::derive_local(move || source_kind_key(active_kind.get()).to_owned());

        let view = self.clone();
        let apply_view = self.clone();
        let reset_view = self.clone();
        let modified = Signal::derive_local(move || {
            pending.get() || preferences.get().unwrap_or_default().source != Source::default()
        });
        let reset = reset_button(
            modified,
            self.focus,
            Some("theme-source-reset"),
            move || {
                pending.set(false);
                reset_view.set_source(Source::default());
            },
        );

        let header = self.select_row(
            "theme-source",
            "theme-custom",
            reset,
            selected_kind,
            options,
            move |key| match parse_source_kind(&key) {
                SourceKind::System => {
                    pending.set(false);
                    apply_view.set_source(Source::System);
                }
                SourceKind::Builtin => {
                    pending.set(false);
                    apply_view.set_source(Source::Builtin);
                }
                SourceKind::Custom => {
                    if let Some(path) = last_custom.get_untracked() {
                        pending.set(false);
                        apply_view.set_source(Source::Custom(path));
                    } else {
                        pending.set(true);
                    }
                }
            },
        );

        AnyView::new(view! {
            column(class = "gds-settings-appearance") {
                {header}
                {move || match active_kind.get() {
                    SourceKind::System => AnyView::new(()),
                    SourceKind::Builtin => {
                        let builtin_options = theme::BuiltinTheme::ALL
                            .into_iter()
                            .map(|entry| SelectOption {
                                value: serde_json::to_string(&entry).unwrap_or_default(),
                                label_id: entry.label_id(),
                                disabled: false,
                            })
                            .collect();
                        view.palette_choice(
                            "theme-builtin-selection",
                            |p| p.builtin,
                            |p, v| p.builtin = v,
                            builtin_options,
                        )
                    }
                    SourceKind::Custom => view.custom_choice(),
                }}
            }
        })
    }

    fn custom_choice(&self) -> AnyView {
        let preferences = self.appearance;
        let view = self.clone();
        let catalogue_sig = self.themes.catalogue;
        let last_custom = self.themes.last_custom_path;
        let title_ref = NodeRef::new();

        let selected =
            Signal::derive_local(
                move || match &preferences.get().unwrap_or_default().source {
                    Source::Custom(path) => path.to_string_lossy().into_owned(),
                    _ => String::new(),
                },
            );

        let options = Signal::derive_local(move || {
            let _ = Gds::current().i18n().language();
            let locale = Gds::current().i18n().language().locale();
            let mut items: Vec<SearchOption> = catalogue_sig.with(|catalogue| {
                catalogue.as_ref().ok().map_or_else(Vec::new, |entries| {
                    entries
                        .iter()
                        .map(|entry| {
                            let name = entry.metadata.display_name(locale).to_owned();
                            SearchOption::new(entry.path.to_string_lossy().into_owned(), name)
                        })
                        .collect()
                })
            });
            let current_path = selected.get();
            if !current_path.is_empty() && !items.iter().any(|item| item.value == current_path) {
                items.push(SearchOption::new(current_path.clone(), current_path));
            }
            items
        });

        let error = Signal::derive_local(move || {
            catalogue_sig.with(|entries| entries.as_ref().err().cloned())
        });

        AnyView::new(view! {
            SettingRow {
                SettingLabel {
                    FieldLabel(node_ref = title_ref) {{move || i18n::text("theme-custom-selection")}}
                    {move || error.get().map(|message| view! {
                        text(class = "gds-setting-error") {{message}}
                    }.into_view())}
                }
                SettingActions {
                    SettingReset {}
                    SettingControl {
                        SearchChoiceField(
                            value = Binding::controlled(selected, move |path: String| {
                                let chosen = PathBuf::from(path);
                                last_custom.set(Some(chosen.clone()));
                                view.themes.custom_pending.set(false);
                                view.set_source(Source::Custom(chosen));
                            }),
                            options = options,
                            placeholder = i18n::text_untracked("theme-search"),
                            empty_text = Signal::derive_local(|| i18n::text("empty-filter")),
                            label = i18n::text_untracked("theme-custom-selection"),
                            labelled_by = Some(title_ref),
                        )
                    }
                }
            }
        })
    }

    fn palette_choice<T>(
        &self,
        label_id: &'static str,
        get: fn(&theme::Appearance) -> T,
        set: fn(&mut theme::Appearance, T),
        options: Vec<SelectOption>,
    ) -> AnyView
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Clone + PartialEq + 'static,
    {
        let value = self.field(get, set);
        let default = get(&theme::Appearance::default());
        let modified = Signal::derive_local(move || {
            value
                .get()
                .is_some_and(|v| v != get(&theme::Appearance::default()))
        });
        self.select_row(
            label_id,
            label_id,
            reset_button(modified, self.focus, None, move || {
                value.write(default.clone())
            }),
            Signal::derive_local(move || serde_json::to_string(&value.get()).unwrap_or_default()),
            Signal::stored_local(options),
            move |raw| {
                if let Ok(parsed) = serde_json::from_str::<T>(&raw) {
                    value.write(parsed);
                }
            },
        )
    }

    pub fn view(&self) -> AnyView {
        install_stylesheet(APPEARANCE_SHEET, APPEARANCE_CSS);
        let mode_options = vec![
            SelectOption {
                value: serde_json::to_string(&theme::ModePreference::System).unwrap_or_default(),
                label_id: "theme-mode-system",
                disabled: false,
            },
            SelectOption {
                value: serde_json::to_string(&theme::ModePreference::Light).unwrap_or_default(),
                label_id: "theme-light",
                disabled: false,
            },
            SelectOption {
                value: serde_json::to_string(&theme::ModePreference::Dark).unwrap_or_default(),
                label_id: "theme-dark",
                disabled: false,
            },
        ];
        let defaults = theme::Appearance::default();
        let font = Editor::new(self.font).reset_focus(self.focus).view();
        let animations = self.animations;
        let motion = Binding::controlled(
            Signal::derive_local(move || animations.get().flatten().unwrap_or(true)),
            move |v| animations.write(Some(v)),
        );
        let opacity = |label,
                       get: fn(&theme::Appearance) -> f32,
                       set: fn(&mut theme::Appearance, f32)| {
            let fraction = self.field(get, set);
            let percent = Binding::controlled(
                Signal::derive_local(move || {
                    f64::from(PERCENTAGE_RANGE.quantize(fraction.get().unwrap_or_default() * 100.0))
                }),
                move |v: f64| fraction.write(PERCENTAGE_RANGE.quantize(v as f32) / 100.0),
            );
            number_setting(
                label,
                None,
                percent,
                PERCENTAGE_RANGE,
                "%",
                f64::from(PERCENTAGE_RANGE.quantize(get(&defaults) * 100.0)),
                self.focus,
            )
        };
        AnyView::new(view! {
            column(class = "gds-settings-appearance") {
                {self.theme_source()}
                {self.palette_choice("theme-mode", |p| p.mode, |p,v| p.mode=v, mode_options)}
                {toggle_setting("appearance-rounded-corners", None,
                    self.field(|p|p.rounded_corners, |p,v|p.rounded_corners=v), defaults.rounded_corners, self.focus)}
                {super::toggle_with_reset("appearance-animations", Some("appearance-animations-help"), motion, true, self.focus, move || animations.write(None))}
                {opacity("theme-opacity", |p| p.content_opacity, |p,v| p.content_opacity=v)}
                {opacity("sidebar-opacity", |p| p.sidebar_opacity, |p,v| p.sidebar_opacity=v)}
                {font}
                text(class = "gds-settings-appearance__sample") {{move || i18n::text("font-sample")}}
            }
        })
    }
}

fn source_kind_key(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::System => "system",
        SourceKind::Builtin => "builtin",
        SourceKind::Custom => "custom",
    }
}

fn parse_source_kind(key: &str) -> SourceKind {
    match key {
        "system" => SourceKind::System,
        "custom" => SourceKind::Custom,
        _ => SourceKind::Builtin,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::appearance::theme::{Source, SourceKind};

    use super::{effective_source_kind, system_option_disabled};

    #[test]
    fn unprobed_system_availability_preserves_system_source_and_keeps_option_enabled() {
        assert!(!system_option_disabled(None));
        assert!(!system_option_disabled(Some(true)));
        assert!(system_option_disabled(Some(false)));

        assert_eq!(
            effective_source_kind(&Source::System, false, None),
            SourceKind::System,
            "None (async probe in flight) must never coerce Source::System to Builtin"
        );
        assert_eq!(
            effective_source_kind(&Source::System, false, Some(true)),
            SourceKind::System
        );
        assert_eq!(
            effective_source_kind(&Source::System, false, Some(false)),
            SourceKind::Builtin
        );
        assert_eq!(
            effective_source_kind(&Source::Builtin, true, Some(true)),
            SourceKind::Custom
        );
        assert_eq!(
            effective_source_kind(&Source::Custom(PathBuf::from("nord.toml")), false, None),
            SourceKind::Custom
        );
    }
}
