use crate::entities::home::{self, OptionDefinition, Presentation, Target};
use crate::shared::i18n::text;
use gds::ui::{Frame, IconButton, NavItem, page::*, settings::*, sidebar::*};
use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable,
        input::{Input, InputState},
        switch::Switch,
    },
    prelude::*,
    *,
};
use std::collections::BTreeSet;

pub struct HomePage {
    runtime: tokio::runtime::Handle,
    path: Entity<InputState>,
    search: Entity<InputState>,
    flake: String,
    targets: Vec<Target>,
    selected: Option<usize>,
    options: Vec<OptionDefinition>,
    group: String,
    loading: bool,
    error: Option<String>,
    error_expanded: bool,
    opened: bool,
    _search_subscription: Subscription,
}

impl HomePage {
    pub fn new(
        runtime: tokio::runtime::Handle,
        flake: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(text("nixes-path", cx))
                .default_value(flake.clone().unwrap_or_default())
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(text("nixes-search", cx)));
        let subscription = cx.observe(&search, |_, _, cx| cx.notify());
        let mut page = Self {
            runtime,
            path,
            search,
            flake: String::new(),
            targets: vec![],
            selected: None,
            options: vec![],
            group: String::new(),
            loading: false,
            error: None,
            error_expanded: false,
            opened: false,
            _search_subscription: subscription,
        };
        if flake.is_some() {
            page.open(cx);
        }
        page
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.flake = self.path.read(cx).value().trim().to_string();
        self.loading = true;
        self.error = None;
        self.error_expanded = false;
        self.targets.clear();
        self.options.clear();
        self.selected = None;
        self.opened = true;
        let flake = self.flake.clone();
        let task = self
            .runtime
            .spawn(async move { home::discover(&flake).await });
        cx.spawn(async move |page, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.into()));
            let _ = page.update(cx, |page, cx| {
                page.loading = false;
                match result {
                    Ok(targets) => {
                        page.targets = targets;
                        if page.targets.len() == 1 {
                            page.select(0, cx);
                        }
                    }
                    Err(error) => page.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.selected = Some(index);
        self.options.clear();
        self.error = None;
        self.error_expanded = false;
        self.loading = true;
        let target = self.targets[index].clone();
        let flake = self.flake.clone();
        let task = self
            .runtime
            .spawn(async move { target.options(&flake).await });
        cx.spawn(async move |page, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.into()));
            let _ = page.update(cx, |page, cx| {
                page.loading = false;
                match result {
                    Ok(options) => {
                        page.group = if options.iter().any(|option| option.group() == "home") {
                            "home".into()
                        } else {
                            options
                                .iter()
                                .map(OptionDefinition::group)
                                .min()
                                .unwrap_or_default()
                        };
                        page.options = options;
                    }
                    Err(error) => page.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let targets = self
            .targets
            .iter()
            .enumerate()
            .map(|(index, target)| {
                NavItem::new(
                    SharedString::from(format!("target-{index}")),
                    IconName::User,
                    target.label(),
                )
                .selected(self.selected == Some(index))
                .disabled(self.loading)
                .on_click(cx.listener(move |this, _, _, cx| this.select(index, cx)))
            })
            .collect::<Vec<_>>();
        let groups = self
            .options
            .iter()
            .map(OptionDefinition::group)
            .collect::<BTreeSet<_>>();
        let groups = groups
            .into_iter()
            .map(|group| {
                let label = group.clone();
                NavItem::new(
                    SharedString::from(format!("group-{group}")),
                    IconName::Settings,
                    label,
                )
                .selected(self.group == group)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.group = group.clone();
                    cx.notify();
                }))
            })
            .collect::<Vec<_>>();
        el! { .SideBar::default [
            .SideBarHeader::default { p_4; text_sm; } [$(text("nixes-targets", cx))],
            .SideBarContent::default { # "nixes-navigation"; overflow_y_scroll; } [
                ..targets,
                .SideBarSection::default { p_2; } [
                    $(SideBarSectionTitle::new(text("nixes-modules", cx))),
                    ..groups,
                ],
            ],
            .SideBarFooter::default { p_4; text_sm; } ["Home Manager"],
        ] }
        .into_any_element()
    }

    fn value(value: &Presentation, cx: &App) -> String {
        match &value.data {
            serde_json::Value::Null => text(
                if value.kind == "unsupported" {
                    "nixes-complex"
                } else {
                    "nixes-null"
                },
                cx,
            )
            .to_string(),
            serde_json::Value::Array(values) if values.is_empty() => {
                text("nixes-empty-value", cx).to_string()
            }
            serde_json::Value::String(value) => value.clone(),
            serde_json::Value::Bool(value) => {
                text(if *value { "nixes-on" } else { "nixes-off" }, cx).to_string()
            }
            serde_json::Value::Array(values) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                })
                .collect::<Vec<_>>()
                .join(" · "),
            value => value.to_string(),
        }
    }

    fn option_row(option: &OptionDefinition, cx: &App) -> AnyElement {
        let id = option.path.join(".");
        let value = option
            .current
            .value
            .as_ref()
            .filter(|_| option.current.available);
        let control = match value {
            Some(Presentation { kind, data: serde_json::Value::Bool(enabled) }) if kind == "scalar" => {
                Switch::new(SharedString::from(id.clone())).checked(*enabled).disabled(true).into_any_element()
            }
            Some(value) => el! { .div { text_sm; max_w_full; line_clamp: 3; } [$(Self::value(value, cx))] }.into_any_element(),
            None => el! { .div { text_sm; text_color: $(cx.theme().muted_foreground); } [$(text("nixes-unavailable", cx))] }.into_any_element(),
        };
        let mut details = format!("{} · {}", id, option.type_name);
        if option.read_only {
            details.push_str(&format!(" · {}", text("nixes-defined", cx)));
        }
        if let Some(default) = option
            .default
            .value
            .as_ref()
            .filter(|_| option.default.available)
        {
            details.push_str(&format!(
                "\n{}: {}",
                text("nixes-default", cx),
                Self::value(default, cx)
            ));
        }
        // Descriptions come from modules; keep the first paragraph beside the field.
        let description = option
            .description
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .replace('`', "");
        let row = el! { .SettingRow::default [
            .SettingLabel::default [
                $(option.title()),
                .SettingDescription::default { line_clamp: 2; } [description],
            ],
            .SettingActions::default [ .SettingControl::default [control] ],
        ] };
        let hint = format!(
            "{}\n{}\n{}",
            option.description,
            details,
            value
                .map(|value| Self::value(value, cx))
                .unwrap_or_default()
        );
        gds::ui::tooltip::bounded(SharedString::from(format!("option-{id}")), row, hint)
            .w_full()
            .into_any_element()
    }

    fn content(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.read(cx).value().to_lowercase();
        let mut options = self
            .options
            .iter()
            .filter(|option| {
                if query.is_empty() {
                    option.group() == self.group
                } else {
                    option.path.join(".").to_lowercase().contains(&query)
                        || option.description.to_lowercase().contains(&query)
                }
            })
            .collect::<Vec<_>>();
        options.sort_by_key(|option| match option.path.join(".").as_str() {
            "home.username" => 0,
            "home.homeDirectory" => 1,
            "home.stateVersion" => 2,
            "home.packages" => 3,
            "home.sessionVariables" => 4,
            _ => 5,
        });
        let title = if self.loading || self.error.is_some() {
            self.selected
                .map(|index| self.targets[index].label())
                .unwrap_or_else(|| "Home Manager".into())
        } else if self.selected.is_some() {
            self.group.clone()
        } else {
            text("nixes-welcome", cx).to_string()
        };
        let body = if self.loading {
            el! { .div { p_6; } [$(text("nixes-loading", cx))] }.into_any_element()
        } else if let Some(error) = &self.error {
            let details = gds::ui::button::Button::new("nixes-error-details")
                .label(text(
                    if self.error_expanded {
                        "nixes-hide-details"
                    } else {
                        "nixes-show-details"
                    },
                    cx,
                ))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.error_expanded = !this.error_expanded;
                    cx.notify();
                }));
            el! { .div { # "nixes-error"; size_full; overflow_y_scroll; p_6; } [
                .div { flex; flex_col; gap_4; } [
                    $(text("nixes-error", cx)),
                    .div { text_sm; text_color: $(cx.theme().muted_foreground); } [$(text("nixes-error-help", cx))],
                    .div [details],
                    @if (self.error_expanded)
                        .div { text_sm; } [$(error.clone())],
                    @end,
                ],
            ] }.into_any_element()
        } else if self.selected.is_none() {
            let message = if !self.opened {
                "nixes-start"
            } else if self.targets.is_empty() {
                "nixes-no-targets"
            } else {
                "nixes-select"
            };
            el! { .div { p_6; text_color: $(cx.theme().muted_foreground); } [$(text(message, cx))] }
                .into_any_element()
        } else {
            let rows = options
                .iter()
                .map(|option| Self::option_row(option, cx))
                .collect::<Vec<_>>();
            el! { .div { # "nixes-options"; size_full; overflow_y_scroll; px_6; } [
                @if (rows.is_empty()) $(text("nixes-empty", cx)), @end,
                ..rows,
            ] }
            .into_any_element()
        };
        el! { .ContentPage::default [
            .PageHeader::default [ .PageTitle::default [title], .PageActions::default [
                @if (!self.loading && self.error.is_none() && self.selected.is_some())
                    format!("{} {}", options.len(), text("nixes-options", cx)),
                @end,
            ] ],
            .PageContent::default [body],
        ] }
        .into_any_element()
    }
}

impl Render for HomePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = Frame::default();
        let controls = el! { .div { flex; items_center; gap_2; w_full; } [
            .div { flex_1; min_w_0; } [$(Input::new(&self.path))],
            $(IconButton::new("open-flake").icon(IconName::FolderOpen).label(text("nixes-open", cx)).disabled(self.loading).on_click(cx.listener(|this, _, _, cx| this.open(cx)))),
            .div { w: 240.0.px; } [$(Input::new(&self.search))],
        ] }.into_any_element();
        let toolbar = frame.toolbar(
            el! { .div { text_sm; } ["GDS Nixes"] }.into_any_element(),
            controls,
            cx,
        );
        let sidebar = self.sidebar(cx);
        let content = self.content(cx);
        let workspace = frame.workspace(sidebar, content, cx);
        let footer = frame.footer(el! { .div { px_4; text_sm; } ["flakes"] }.into_any_element(), el! { .div { text_sm; text_color: $(cx.theme().muted_foreground); } [$(text("nixes-readonly", cx))] }.into_any_element(), div().into_any_element(), cx);
        el! { .div { size_full; flex; flex_col; text_color: $(cx.theme().foreground); } [toolbar, workspace, footer] }
    }
}
