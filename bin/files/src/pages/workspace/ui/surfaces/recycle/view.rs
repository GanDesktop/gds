//! Trash content view: header actions, table, explicit states and the frozen
//! confirmation for permanent deletion.

use gds::ui::ListSurface;

use crate::shared::ui::actions::Command;
use gds::ui::page::*;
use gds::{
    appearance::tokens::DataListStyle,
    i18n,
    ui::{ContentPage, IconButton, menu},
};
use gds_uim::{component, el};
use gpui_kit::IntoElement;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::Button,
    empty::{Empty as EmptyState, EmptyContent, EmptyDescription, EmptyMedia, EmptyTitle},
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    table::DataTable,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{
    backend::{Action, Entry},
    table::{self, TrashTable},
};
use crate::pages::workspace::ui::Browser;
use crate::pages::workspace::ui::file_actions::{
    Confirmation as SharedConfirmation, NAME_PREVIEW_LIMIT,
};

const CONTENT_PADDING: f32 = 24.;
const TABLE_INSET: f32 = ContentPage::TABLE_INSET;
const STATE_GAP: f32 = 8.;
const STATE_ICON_SIZE: f32 = 28.;
const STATE_MAX_WIDTH: f32 = 420.;

impl Browser {
    pub(in crate::pages::workspace) fn render_trash(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let actions = self.trash_header_actions(cx);
        let body = self.render_trash_body(window, cx);
        el! {
            .ContentPage::default { # "trash-content" } [
                .PageHeader::default { @if (self.window.layout.compact) hidden; @end } [
                    .PageTitle::default [$i18n::text("trash", cx)],
                    .PageActions::default [..actions],
                ],
                .PageContent::default [body],
            ]
        }
        .into_any_element()
    }

    fn trash_header_actions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let busy = self.collections.trash.is_busy() || self.operations.in_progress;
        let has_selection = self.collections.trash.selected_count() > 0;
        let empty = self.collections.trash.rows().is_empty();
        let on_restore = cx.listener(Browser::on_trash_restore);
        let on_purge = cx.listener(Browser::on_trash_purge);
        let on_empty = cx.listener(Browser::on_trash_empty);
        let on_refresh = cx.listener(Browser::on_trash_refresh);
        let restore = el! {
            "trash-restore".IconButton::new {
                icon: IconName::Undo2;
                label: $i18n::text("recycle-restore", cx);
                command: Command::RestoreSelection;
                disabled: $(busy || !has_selection);
                on_click: on_restore;
            }
        };
        let purge = el! {
            "trash-purge".IconButton::new {
                icon: IconName::Trash;
                label: $i18n::text("delete-permanently", cx);
                command: Command::PurgeSelection;
                disabled: $(busy || !has_selection);
                on_click: on_purge;
            }
        };
        let refresh = el! {
            "trash-refresh".IconButton::new {
                icon: IconName::RefreshCw;
                label: $i18n::text("refresh", cx);
                command: Command::Refresh;
                disabled: busy;
                on_click: on_refresh;
            }
        };
        let empty_action = el! {
            "trash-empty".IconButton::new {
                icon: $(Intent::EmptyTrash.icon());
                label: $i18n::text("recycle-empty-action", cx);
                command: Command::EmptyTrash;
                disabled: $(busy || empty);
                on_click: on_empty;
            }
        };
        vec![
            restore.into_any_element(),
            purge.into_any_element(),
            refresh.into_any_element(),
            empty_action.into_any_element(),
        ]
    }

    fn render_trash_body(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.collections.trash;
        if state.rows().is_empty() {
            if let Some(error) = state.error() {
                let retry = state.supported().then(|| {
                    el! {
                        "trash-retry".Button::new {
                            small; label: $(i18n::text("retry", cx));
                            on_click: $(cx.listener(Browser::on_trash_refresh));
                        }
                    }
                    .into_any_element()
                });
                return el! { .TrashMessage::default {
                title: $(i18n::text("recycle-load-failed", cx)); detail: $(Some(error.to_owned().into()));
                icon: IconName::CircleAlert; action: retry;
            } }.into_any_element();
            }
            if state.is_loading() {
                return el! { .TrashMessage::default {
                    title: $(i18n::text("recycle-loading", cx)); icon: IconName::LoaderCircle;
                } }
                .into_any_element();
            }
            return el! { .TrashMessage::default { title: $(i18n::text("recycle-empty", cx)); } }
                .into_any_element();
        }
        self.render_trash_table(window, cx)
    }

    fn render_trash_table(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content_width = f32::from(window.bounds().size.width) - self.width() - TABLE_INSET * 2.;
        let data = table::Data {
            rows: self.collections.trash.entries.clone(),
            selected: self.collections.trash.selection(),
            content_width,
        };
        TrashTable::sync_state(&self.collections.trash.table, data, cx);
        let table = el! {
            ($&self.collections.trash.table).DataTable::new {
                bordered: false;
                with_size: $(gpui_kit::component::Size::Size(px(DataListStyle::ROW_HEIGHT)));
                header_row_height: DataListStyle::HEADER_HEIGHT.px;
                into_any_element;
            }
        };
        let menu = area_menu(
            cx.entity().downgrade(),
            !self.collections.trash.is_busy()
                && !self.operations.in_progress
                && !self.collections.trash.rows().is_empty(),
        );
        let area = el! { .PageTableArea::default {
            # "trash-table-area";
            context_menu: menu;
        } [
            table,
        ] }
        .into_any_element();
        let scroll = self
            .collections
            .trash
            .table
            .read(cx)
            .vertical_scroll_handle
            .0
            .borrow()
            .base_handle
            .clone();
        el! { ("trash-selection-surface", area, $(self.collections.trash.rows().len()), scroll).ListSurface::new {
        inset_x: $(px(TABLE_INSET));
        on_blank_press: $(cx.listener(Browser::on_list_background_press));
    } }
    .into_any_element()
    }
}

#[component]
#[derive(IntoElement)]
struct TrashMessage {
    #[attr(into)]
    title: SharedString,
    #[attr]
    detail: Option<SharedString>,
    #[attr]
    icon: IconName,
    #[attr]
    action: Option<AnyElement>,
}

impl Default for TrashMessage {
    fn default() -> Self {
        Self {
            title: SharedString::default(),
            detail: None,
            icon: IconName::Trash,
            action: None,
        }
    }
}

impl RenderOnce for TrashMessage {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! {
            .div { # "trash-state"; flex_1; min_h_0; flex; flex_col; } [
                .EmptyState::default { p_0; gap: STATE_GAP.px; px: CONTENT_PADDING.px; } [
                    .EmptyMedia::default [$(self.icon).Icon::new {
                        size: STATE_ICON_SIZE.px; text_color: $(cx.theme().muted_foreground);
                    }],
                    .EmptyTitle::default { text_sm; font_weight: FontWeight::NORMAL; } [$(self.title)],
                    @if (let Some(detail) = self.detail)
                        .EmptyDescription::default {
                            text_xs; text_center; max_w: STATE_MAX_WIDTH.px;
                            text_color: $(cx.theme().muted_foreground);
                        } [detail],
                    @end,
                    @if (let Some(action) = self.action) .EmptyContent::default [action], @end,
                ],
            ]
        }
    }
}

fn menu_row(
    label: SharedString,
    command: Option<Command>,
) -> impl Fn(&mut Window, &mut App) -> AnyElement {
    menu::row(label, command.map(Into::into))
}

fn refresh_action(view: WeakEntity<Browser>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
    move |_, _, cx| {
        if let Some(browser) = view.upgrade() {
            browser.update(cx, |browser, cx| browser.refresh_trash(cx));
        }
    }
}

fn empty_action(view: WeakEntity<Browser>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
    move |_, window, cx| {
        if let Some(browser) = view.upgrade() {
            browser.update(cx, |browser, cx| browser.request_empty_trash(window, cx));
        }
    }
}

fn restore_action(
    view: WeakEntity<Browser>,
    entries: Vec<Entry>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
    move |_, _, cx| {
        if let Some(browser) = view.upgrade() {
            let entries = entries.clone();
            browser.update(cx, |browser, cx| {
                browser.start_trash_operation(Action::Restore, entries, cx)
            });
        }
    }
}

fn purge_action(
    view: WeakEntity<Browser>,
    entries: Vec<Entry>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
    move |_, window, cx| {
        if let Some(browser) = view.upgrade() {
            let entries = entries.clone();
            browser.update(cx, |browser, cx| {
                browser.confirm_purge_entries(entries, window, cx)
            });
        }
    }
}

fn area_menu(
    view: WeakEntity<Browser>,
    can_empty: bool,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu {
    move |menu, _, cx| {
        let menu = menu::popup(menu);
        let refresh = {
            let on_click = refresh_action(view.clone());
            let render = menu_row(i18n::text("refresh", cx), Some(Command::Refresh));
            el! {
                render.PopupMenuItem::element {
                    on_click: on_click;
                    icon: IconName::RefreshCw;
                }
            }
        };
        let empty = {
            let on_click = empty_action(view.clone());
            let render = menu_row(
                i18n::text("recycle-empty-action", cx),
                can_empty.then_some(Command::EmptyTrash),
            );
            el! {
                render.PopupMenuItem::element {
                    on_click: on_click;
                    icon: $(Intent::EmptyTrash.icon());
                }
            }
            .disabled(!can_empty)
        };
        el! { menu { item: refresh; separator; item: empty; } }
    }
}

pub(in crate::pages::workspace) fn entry_menu(
    browser: &Browser,
    menu: PopupMenu,
    cx: &Context<Browser>,
) -> PopupMenu {
    let menu = menu::popup(menu);
    let view = cx.entity().downgrade();
    let entries = browser.collections.trash.selected_entries();
    let count = entries.len();
    let busy = browser.collections.trash.is_busy() || browser.operations.in_progress;
    let restore_label = if count == 1 {
        i18n::text("recycle-restore", cx)
    } else {
        i18n::count("recycle-restore-selected", count, cx)
    };
    let purge_label = i18n::count("recycle-purge-selected", count, cx);
    let actionable = !(busy || count == 0);
    let restore = {
        let on_click = restore_action(view.clone(), entries.clone());
        let render = menu_row(
            restore_label,
            actionable.then_some(Command::RestoreSelection),
        );
        el! {
            render.PopupMenuItem::element {
                on_click: on_click;
                icon: IconName::Undo2;
            }
        }
        .disabled(busy || count == 0)
    };
    let purge = {
        let on_click = purge_action(view.clone(), entries);
        let render = menu_row(purge_label, actionable.then_some(Command::PurgeSelection));
        el! {
            render.PopupMenuItem::element {
                on_click: on_click;
                icon: IconName::Trash;
            }
        }
        .disabled(busy || count == 0)
    };
    let refresh = {
        let on_click = refresh_action(view.clone());
        let render = menu_row(i18n::text("refresh", cx), Some(Command::Refresh));
        el! {
            render.PopupMenuItem::element {
                on_click: on_click;
                icon: IconName::RefreshCw;
            }
        }
    };
    let empty = {
        let can_empty = !busy && !browser.collections.trash.rows().is_empty();
        let on_click = empty_action(view);
        let render = menu_row(
            i18n::text("recycle-empty-action", cx),
            can_empty.then_some(Command::EmptyTrash),
        );
        el! {
            render.PopupMenuItem::element {
                on_click: on_click;
                icon: $(Intent::EmptyTrash.icon());
            }
        }
        .disabled(busy || browser.collections.trash.rows().is_empty())
    };
    el! {
        menu {
            item: restore;
            item: purge;
            separator;
            item: refresh;
            separator;
            item: empty;
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::pages::workspace) enum Intent {
    PurgeSelection,
    EmptyTrash,
}

impl Intent {
    fn title_id(self) -> &'static str {
        match self {
            Self::PurgeSelection => "recycle-purge-confirm-title",
            Self::EmptyTrash => "recycle-empty-confirm-title",
        }
    }

    fn message_id(self) -> &'static str {
        match self {
            Self::PurgeSelection => "recycle-purge-confirm-message",
            Self::EmptyTrash => "recycle-empty-confirm-message",
        }
    }

    fn ok_id(self) -> &'static str {
        match self {
            Self::PurgeSelection => "recycle-purge-confirm-ok",
            Self::EmptyTrash => "recycle-empty-confirm-ok",
        }
    }

    /// Purging selected records keeps the trash-can action icon. Emptying the
    /// whole Trash is a distinct "clear everything" action, so the two entry
    /// points never look like the same button.
    fn icon(self) -> IconName {
        match self {
            Self::PurgeSelection => IconName::Trash,
            Self::EmptyTrash => IconName::Broom,
        }
    }

    /// Body text for the frozen request; only the empty-Trash template states
    /// how many records the request holds.
    fn message(self, count: usize, cx: &App) -> SharedString {
        match self {
            Self::PurgeSelection => i18n::text(self.message_id(), cx),
            Self::EmptyTrash => i18n::count(self.message_id(), count, cx),
        }
    }
}

/// Confirmation for permanent deletion.
///
/// The entries are frozen when the dialog opens; confirmation never re-lists
/// Trash, so records deleted there after the dialog appeared are untouched.
pub(in crate::pages::workspace) struct Confirmation;

impl Confirmation {
    pub(in crate::pages::workspace) fn open(
        browser: WeakEntity<Browser>,
        intent: Intent,
        entries: Vec<Entry>,
        window: &mut Window,
    ) {
        let names: Vec<SharedString> = entries
            .iter()
            .take(NAME_PREVIEW_LIMIT)
            .map(|entry| {
                entry
                    .name
                    .to_string_lossy()
                    .escape_debug()
                    .to_string()
                    .into()
            })
            .collect();
        let count = entries.len();
        let confirmation = SharedConfirmation::new(
            browser,
            names,
            count,
            intent.icon(),
            move |count, cx| i18n::count(intent.title_id(), count, cx),
            move |count, cx| intent.message(count, cx),
            move |_, cx| i18n::text(intent.ok_id(), cx),
            move |browser, cx| browser.start_trash_operation(Action::Purge, entries, cx),
            move |browser, _, window, _| {
                window.on_next_frame(move |window, cx| {
                    if let Some(browser) = browser.upgrade() {
                        browser.update(cx, |browser, cx| {
                            browser.window.focus.focus(window, cx);
                        });
                    }
                });
            },
        );
        SharedConfirmation::open(confirmation, window);
    }
}
