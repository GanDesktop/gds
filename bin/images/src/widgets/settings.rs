//! Image preferences presented in the suite settings workspace.

use gds::{
    i18n,
    ui::{
        IconName, keytips, notifications,
        settings::{
            self, NumberRange, SettingsCategoryProps, SettingsColumnProps, SettingsNavigationProps,
            SettingsPageProps,
            appearance::{AppearanceEditor, Themes},
        },
        sidebar::SidebarPresentation,
    },
};
use zgui::prelude::*;
use zgui_ui_primitives::Binding;

use crate::entities::preferences::{Preferences, Service};

const JPEG_QUALITY_MIN: f64 = 10.0;
const JPEG_QUALITY_MAX: f64 = 100.0;
const JPEG_QUALITY_RANGE: NumberRange = NumberRange {
    min: JPEG_QUALITY_MIN as f32,
    max: JPEG_QUALITY_MAX as f32,
    step: 1.0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Appearance,
    Viewer,
    Export,
}

impl Page {
    const ALL: [Self; 3] = [Self::Appearance, Self::Viewer, Self::Export];

    const fn index(self) -> usize {
        match self {
            Self::Appearance => 0,
            Self::Viewer => 1,
            Self::Export => 2,
        }
    }

    const fn label_id(self) -> &'static str {
        match self {
            Self::Appearance => "settings-appearance",
            Self::Viewer => "settings-viewer",
            Self::Export => "settings-export",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::Viewer => IconName::Image,
            Self::Export => IconName::ShieldCheck,
        }
    }

    fn adjacent(self, previous: bool) -> Self {
        Self::ALL[settings::adjacent_page(self.index(), Self::ALL.len(), previous)]
    }
}

#[derive(Clone)]
pub struct View {
    service: Service,
    page: RwSignal<Page, LocalStorage>,
    focus: NodeRef,
    themes: Themes,
}

impl View {
    pub fn new(service: Service) -> Self {
        let themes = service
            .state()
            .with_untracked(|preferences| Themes::new(&preferences.appearance.source));
        Self {
            service,
            themes,
            page: RwSignal::new_local(Page::Appearance),
            focus: NodeRef::new(),
        }
    }

    fn apply(&self, change: impl FnOnce(&mut Preferences)) {
        if let Err(error) = self.service.edit(|preferences| {
            change(preferences);
            self.themes
                .normalize_source(&mut preferences.appearance.source);
        }) {
            notifications::error(error.to_string());
        }
    }

    fn binding<T: Clone + PartialEq + 'static>(
        &self,
        get: fn(&Preferences) -> T,
        set: fn(&mut Preferences, T),
    ) -> Binding<T> {
        let state = self.service.state();
        let view = self.clone();
        Binding::controlled(
            Signal::derive_local(move || state.with(get)),
            move |value| {
                view.apply(|preferences| set(preferences, value));
            },
        )
    }

    fn cycle(&self, previous: bool) {
        if keytips::has_active_modal_scope() {
            return;
        }
        self.page.update(|page| *page = page.adjacent(previous));
        self.focus.focus();
    }

    fn handle_cycle_shortcut(
        &self,
        event: &mut zgui::view::EventCx<'_, zgui::view::events::KeyDown>,
    ) {
        settings::handle_page_cycle(event, |previous| self.cycle(previous));
    }

    pub fn navigation(&self, presentation: Signal<SidebarPresentation, LocalStorage>) -> AnyView {
        let page = self.page;
        let items: Vec<_> = Page::ALL
            .into_iter()
            .map(|destination| {
                view! {
                    SettingsCategory(
                        label = Signal::derive_local(move || i18n::text(destination.label_id())),
                        icon = destination.icon(),
                        selected = Signal::derive_local(move || page.get() == destination),
                        shortcut = Signal::derive_local(move || {
                            settings::category_shortcut(
                                page.get().index(),
                                destination.index(),
                                Page::ALL.len(),
                            )
                        }),
                        on_click = UnsyncCallback::new(move |()| {
                            if page.get_untracked() != destination {
                                page.set(destination);
                            }
                        }),
                    )
                }
                .into_view()
            })
            .collect();

        let navigation = self.clone();
        AnyView::new(view! {
            SettingsNavigation(
                presentation = presentation,
                label = i18n::text_untracked("settings-title"),
                on:key_down = move |event| navigation.handle_cycle_shortcut(event),
            ) {
                {items}
            }
        })
    }

    pub fn content(&self) -> AnyView {
        self.themes.refresh();
        let page = self.page;
        let focus = self.focus;
        let navigation = self.clone();
        let contents = self.clone();
        AnyView::new(view! {
            SettingsPage(
                heading = Signal::derive_local(move || i18n::text(page.get().label_id())),
                node_ref = focus,
                on:key_down = move |event| navigation.handle_cycle_shortcut(event),
            ) {
                {move || contents.fields(page.get())}
            }
        })
    }

    fn fields(&self, page: Page) -> AnyView {
        let defaults = Preferences::default();
        let focus = self.focus;
        match page {
            Page::Appearance => AppearanceEditor::new(
                self.binding(
                    |preferences| preferences.appearance.clone(),
                    |preferences, value| preferences.appearance = value,
                ),
                self.binding(
                    |preferences| preferences.font.clone(),
                    |preferences, value| preferences.font = value,
                ),
                self.binding(
                    |preferences| preferences.animations_enabled,
                    |preferences, value| preferences.animations_enabled = value,
                ),
                self.themes.clone(),
                focus,
            )
            .view(),
            Page::Viewer => {
                let checkerboard = self.binding(
                    |preferences| preferences.checkerboard_backdrop,
                    |preferences, value| preferences.checkerboard_backdrop = value,
                );
                AnyView::new(view! {
                    SettingsColumn {
                        {settings::toggle_setting(
                            "pref-checkerboard",
                            Some("pref-checkerboard-desc"),
                            checkerboard,
                            defaults.checkerboard_backdrop,
                            focus,
                        )}
                    }
                })
            }
            Page::Export => {
                let strip_on_copy = self.binding(
                    |preferences| preferences.strip_privacy_on_copy,
                    |preferences, value| preferences.strip_privacy_on_copy = value,
                );
                let jpeg_quality = self.binding(
                    |preferences| f64::from(preferences.default_jpeg_quality),
                    |preferences, value| {
                        preferences.default_jpeg_quality =
                            value.round().clamp(JPEG_QUALITY_MIN, JPEG_QUALITY_MAX) as u8;
                    },
                );
                AnyView::new(view! {
                    SettingsColumn {
                        {settings::toggle_setting(
                            "pref-strip-on-copy",
                            Some("pref-strip-on-copy-desc"),
                            strip_on_copy,
                            defaults.strip_privacy_on_copy,
                            focus,
                        )}
                        {settings::number_setting(
                            "pref-jpeg-quality",
                            Some("pref-jpeg-quality-desc"),
                            jpeg_quality,
                            JPEG_QUALITY_RANGE,
                            "",
                            f64::from(defaults.default_jpeg_quality),
                            focus,
                        )}
                    }
                })
            }
        }
    }
}
