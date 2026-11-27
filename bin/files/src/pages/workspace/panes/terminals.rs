//! Window-owned embedded terminal dock independent of folder navigation.

use std::{cell::Cell, path::PathBuf, rc::Rc};

use gds::{
    app::Gds,
    appearance::{motion, tokens},
    i18n,
    service::terminal::{SpawnOptions, Status},
    ui::{
        HeldModifier, IconButtonProps, IconName, IconProps, Shortcut,
        drag::{
            DRAG_THRESHOLD, ExternalFileDrop, FileDropOptions, FileDropTarget, drop_hit_test,
            file_drop_target_attrs,
        },
        feedback::{
            dialog::{
                DialogActionsProps, DialogDecision, DialogFooterProps, DialogHeaderProps,
                DialogTitleProps, ModalDialogProps,
            },
            tooltip::DetailsTooltipProps,
        },
        keytips::{KeytipRegistry, anchor_when},
        navigation::{
            CornerActionProps, CornerButtonProps, HoverActionProps, NavItemLabelProps, NavItemProps,
        },
        sidebar::{SideBarSectionTitleProps, SidebarPresentation},
        terminal::{Event as TerminalEvent, Terminal, TerminalViewProps},
    },
};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, RwSignal, Signal, on_cleanup_local, spawn_local};
use zgui::view::{EventCx, current_host, events, install_stylesheet, request_frame};
use zgui::vocab::{
    ImeEvent, Key, KeyEvent, KeyLocation, Modifiers, NamedKey, PhysicalKey, PointerButton, Role,
};
use zgui::{css, view};
use zgui_ui::InputProps;
use zgui_ui_primitives::Binding;

use super::super::{
    projection::{Geometry, Rows},
    state::{
        context::Context,
        dock::{
            Dock, InternalFileDrag, MIN_CONTENT_HEIGHT, MIN_DOCK_HEIGHT, RenameDialogState,
            SessionEntry,
        },
        model::{Pane, Row, RowKey},
    },
};
use crate::{entities::preferences::Preferences, shared::ui::actions::Command};

const COLLAPSED_ACTION_INSET: f32 =
    (tokens::COLLAPSED_SIDEBAR_WIDTH - 2.0 * tokens::SIDEBAR_PADDING - tokens::ICON_BUTTON_SIZE)
        / 2.0;

const TERMINALS_SHEET: &str = "gds-files-workspace-terminals";

const TERMINALS_CSS: &str = css!(
    r#"
    .gds-files-terminal-dock {
        display: flex;
        flex-direction: column;
        width: 100%;
        flex-shrink: 0;
        min-height: 0;
        min-width: 0;
        background: transparent;
        overflow: hidden;
    }

    .gds-split__trailing .gds-files-terminal-dock {
        height: 100% !important;
        flex: 1 1 0%;
    }

    .gds-split__trailing .gds-files-terminal-handle-slot {
        display: none;
    }

    .gds-files-terminal-handle-slot {
        position: relative;
        height: 7px;
        width: 100%;
        flex-shrink: 0;
        display: flex;
        align-items: center;
        cursor: ns-resize;
        user-select: none;
    }

    .gds-files-terminal-handle__bg {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: row;
        pointer-events: none;
    }

    .gds-files-terminal-handle__bg-left {
        height: 100%;
        flex-shrink: 0;
        background: var(--gds-sidebar-bg, var(--gds-sidebar));
        border-right: 1px solid var(--gds-border);
        box-sizing: border-box;
    }

    .gds-files-terminal-handle__bg-right {
        flex: 1 1 0%;
        height: 100%;
        background: var(--gds-content-bg, var(--gds-content));
    }

    .gds-files-terminal-handle__line {
        position: relative;
        width: 100%;
        height: 1px;
        background: var(--gds-border);
        z-index: 1;
        transition:
            height var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-terminal-handle-slot:hover .gds-files-terminal-handle__line,
    .gds-files-terminal-handle-slot[data-dragging="true"] .gds-files-terminal-handle__line,
    .gds-files-terminal-handle-slot:focus-visible .gds-files-terminal-handle__line {
        height: 2px;
        background: var(--gds-primary);
    }

    .gds-files-terminal-handle-slot:focus-visible {
        outline: none;
    }

    .gds-files-terminal-header {
        display: flex;
        flex-direction: row;
        align-items: stretch;
        width: 100%;
        height: var(--gds-terminal-header-height, 40px);
        flex-shrink: 0;
        box-sizing: border-box;
    }

    .gds-files-terminal-header__left {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        height: 100%;
        min-width: 0;
        padding-left: var(--gds-sidebar-padding, 10px);
        background: var(--gds-sidebar-bg, var(--gds-sidebar));
        border-right: 1px solid var(--gds-border);
        color: var(--gds-muted-foreground);
        box-sizing: border-box;
    }

    .gds-files-terminal-header__title-wrap {
        flex: 1 1 0%;
        min-width: 0;
        overflow: hidden;
    }

    .gds-files-terminal-header__right {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0%;
        height: 100%;
        min-width: 0;
        padding: 0 var(--gds-terminal-content-inset, 12px);
        gap: 4px;
        background: var(--gds-content-bg, var(--gds-content));
        box-sizing: border-box;
    }

    .gds-files-terminal-header__active-wrap {
        flex: 1 1 0%;
        min-width: 0;
        overflow: hidden;
        display: flex;
        align-items: center;
    }

    .gds-files-terminal-header__active-wrap .zui-tooltip__trigger {
        display: flex;
        min-width: 0;
        max-width: 100%;
        overflow: hidden;
    }

    .gds-files-terminal-header__active-title {
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
    }

    .gds-files-terminal-body {
        display: flex;
        flex-direction: row;
        flex: 1 1 0%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
    }

    .gds-files-terminal-rail {
        display: flex;
        flex-direction: column;
        flex-shrink: 0;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow-y: auto;
        overflow-x: hidden;
        padding: var(--gds-sidebar-padding, 10px);
        gap: 4px;
        background: var(--gds-sidebar-bg, var(--gds-sidebar));
        color: var(--gds-sidebar-foreground);
        border-right: 1px solid var(--gds-border);
        box-sizing: border-box;
    }

    .gds-files-terminal-rail:focus:not(:focus-visible) {
        outline: none;
    }

    .gds-files-terminal-rail:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
    }

    .gds-files-terminal-row {
        position: relative;
        width: 100%;
        flex-shrink: 0;
        min-width: 0;
    }

    .gds-files-terminal-row > .zui-tooltip__trigger {
        display: block;
        width: 100%;
        min-width: 0;
    }

    .gds-files-terminal-pane {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background: var(--gds-content-bg, var(--gds-content));
    }

    .gds-files-terminal-pane__surface {
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }

    .gds-files-terminal-empty {
        flex: 1 1 0%;
        width: 100%;
        height: 100%;
        padding: var(--gds-terminal-content-inset, 12px);
        background: transparent;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-text-sm, 14px);
        box-sizing: border-box;
    }

    .gds-files-terminal-rename-body {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    .gds-files-terminal-rename-title {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }
    "#
);

fn install_terminals_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(TERMINALS_SHEET, TERMINALS_CSS);
    }
}

pub(in crate::pages::workspace) struct TerminalDock<'a>(pub &'a Context);

impl TerminalDock<'_> {
    pub(in crate::pages::workspace) fn terminal_visible(&self) -> bool {
        self.0.0.terminal.try_get().unwrap_or(false)
            && self
                .0
                .0
                .preferences
                .state()
                .with(Preferences::terminal_enabled)
            && self.0.pane() == Pane::Files
            && self.0.read(|state| state.picker.is_none())
    }

    fn terminal_visible_untracked(&self) -> bool {
        self.0.0.terminal.try_get_untracked().unwrap_or(false)
            && self.0.0.preferences.snapshot().terminal_enabled()
            && self.0.pane_untracked() == Pane::Files
            && self.0.read_untracked(|state| state.picker.is_none())
    }

    pub(in crate::pages::workspace) fn terminal_focused(&self) -> bool {
        if self.0.0.terminals.inner.sealed.get() || !self.terminal_visible_untracked() {
            return false;
        }
        let Some(focused) = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten())
        else {
            return false;
        };
        let dock_node = self.0.0.terminals.inner.dock_node;
        dock_node.get_untracked() == Some(focused) || dock_node.contains(focused)
    }

    pub(in crate::pages::workspace) fn cancel_terminal_drag_hover(&self) -> bool {
        self.0.0.terminals.cancel_drag_hover()
    }

    fn active_local_directory(&self) -> Option<PathBuf> {
        if self.0.read(|state| state.picker.is_some()) {
            return None;
        }
        let (location, available) = self.0.read(|state| {
            let tab = state.tab();
            let loc = tab.navigation.location().clone();
            let ready = tab.navigation.has_directory()
                && !tab.navigation.is_loading()
                && tab
                    .navigation
                    .path()
                    .is_some_and(|path| !self.0.0.volumes.removal_in_progress(path));
            (loc, ready)
        });
        if !available {
            return None;
        }
        Rows(self.0)
            .resolve_path(&location)
            .filter(|path| path.is_absolute() && path.is_dir())
    }

    fn active_local_directory_untracked(&self) -> Option<PathBuf> {
        if self.0.read_untracked(|state| state.picker.is_some()) {
            return None;
        }
        let (location, available) = self.0.read_untracked(|state| {
            let tab = state.tab();
            let loc = tab.navigation.location().clone();
            let ready = tab.navigation.has_directory()
                && !tab.navigation.is_loading()
                && tab
                    .navigation
                    .path()
                    .is_some_and(|path| !self.0.0.volumes.removal_in_progress(path));
            (loc, ready)
        });
        if !available {
            return None;
        }
        Rows(self.0)
            .resolve_path_untracked(&location)
            .filter(|path| path.is_absolute() && path.is_dir())
    }

    fn default_terminal_cwd(&self) -> Option<PathBuf> {
        if self.0.read_untracked(|state| state.picker.is_some())
            || !self.0.0.preferences.snapshot().terminal_enabled()
        {
            return None;
        }
        self.active_local_directory_untracked().or_else(|| {
            let home = self.0.read_untracked(|state| state.home.clone());
            (home.is_absolute() && home.is_dir()).then_some(home)
        })
    }

    fn focus_active_terminal(&self) {
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        if let Some(id) = self.0.0.terminals.active.try_get_untracked().flatten()
            && let Some(entry) = self.0.0.terminals.find_session(id)
            && !entry.closing.get()
        {
            entry.terminal.focus();
        } else {
            self.0.0.terminals.inner.rail_node.focus();
        }
    }

    fn cycle_terminal_session(&self, backward: bool) {
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        let next_id = {
            let sessions = self.0.0.terminals.inner.sessions.borrow();
            if sessions.len() <= 1 {
                return;
            }
            let active = self.0.0.terminals.active.try_get_untracked().flatten();
            let current = active
                .and_then(|id| sessions.iter().position(|entry| entry.id == id))
                .unwrap_or(0);
            let target = if backward {
                (current + sessions.len() - 1) % sessions.len()
            } else {
                (current + 1) % sessions.len()
            };
            sessions
                .get(target)
                .map(|entry| (entry.id, entry.terminal.clone()))
        };
        if let Some((id, terminal)) = next_id {
            let _ = self.0.0.terminals.active.try_set(Some(id));
            terminal.focus();
        }
    }

    fn close_terminal_session(&self, id: u64) {
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        let rail_was_focused = zgui::view::current_host()
            .and_then(|host| host.focused().try_get_untracked().flatten())
            .is_some_and(|node| {
                let rail = self.0.0.terminals.inner.rail_node;
                rail.get_untracked() == Some(node) || rail.contains(node)
            });
        let dock_was_focused = self.terminal_focused();

        let removed_and_next = {
            let mut sessions = self.0.0.terminals.inner.sessions.borrow_mut();
            let Some(pos) = sessions.iter().position(|entry| entry.id == id) else {
                return;
            };
            let removed = sessions.remove(pos);
            removed.closing.set(true);
            let current_active = self.0.0.terminals.active.try_get_untracked().flatten();
            let was_active = current_active == Some(id);
            let next_active = if was_active {
                sessions
                    .get(pos)
                    .or_else(|| pos.checked_sub(1).and_then(|i| sessions.get(i)))
                    .map(|entry| entry.id)
            } else {
                current_active
            };
            (removed, was_active, next_active)
        };

        let (removed, was_active, next_active) = removed_and_next;
        if self
            .0
            .0
            .terminals
            .inner
            .rename_dialog
            .try_get_untracked()
            .flatten()
            .is_some_and(|dlg| dlg.session_id == id)
        {
            let _ = self.0.0.terminals.inner.rename_dialog.try_set(None);
        }

        let _ = self.0.0.terminals.active.try_set(next_active);
        self.0.0.terminals.bump();

        if let Some(gds) = Gds::try_current() {
            gds.tasks()
                .scope()
                .spawn_named("Shut down closed terminal", removed.terminal.shutdown());
        } else {
            self.0.0.owner.with(|| {
                drop(spawn_local(removed.terminal.shutdown()));
            });
        }

        if was_active && dock_was_focused {
            if let Some(next_id) = next_active
                && let Some(next_entry) = self.0.0.terminals.find_session(next_id)
            {
                if rail_was_focused {
                    self.0.0.terminals.inner.rail_node.focus();
                } else {
                    next_entry.terminal.focus();
                }
            } else {
                self.0.0.terminals.inner.rail_node.focus();
            }
        }
    }

    fn begin_terminal_rename(&self, id: u64) {
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        let Some(entry) = self.0.0.terminals.find_session(id) else {
            return;
        };
        let current = entry.display_title_untracked();
        let (draft, input_ref) = self
            .0
            .0
            .owner
            .with(|| (RwSignal::new_local(current.clone()), NodeRef::new()));
        let _ = self
            .0
            .0
            .terminals
            .inner
            .rename_dialog
            .try_set(Some(RenameDialogState {
                session_id: id,
                draft,
                input_ref,
            }));

        let dock = self.0.0.terminals.clone();
        let initial = current;
        let handle = self.0.0.owner.with(|| {
            request_frame(move |_| {
                if dock.inner.sealed.get() {
                    return;
                }
                if dock
                    .inner
                    .rename_dialog
                    .try_get_untracked()
                    .flatten()
                    .is_some_and(|state| state.session_id == id)
                    && input_ref.is_bound()
                {
                    input_ref.focus();
                    input_ref.set_value(&initial);
                    input_ref.select_all();
                }
            })
        });
        *self.0.0.terminals.inner.pending_rename_focus.borrow_mut() = Some(handle);
    }

    fn finish_terminal_rename(&self, accept: bool) {
        *self.0.0.terminals.inner.pending_rename_focus.borrow_mut() = None;
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        let Some(dialog) = self
            .0
            .0
            .terminals
            .inner
            .rename_dialog
            .try_get_untracked()
            .flatten()
        else {
            return;
        };
        let _ = self.0.0.terminals.inner.rename_dialog.try_set(None);
        if let Some(entry) = self.0.0.terminals.find_session(dialog.session_id) {
            if accept && let Some(raw) = dialog.draft.try_get_untracked() {
                let trimmed = raw.trim();
                let _ = entry
                    .custom_title
                    .try_set((!trimmed.is_empty()).then(|| trimmed.to_owned()));
            }
            entry.terminal.focus();
        } else {
            self.focus_active_terminal();
        }
    }

    pub(in crate::pages::workspace) fn apply_terminal_preferences(
        &self,
        preferences: &Preferences,
    ) {
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        if !preferences.terminal_enabled() {
            let _ = self.0.0.terminal.try_set(false);
            let _ = self.0.0.terminals.inner.maximized.try_set(false);
        }
        if let Some(content_height) = preferences.files.terminal_content_height
            && let Some((_, available)) =
                self.0.0.terminals.inner.workspace_size.try_get_untracked()
            && available > MIN_CONTENT_HEIGHT + MIN_DOCK_HEIGHT
        {
            let target = self
                .0
                .0
                .terminals
                .clamped_height((available - content_height).max(MIN_DOCK_HEIGHT));
            let _ = self.0.0.terminals.inner.height.try_set(target);
        }
        let term_prefs = preferences.terminal.clone();
        let terminals: Vec<Terminal> = self
            .0
            .0
            .terminals
            .inner
            .sessions
            .borrow()
            .iter()
            .map(|entry| entry.terminal.clone())
            .collect();
        for terminal in terminals {
            terminal.apply_preferences(term_prefs.clone());
        }
    }

    pub(in crate::pages::workspace) fn toggle_terminal(&self) {
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        if self.0.read_untracked(|state| state.picker.is_some())
            || !self.0.0.preferences.snapshot().terminal_enabled()
        {
            let _ = self.0.0.terminal.try_set(false);
            let _ = self.0.0.terminals.inner.maximized.try_set(false);
            return;
        }
        if self.0.pane_untracked() != Pane::Files {
            self.0.show_files();
            if !self.0.0.terminal.try_get_untracked().unwrap_or(false) {
                let _ = self.0.0.terminal.try_set(true);
            }
            let empty = self.0.0.terminals.inner.sessions.borrow().is_empty();
            if empty && self.0.0.terminals.inner.next_id.get() == 1 {
                self.new_terminal();
            } else {
                self.focus_active_terminal();
            }
            return;
        }

        let next = !self.0.0.terminal.try_get_untracked().unwrap_or(false);
        let _ = self.0.0.terminal.try_set(next);
        if next {
            let empty = self.0.0.terminals.inner.sessions.borrow().is_empty();
            if empty {
                self.new_terminal();
            } else {
                self.focus_active_terminal();
            }
        } else {
            if Geometry(self.0).height_tier_untracked() == gds::ui::window::HeightTier::Normal {
                let _ = self.0.0.terminals.inner.maximized.try_set(false);
            }
            self.0.focus_content();
        }
    }

    pub(in crate::pages::workspace) fn new_terminal(&self) {
        if self.0.0.terminals.inner.sealed.get()
            || self.0.read_untracked(|state| state.picker.is_some())
            || !self.0.0.preferences.snapshot().terminal_enabled()
        {
            return;
        }
        let Some(cwd) = self.default_terminal_cwd() else {
            self.0
                .report_notice(i18n::text_untracked("files-terminal-cwd-unavailable"));
            return;
        };
        self.new_terminal_at(cwd);
    }

    pub(in crate::pages::workspace) fn new_terminal_at(&self, path: PathBuf) {
        if self.0.0.terminals.inner.sealed.get()
            || self.0.read_untracked(|state| state.picker.is_some())
            || !self.0.0.preferences.snapshot().terminal_enabled()
        {
            return;
        }
        if !path.is_absolute() || !path.is_dir() {
            self.0
                .report_notice(i18n::text_untracked("files-terminal-cwd-unavailable"));
            return;
        }
        if self.0.pane_untracked() != Pane::Files {
            self.0.show_files();
        }
        let _ = self.0.0.terminal.try_set(true);

        let term_prefs = self.0.0.preferences.snapshot().terminal;
        let options = SpawnOptions {
            cwd: path.clone(),
            ..SpawnOptions::default()
        };
        let id = self.0.0.terminals.inner.next_id.get();
        let weak = self.0.downgrade();
        let spawned = self.0.0.owner.with(|| {
            Terminal::spawn(options, term_prefs, &self.0.0.tasks).map(|terminal| {
                let observed_term = terminal.clone();
                let on_event = UnsyncCallback::new(move |event| {
                    let Some(ctx) = weak.upgrade() else {
                        return;
                    };
                    if ctx.0.terminals.inner.sealed.get() {
                        return;
                    }
                    if let TerminalEvent::Closed(_) = event
                        && matches!(
                            observed_term.status().try_get_untracked(),
                            Some(Status::Exited { .. } | Status::Closed)
                        )
                    {
                        TerminalDock(&ctx).close_terminal_session(id);
                    }
                });
                terminal.on_event(on_event);
                let entry = SessionEntry {
                    id,
                    terminal: terminal.clone(),
                    view_ref: NodeRef::new(),
                    started_in: path,
                    custom_title: RwSignal::new_local(None),
                    closing: Cell::new(false),
                };
                (terminal, entry)
            })
        });

        let (terminal, entry) = match spawned {
            Ok(pair) => pair,
            Err(err) => {
                self.0.report_notice(err.to_string());
                return;
            }
        };

        self.0.0.terminals.inner.next_id.set(id + 1);
        {
            self.0.0.terminals.inner.sessions.borrow_mut().push(entry);
        }
        let _ = self.0.0.terminals.active.try_set(Some(id));
        self.0.0.terminals.bump();
        terminal.focus();
    }

    fn switch_terminal_directory(&self) {
        if self.0.0.terminals.inner.sealed.get()
            || self
                .0
                .0
                .terminals
                .inner
                .rename_dialog
                .try_get_untracked()
                .flatten()
                .is_some()
        {
            return;
        }
        let active_running = self
            .0
            .0
            .terminals
            .active
            .try_get_untracked()
            .flatten()
            .and_then(|id| self.0.0.terminals.find_session(id))
            .is_some_and(|entry| {
                !entry.closing.get()
                    && entry.terminal.status().try_get_untracked() == Some(Status::Running)
            });
        if !active_running {
            return;
        }
        let Some(dir) = self.active_local_directory_untracked() else {
            return;
        };
        // Never inject a cd/Enter command into an existing shell buffer: start a fresh session at cwd.
        self.new_terminal_at(dir);
    }

    fn paste_paths_to_active_terminal(&self, paths: &[PathBuf]) -> bool {
        if self.0.0.terminals.inner.sealed.get()
            || paths.is_empty()
            || !self.terminal_visible_untracked()
        {
            return false;
        }
        if let Some(id) = self.0.0.terminals.active.try_get_untracked().flatten()
            && let Some(entry) = self.0.0.terminals.find_session(id)
            && !entry.closing.get()
        {
            entry.terminal.paste_paths(paths);
            entry.terminal.focus();
            return true;
        }
        false
    }

    pub(in crate::pages::workspace) fn observe_terminals(&self) {
        if let Some(content_height) = self
            .0
            .0
            .preferences
            .snapshot()
            .files
            .terminal_content_height
            && let Some((_, available)) =
                self.0.0.terminals.inner.workspace_size.try_get_untracked()
            && available > MIN_CONTENT_HEIGHT + MIN_DOCK_HEIGHT
        {
            let target = self
                .0
                .0
                .terminals
                .clamped_height((available - content_height).max(MIN_DOCK_HEIGHT));
            let _ = self.0.0.terminals.inner.height.try_set(target);
        }

        // Folder navigation NEVER injects auto-cd commands into running shells.
        // Register clean window-close PTY shutdown and internal file-drag drop forwarding.
        let shutdown_dock = self.0.0.terminals.clone();
        let root_node = self.0.0.root_focus;
        let weak = self.0.downgrade();

        self.0.0.owner.with(|| {
            on_cleanup_local(move || {
                shutdown_dock.shutdown_all();
            });

            let attach_weak = weak.clone();
            let attach_fx = RenderEffect::new(move |_| {
                if root_node.get().is_none() {
                    return;
                }
                let Some(ctx) = attach_weak.upgrade() else {
                    return;
                };
                if !ctx.0.terminals.inner.drag_listeners.borrow().is_empty() {
                    return;
                }

                let down_weak = ctx.downgrade();
                let g_down = root_node.listen(
                    events::POINTER_DOWN,
                    ListenerOptions::CAPTURE,
                    move |ev: &mut EventCx<'_, events::PointerDown>| {
                        if ev.button != Some(PointerButton::Primary) {
                            return;
                        }
                        let Some(c) = down_weak.upgrade() else {
                            return;
                        };
                        if c.0.terminals.inner.sealed.get() {
                            return;
                        }
                        let pos = (ev.position.x.0, ev.position.y.0);
                        if !drop_hit_test(&c.0.list, pos) {
                            *c.0.terminals.inner.internal_drag.borrow_mut() = None;
                            return;
                        }
                        let candidate_paths = c.read_untracked(|state| {
                            let tab = state.tab();
                            let selected: Vec<PathBuf> = tab
                                .navigation
                                .view()
                                .selection
                                .iter()
                                .filter_map(|key| match key {
                                    RowKey::Local(path) => Some(path.clone()),
                                    _ => None,
                                })
                                .collect();
                            if !selected.is_empty() {
                                selected
                            } else {
                                tab.rows
                                    .iter()
                                    .filter_map(|row| match row {
                                        Row::File(entry) => Some(entry.path.clone()),
                                        _ => None,
                                    })
                                    .take(1)
                                    .collect()
                            }
                        });
                        if !candidate_paths.is_empty() {
                            *c.0.terminals.inner.internal_drag.borrow_mut() =
                                Some(InternalFileDrag {
                                    start: pos,
                                    active: false,
                                    paths: candidate_paths,
                                });
                        }
                    },
                );

                let move_weak = ctx.downgrade();
                let g_move = root_node.listen(
                    events::POINTER_MOVE,
                    ListenerOptions::CAPTURE,
                    move |ev: &mut EventCx<'_, events::PointerMove>| {
                        let Some(c) = move_weak.upgrade() else {
                            return;
                        };
                        if c.0.terminals.inner.sealed.get() || c.0.native_drag_active.get() {
                            return;
                        }
                        let pos = (ev.position.x.0, ev.position.y.0);
                        let Some(shared_sources) = c.drag_sources().try_get_untracked() else {
                            return;
                        };
                        let active = {
                            let mut drag = c.0.terminals.inner.internal_drag.borrow_mut();
                            if !shared_sources.is_empty() {
                                if let Some(st) = drag.as_mut() {
                                    st.active = true;
                                }
                                true
                            } else if let Some(st) = drag.as_mut() {
                                if !st.active
                                    && (pos.0 - st.start.0).hypot(pos.1 - st.start.1)
                                        >= DRAG_THRESHOLD
                                {
                                    st.active = true;
                                }
                                st.active
                            } else {
                                false
                            }
                        };
                        let over_pane = active
                            && TerminalDock(&c).terminal_visible_untracked()
                            && drop_hit_test(&c.0.terminals.inner.pane_node, pos);
                        if c.0
                            .terminals
                            .inner
                            .internal_drop_hover
                            .try_get_untracked()
                            .is_some_and(|cur| cur != over_pane)
                        {
                            let _ = c.0.terminals.inner.internal_drop_hover.try_set(over_pane);
                        }
                    },
                );

                let up_weak = ctx.downgrade();
                let g_up = root_node.listen(
                    events::POINTER_UP,
                    ListenerOptions::CAPTURE,
                    move |ev: &mut EventCx<'_, events::PointerUp>| {
                        if ev.button != Some(PointerButton::Primary) {
                            return;
                        }
                        let Some(c) = up_weak.upgrade() else {
                            return;
                        };
                        if c.0.terminals.inner.sealed.get() || c.0.native_drag_active.get() {
                            return;
                        }
                        if c.0
                            .terminals
                            .inner
                            .internal_drop_hover
                            .try_get_untracked()
                            .unwrap_or(false)
                        {
                            let _ = c.0.terminals.inner.internal_drop_hover.try_set(false);
                        }
                        let tracked = c.0.terminals.inner.internal_drag.borrow_mut().take();
                        let shared: Vec<PathBuf> = c
                            .drag_sources()
                            .try_get_untracked()
                            .map(|s| s.into_iter().collect())
                            .unwrap_or_default();
                        let dropped_paths = if !shared.is_empty() {
                            Some(shared)
                        } else {
                            tracked.filter(|d| d.active).map(|d| d.paths)
                        };
                        let pos = (ev.position.x.0, ev.position.y.0);
                        let dock = TerminalDock(&c);
                        if let Some(paths) = dropped_paths
                            && dock.terminal_visible_untracked()
                            && drop_hit_test(&c.0.terminals.inner.pane_node, pos)
                        {
                            let _ = dock.paste_paths_to_active_terminal(&paths);
                        }
                    },
                );

                let cancel_weak = ctx.downgrade();
                let g_cancel = root_node.listen(
                    events::POINTER_CANCEL,
                    ListenerOptions::CAPTURE,
                    move |_| {
                        if let Some(c) = cancel_weak.upgrade() {
                            let _ = c.0.terminals.cancel_drag_hover();
                        }
                    },
                );

                let mut guards = ctx.0.terminals.inner.drag_listeners.borrow_mut();
                guards.extend([g_down, g_move, g_up, g_cancel].into_iter().flatten());
            });
            let detach_dock = self.0.0.terminals.clone();
            on_cleanup_local(move || {
                drop(attach_fx);
                detach_dock.inner.drag_listeners.borrow_mut().clear();
            });
        });
    }

    pub(in crate::pages::workspace) fn terminal_workspace_resized(
        &self,
        width: f32,
        available: f32,
    ) {
        if self.0.0.terminals.inner.sealed.get() {
            return;
        }
        let _ = self
            .0
            .0
            .terminals
            .inner
            .workspace_size
            .try_set((width.max(320.0), available.max(MIN_DOCK_HEIGHT)));
        if let Some(content_height) = self
            .0
            .0
            .preferences
            .snapshot()
            .files
            .terminal_content_height
        {
            if available > MIN_CONTENT_HEIGHT + MIN_DOCK_HEIGHT {
                let target = self
                    .0
                    .0
                    .terminals
                    .clamped_height((available - content_height).max(MIN_DOCK_HEIGHT));
                let _ = self.0.0.terminals.inner.height.try_set(target);
            }
        } else if let Some(cur_h) = self.0.0.terminals.inner.height.try_get_untracked() {
            let clamped = self.0.0.terminals.clamped_height(cur_h);
            let _ = self.0.0.terminals.inner.height.try_set(clamped);
        }
    }

    fn handle_terminal_shortcut(&self, key: &Key, modifiers: Modifiers) -> bool {
        if self.0.0.terminals.inner.sealed.get() {
            return false;
        }
        let key_event = KeyEvent {
            key: key.clone(),
            key_without_modifiers: key.clone(),
            physical: PhysicalKey::Unidentified(0),
            location: KeyLocation::Standard,
            repeat: false,
        };
        if Command::Terminal.matches(&key_event, modifiers) {
            self.toggle_terminal();
            return true;
        }
        if Command::NextTab.matches(&key_event, modifiers) {
            self.cycle_terminal_session(false);
            return true;
        }
        if Command::PreviousTab.matches(&key_event, modifiers) {
            self.cycle_terminal_session(true);
            return true;
        }
        if *key == Key::Named(NamedKey::F6) && modifiers.is_empty() {
            if Geometry(self.0).height_tier_untracked() == gds::ui::window::HeightTier::Micro {
                let _ = self.0.0.terminal.try_set(false);
            } else {
                let _ = self.0.0.terminals.inner.maximized.try_set(false);
            }
            self.0.focus_content();
            return true;
        }
        if *key == Key::Named(NamedKey::F2) && modifiers.is_empty() {
            if let Some(id) = self.0.0.terminals.active.try_get_untracked().flatten() {
                self.begin_terminal_rename(id);
            }
            return true;
        }
        let ctrl_only = modifiers.contains(Modifiers::CONTROL)
            && !modifiers.contains(Modifiers::ALT)
            && !modifiers.contains(Modifiers::META);
        if ctrl_only && let Key::Character(ch) = key {
            if ch.eq_ignore_ascii_case("n") {
                self.new_terminal();
                return true;
            }
            if ch.eq_ignore_ascii_case("w") {
                if let Some(id) = self.0.0.terminals.active.try_get_untracked().flatten() {
                    self.close_terminal_session(id);
                }
                return true;
            }
        }
        false
    }

    pub(in crate::pages::workspace) fn terminal_view(
        &self,
        _presentation: SidebarPresentation,
    ) -> AnyView {
        install_terminals_stylesheet();

        let ctx = self.0.clone();
        let dock = self.0.0.terminals.clone();
        let active_sig = dock.active;
        let rev_sig = dock.inner.revision;
        let dragging_sig = dock.inner.dragging;
        let maximized_sig = dock.inner.maximized;
        let dock_node = dock.inner.dock_node;
        let rail_node = dock.inner.rail_node;
        let pane_node = dock.inner.pane_node;
        let rename_sig = dock.inner.rename_dialog;

        let presentation = self.0.sidebar_presentation();
        let sidebar_width = self.0.sidebar_width();

        let visible_sig = Signal::derive_local({
            let c = self.0.clone();
            move || TerminalDock(&c).terminal_visible()
        });
        let open_target = Signal::derive_local(move || if visible_sig.get() { 1.0 } else { 0.0 });
        let open_progress = motion::use_transition(open_target, motion::WORKSPACE_DURATION_MS);

        let micro_height = Signal::derive_local({
            let c = self.0.clone();
            move || Geometry(&c).height_tier() == gds::ui::window::HeightTier::Micro
        });
        let max_target = Signal::derive_local(move || {
            if maximized_sig.get() || micro_height.get() {
                1.0
            } else {
                0.0
            }
        });
        let max_progress = motion::use_transition(max_target, motion::WORKSPACE_DURATION_MS);

        let dock_for_height = dock.clone();
        let dock_for_drag = dock.clone();
        let ctx_for_commit = ctx.clone();
        let ctx_for_cancel = ctx.clone();
        let ctx_for_key_resize = ctx.clone();
        let dock_for_key_resize = dock.clone();

        let on_handle_pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button != Some(PointerButton::Primary) {
                return;
            }
            ev.capture_pointer();
            ev.stop_propagation();
            ev.prevent_default();
            dragging_sig.set(Some((
                ev.position.y.0,
                dock_for_drag.inner.height.get_untracked(),
            )));
        };

        let dock_for_move = dock.clone();
        let on_handle_pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
            if let Some((start_y, start_h)) = dragging_sig.get_untracked() {
                let delta = start_y - ev.position.y.0;
                let next = dock_for_move.clamped_height(start_h + delta);
                dock_for_move.inner.height.set(next);
            }
        };

        let dock_for_up = dock.clone();
        let dock_for_cancel = dock.clone();
        let finish_resize = move |ctx: &Context, dock: &Dock| {
            if dragging_sig.get_untracked().is_some() {
                dragging_sig.set(None);
                let available = dock.inner.workspace_size.get_untracked().1;
                let h = dock.inner.height.get_untracked();
                let content_h = (available - h).max(MIN_CONTENT_HEIGHT);
                ctx.edit_preferences(|prefs| {
                    prefs.files.terminal_content_height = Some(content_h);
                });
            }
        };

        let dock_list = dock.clone();
        let dock_pane = dock.clone();
        let dock_header = dock.clone();
        let c_list = self.0.clone();
        let c_pane = self.0.clone();
        let c_new = self.0.clone();
        let c_cwd = self.0.clone();
        let c_rail_key = self.0.clone();
        let c_dock_key = self.0.clone();
        let c_drop_ext = self.0.clone();
        let c_rename_decision = self.0.clone();

        let cwd_disabled = Signal::derive_local({
            let c = self.0.clone();
            let dock = dock.clone();
            move || {
                let _ = rev_sig.get();
                let has_dir = TerminalDock(&c).active_local_directory().is_some();
                let running = active_sig
                    .get()
                    .and_then(|id| dock.find_session(id))
                    .is_some_and(|entry| {
                        !entry.closing.get()
                            && entry.terminal.status().try_get() == Some(Status::Running)
                    });
                !has_dir || !running
            }
        });

        let max_icon = Signal::derive_local(move || {
            if maximized_sig.get() {
                IconName::Minimize2
            } else {
                IconName::Maximize2
            }
        });
        let max_label = Signal::derive_local(move || {
            if maximized_sig.get() {
                i18n::text("files-terminal-restore")
            } else {
                i18n::text("files-terminal-maximize")
            }
        });

        let active_summary = Signal::derive_local({
            let dock = dock_header.clone();
            move || {
                let _ = rev_sig.get();
                active_sig
                    .get()
                    .and_then(|id| dock.find_session(id))
                    .map_or_else(
                        || i18n::text("settings-terminal"),
                        |entry| entry.display_title(),
                    )
            }
        });
        let active_details = Signal::derive_local({
            let dock = dock_header;
            move || {
                let _ = rev_sig.get();
                active_sig
                    .get()
                    .and_then(|id| dock.find_session(id))
                    .map_or_else(Vec::new, |entry| entry.detail_lines())
            }
        });

        let pane_drop_attrs = file_drop_target_attrs(FileDropTarget {
            active: dock.inner.internal_drop_hover.into(),
            options: FileDropOptions::copy_only(),
            can_accept: None,
            on_hover_change: None,
            on_drop: Some(UnsyncCallback::new(move |drop: ExternalFileDrop| {
                let pasted = TerminalDock(&c_drop_ext).paste_paths_to_active_terminal(&drop.paths);
                if let Some(handle) = drop.handle {
                    if pasted {
                        handle.complete();
                    } else {
                        handle.reject();
                    }
                }
            })),
            on_reject: None,
        });

        let rename_open = Signal::derive_local(move || rename_sig.get().is_some());
        let rename_composing = Rc::new(Cell::new(false));
        let rename_dialog_view = view! {
            ModalDialog(
                open = Binding::controlled(rename_open, {
                    let c = self.0.clone();
                    move |next: bool| {
                        if !next
                            && !c.0.terminals.inner.sealed.get()
                            && c.0
                                .terminals
                                .inner
                                .rename_dialog
                                .try_get_untracked()
                                .flatten()
                                .is_some()
                        {
                            TerminalDock(&c).finish_terminal_rename(false);
                        }
                    }
                }),
                on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                    if c_rename_decision.0.terminals.inner.sealed.get() {
                        return;
                    }
                    let dock = TerminalDock(&c_rename_decision);
                    match decision {
                        DialogDecision::Confirm => dock.finish_terminal_rename(true),
                        DialogDecision::Cancel | DialogDecision::Dismiss => {
                            dock.finish_terminal_rename(false);
                        }
                    }
                }),
            ) {
                {{
                    let dialog_composing = Rc::clone(&rename_composing);
                    let dialog_ctx = ctx.clone();
                    move || match rename_sig.get() {
                        None => AnyView::new(view! { box() }),
                        Some(state) => {
                            let draft = state.draft;
                            let input_ref = state.input_ref;
                            let c_start = Rc::clone(&dialog_composing);
                            let c_pre = Rc::clone(&dialog_composing);
                            let c_commit = Rc::clone(&dialog_composing);
                            let c_end = Rc::clone(&dialog_composing);
                            let c_key = Rc::clone(&dialog_composing);
                            let c_enter = dialog_ctx.clone();
                            let c_esc = dialog_ctx.clone();
                            let c_actions = dialog_ctx.clone();
                            AnyView::new(view! {
                                column(class = "gds-files-terminal-rename-body") {
                                    DialogHeader {
                                        DialogTitle {
                                            row(class = "gds-files-terminal-rename-title") {
                                                Icon(name = IconName::Pencil) {}
                                                text {{i18n::text("files-terminal-rename")}}
                                            }
                                        }
                                    }
                                    Input(
                                        node_ref = input_ref,
                                        label = i18n::text("files-terminal-rename"),
                                        value = Binding::controlled(draft, move |next| draft.set(next)),
                                        on:ime_start = move |_| c_start.set(true),
                                        on:ime_preedit = move |ev| {
                                            let active = match &**ev {
                                                ImeEvent::Preedit { text, .. } => !text.is_empty(),
                                                _ => ev.is_composing(),
                                            };
                                            c_pre.set(active);
                                        },
                                        on:ime_commit = move |_| c_commit.set(false),
                                        on:ime_end = move |_| c_end.set(false),
                                        on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                            if c_key.get() {
                                                return;
                                            }
                                            if ev.key == Key::Named(NamedKey::Enter)
                                                && ev.modifiers.is_empty()
                                            {
                                                TerminalDock(&c_enter).finish_terminal_rename(true);
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                            } else if ev.key == Key::Named(NamedKey::Escape)
                                                && ev.modifiers.is_empty()
                                            {
                                                TerminalDock(&c_esc).finish_terminal_rename(false);
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                            }
                                        },
                                    )
                                    DialogFooter {
                                        DialogActions(
                                            primary_label = i18n::text("rename-confirm"),
                                            cancel_label = i18n::text("operation-cancel"),
                                            on_decision = UnsyncCallback::new(move |decision: DialogDecision| {
                                                let dock = TerminalDock(&c_actions);
                                                match decision {
                                                    DialogDecision::Confirm => {
                                                        dock.finish_terminal_rename(true);
                                                    }
                                                    DialogDecision::Cancel | DialogDecision::Dismiss => {
                                                        dock.finish_terminal_rename(false);
                                                    }
                                                }
                                            }),
                                        )
                                    }
                                }
                            })
                        }
                    }
                }}
            }
        };

        AnyView::new(view! {
            box(
                node_ref = dock_node,
                class = "gds-files-terminal-dock",
                style:display = move || {
                    if open_progress.get() > 0.001 {
                        Some("flex".to_owned())
                    } else {
                        Some("none".to_owned())
                    }
                },
                style:height = move || {
                    let p = open_progress.get();
                    if p <= 0.001 {
                        Some("0px".to_owned())
                    } else {
                        let m = max_progress.get();
                        let base = dock_for_height.base_height();
                        let full = dock_for_height.maximized_height();
                        let target_h = base + (full - base) * m;
                        Some(format!("{:.1}px", target_h * p))
                    }
                },
                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                    if !ev.repeat
                        && TerminalDock(&c_dock_key)
                            .handle_terminal_shortcut(&ev.key, ev.modifiers)
                    {
                        ev.prevent_default();
                        ev.stop_propagation();
                    }
                },
            ) {
                box(
                    class = "gds-files-terminal-handle-slot",
                    style:display = move || {
                        if max_progress.get() >= 0.999 {
                            Some("none".to_owned())
                        } else {
                            Some("flex".to_owned())
                        }
                    },
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Slider,
                    a11y:label = move || i18n::text("files-terminal-resize"),
                    attr:data-dragging = move || dragging_sig.get().is_some().then(|| "true".to_owned()),
                    on:pointer_down = on_handle_pointer_down,
                    on:pointer_move = on_handle_pointer_move,
                    on:pointer_up = move |ev| {
                        ev.release_pointer();
                        finish_resize(&ctx_for_commit, &dock_for_up);
                    },
                    on:pointer_cancel = move |ev| {
                        ev.release_pointer();
                        finish_resize(&ctx_for_cancel, &dock_for_cancel);
                    },
                    on:key_down = move |ev| {
                        let step = match &ev.key {
                            Key::Named(NamedKey::ArrowUp) => Some(16.0),
                            Key::Named(NamedKey::ArrowDown) => Some(-16.0),
                            _ => None,
                        };
                        if let Some(delta) = step {
                            let next = dock_for_key_resize.clamped_height(
                                dock_for_key_resize.inner.height.get_untracked() + delta,
                            );
                            dock_for_key_resize.inner.height.set(next);
                            let available = dock_for_key_resize.inner.workspace_size.get_untracked().1;
                            let content_h = (available - next).max(MIN_CONTENT_HEIGHT);
                            ctx_for_key_resize.edit_preferences(|prefs| {
                                prefs.files.terminal_content_height = Some(content_h);
                            });
                            ev.prevent_default();
                            ev.stop_propagation();
                        }
                    },
                ) {
                    box(class = "gds-files-terminal-handle__bg", a11y:hidden = true) {
                        box(
                            class = "gds-files-terminal-handle__bg-left",
                            style:width = move || Some(format!("{:.2}px", sidebar_width.get())),
                        ) {}
                        box(class = "gds-files-terminal-handle__bg-right") {}
                    }
                    box(class = "gds-files-terminal-handle__line") {}
                }

                row(class = "gds-files-terminal-header") {
                    row(
                        class = "gds-files-terminal-header__left",
                        style:width = move || Some(format!("{:.2}px", sidebar_width.get())),
                        style:padding-right = move || {
                            let pr = tokens::SIDEBAR_PADDING
                                + presentation.get().interpolate(0.0, COLLAPSED_ACTION_INSET);
                            Some(format!("{pr:.2}px"))
                        },
                    ) {
                        box(
                            class = "gds-files-terminal-header__title-wrap",
                            var:--gds-sidebar-label-pct = move || {
                                Some(format!("{:.2}%", presentation.get().label_opacity() * 100.0))
                            },
                        ) {
                            SideBarSectionTitle {
                                {move || i18n::text("settings-terminal")}
                            }
                        }
                        IconButton(
                            icon = IconName::Plus,
                            label = Signal::derive_local(|| i18n::text("files-terminal-new")),
                            shortcut = Some(Shortcut::new(Key::character("n"), Modifiers::CONTROL)),
                            shortcut_enabled = Signal::derive_local(move || {
                                zgui::view::current_host()
                                    .and_then(|host| host.focused().try_get().flatten())
                                    .is_some_and(|node| {
                                        dock_node.get_untracked() == Some(node)
                                            || dock_node.contains(node)
                                    })
                            }),
                            on:click = move |_| TerminalDock(&c_new).new_terminal(),
                        )
                    }
                    row(class = "gds-files-terminal-header__right") {
                        box(class = "gds-files-terminal-header__active-wrap") {
                            DetailsTooltip(summary = active_summary, details = active_details) {
                                text(class = "gds-files-terminal-header__active-title") {
                                    {move || active_summary.get()}
                                }
                            }
                        }
                        IconButton(
                            icon = IconName::FolderInput,
                            label = Signal::derive_local(|| i18n::text("files-terminal-switch-directory")),
                            disabled = cwd_disabled,
                            on:click = move |_| TerminalDock(&c_cwd).switch_terminal_directory(),
                        )
                        Show(when = move || !micro_height.get()) {
                            IconButton(
                                icon = max_icon,
                                label = max_label,
                                on:click = move |_| {
                                    maximized_sig.update(|m| *m = !*m);
                                },
                            )
                        }
                    }
                }

                row(class = "gds-files-terminal-body") {
                    column(
                        node_ref = rail_node,
                        class = "gds-files-terminal-rail",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::TabList,
                        a11y:label = move || i18n::text("settings-terminal"),
                        style:width = move || Some(format!("{:.2}px", sidebar_width.get())),
                        on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                            if !ev.repeat
                                && TerminalDock(&c_rail_key)
                                    .handle_terminal_shortcut(&ev.key, ev.modifiers)
                            {
                                ev.prevent_default();
                                ev.stop_propagation();
                            }
                        },
                    ) {
                        {move || {
                            let _ = rev_sig.get();
                            let sessions = dock_list.session_snapshot();
                            sessions
                                .into_iter()
                                .map(|entry| {
                                    let id = entry.id;
                                    let selected =
                                        Signal::derive_local(move || active_sig.get() == Some(id));
                                    let c_select = c_list.clone();
                                    let c_middle = c_list.clone();
                                    let c_close_hover = c_list.clone();
                                    let c_close_corner = c_list.clone();
                                    let c_reveal = c_list.clone();
                                    let c_row_key = c_list.clone();
                                    let entry_for_term = entry.clone();
                                    let entry_for_title = entry.clone();
                                    let entry_for_summary = entry.clone();
                                    let entry_for_details = entry;

                                    let summary_sig = Signal::derive_local(move || {
                                        entry_for_summary.display_title()
                                    });
                                    let details_sig = Signal::derive_local(move || {
                                        entry_for_details.detail_lines()
                                    });

                                    let shortcut_target = Signal::derive_local(move || {
                                        let focused = zgui::view::current_host()
                                            .and_then(|host| host.focused().try_get().flatten())
                                            .is_some_and(|node| {
                                                let dock_root =
                                                    c_reveal.0.terminals.inner.dock_node;
                                                dock_root.get_untracked() == Some(node)
                                                    || dock_root.contains(node)
                                            });
                                        selected.get() && focused
                                    });

                                    let expanded_shortcut_target = Signal::derive_local(move || {
                                        shortcut_target.get()
                                            && presentation.get().actions_visible()
                                    });
                                    let collapsed_shortcut_target =
                                        Signal::derive_local(move || {
                                            shortcut_target.get()
                                                && presentation.get().is_collapsed()
                                        });

                                    let ctrl_revealed = Signal::derive_local(move || {
                                        let held_ctrl = KeytipRegistry::current().is_some_and(
                                            |reg| reg.held().get() == Some(HeldModifier::Ctrl),
                                        );
                                        held_ctrl && shortcut_target.get()
                                    });
                                    let expanded_ctrl_revealed = Signal::derive_local(move || {
                                        ctrl_revealed.get() && presentation.get().actions_visible()
                                    });
                                    let collapsed_ctrl_revealed = Signal::derive_local(move || {
                                        ctrl_revealed.get() && presentation.get().is_collapsed()
                                    });

                                    let close_shortcut =
                                        Shortcut::new(Key::character("w"), Modifiers::CONTROL);
                                    let corner_close_shortcut = close_shortcut.clone();
                                    let corner_ref = NodeRef::new();
                                    anchor_when(
                                        corner_ref,
                                        corner_close_shortcut,
                                        collapsed_shortcut_target,
                                    );
                                    let close_action = AnyView::new(view! {
                                        IconButton(
                                            icon = IconName::Close,
                                            label = Signal::derive_local(|| i18n::text("files-terminal-end")),
                                            size = zgui_ui::ButtonSize::IconXs,
                                            shortcut = Some(close_shortcut.clone()),
                                            shortcut_enabled = expanded_shortcut_target,
                                            on:click = move |ev: &mut EventCx<'_, events::Click>| {
                                                ev.stop_propagation();
                                                TerminalDock(&c_close_hover)
                                                    .close_terminal_session(id);
                                            },
                                        )
                                    });
                                    let trailing_view = AnyView::new(view! {
                                        HoverAction(
                                            status = AnyView::new(view! { box() }),
                                            action = Some(close_action),
                                            revealed = expanded_ctrl_revealed,
                                        )
                                    });

                                    AnyView::new(view! {
                                        box(class = "gds-files-terminal-row") {
                                            DetailsTooltip(
                                                summary = summary_sig,
                                                details = details_sig,
                                            ) {
                                                NavItem(
                                                    selected = selected,
                                                    presentation = Some(presentation),
                                                    tooltip_enabled = false,
                                                    icon = Some(AnyView::new(view! {
                                                        Icon(
                                                            name = IconName::Terminal,
                                                            size = tokens::SIDEBAR_ICON_SIZE,
                                                        )
                                                    })),
                                                    label = Some(AnyView::new(view! {
                                                        NavItemLabel {
                                                            {move || entry_for_title.display_title()}
                                                        }
                                                    })),
                                                    trailing = Some(trailing_view),
                                                    on_click = Some(UnsyncCallback::new(move |()| {
                                                        if c_select.0.terminals.inner.sealed.get() {
                                                            return;
                                                        }
                                                        let _ = c_select
                                                            .0
                                                            .terminals
                                                            .active
                                                            .try_set(Some(id));
                                                        entry_for_term.terminal.focus();
                                                    })),
                                                    on_middle_click = Some(UnsyncCallback::new(move |()| {
                                                        TerminalDock(&c_middle)
                                                            .close_terminal_session(id);
                                                    })),
                                                    on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                                        if ev.key == Key::Named(NamedKey::F2)
                                                            && ev.modifiers.is_empty()
                                                        {
                                                            TerminalDock(&c_row_key)
                                                                .begin_terminal_rename(id);
                                                            ev.prevent_default();
                                                            ev.stop_propagation();
                                                        } else if close_shortcut.matches(&ev.key, ev.modifiers) {
                                                            TerminalDock(&c_row_key)
                                                                .close_terminal_session(id);
                                                            ev.prevent_default();
                                                            ev.stop_propagation();
                                                        }
                                                    },
                                                ) {
                                                    CornerAction(
                                                        revealed = collapsed_ctrl_revealed,
                                                        button = AnyView::new(view! {
                                                            CornerButton(
                                                                node_ref = Some(corner_ref),
                                                                icon = IconName::Close,
                                                                label = Signal::derive_local(|| i18n::text("files-terminal-end")),
                                                                on:click = move |_| {
                                                                    TerminalDock(&c_close_corner)
                                                                        .close_terminal_session(id);
                                                                },
                                                            )
                                                        }),
                                                    ) {}
                                                }
                                            }
                                        }
                                    })
                                })
                                .collect::<Vec<_>>()
                        }}
                    }

                    column(
                        node_ref = pane_node,
                        class = "gds-files-terminal-pane",
                        {..pane_drop_attrs}
                    ) {
                        {move || {
                            let _ = rev_sig.get();
                            let active_entry = active_sig
                                .get()
                                .and_then(|id| dock_pane.find_session(id));
                            let Some(entry) = active_entry else {
                                let has_cwd = TerminalDock(&c_pane).active_local_directory().is_some();
                                return AnyView::new(view! {
                                    box(class = "gds-files-terminal-empty") {
                                        text {{
                                            i18n::text(if has_cwd {
                                                "files-terminal-empty"
                                            } else {
                                                "files-terminal-cwd-unavailable"
                                            })
                                        }}
                                    }
                                });
                            };

                            let term = entry.terminal.clone();
                            let c_keys = c_pane.clone();
                            let on_key =
                                UnsyncCallback::new(move |(key, modifiers): (Key, Modifiers)| -> bool {
                                    TerminalDock(&c_keys).handle_terminal_shortcut(&key, modifiers)
                                });

                            AnyView::new(view! {
                                TerminalView(
                                    terminal = term,
                                    paint_surface = false,
                                    on_key = on_key,
                                    node_ref = entry.view_ref,
                                    class = "gds-files-terminal-pane__surface",
                                )
                            })
                        }}
                    }
                }
                {rename_dialog_view}
            }
        })
    }
}

#[cfg(test)]
mod tests;
