//! Column visibility and order: the editor dialog and where its result applies.
//!
//! Opening the editor freezes the folder it was opened for. Applying it writes
//! either exactly that folder's override or the global layout; the global case
//! updates the database defaults and clears this location's override in one
//! transaction. Other locations retain their own settings.

use crate::entities::file::columns::{Column as LayoutColumn, Layout};
use crate::entities::location::Location;
use gds::appearance::tokens;
use gds::ui::DialogActions;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use gds::ui::button::Button;
use gds::{i18n, ui::notifications};
use gds_uim::{component, el};
use gpui_kit::IntoElement;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, Sizable, WindowExt,
    button::ButtonVariants,
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{Browser, ColumnNotice};
use crate::entities::file::sorting::{Key, Sort};
use crate::entities::location::places::Places;

const DIALOG_WIDTH: f32 = 460.;
const DIALOG_GAP: f32 = 10.;
const ROW_HEIGHT: f32 = 30.;
const ROW_PADDING_X: f32 = 6.;
const ROW_GAP: f32 = 8.;
const ROW_RADIUS: f32 = 6.;
const HANDLE_SIZE: f32 = 18.;
const DROP_LANE_HEIGHT: f32 = 14.;
const BUFFER_GAP: f32 = 4.;
const DRAG_PREVIEW_INSET: f32 = 12.;

static NEXT_EDITOR_ID: AtomicU64 = AtomicU64::new(1);

/// Identity of one editor instance. Column drags carry it so that payloads from
/// another editor can never reorder this draft.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EditorId(u64);

impl EditorId {
    fn next() -> Self {
        Self(NEXT_EDITOR_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// The payload of a column drag. It is deliberately not a file drag: file
/// drops never reorder columns, and column drops never transfer files.
#[derive(Clone, Copy, Debug)]
struct ColumnDrag {
    editor: EditorId,
    column: LayoutColumn,
}

/// Where an editor draft applies when the user confirms it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::pages::workspace) enum Scope {
    /// The folder the editor was opened for, exactly.
    #[default]
    Folder,
    /// The preference every folder without its own override inherits.
    Global,
}

impl Scope {
    const ALL: [Self; 2] = [Self::Folder, Self::Global];

    fn label_id(self) -> &'static str {
        match self {
            Self::Folder => "columns-scope-folder",
            Self::Global => "columns-scope-global",
        }
    }
}

/// One row of the editor: the column, its draft visibility and, for a visible
/// column, its position in the draft order.
#[derive(Clone, Copy)]
struct Row {
    column: LayoutColumn,
    visible: bool,
    position: Option<usize>,
}

pub(in crate::pages::workspace) struct Target {
    path: PathBuf,
    location: Location,
}
impl From<&Path> for Target {
    fn from(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            location: Location::from(path),
        }
    }
}

/// A confirmed editor draft, bound to the folder frozen when it was opened.
pub(in crate::pages::workspace) struct Request {
    pub(in crate::pages::workspace) location: Location,
    pub(in crate::pages::workspace) scope: Scope,
    pub(in crate::pages::workspace) layout: Layout,
}

/// Dialog-scoped draft. Hidden columns are listed after the visible ones and
/// appended to the end when shown again.
pub(in crate::pages::workspace) struct Editor {
    id: EditorId,
    target: Option<Target>,
    draft: Layout,
    scope: Scope,
    /// Whether the frozen folder currently has its own override.
    overrides: bool,
}

impl Editor {
    pub(in crate::pages::workspace) fn new() -> Self {
        Self {
            id: EditorId::next(),
            target: None,
            draft: Layout::default(),
            scope: Scope::Folder,
            overrides: false,
        }
    }

    /// Start a new opening from the folder's effective layout.
    ///
    /// The scope always starts at the current folder; an earlier global choice
    /// must not carry over to a later opening.
    pub(in crate::pages::workspace) fn load(
        &mut self,
        target: impl Into<Target>,
        layout: Layout,
        overrides: bool,
    ) {
        self.target = Some(target.into());
        self.draft = layout;
        self.scope = Scope::Folder;
        self.overrides = overrides;
    }

    pub(in crate::pages::workspace) fn finish(&mut self) {
        self.target = None;
    }

    pub(in crate::pages::workspace) fn is_open(&self) -> bool {
        self.target.is_some()
    }

    fn has_override(&self) -> bool {
        self.overrides
    }

    fn target_name(&self) -> Option<SharedString> {
        let path = &self.target.as_ref()?.path;
        Some(
            path.file_name()
                .map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
                .into(),
        )
    }

    fn target_tooltip(&self) -> Option<SharedString> {
        self.target
            .as_ref()
            .map(|target| target.path.display().to_string().into())
    }

    fn request(&self) -> Option<Request> {
        let target = self.target.as_ref()?;
        Some(Request {
            location: target.location.clone(),
            scope: self.scope,
            layout: Layout::new(self.draft.columns().iter().copied()),
        })
    }

    /// Visible columns in draft order, then hidden columns in canonical order.
    fn rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .draft
            .columns()
            .iter()
            .copied()
            .enumerate()
            .map(|(position, column)| Row {
                column,
                visible: true,
                position: Some(position),
            })
            .collect();
        rows.extend(
            LayoutColumn::ALL
                .into_iter()
                .filter(|column| !self.draft.contains(*column))
                .map(|column| Row {
                    column,
                    visible: false,
                    position: None,
                }),
        );
        rows
    }

    fn set_visible(&mut self, column: LayoutColumn, visible: bool) -> bool {
        self.draft.set_visible(column, visible)
    }

    fn move_column(&mut self, column: LayoutColumn, delta: isize) -> bool {
        let Some(position) = self
            .draft
            .columns()
            .iter()
            .position(|visible| *visible == column)
        else {
            return false;
        };
        let target = position.saturating_add_signed(delta);
        self.draft.move_to(column, target)
    }

    fn drag_payload(&self, column: LayoutColumn) -> ColumnDrag {
        ColumnDrag {
            editor: self.id,
            column,
        }
    }

    /// Move the dragged column directly before `target`. A drag owned by another
    /// editor or a hidden column never changes this draft.
    fn drop_before(&mut self, drag: &ColumnDrag, target: LayoutColumn) -> bool {
        if drag.editor != self.id {
            return false;
        }
        let columns = self.draft.columns();
        let Some(target) = columns.iter().position(|column| *column == target) else {
            return false;
        };
        let Some(source) = columns.iter().position(|column| *column == drag.column) else {
            return false;
        };
        if source == target {
            return false;
        }
        // `Layout::move_to` takes the final index after the source is removed,
        // so a downward drag needs the target's shifted position to land above
        // the row the insertion line marks.
        let index = if source < target { target - 1 } else { target };
        self.move_to_index(drag.column, index)
    }

    /// Move the dragged column to the last position.
    fn drop_at_end(&mut self, drag: &ColumnDrag) -> bool {
        if drag.editor != self.id {
            return false;
        }
        let Some(index) = self.draft.columns().len().checked_sub(1) else {
            return false;
        };
        self.move_to_index(drag.column, index)
    }

    fn move_to_index(&mut self, column: LayoutColumn, index: usize) -> bool {
        if !self.draft.contains(column) {
            return false;
        }
        self.draft.move_to(column, index)
    }

    fn on_visible_changed(
        column: LayoutColumn,
    ) -> impl Fn(&mut Self, &bool, &mut Window, &mut Context<Self>) {
        move |editor, visible, _, cx| {
            if editor.set_visible(column, *visible) {
                cx.notify();
            }
        }
    }

    fn on_move(
        column: LayoutColumn,
        delta: isize,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) {
        move |editor, _, _, cx| {
            if editor.move_column(column, delta) {
                cx.notify();
            }
        }
    }

    fn on_scope(editor: Entity<Self>, scope: Scope) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, _, cx| {
            editor.update(cx, |editor, cx| {
                if editor.scope != scope {
                    editor.scope = scope;
                    cx.notify();
                }
            });
        }
    }

    fn scope_menu(
        editor: Entity<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu {
        move |menu, _, cx| {
            let current = editor.read(cx).scope;
            Scope::ALL.into_iter().fold(menu, |menu, scope| {
                menu.item(
                    el! { $(i18n::text(scope.label_id(), cx)).PopupMenuItem::new {
                        checked: $(scope == current);
                        on_click: $(Self::on_scope(editor.clone(), scope));
                    } },
                )
            })
        }
    }

    fn text_tooltip(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        move |window, cx| Tooltip::new(text.clone()).build(window, cx)
    }

    fn drag_preview(
        drag: &ColumnDrag,
        cursor_offset: Point<Pixels>,
        _: &mut Window,
        cx: &mut App,
    ) -> Entity<ColumnDragPreview> {
        let label = i18n::text(drag.column.label_id(), cx);
        cx.new(|_| ColumnDragPreview {
            label,
            cursor_offset,
        })
    }

    /// Accept only this editor's column drags, so one editor can never rewrite
    /// another's draft and file drags never highlight a column.
    fn drop_target(
        element: Stateful<Div>,
        editor: Entity<Self>,
        editor_id: EditorId,
        target: Option<LayoutColumn>,
    ) -> Stateful<Div> {
        element
            .can_drop(move |value, _, _| {
                value.downcast_ref::<ColumnDrag>().is_some_and(|drag| {
                    // The source row is never a target: dropping a column on
                    // itself must not even show an insertion line.
                    drag.editor == editor_id && target != Some(drag.column)
                })
            })
            .on_drop(move |drag: &ColumnDrag, _, cx| {
                editor.update(cx, |editor, cx| {
                    let moved = match target {
                        Some(target) => editor.drop_before(drag, target),
                        None => editor.drop_at_end(drag),
                    };
                    if moved {
                        cx.notify();
                    }
                });
            })
            .drag_over::<ColumnDrag>(|style, _, _, cx| {
                style
                    .bg(cx.theme().drop_target)
                    .border_color(cx.theme().drag_border)
            })
    }

    fn render_visible_row(
        &self,
        row: &Row,
        visible_count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let column = row.column;
        let label = i18n::text(column.label_id(), cx);
        let row_id: ElementId = format!("columns-row:{}", column.id()).into();
        let handle_id: ElementId = format!("columns-handle:{}", column.id()).into();
        let checkbox_id: ElementId = format!("columns-visible:{}", column.id()).into();
        let up_id: ElementId = format!("columns-up:{}", column.id()).into();
        let down_id: ElementId = format!("columns-down:{}", column.id()).into();
        let move_up = i18n::text("columns-move-up", cx);
        let move_down = i18n::text("columns-move-down", cx);
        let on_visible_changed = cx.listener(Self::on_visible_changed(column));
        let on_up = cx.listener(Self::on_move(column, -1));
        let on_down = cx.listener(Self::on_move(column, 1));
        let can_up = row.position.is_some_and(|position| position > 0);
        let can_down = row
            .position
            .is_some_and(|position| position + 1 < visible_count);
        let drag_tooltip = Self::text_tooltip(i18n::text("columns-drag", cx));
        let payload = self.drag_payload(column);
        let element = el! {
            .div {
                # row_id;
                flex; items_center; gap: ROW_GAP.px; w_full; min_w_0; min_h: ROW_HEIGHT.px;
                px: ROW_PADDING_X.px; rounded: ROW_RADIUS.px;
                border_t_2; border_color: $(cx.theme().border.opacity(0.));
                hover: { bg: $(cx.theme().list_hover) };
            } [
                .div {
                    # handle_id;
                    w: HANDLE_SIZE.px; h: HANDLE_SIZE.px; flex_shrink_0;
                    flex; items_center; justify_center; rounded: ROW_RADIUS.px;
                    cursor_grab;
                    tooltip_show_delay: tokens::TOOLTIP_DELAY;
                    tooltip: drag_tooltip;
                    on_drag: payload, Self::drag_preview;
                } [
                    IconName::GripVertical.Icon::new {
                        size: 14.0.px; text_color: $(cx.theme().muted_foreground)
                    },
                ],
                checkbox_id.Checkbox::new {
                    with_size: gpui_kit::component::Size::Small;
                    checked: true;
                    disabled: $(column.is_required());
                    accessibility_label: $(label.clone());
                    on_change: on_visible_changed;
                },
                .div { flex_1; min_w_0; truncate } [$(label.clone())],
                up_id.Button::new {
                    ghost; xsmall;
                    icon: IconName::ArrowUp.Icon::new;
                    tooltip: $(move_up.clone());
                    accessibility_label: $(move_up.clone());
                    disabled: $(!can_up);
                    on_click: on_up;
                },
                down_id.Button::new {
                    ghost; xsmall;
                    icon: IconName::ArrowDown.Icon::new;
                    tooltip: $(move_down.clone());
                    accessibility_label: $(move_down.clone());
                    disabled: $(!can_down);
                    on_click: on_down;
                },
            ]
        };
        Self::drop_target(element, cx.entity(), self.id, Some(column)).into_any_element()
    }

    fn render_hidden_row(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let column = row.column;
        let label = i18n::text(column.label_id(), cx);
        let row_id: ElementId = format!("columns-row:{}", column.id()).into();
        let checkbox_id: ElementId = format!("columns-visible:{}", column.id()).into();
        let on_visible_changed = cx.listener(Self::on_visible_changed(column));
        el! {
            .div {
                # row_id;
                flex; items_center; gap: ROW_GAP.px; w_full; min_w_0; min_h: ROW_HEIGHT.px;
                px: ROW_PADDING_X.px;
                text_color: $(cx.theme().muted_foreground);
            } [
                // Keeps the checkboxes of both groups on one vertical line.
                .div { w: HANDLE_SIZE.px; flex_shrink_0 },
                checkbox_id.Checkbox::new {
                    with_size: gpui_kit::component::Size::Small;
                    checked: false;
                    accessibility_label: $(label.clone());
                    on_change: on_visible_changed;
                },
                .div { flex_1; min_w_0; truncate } [label],
            ]
        }
        .into_any_element()
    }
}

#[component]
#[derive(Default, IntoElement)]
struct ColumnGroupHeading {
    #[attr(into)]
    label: SharedString,
}

impl RenderOnce for ColumnGroupHeading {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! {
            .div { flex; items_center; w_full; min_w_0; px: ROW_PADDING_X.px;
                text_xs; text_color: $(cx.theme().muted_foreground);
            } [$(self.label)]
        }
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(folder) = self.target_name() else {
            return el! { .div }.into_any_element();
        };
        let target_tooltip =
            Self::text_tooltip(self.target_tooltip().unwrap_or_else(|| folder.clone()));
        let scope_label = i18n::text(self.scope.label_id(), cx);
        let scope_menu = Self::scope_menu(cx.entity());
        let rows = self.rows();
        let visible_count = self.draft.columns().len();
        let visible_rows: Vec<_> = rows
            .iter()
            .filter(|row| row.visible)
            .map(|row| self.render_visible_row(row, visible_count, cx))
            .collect();
        let hidden_rows: Vec<_> = rows
            .iter()
            .filter(|row| !row.visible)
            .map(|row| self.render_hidden_row(row, cx))
            .collect();
        let hidden_count = hidden_rows.len();
        let drop_tooltip = Self::text_tooltip(i18n::text("columns-drop-end", cx));
        let drop_lane = el! {
            .div {
                # "columns-drop-end";
                h: DROP_LANE_HEIGHT.px; w_full; flex_shrink_0;
                border_t_2; border_color: $(cx.theme().border.opacity(0.));
                tooltip_show_delay: tokens::TOOLTIP_DELAY;
                tooltip: drop_tooltip;
            }
        };
        let drop_lane = Self::drop_target(drop_lane, cx.entity(), self.id, None);
        el! {
            .div { flex; flex_col; gap: DIALOG_GAP.px; w_full; min_w_0 } [
                .div { flex; items_center; gap: ROW_GAP.px; w_full; min_w_0 } [
                    .div {
                        # "columns-target";
                        flex; items_center; gap: 6.0.px; flex_1; min_w_0;
                        px: ROW_PADDING_X.px; py: 2.0.px; rounded: ROW_RADIUS.px;
                        bg: $(cx.theme().secondary);
                        tooltip_show_delay: tokens::TOOLTIP_DELAY;
                        tooltip: target_tooltip;
                    } [
                        IconName::Folder.Icon::new {
                            size: 14.0.px; text_color: $(cx.theme().muted_foreground)
                        },
                        .div {
                            min_w_0; truncate; text_xs;
                            text_color: $(cx.theme().muted_foreground);
                        } [folder],
                    ],
                    .div {
                        flex_shrink_0; text_xs;
                        text_color: $(cx.theme().muted_foreground);
                    } [$i18n::text("columns-scope", cx)],
                    "columns-scope".Button::new {
                        ghost; xsmall; label: scope_label;
                        icon: IconName::ChevronDown.Icon::new;
                        tooltip: $i18n::text("columns-scope", cx);
                        dropdown_menu: scope_menu;
                    },
                ],
                .ColumnGroupHeading::default { label: $(i18n::count("columns-visible-group", visible_count, cx)); },
                .div {
                    # "columns-list";
                    flex; flex_col; w_full; min_w_0;
                } [..visible_rows],
                drop_lane,
                @if (hidden_count > 0)
                    .ColumnGroupHeading::default { label: $(i18n::count("columns-hidden-group", hidden_count, cx)); },
                    .div {
                        # "columns-hidden-list";
                        flex; flex_col; w_full; min_w_0;
                    } [..hidden_rows],
                @end,
            ]
        }
        .into_any_element()
    }
}

/// The element that follows the pointer while reordering columns.
struct ColumnDragPreview {
    label: SharedString,
    cursor_offset: Point<Pixels>,
}

impl Render for ColumnDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        el! {
            .div {
                pl: $(self.cursor_offset.x + px(DRAG_PREVIEW_INSET));
                pt: $(self.cursor_offset.y + px(DRAG_PREVIEW_INSET));
            } [
                .div {
                    px: 8.0.px; py: 3.0.px; rounded: ROW_RADIUS.px;
                    flex; items_center; gap: 6.0.px;
                    bg: $(cx.theme().popover); border_1; border_color: $(cx.theme().border);
                    text_color: $(cx.theme().popover_foreground); text_sm;
                } [
                    IconName::GripVertical.Icon::new {
                        size: 14.0.px; text_color: $(cx.theme().muted_foreground)
                    },
                    .div { truncate } [$(self.label.clone())],
                ],
            ]
        }
    }
}

impl Browser {
    /// The layout that applies at a location, otherwise the database defaults.
    pub(in crate::pages::workspace) fn columns_layout(&self, location: &Location) -> &Layout {
        self.locations.places.effective_columns(location)
    }

    /// A loaded file view with no pending navigation. The column editor freezes
    /// the visible folder and Ctrl+D bookmarks it, so neither may run while the
    /// folder is still being replaced.
    pub(in crate::pages::workspace) fn folder_settled(&self) -> bool {
        self.pages.is_empty()
            && self.collection().is_none()
            && self.active().is_some_and(|slot| {
                slot.files_available()
                    && slot.navigation.directory().is_some()
                    && !slot.is_loading()
            })
    }

    pub(in crate::pages::workspace) fn browse_settled(&self) -> bool {
        self.folder_settled()
            || (self.pages.is_empty()
                && self.place_collection().is_some()
                && self.current_tab().is_some_and(|slot| !slot.is_loading()))
    }

    pub(in crate::pages::workspace) fn columns_available(&self) -> bool {
        self.folder_settled() && !self.operations.in_progress
    }

    /// Open the column editor only after navigation settles, so its frozen
    /// target cannot be the folder a pending navigation is about to replace.
    pub(in crate::pages::workspace) fn open_columns_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.columns_available()
            || window.has_active_dialog(cx)
            || self.files.columns.read(cx).is_open()
        {
            return;
        }
        let Some(path) = self.current_path() else {
            return;
        };
        let location = self.current_location();
        let overrides = self.locations.places.columns_for(&location).is_some();
        let layout = self.columns_layout(&location).clone();
        self.files.columns.update(cx, |editor, cx| {
            editor.load(Target { path, location }, layout, overrides);
            cx.notify();
        });
        let editor = self.files.columns.clone();
        let browser = cx.entity().downgrade();
        // Let a closing popup leave the focus tree before the dialog records its
        // own return target.
        window.on_next_frame(move |window, cx| {
            let Some(browser) = browser.upgrade() else {
                return;
            };
            let confirm_browser = browser.downgrade();
            let close_browser = browser.downgrade();
            let restore_browser = browser.downgrade();
            let confirm_editor = editor.clone();
            let close_editor = editor.clone();
            let restore_editor = editor.clone();
            let cancel_editor = editor.clone();
            let body = editor.clone();
            window.open_alert_dialog(cx, move |dialog, _, cx| {
                let on_ok = Self::columns_confirm(confirm_browser.clone(), confirm_editor.clone());
                let on_close = Self::columns_close(close_browser.clone(), close_editor.clone());
                let on_cancel = Self::columns_cancel(cancel_editor.clone());
                let on_restore =
                    Self::columns_restore(restore_browser.clone(), restore_editor.clone());
                let can_restore = body.read(cx).has_override();
                let footer = el! { .DialogActions::default {
                    primary_label: $(i18n::text("columns-apply", cx));
                    primary_icon: IconName::Check;
                    cancel_label: $(i18n::text("operation-cancel", cx));
                } };
                let restore = can_restore.then(|| {
                    el! {
                        .div {
                            flex; items_center; w_full; min_w_0;
                            pt: BUFFER_GAP.px; border_t_1; border_color: $(cx.theme().border);
                        } [
                            "restore-columns".Button::new {
                                ghost; xsmall;
                                icon: IconName::RotateCcw.Icon::new;
                                label: $i18n::text("columns-restore", cx);
                                on_click: on_restore;
                            },
                        ]
                    }
                });
                el! {
                    dialog {
                        confirm; width: DIALOG_WIDTH.px;
                        bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                        title: $(i18n::text("columns-title", cx));
                        icon: IconName::Columns3Cog.Icon::new;
                        footer: footer;
                        on_ok: on_ok;
                        on_cancel: on_cancel;
                        on_close: on_close;
                    } [
                        .div { flex; flex_col; gap: DIALOG_GAP.px; w_full; min_w_0 } [
                            $(body.clone()),
                            ..restore,
                        ]
                    ]
                }
            });
        });
        cx.notify();
    }

    fn columns_confirm(
        browser: WeakEntity<Self>,
        editor: Entity<Editor>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, _, cx| {
            let Some(browser) = browser.upgrade() else {
                return true;
            };
            let applied = browser.update(cx, |browser, cx| browser.commit_columns(&editor, cx));
            if applied {
                editor.update(cx, |editor, cx| {
                    editor.finish();
                    cx.notify();
                });
            }
            applied
        }
    }

    /// Escape and the Cancel button first end an active column drag. The draft
    /// stays open and unchanged; only a second cancel discards it.
    fn columns_cancel(
        editor: Entity<Editor>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool {
        move |_, window, cx| {
            if cx.stop_active_drag(window) {
                editor.update(cx, |_, cx| cx.notify());
                return false;
            }
            true
        }
    }

    fn columns_close(
        browser: WeakEntity<Self>,
        editor: Entity<Editor>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.finish();
                cx.notify();
            });
            let Some(browser) = browser.upgrade() else {
                return;
            };
            let origin_tab = browser.read(cx).tabs.active_id;
            browser.update(cx, |_, cx| cx.notify());
            Self::restore_file_focus(browser.downgrade(), origin_tab, window);
        }
    }

    fn columns_restore(
        browser: WeakEntity<Self>,
        editor: Entity<Editor>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            let Some(browser) = browser.upgrade() else {
                return;
            };
            if browser.update(cx, |browser, cx| browser.restore_columns(&editor, cx)) {
                editor.update(cx, |editor, cx| {
                    editor.finish();
                    cx.notify();
                });
                window.close_dialog(cx);
                Self::restore_file_focus(
                    browser.downgrade(),
                    browser.read(cx).tabs.active_id,
                    window,
                );
            }
        }
    }

    /// Persist the editor draft for the folder it was opened for and re-project
    /// every tab. Returns whether the dialog can close.
    fn commit_columns(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) -> bool {
        let Some(request) = editor.read(cx).request() else {
            return true;
        };
        let close = match request.scope {
            Scope::Folder => self.store_folder_columns(request.location, request.layout, cx),
            Scope::Global => self.store_global_columns(request.location, request.layout, cx),
        };
        self.reproject_columns(cx);
        close
    }

    /// Record the layout for exactly one folder. The in-memory override is kept
    /// when saving fails, so the change stays active for this session and the
    /// failure is reported instead of being hidden. Returns `false` only when
    /// nothing could be written at all.
    fn store_folder_columns(
        &mut self,
        location: Location,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.locations.load_error.is_some() {
            report_column_state_unavailable(cx);
            return false;
        }
        self.locations.places.set_columns(&location, layout);
        self.save_places(cx);
        true
    }

    /// Save the defaults and clear this location's column override in the same
    /// database transaction. Other view settings and locations are preserved.
    fn store_global_columns(
        &mut self,
        location: Location,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.locations.load_error.is_some() {
            report_column_state_unavailable(cx);
            return false;
        }
        self.locations.places.set_default_columns(layout);
        self.locations.places.clear_columns(&location);
        self.save_places(cx);
        true
    }

    /// Remove the frozen folder's override so it inherits the global layout.
    /// Returns whether the editor can be closed.
    fn restore_columns(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) -> bool {
        let Some(location) = editor
            .read(cx)
            .target
            .as_ref()
            .map(|target| target.location.clone())
        else {
            return true;
        };
        if self.locations.load_error.is_some() {
            report_column_state_unavailable(cx);
            return false;
        }
        self.locations.places.clear_columns(&location);
        self.save_places(cx);
        self.reproject_columns(cx);
        true
    }

    /// Re-apply the effective layout to every tab after a layout changed.
    ///
    /// A sort key whose column is now hidden falls back to the default ordering
    /// so the table never keeps an ordering with no visible header arrow.
    pub(in crate::pages::workspace) fn reproject_columns(&mut self, cx: &mut Context<Self>) {
        self.release_active_selection_mirrors(cx);
        for slot in &mut self.tabs.items {
            let sort = sort_for(&slot.location(), &self.locations.places);
            slot.sort = sort;
            slot.refresh_visible();
        }
        self.schedule_enrichment(self.tabs.active_id, cx);
        cx.notify();
    }
}

/// The ordering to show at `path`: unchanged while its column is visible there,
/// otherwise the default ordering so no hidden column stays active.
pub(in crate::pages::workspace) fn sort_for(location: &Location, places: &Places) -> Sort {
    let sort = places.effective_sort(location);
    if places.effective_columns(location).contains(sort.key) {
        sort
    } else {
        Sort::default()
    }
}

fn report_column_state_unavailable(cx: &mut App) {
    notifications::Report::new(
        notifications::Level::Error,
        i18n::text("columns-state-unavailable", cx),
    )
    .replacing::<ColumnNotice>()
    .show(cx);
}

/// The sort key a column header toggles.
#[allow(dead_code)]
pub(in crate::pages::workspace) const fn sort_key(column: LayoutColumn) -> Key {
    column
}
