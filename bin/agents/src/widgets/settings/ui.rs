//! Settings navigation is independent of the configuration pages it hosts.

use crate::shared::i18n::tr;
use std::collections::HashSet;

use crate::shared::ui::Button;
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::{
    App, ClickEvent, Context, Entity, FocusHandle, Focusable, Render, Subscription, TestSupportExt,
    Window,
    component::{button::*, input::*, *},
    div,
    prelude::*,
    px,
};

mod form;
pub(crate) mod pages;
use pages::{acp, agents, appearance, connections, keybindings, terminal};
pub(crate) mod window;

pub(crate) enum Event {
    Close,
    CloseWindow,
}

const SAVE_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

/// Settings feedback is informational; copying remains available on other UI surfaces.
fn feedback(
    id: impl Into<gpui_kit::SharedString>,
    text: impl Into<gpui_kit::SharedString>,
    color: gpui_kit::Hsla,
) -> crate::shared::ui::Feedback {
    crate::shared::ui::feedback(id, text, color).without_copy()
}

// Keep actions distinct from the ghost navigation items, using upstream state styling.
fn action_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    crate::shared::ui::action_button(id)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Category {
    Appearance,
    Sessions,
    Connections,
    Keybindings,
    Terminal,
    Agents,
    SessionManagement,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Frontend,
    Backend,
}

impl Scope {
    const ALL: [Self; 2] = [Self::Frontend, Self::Backend];

    fn categories(self) -> &'static [Category] {
        match self {
            Self::Frontend => &[
                Category::Appearance,
                Category::Sessions,
                Category::Connections,
                Category::Keybindings,
                Category::Terminal,
            ],
            Self::Backend => &[Category::SessionManagement, Category::Agents],
        }
    }
}

impl Category {
    const ALL: [Self; 7] = [
        Self::Appearance,
        Self::Sessions,
        Self::Connections,
        Self::Keybindings,
        Self::Terminal,
        Self::SessionManagement,
        Self::Agents,
    ];
    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|category| *category == self)
            .expect("registered category")
    }
    fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::Sessions => IconName::MessageSquare,
            Self::Connections => IconName::Server,
            Self::Keybindings => IconName::Keyboard,
            Self::Terminal => IconName::Terminal,
            Self::Agents => IconName::Bot,
            Self::SessionManagement => IconName::Settings,
        }
    }

    fn scope(self) -> Scope {
        match self {
            Self::Appearance
            | Self::Sessions
            | Self::Connections
            | Self::Keybindings
            | Self::Terminal => Scope::Frontend,
            Self::SessionManagement | Self::Agents => Scope::Backend,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Appearance => "settings-appearance",
            Self::Sessions => "settings-sessions",
            Self::Connections => "settings-connections",
            Self::Keybindings => "settings-keybindings",
            Self::Terminal => "settings-terminal",
            Self::Agents => "settings-agents",
            Self::SessionManagement => "settings-session-management",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => tr!("settings-appearance"),
            Self::Sessions => tr!("settings-sessions"),
            Self::Connections => tr!("settings-connections"),
            Self::Keybindings => tr!("settings-keybindings"),
            Self::Terminal => tr!("local-terminal-title"),
            Self::Agents => tr!("settings-agents"),
            Self::SessionManagement => tr!("settings-session-management"),
        }
    }

    fn for_query(query: &str) -> Self {
        let query = query.trim().to_lowercase();
        if ["terminal", "shell", "终端"]
            .iter()
            .any(|word| query.contains(word))
        {
            return Self::Terminal;
        }
        if ["shortcut", "keybinding", "快捷键"]
            .iter()
            .any(|word| query.contains(word))
        {
            return Self::Keybindings;
        }
        if query == "acp"
            || [
                "session management",
                "会话管理",
                "pool",
                "idle",
                "池",
                "回收",
                "预热",
            ]
            .iter()
            .any(|word| query.contains(word))
        {
            return Self::SessionManagement;
        }
        if !query.is_empty() && appearance::AppearancePage::matches_session_query(&query) {
            return Self::Sessions;
        }
        if ["connection", "server", "连接", "服务器", "端点"]
            .iter()
            .any(|word| query.contains(word))
        {
            return Self::Connections;
        }
        let agents = query.split_whitespace().any(|word| {
            matches!(
                word,
                "agent" | "agents" | "acp" | "codex" | "opencode" | "pi"
            )
        }) || [
            "智能体",
            "服务",
            "后端",
            "命令",
            "参数",
            "service",
            "backend",
            "command",
            "arguments",
        ]
        .iter()
        .any(|word| query.contains(word));
        if agents {
            Self::Agents
        } else {
            Self::Appearance
        }
    }
}

pub(crate) struct Settings {
    focus: FocusHandle,
    page_focus: [FocusHandle; 7],
    sidebar_compact: bool,
    search: Entity<InputState>,
    appearance: Entity<appearance::AppearancePage>,
    connections: Entity<connections::Page>,
    terminal: Entity<terminal::Page>,
    profiles: Entity<crate::features::navigation::Profiles>,
    agents: Entity<agents::Agents>,
    _agents_subscription: Subscription,
    acp: Entity<acp::Page>,
    _acp_subscription: Subscription,
    connection_message: String,
    category: Category,
    /// Section the sidebar points at inside each page.
    appearance_group: Option<appearance::Group>,
    agent_section: Option<usize>,
    expanded: HashSet<Category>,
    _subscriptions: Vec<Subscription>,
}

impl gpui_kit::EventEmitter<Event> for Settings {}

impl Settings {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr!("settings-search")));
        let editor = crate::shared::lib::appearance::Editor::current(cx);
        let agents = agents::shared(window, cx);
        let appearance = cx.new(|cx| appearance::AppearancePage::new(editor, window, cx));
        let profiles = crate::features::navigation::shared(cx);
        let connections = cx.new(|cx| connections::Page::new(window, cx));
        let terminal = cx.new(|cx| terminal::Page::new(window, cx));
        let acp = acp::shared(window, cx);
        let acp_subscription = cx.observe(&acp, |_, _, cx| cx.notify());
        let agents_subscription = cx.observe(&agents, Self::on_agents_changed);
        let subscriptions = vec![
            cx.subscribe_in(&search, window, Self::on_search),
            cx.observe(&appearance, |_, _, cx| cx.notify()),
            cx.observe(&connections, |_, _, cx| cx.notify()),
            cx.observe(&terminal, |_, _, cx| cx.notify()),
            cx.observe(&profiles, |_, _, cx| cx.notify()),
            cx.observe_global_in::<crate::entities::agent::catalog::Service>(
                window,
                Self::on_backend_changed,
            ),
            cx.observe_global_in::<crate::shared::lib::appearance::Appearance>(
                window,
                Self::on_appearance_changed,
            ),
        ];
        Self {
            focus: cx.focus_handle(),
            page_focus: std::array::from_fn(|_| cx.focus_handle()),
            sidebar_compact: false,
            search,
            appearance,
            connections,
            terminal,
            profiles,
            agents,
            _agents_subscription: agents_subscription,
            acp,
            _acp_subscription: acp_subscription,
            connection_message: String::new(),
            category: Category::Appearance,
            appearance_group: None,
            agent_section: None,
            expanded: Scope::ALL
                .into_iter()
                .flat_map(|scope| scope.categories().iter().copied())
                .collect(),
            _subscriptions: subscriptions,
        }
    }

    fn on_search(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            let query = self.search.read(cx).value().to_string();
            if !query.trim().is_empty() {
                let category = Category::for_query(&query);
                if !matches!(self.category, Category::Agents | Category::Keybindings)
                    || matches!(
                        category,
                        Category::Sessions
                            | Category::Connections
                            | Category::SessionManagement
                            | Category::Keybindings
                            | Category::Terminal
                    )
                {
                    self.category = category;
                }
                self.expanded.insert(self.category);
                if self.category == Category::SessionManagement {
                    self.acp
                        .update(cx, |page, cx| page.ensure_loaded(window, cx));
                }
            }
            self.appearance.update(cx, |page, cx| {
                page.query = query.clone();
                cx.notify();
            });
            self.agents.update(cx, |page, cx| {
                page.query = query;
                cx.notify();
            });
            cx.notify();
        }
    }

    fn on_agents_changed(&mut self, _: Entity<agents::Agents>, cx: &mut Context<Self>) {
        cx.notify();
    }

    pub(crate) fn on_backend_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agents = agents::shared(window, cx);
        if agents.entity_id() == self.agents.entity_id() {
            return;
        }
        self.acp.update(cx, |page, cx| page.prepare_to_leave(cx));
        self.agents = agents;
        self._agents_subscription = cx.observe(&self.agents, Self::on_agents_changed);
        self.acp = acp::shared(window, cx);
        self._acp_subscription = cx.observe(&self.acp, |_, _, cx| cx.notify());
        if self.category == Category::SessionManagement {
            self.acp.update(cx, |page, cx| page.open(window, cx));
        }
        self.agent_section = None;
        cx.notify();
    }

    fn choose_backend(&mut self, id: &str, cx: &mut Context<Self>) {
        match self
            .profiles
            .update(cx, |profiles, cx| profiles.select(id, cx))
        {
            Ok(target) => {
                self.connection_message.clear();
                crate::features::navigation::manage(target, cx);
            }
            Err(error) => self.connection_message = error,
        }
        cx.notify();
    }

    fn on_appearance_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| {
            input.set_placeholder(tr!("settings-search"), window, cx)
        });
        self.agents.update(cx, |page, cx| page.localize(window, cx));
        cx.notify();
    }

    fn toggle_category(&mut self, category: Category, cx: &mut Context<Self>) {
        if !self.expanded.remove(&category) {
            self.expanded.insert(category);
        }
        cx.notify();
    }

    fn select_category(&mut self, category: Category, window: &mut Window, cx: &mut Context<Self>) {
        match category {
            Category::Appearance => self.select_appearance(None, window, cx),
            Category::Sessions => {
                self.category = Category::Sessions;
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.appearance.update(cx, |page, cx| {
                    page.query.clear();
                    cx.notify();
                });
                cx.notify();
            }
            Category::Agents => self.select_agents(None, window, cx),
            Category::SessionManagement => {
                self.category = Category::SessionManagement;
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.acp.update(cx, |page, cx| page.open(window, cx));
                cx.notify();
            }
            Category::Terminal => {
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.category = Category::Terminal;
                cx.notify();
            }
            Category::Keybindings => {
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.category = Category::Keybindings;
                cx.notify();
            }
            Category::Connections => {
                self.category = Category::Connections;
                self.search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.connections
                    .update(cx, |page, cx| page.focus_page(window, cx));
                cx.notify();
            }
        }
        self.page_focus[category.index()].focus(window, cx);
    }

    pub(crate) fn flush_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance
            .update(cx, |page, cx| page.flush_pending(window, cx));
        self.acp.update(cx, |page, cx| page.flush_pending(cx));
    }

    pub(crate) fn prepare_to_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance
            .update(cx, |page, cx| page.flush_pending(window, cx));
        self.acp.update(cx, |page, cx| page.prepare_to_leave(cx));
    }

    pub(crate) fn show_agent(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.select_agents(None, window, cx);
        self.agent_section = self.agents.update(cx, |page, cx| page.reveal(id, cx));
        cx.notify();
    }

    fn settings_path(&self, cx: &App) -> Option<std::path::PathBuf> {
        match self.category {
            Category::Appearance | Category::Sessions => Some(
                crate::shared::lib::appearance::Editor::current(cx)
                    .appearance_path()
                    .to_owned(),
            ),
            Category::Agents | Category::SessionManagement => {
                self.agents.read(cx).settings_path().map(ToOwned::to_owned)
            }
            Category::Keybindings => None,
            Category::Connections | Category::Terminal => {
                Some(crate::shared::config::store(cx).path().to_owned())
            }
        }
    }

    fn can_edit_file(&self, cx: &App) -> bool {
        self.settings_path(cx).is_some_and(|path| path.is_file())
    }

    fn on_edit_file(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending(window, cx);
        if let Some(path) = self.settings_path(cx) {
            if self.category.scope() == Scope::Backend
                && let Some(write) = self.acp.read(cx).pending_write()
            {
                cx.spawn(async move |_, cx| {
                    write.await;
                    cx.update(|cx| cx.open_with_system(&path));
                })
                .detach();
                return;
            }
            cx.open_with_system(&path);
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.settings_path(cx);
        let file_hint = path.as_ref().map_or_else(
            || tr!("settings-backend-file-unavailable").to_owned(),
            |path| {
                if path.is_file() {
                    path.display().to_string()
                } else {
                    tr!("settings-file-not-created").to_owned()
                }
            },
        );
        let backend_context = (self.category.scope() == Scope::Backend).then(|| {
            let backend = cx
                .try_global::<crate::entities::agent::catalog::Service>()
                .map_or_else(
                    || tr!("settings-backend-disconnected").to_owned(),
                    |service| service.address().to_string(),
                );
            tr!("settings-backend-context", address = backend)
        });
        let edit = gds::ui::IconButton::new("settings-edit-file")
            .icon(IconName::File)
            .label(format!("{} · {file_hint}", tr!("settings-edit-file")))
            .disabled(!self.can_edit_file(cx))
            .on_click(cx.listener(Self::on_edit_file));
        let reset = (self.category == Category::Appearance).then(|| {
            gds::ui::IconButton::new("settings-reset")
                .icon(IconName::RotateCcw)
                .label(format!(
                    "{} · {}",
                    tr!("appearance-reset-all"),
                    tr!(
                        "appearance-current-theme",
                        theme = cx
                            .global::<crate::shared::lib::appearance::Appearance>()
                            .name
                            .clone()
                    )
                ))
                .on_click(cx.listener(|this, event, window, cx| {
                    this.appearance
                        .update(cx, |page, cx| page.on_reset_all(event, window, cx));
                }))
        });
        el! {
            .div { # "settings-header"; test_support; px_6; pt_6; flex_shrink_0; flex; flex_col; gap_2 } [
                @if (let Some(context) = backend_context)
                    .div { flex; items_center; gap_3 } [
                        .div { # "settings-backend-context"; test_support; role: gpui_kit::Role::Label; aria_label: $(context.clone());
                            flex_1; min_w_0; truncate; text_xs; text_color: $(cx.theme().muted_foreground);
                        } [context],
                        $(self.backend_selector(cx)),
                    ],
                @end,
                .div { flex; items_center; gap_2 } [
                    .div { flex_1; min_w_0; text_xl } [$(self.category.title())],
                    ..reset,
                    edit,
                ],
            ]
        }
    }

    fn backend_selector(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        use crate::entities::backend::presentation;
        let profiles = self.profiles.read(cx).entries().to_vec();
        let active = crate::features::navigation::current(cx);
        let selected = active
            .as_ref()
            .and_then(|options| self.profiles.read(cx).matching(options))
            .cloned();
        let label = selected
            .as_ref()
            .map(|profile| profile.name.clone())
            .or_else(|| {
                active
                    .as_ref()
                    .map(|options| presentation::endpoint(&options.target))
            })
            .unwrap_or_else(|| tr!("connections-choose").into());
        let hint = active.as_ref().map_or_else(
            || label.clone(),
            |options| format!("{label} · {}", presentation::tooltip(&options.target)),
        );
        let owner = cx.entity().downgrade();
        let button = el! {
            ("settings-backend-selector").Button::new {
                small;
                ghost;
                min_w_0;
                max_w: $(px(200.));
                accessibility_label: $(hint.clone());
                tooltip: hint;
            } [
                ..$(active.as_ref().map(|options| presentation::icon(&options.target).size(px(crate::shared::ui::ICON)))),
                .div {
                    min_w_0;
                    truncate;
                } [
                    label,
                ],
                (IconName::ChevronDown).Icon::new {
                    xsmall;
                },
            ]
        };
        el! {
            button {
                dropdown_menu: $(move |menu, _, _| {
                        let mut menu = crate::shared::ui::menu(menu).check_side(gpui_kit::component::Side::Right);
                        for profile in &profiles {
                            let owner = owner.clone();
                            let id = profile.id.clone();
                            menu = menu.item(presentation::menu_item(profile)
                                .checked(selected.as_ref().is_some_and(|selected| selected.id == profile.id))
                                .on_click(move |_, _, cx| {
                                        let _ = owner.update(cx, |this, cx| this.choose_backend(&id, cx));
                            }));
                        }
                        let owner = owner.clone();
                        menu.separator().item(PopupMenuItem::new(tr!("connections-manage"))
                            .on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| this.select_category(Category::Connections, window, cx));
                        }))
                });
            }
        }.into_any_element()
    }

    /// Point the appearance page at one of its sections without hiding the others.
    fn select_appearance(
        &mut self,
        group: Option<appearance::Group>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.category = Category::Appearance;
        self.expanded.insert(Category::Appearance);
        self.appearance_group = group;
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        let target = group.map_or(0, appearance::Group::index);
        self.appearance.update(cx, |page, cx| {
            page.scroll_to = Some(target);
            page.query.clear();
            cx.notify();
        });
        cx.notify();
    }

    /// Point the agents page at one agent section; `None` selects the page top.
    fn select_agents(
        &mut self,
        section: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.category = Category::Agents;
        self.expanded.insert(Category::Agents);
        self.agent_section = section;
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.agents.update(cx, |page, cx| {
            page.show_section(section, cx);
        });
        cx.notify();
    }

    fn navigation_shortcut(&self, category: Category) -> Option<gds::ui::Shortcut> {
        let count = Category::ALL.len();
        let current = self.category.index();
        if category.index() == (current + 1) % count {
            Some(gds::ui::Shortcut::new(
                Box::new(window::NextPage),
                Some("BasSettings"),
            ))
        } else if category.index() == (current + count - 1) % count {
            Some(gds::ui::Shortcut::new(
                Box::new(window::PreviousPage),
                Some("BasSettings"),
            ))
        } else {
            None
        }
    }

    fn section_navigation(
        &self,
        category: Category,
        cx: &mut Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        if self.sidebar_compact {
            return Vec::new();
        }
        match category {
            Category::Appearance => appearance::Group::ALL
                .into_iter()
                .map(|group| {
                    gds::ui::NavItem::new(group.id(), IconName::Palette, group.title())
                        .selected(self.category == category && self.appearance_group == Some(group))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_appearance(Some(group), window, cx)
                        }))
                        .into_any_element()
                })
                .collect(),
            Category::Agents => self
                .agents
                .read(cx)
                .sections()
                .into_iter()
                .enumerate()
                .map(|(index, section)| {
                    gds::ui::NavItem::new(section.id, IconName::Bot, section.title)
                        .selected(self.category == category && self.agent_section == Some(index))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_agents(Some(index), window, cx)
                        }))
                        .into_any_element()
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn category_navigation(
        &self,
        category: Category,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let expanded = self.expanded.contains(&category);
        let mut item = gds::ui::NavItem::new(category.id(), category.icon(), category.title())
            .compact(self.sidebar_compact)
            .selected(self.category == category)
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_category(category, window, cx)),
            );
        if let Some(shortcut) = self.navigation_shortcut(category) {
            item = item.shortcut(shortcut);
        }
        let sections = self.section_navigation(category, cx);
        if !self.sidebar_compact && !sections.is_empty() {
            item = item.trailing(vec![
                gds::ui::IconButton::new(gpui_kit::SharedString::from(format!(
                    "{}-expand",
                    category.id()
                )))
                .icon(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .label(format!(
                    "{} {}",
                    if expanded {
                        tr!("action-collapse")
                    } else {
                        tr!("action-expand")
                    },
                    category.title()
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_category(category, cx);
                }))
                .into_any_element(),
            ]);
        }
        el! {
            .div { flex; flex_col; gap_1 } [
                item,
                @if (expanded && !sections.is_empty())
                    .div { pl_3; flex; flex_col; gap_1 } [..sections],
                @end,
            ]
        }
        .into_any_element()
    }

    fn on_return(&mut self, _: &window::CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        self.prepare_to_close(window, cx);
        cx.emit(Event::Close);
    }

    fn on_return_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.on_return(&window::CloseWindow, window, cx);
    }

    fn on_toggle_sidebar(
        &mut self,
        _: &window::ToggleSidebar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_compact = !self.sidebar_compact;
        cx.notify();
    }

    fn cycle(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let count = Category::ALL.len();
        let index = (self.category.index() + if reverse { count - 1 } else { 1 }) % count;
        self.select_category(Category::ALL[index], window, cx);
        self.page_focus[index].focus(window, cx);
    }

    fn on_next_page(&mut self, _: &window::NextPage, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle(false, window, cx);
    }
    fn on_previous_page(
        &mut self,
        _: &window::PreviousPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle(true, window, cx);
    }
}

impl Focusable for Settings {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gds::ui::{Frame, IconButton, Shortcut};
        let feedback = if self.connection_message.is_empty() {
            match self.category {
                Category::Appearance | Category::Sessions => {
                    self.appearance.read(cx).message.clone()
                }
                Category::Agents => self.agents.read(cx).save_status().unwrap_or_default(),
                Category::SessionManagement => self.acp.read(cx).status(cx),
                Category::Connections => self.connections.read(cx).status(cx),
                Category::Keybindings => String::new(),
                Category::Terminal => self.terminal.read(cx).message.clone(),
            }
        } else {
            self.connection_message.clone()
        };
        let frame = Frame::default().sidebar_width(if self.sidebar_compact {
            gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH
        } else {
            gds::appearance::tokens::SIDEBAR_WIDTH
        });
        let back = IconButton::new("settings-return")
            .icon(IconName::ArrowLeft)
            .label(tr!("settings-return"))
            .shortcut(Shortcut::new(
                Box::new(window::CloseWindow),
                Some("BasSettings"),
            ))
            .on_click(cx.listener(Self::on_return_click))
            .into_any_element();
        let sidebar = IconButton::new("settings-sidebar-toggle")
            .icon(IconName::PanelLeft)
            .label(tr!("sidebar-toggle"))
            .shortcut(Shortcut::new(
                Box::new(window::ToggleSidebar),
                Some("BasSettings"),
            ))
            .on_click(cx.listener(|this, _, window, cx| {
                this.on_toggle_sidebar(&window::ToggleSidebar, window, cx)
            }));
        let toolbar_controls = el! {
            .div { flex; items_center; gap_3; w_full } [
                sidebar,
                .div { flex_1; min_w_0; truncate } [tr!("settings-title")],
                .div { # "settings-search"; test_support; w: 260.0.px } [
                    $(crate::shared::ui::input::placeholder(&self.search, gds::ui::field::input(&self.search, cx).small())),
                ],
            ]
        }.into_any_element();
        let window_controls = gds::ui::window::WindowControls::default().on_close_window(
            cx.listener(|_, _, _, cx| {
                // The owner decides whether closing requires PTY confirmation.
                cx.emit(Event::CloseWindow);
            }),
        );
        let toolbar =
            frame.toolbar_with_window_controls(back, toolbar_controls, window_controls, cx);
        let navigation = el! {
            .div { # "settings-navigation"; test_support; size_full; min_h_0; overflow_y_scroll;
                p: gds::appearance::tokens::SIDEBAR_PADDING.px; flex; flex_col; gap_1;
            } [
                ..$(Scope::ALL.into_iter().map(|scope| el! {
                    .div { # $(if scope == Scope::Frontend { "settings-frontend-group" } else { "settings-backend-group" });
                        test_support; flex; flex_col; gap_1;
                    } [
                        @if (!self.sidebar_compact && scope == Scope::Backend)
                            .div { px_2; pt_3; pb_1; text_xs; text_color: $(cx.theme().muted_foreground) } [tr!("settings-scope-backend")],
                        @end,
                        ..$(scope.categories().iter().map(|category| self.category_navigation(*category, cx))),
                    ]
                })),
            ]
        }.into_any_element();
        let page = match self.category {
            Category::Appearance => self.appearance.clone().into_any_element(),
            Category::Sessions => self
                .appearance
                .update(cx, |page, cx| page.render_sessions(window, cx)),
            Category::Agents => self.agents.clone().into_any_element(),
            Category::SessionManagement => self.acp.clone().into_any_element(),
            Category::Connections => self.connections.clone().into_any_element(),
            Category::Terminal => self.terminal.clone().into_any_element(),
            Category::Keybindings => keybindings::render(self.search.read(cx).value().as_ref(), cx),
        };
        let header = self.header(cx).into_any_element();
        let feedback = crate::widgets::settings::ui::feedback(
            "settings-save-status",
            feedback,
            cx.theme().muted_foreground,
        )
        .compact();
        let content = el! {
            .div { track_focus: $(&self.page_focus[self.category.index()]); size_full; min_w_0; flex; flex_col } [
                header,
                .div { px_6; flex_shrink_0 } [feedback],
                .div { flex_1; min_h_0 } [page],
            ]
        }.into_any_element();
        let workspace = frame.workspace(navigation, content, cx);
        el! {
            .div { # "settings"; test_support; key_context: "BasSettings"; track_focus: $(&self.focus);
                size_full; min_h_0; flex; flex_col; text_color: $(cx.theme().foreground);
                on_action: $(cx.listener(Self::on_return));
                on_action: $(cx.listener(Self::on_toggle_sidebar));
                on_action: $(cx.listener(Self::on_next_page));
                on_action: $(cx.listener(Self::on_previous_page));
            } [toolbar, workspace]
        }
    }
}
