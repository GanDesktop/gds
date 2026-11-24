//! Default application chooser dialog backed by FreeDesktop `mimeapps.list`.

use std::{collections::HashSet, rc::Rc};

use gds::{
    i18n,
    service::desktop::mime_apps::{self, DesktopApp, MimeAssociations},
    ui::{IconName, IconProps},
};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal, Signal};
use zgui::view::{current_host, install_stylesheet};
use zgui::{css, view};
use zgui_ui::{InputProps, SpinnerProps};
use zgui_ui_primitives::Binding;

use super::super::ui::{Browser, WeakBrowser};

const DEFAULT_APP_SHEET: &str = "gds-files-default-app-dialog";

const DEFAULT_APP_CSS: &str = css!(
    r#"
    .gds-files-default-app__body {
        display: flex;
        flex-direction: column;
        gap: 10px;
        min-width: 0;
    }

    .gds-files-default-app__list {
        display: flex;
        flex-direction: column;
        gap: 2px;
        max-height: 300px;
        overflow-y: auto;
        padding: 2px 0;
    }

    .gds-files-default-app__group {
        font-size: var(--gds-text-xs, 12px);
        font-weight: 600;
        color: var(--gds-muted-foreground);
        padding: 6px 8px 2px 8px;
    }

    .gds-files-default-app__row {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 6px 8px;
        border-radius: var(--gds-radius-control, 6px);
        cursor: pointer;
        user-select: none;
        min-width: 0;
    }

    .gds-files-default-app__row:hover {
        background: var(--gds-hover);
    }

    .gds-files-default-app__row[data-selected="true"] {
        background: var(--gds-selection);
        color: var(--gds-selection-foreground);
    }

    .gds-files-default-app__names {
        display: flex;
        flex-direction: column;
        min-width: 0;
        flex: 1 1 0%;
        gap: 1px;
    }

    .gds-files-default-app__title {
        font-size: var(--gds-text-sm, 14px);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-default-app__sub {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-default-app__row[data-selected="true"] .gds-files-default-app__sub {
        color: var(--gds-selection-foreground);
        opacity: 0.85;
    }
    "#
);

fn install_stylesheet_once() {
    if current_host().is_some() {
        install_stylesheet(DEFAULT_APP_SHEET, DEFAULT_APP_CSS);
    }
}

struct DefaultAppEditorInner {
    browser: WeakBrowser,
    mime: String,
    on_updated: Rc<dyn Fn(DesktopApp)>,
    loading: RwSignal<bool, LocalStorage>,
    saving: RwSignal<bool, LocalStorage>,
    associations: RwSignal<Option<MimeAssociations>, LocalStorage>,
    selected_id: RwSignal<Option<String>, LocalStorage>,
    filter: RwSignal<String, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
}

#[derive(Clone)]
pub(super) struct DefaultAppEditor(Rc<DefaultAppEditorInner>);

impl DefaultAppEditor {
    pub(super) fn new(
        browser: &Browser,
        mime: String,
        on_updated: impl Fn(DesktopApp) + 'static,
    ) -> Self {
        let editor = browser.0.owner.with(|| {
            Self(Rc::new(DefaultAppEditorInner {
                browser: browser.downgrade(),
                mime: mime.clone(),
                on_updated: Rc::new(on_updated),
                loading: RwSignal::new_local(true),
                saving: RwSignal::new_local(false),
                associations: RwSignal::new_local(None),
                selected_id: RwSignal::new_local(None),
                filter: RwSignal::new_local(String::new()),
                error: RwSignal::new_local(None),
            }))
        });

        let weak = Rc::downgrade(&editor.0);
        let query_mime = mime;
        browser.blocking(
            "Load MIME applications",
            move || {
                mime_apps::resolve_system(&query_mime)
                    .ok_or_else(|| "cannot inspect desktop applications".to_owned())
            },
            move |_, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.loading.set(false);
                match result {
                    Ok(assoc) => {
                        let initial = assoc.default_app.as_ref().map(|app| app.id.clone());
                        inner.selected_id.set(initial);
                        inner.associations.set(Some(assoc));
                    }
                    Err(err) => {
                        inner.error.set(Some(err));
                    }
                }
            },
        );
        editor
    }

    pub(super) fn mime(&self) -> &str {
        &self.0.mime
    }

    pub(super) fn is_saving(&self) -> bool {
        self.0.saving.get()
    }

    pub(super) fn is_saving_untracked(&self) -> bool {
        self.0.saving.get_untracked()
    }

    pub(super) fn can_save(&self) -> bool {
        !self.0.loading.get() && !self.0.saving.get() && self.0.selected_id.get().is_some()
    }

    pub(super) fn save(&self, on_done: impl FnOnce() + 'static) {
        if self.0.loading.get_untracked() || self.0.saving.get_untracked() {
            return;
        }
        let Some(selected_id) = self.0.selected_id.get_untracked() else {
            return;
        };
        let Some(browser) = self.0.browser.upgrade() else {
            return;
        };
        let chosen_app = self.0.associations.get_untracked().and_then(|assoc| {
            assoc
                .recommended
                .into_iter()
                .chain(assoc.all)
                .find(|app| app.id == selected_id)
        });
        let mime = self
            .0
            .associations
            .get_untracked()
            .map_or_else(|| self.0.mime.clone(), |assoc| assoc.mime_type);

        self.0.saving.set(true);
        self.0.error.set(None);

        let weak = Rc::downgrade(&self.0);
        let save_id = selected_id.clone();
        browser.blocking(
            "Save default MIME application",
            move || mime_apps::set_default(&mime, &save_id).map_err(|err| err.to_string()),
            move |browser, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.saving.set(false);
                match result {
                    Ok(()) => {
                        if let Some(app) = chosen_app {
                            let name = app.name.clone();
                            (inner.on_updated)(app);
                            browser.report_success(gds::t_untracked!(
                                "details-default-app-saved",
                                app = name
                            ));
                        }
                        on_done();
                    }
                    Err(err) => {
                        inner.error.set(Some(err));
                    }
                }
            },
        );
    }

    pub(super) fn fields(&self) -> AnyView {
        install_stylesheet_once();
        let loading = self.0.loading;
        let saving = self.0.saving;
        let associations = self.0.associations;
        let selected_id = self.0.selected_id;
        let filter = self.0.filter;
        let error = self.0.error;

        let input_disabled = Signal::derive_local(move || loading.get() || saving.get());

        AnyView::new(view! {
            column(class = "gds-files-default-app__body") {
                Input(
                    value = Binding::controlled(
                        filter.read_only(),
                        move |next: String| filter.set(next),
                    ),
                    placeholder = i18n::text("details-default-app-search"),
                    disabled = input_disabled,
                ) {}
                {move || {
                    if loading.get() {
                        return AnyView::new(view! {
                            row(style:gap = "8px", style:align-items = "center") {
                                Spinner(label = i18n::text("loading"))
                                text(class = "gds-files-details__label") {{i18n::text("loading")}}
                            }
                        });
                    }
                    let Some(assoc) = associations.get() else {
                        return AnyView::new(());
                    };
                    let query = filter.get().trim().to_lowercase();
                    let matches_query = |app: &DesktopApp| {
                        query.is_empty()
                            || app.name.to_lowercase().contains(&query)
                            || app.id.to_lowercase().contains(&query)
                            || app
                                .generic_name
                                .as_ref()
                                .is_some_and(|g| g.to_lowercase().contains(&query))
                    };

                    let rec_ids: HashSet<String> =
                        assoc.recommended.iter().map(|app| app.id.clone()).collect();
                    let recommended: Vec<DesktopApp> = assoc
                        .recommended
                        .into_iter()
                        .filter(&matches_query)
                        .collect();
                    let others: Vec<DesktopApp> = assoc
                        .all
                        .into_iter()
                        .filter(|app| !rec_ids.contains(&app.id) && matches_query(app))
                        .collect();

                    let mut items: Vec<AnyView> = Vec::new();
                    if !recommended.is_empty() {
                        items.push(AnyView::new(view! {
                            text(class = "gds-files-default-app__group") {
                                {i18n::text("details-default-app-recommended")}
                            }
                        }));
                        for app in recommended {
                            items.push(app_row_view(app, selected_id));
                        }
                    }
                    if !others.is_empty() {
                        items.push(AnyView::new(view! {
                            text(class = "gds-files-default-app__group") {
                                {i18n::text("details-default-app-other")}
                            }
                        }));
                        for app in others {
                            items.push(app_row_view(app, selected_id));
                        }
                    }
                    if items.is_empty() {
                        return AnyView::new(view! {
                            text(class = "gds-files-details__label") {
                                {i18n::text("details-default-app-empty")}
                            }
                        });
                    }
                    AnyView::new(view! {
                        column(class = "gds-files-default-app__list") {
                            {items}
                        }
                    })
                }}
                {move || error.get().map(|err| {
                    view! {
                        text(class = "gds-files-permissions-error") {{err}}
                    }
                    .into_view()
                })}
            }
        })
    }
}

fn app_row_view(app: DesktopApp, selected_id: RwSignal<Option<String>, LocalStorage>) -> AnyView {
    let id_for_select = app.id.clone();
    let id_for_click = app.id.clone();
    let id_for_check = app.id.clone();
    let subtitle = app.generic_name.unwrap_or(app.id);
    let title = app.name;

    AnyView::new(view! {
        row(
            class = "gds-files-default-app__row",
            attr:data-selected = move || {
                (selected_id.get().as_deref() == Some(id_for_select.as_str()))
                    .then(|| "true".to_owned())
            },
            on:click = move |_| {
                selected_id.set(Some(id_for_click.clone()));
            },
        ) {
            column(class = "gds-files-default-app__names") {
                text(class = "gds-files-default-app__title") {{title}}
                text(class = "gds-files-default-app__sub") {{subtitle}}
            }
            {move || {
                (selected_id.get().as_deref() == Some(id_for_check.as_str())).then(|| {
                    view! {
                        Icon(name = IconName::Check, size = 14.0) {}
                    }
                    .into_view()
                })
            }}
        }
    })
}
