//! Application and selection menus, and the persisted column-layout editor.
mod pages;

use super::{
    model::{Row, RowKey},
    ui::Browser,
};
use crate::{
    entities::{
        file::columns::{Column, Layout},
        location::{Location, volumes},
    },
    shared::ui::actions::Command,
};
use anti_webui::prelude::*;
use gds::{
    app::Gds,
    i18n,
    ui::{
        self, Icon, IconButton,
        dialog::{Dialog, DialogActions},
        menu::{MenuEntry, MenuItem, menu, menu_item, menu_separator},
    },
};
use gds_uim::el;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct DirectoryTarget {
    tab: u64,
    path: std::path::PathBuf,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SelectionTarget {
    tab: u64,
    location: Location,
    row: RowKey,
    selected: crate::shared::lib::selection::Selection<RowKey>,
}
impl SelectionTarget {
    fn current(browser: &Browser, row: RowKey) -> Self {
        browser.read(|state| Self {
            tab: state.tab().id,
            location: state.tab().navigation.location().clone(),
            row,
            selected: state.tab().navigation.view().selection.clone(),
        })
    }
    fn still_current(&self, browser: &Browser) -> bool {
        browser.read(|state| {
            let tab = state.tab();
            tab.id == self.tab
                && *tab.navigation.location() == self.location
                && tab.rows.iter().any(|row| row.key() == self.row)
                && tab.navigation.view().selection == self.selected
        })
    }
}
impl DirectoryTarget {
    fn current(browser: &Browser) -> Option<Self> {
        browser.read(|state| {
            let tab = state.tab();
            tab.navigation
                .path()
                .filter(|_| !tab.navigation.is_loading())
                .map(|path| Self {
                    tab: tab.id,
                    path: path.to_owned(),
                })
        })
    }
    fn still_current(&self, browser: &Browser) -> bool {
        browser.read(|state| {
            state.tab().id == self.tab && state.tab().navigation.path() == Some(self.path.as_path())
        })
    }
}

#[derive(Clone, Copy)]
enum DirectoryAction {
    NewFolder,
    Paste,
    InternalTerminal,
    Terminal,
    Refresh,
}
impl DirectoryAction {
    fn label(self) -> &'static str {
        match self {
            Self::NewFolder => "new-folder",
            Self::Paste => "paste-files",
            Self::InternalTerminal => "files-terminal-open-here",
            Self::Terminal => "terminal-open-external",
            Self::Refresh => "refresh",
        }
    }
    fn available(self, browser: &Browser, target: &DirectoryTarget) -> bool {
        target.still_current(browser)
            && match self {
                Self::NewFolder | Self::Paste => {
                    !browser.0.volumes.removal_in_progress(&target.path)
                        && !browser.read(|state| {
                            state.operation.busy() || state.tab().navigation.is_loading()
                        })
                        && (!matches!(self, Self::Paste)
                            || crate::shared::lib::clipboard::Clipboard::current().can_paste())
                }
                Self::InternalTerminal => {
                    !browser.0.volumes.removal_in_progress(&target.path)
                        && browser.0.preferences.snapshot().terminal_enabled()
                        && browser.read(|state| state.picker.is_none())
                }
                Self::Terminal => {
                    !browser.0.volumes.removal_in_progress(&target.path)
                        && browser.0.preferences.snapshot().terminal_enabled()
                }
                Self::Refresh => true,
            }
    }
    fn run(self, browser: &Browser, target: DirectoryTarget) {
        if !target.still_current(browser) {
            browser.report_notice(i18n::text("location-changed"));
            return;
        }
        if !self.available(browser, &target) {
            return;
        }
        match self {
            Self::NewFolder => browser.new_folder(),
            Self::Paste => browser.paste(),
            Self::InternalTerminal => browser.new_terminal_at(target.path),
            Self::Terminal => browser.open_external_terminal(target.path),
            Self::Refresh => browser.refresh(),
        }
    }
}

fn command_icon(command: Command) -> Option<Icon> {
    Some(match command {
        Command::NewFolder => Icon::FolderPlus,
        Command::Copy => Icon::Copy,
        Command::Cut => Icon::Cut,
        Command::Paste => Icon::Paste,
        Command::CopyPaths => Icon::ClipboardCopy,
        Command::Open => Icon::ExternalLink,
        Command::OpenInNewTab => Icon::Folder,
        Command::Rename => Icon::TextCursor,
        Command::Trash
        | Command::DeletePermanent
        | Command::PurgeSelection
        | Command::EmptyTrash => Icon::Trash,
        Command::RestoreSelection | Command::Undo => Icon::Undo,
        Command::Redo => Icon::Redo,
        Command::Refresh => Icon::Refresh,
        Command::MountVolume => Icon::Plug,
        Command::UnmountVolume => Icon::Unplug,
        Command::Favorite => Icon::Star,
        Command::Settings => Icon::Settings,
        Command::Help => Icon::Help,
        _ => return None,
    })
}

#[derive(Clone)]
pub(super) enum Surface {
    Closed,
    Application,
    Page,
    Sort,
    Selection {
        target: SelectionTarget,
        at: RectI,
    },
    Directory {
        target: DirectoryTarget,
        at: RectI,
    },
    Trash {
        tab: u64,
        at: RectI,
    },
    Tab {
        id: u64,
        at: RectI,
    },
    Status(super::status::Kind),
    Permissions(super::inspection::PermissionEditor),
    TagEditor(super::inspection::tags::TagEditor),
    Columns {
        location: Location,
        layout: Signal<Layout>,
        scope: Signal<ColumnScope>,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ColumnScope {
    Folder,
    Application,
}

pub(super) struct Menus {
    pub(super) surface: Signal<Surface>,
}
impl Menus {
    pub fn new() -> Self {
        Self {
            surface: signal(Surface::Closed),
        }
    }
    pub fn is_open(&self) -> bool {
        !matches!(self.surface.get_untracked(), Surface::Closed)
    }
    pub(super) fn close(&self) {
        match self.surface.get_untracked() {
            Surface::Permissions(editor) => editor.cancel(),
            Surface::TagEditor(editor) if editor.is_saving() => return,
            _ => {}
        }
        self.surface.set(Surface::Closed);
    }
}

impl Browser {
    pub(super) fn menu_key(&self, event: &KeyboardEvent) -> bool {
        if !self.0.menus.is_open() {
            return false;
        }
        event.stop_propagation();
        if event.key == Key::Named(NamedKey::Escape) {
            event.prevent_default();
            self.0.menus.close();
        }
        true
    }

    fn menu_action(&self, label: &'static str, action: impl Fn(&Browser) + 'static) -> MenuItem {
        let weak = self.downgrade();
        menu_item(i18n::text(label)).on_click(move || {
            if let Some(browser) = weak.upgrade() {
                browser.0.owner.with(|| action(&browser));
            }
        })
    }
    fn command_item(&self, command: Command, action: impl Fn(&Browser) + 'static) -> MenuEntry {
        let item = self
            .menu_action(command.label_id(), action)
            .shortcut(command.default_stroke());
        match command_icon(command) {
            Some(icon) => item.icon(icon).into(),
            None => item.into(),
        }
    }
    fn paste_item(&self) -> MenuEntry {
        self.menu_action(Command::Paste.label_id(), Browser::paste)
            .icon(Icon::Paste)
            .disabled_when(|| !crate::shared::lib::clipboard::Clipboard::current().can_paste())
            .shortcut(Command::Paste.default_stroke())
            .into()
    }
    fn app_entries(&self) -> Vec<MenuEntry> {
        let mut entries = vec![
            self.command_item(Command::NewWindow, |browser| {
                let path = browser.read(|state| {
                    state
                        .tab()
                        .navigation
                        .path()
                        .unwrap_or(&state.home)
                        .to_owned()
                });
                (browser.0.new_window)(path);
            }),
            self.command_item(Command::Settings, Browser::show_settings),
            self.command_item(Command::Help, Browser::show_help),
        ];
        if Gds::current().ui().logs().is_enabled() {
            entries.push(
                self.menu_action("logs-title", |_| {
                    ui::logs::open();
                })
                .shortcut("Ctrl+Shift+L")
                .into(),
            );
        }
        entries.push(self.menu_action("about", Browser::show_about).into());
        entries
    }
    pub(super) fn app_menu(&self) -> Div {
        let browser = self.clone();
        let entries = self.clone();
        el! { .div.{relative}.[
            $(IconButton::application("application-menu", ui::about::Application::cargo("gds-files").artwork())).{
                label_fn: () => $(i18n::text("app-menu"));
                on_click: move _ => $(browser.show_app_menu());
            },
            $(move || {
                let shown = entries.clone();
                let close = entries.clone();
                menu(
                    move || matches!(shown.0.menus.surface.get(), Surface::Application),
                    move || close.0.menus.close(),
                    entries.app_entries(),
                )
            }).dyn_view,
        ] }
    }
    pub(super) fn show_app_menu(&self) {
        self.0.menus.surface.update(|surface| {
            *surface = if matches!(surface, Surface::Application) {
                Surface::Closed
            } else {
                Surface::Application
            };
        });
    }
    pub(super) fn page_menu(&self) -> Div {
        let browser = self.clone();
        let entries = self.clone();
        el! { .div.{relative}.[
            "page-tools".IconButton::new.{
                icon: Icon::More;
                label_fn: () => $(i18n::text("page-tools"));
                on_click: move _ => $(browser.0.menus.surface.update(|surface| {
                    *surface = if matches!(surface, Surface::Page) { Surface::Closed } else { Surface::Page };
                }));
            },
            $(move || {
                let shown = entries.clone();
                let close = entries.clone();
                menu(
                    move || matches!(shown.0.menus.surface.get(), Surface::Page),
                    move || close.0.menus.close(),
                    entries.page_entries(),
                )
            }).dyn_view,
        ] }
    }
    fn page_entries(&self) -> Vec<MenuEntry> {
        if let Some(entries) = self.special_page_entries() {
            return entries;
        }
        let grid = self.read(|state| state.tab().mode == crate::entities::file::ViewMode::Grid);
        let mut entries = vec![
            self.menu_action(
                if grid { "view-list" } else { "view-grid" },
                Browser::toggle_view,
            )
            .icon(if grid { Icon::List } else { Icon::Grid })
            .shortcut(Command::ViewMode.default_stroke())
            .into(),
        ];
        {
            let favorite =
                self.read(|state| state.places.is_favorite(state.tab().navigation.location()));
            entries.push(
                self.menu_action(
                    if favorite {
                        "favorite-remove"
                    } else {
                        "favorite-add"
                    },
                    Browser::favorite,
                )
                .checked_when(move || favorite)
                .shortcut(Command::Favorite.default_stroke())
                .into(),
            );
            let columns = self.configurable_columns();
            entries.push(
                self.menu_action("columns-configure", Browser::configure_columns)
                    .disabled_when(move || grid || !columns)
                    .shortcut(Command::Columns.default_stroke())
                    .into(),
            );
            let hidden = self.clone();
            entries.push(
                self.menu_action(Command::Hidden.label_id(), |browser| {
                    let visible = browser.0.preferences.snapshot().files.show_hidden;
                    browser
                        .edit_preferences(|preferences| preferences.files.show_hidden = !visible);
                    browser.refresh();
                })
                .checked_when(move || hidden.0.preferences.snapshot().files.show_hidden)
                .shortcut(Command::Hidden.default_stroke())
                .into(),
            );
            let details = self.clone();
            entries.push(
                self.menu_action(Command::Details.label_id(), Browser::toggle_details)
                    .disabled_when(move || details.compact())
                    .shortcut(Command::Details.default_stroke())
                    .into(),
            );
        }
        entries.push(self.command_item(Command::Refresh, Browser::refresh));
        let sort = self.sort_entries();
        if !sort.is_empty() {
            entries.push(MenuEntry::Submenu(
                i18n::text("sort"),
                std::rc::Rc::new(sort),
            ));
        }
        entries
    }
    pub(super) fn show_context_menu(&self) {
        let Some(viewport) = self.0.list.rect() else {
            return;
        };
        let (target, grid) = self.read(|state| {
            let tab = state.tab();
            let target = tab
                .navigation
                .view()
                .cursor
                .as_ref()
                .filter(|key| tab.navigation.view().selection.contains(key))
                .and_then(|key| tab.rows.iter().position(|row| row.key() == *key))
                .or_else(|| {
                    tab.rows
                        .iter()
                        .position(|row| tab.navigation.view().selection.contains(&row.key()))
                })
                .map(|index| (index, tab.rows[index].key()));
            (target, tab.mode == crate::entities::file::ViewMode::Grid)
        });
        if let Some((index, target)) = target {
            let tile = if grid {
                self.grid_geometry().tile(index)
            } else {
                let row = gds::appearance::tokens::DataListStyle::ROW_HEIGHT;
                RectF::new(0.0, row * index as f32, 0.0, row)
            };
            let scroll = self.0.list.scroll_offset();
            let x = (tile.x - scroll.x + 11.0).clamp(0.0, viewport.width.max(0) as f32);
            let y = (tile.bottom() - scroll.y).clamp(0.0, viewport.height.max(0) as f32);
            self.0.menus.surface.set(Surface::Selection {
                target: SelectionTarget::current(self, target),
                at: RectI::new(viewport.x + x as i32, viewport.y + y as i32, 1, 1),
            });
        } else if let Some(target) = DirectoryTarget::current(self) {
            self.0.menus.surface.set(Surface::Directory {
                target,
                at: RectI::new(viewport.x + 11, viewport.y + 11, 1, 1),
            });
        }
    }
    pub(super) fn row_menu(&self, target: Div, index: usize) -> Div {
        let row = self.read(|state| state.tab().rows.get(index).cloned());
        let Some(row) = row else { return target };
        let clicked = row.key();
        let open = self.clone();
        el! { target.{on_pointer_down: move event => ${
            if event.button == Some(PointerButton::Secondary) {
                event.stop_propagation();
                let index = open.read(|state| {
                    state.tab().rows.iter().position(|row| row.key() == clicked)
                });
                if let Some(index) = index {
                    open.select_context(index);
                    open.0.menus.surface.set(Surface::Selection {
                        target: SelectionTarget::current(&open, clicked.clone()),
                        at: RectI::new(event.position.x as i32, event.position.y as i32, 1, 1),
                    });
                }
            }
        }} }
    }
    pub(super) fn sidebar_volume_menu(&self, target: Div, volume: &volumes::Volume) -> Div {
        ui::menu::context_menu(target, self.selection_entries(&Row::Volume(volume.clone())))
    }
    pub(super) fn directory_menu(&self, target: Div) -> Div {
        let open = self.clone();
        el! { target.{on_pointer_down: move event => ${
            if event.button != Some(PointerButton::Secondary) {
                return;
            }
            let at = RectI::new(event.position.x as i32, event.position.y as i32, 1, 1);
            if let Some(tab) = open.read(|state| {
                matches!(state.tab().navigation.location(), Location::Trash).then_some(state.tab().id)
            }) {
                event.stop_propagation();
                open.0.menus.surface.set(Surface::Trash { tab, at });
            } else if let Some(target) = DirectoryTarget::current(&open) {
                event.stop_propagation();
                open.0.menus.surface.set(Surface::Directory { target, at });
            }
        }} }
    }
    fn directory_entries(&self, target: DirectoryTarget) -> Vec<MenuEntry> {
        let item = |action: DirectoryAction, icon: Icon, command: Option<Command>| {
            let available = self.clone();
            let browser = self.clone();
            let target = target.clone();
            let enabled = target.clone();
            let item = menu_item(i18n::text(action.label()))
                .icon(icon)
                .disabled_when(move || !action.available(&available, &enabled))
                .on_click(move || {
                    browser
                        .0
                        .owner
                        .with(|| action.run(&browser, target.clone()));
                });
            match command {
                Some(command) => item.shortcut(command.default_stroke()).into(),
                None => item.into(),
            }
        };
        let mut entries = vec![
            item(
                DirectoryAction::NewFolder,
                Icon::FolderPlus,
                Some(Command::NewFolder),
            ),
            menu_separator(),
            item(DirectoryAction::Paste, Icon::Paste, Some(Command::Paste)),
            menu_separator(),
        ];
        if self.0.preferences.snapshot().terminal_enabled() {
            if self.read(|state| state.picker.is_none()) {
                entries.push(item(
                    DirectoryAction::InternalTerminal,
                    Icon::Terminal,
                    None,
                ));
            }
            entries.push(item(DirectoryAction::Terminal, Icon::Terminal, None));
        }
        entries.push(item(
            DirectoryAction::Refresh,
            Icon::Refresh,
            Some(Command::Refresh),
        ));
        entries
    }
    pub(super) fn tab_menu(&self, target: Div, id: u64) -> Div {
        let open = self.clone();
        el! { target.{on_pointer_down: move event => ${
            if event.button == Some(PointerButton::Secondary) {
                event.stop_propagation();
                open.0.menus.surface.set(Surface::Tab {
                    id, at: RectI::new(event.position.x as i32, event.position.y as i32, 1, 1),
                });
            }
        }} }
    }
    fn tab_entries(&self, id: u64, pinned: bool) -> Vec<MenuEntry> {
        let pin = self.clone();
        let close = self.clone();
        let add = self.clone();
        vec![
            menu_item(i18n::text(if pinned { "unpin-tab" } else { "pin-tab" }))
                .on_click(move || {
                    if let Some(index) =
                        pin.read(|state| state.tabs.iter().position(|tab| tab.id == id))
                    {
                        pin.0.owner.with(|| pin.toggle_pin(index));
                    }
                })
                .into(),
            menu_item(i18n::text(Command::NewTab.label_id()))
                .shortcut(Command::NewTab.default_stroke())
                .on_click(move || add.0.owner.with(|| add.new_tab(None)))
                .into(),
            menu_item(i18n::text(Command::CloseTab.label_id()))
                .shortcut(Command::CloseTab.default_stroke())
                .on_click(move || {
                    if let Some(index) =
                        close.read(|state| state.tabs.iter().position(|tab| tab.id == id))
                    {
                        close.0.owner.with(|| close.close_tab(index));
                    }
                })
                .into(),
        ]
    }
    fn selection_entries(&self, row: &Row) -> Vec<MenuEntry> {
        match row {
            Row::Trash(_) => self.trash_entries(true),
            Row::Volume(volume) => {
                let busy =
                    self.0.volumes.is_busy(&volume.id) || self.read(|state| state.operation.busy());
                let rename = volume.clone();
                let mut items = Vec::new();
                if !volume.mount_points.is_empty() {
                    let open = volume.id.clone();
                    let new_tab = open.clone();
                    items.push(
                        self.menu_action("open", move |browser| browser.open_volume(&open, false))
                            .icon(Icon::Folder)
                            .shortcut(Command::Open.default_stroke())
                            .disabled_when(move || busy)
                            .into(),
                    );
                    items.push(
                        self.menu_action(Command::OpenInNewTab.label_id(), move |browser| {
                            browser.open_volume(&new_tab, true)
                        })
                        .icon(Icon::Folder)
                        .shortcut(Command::OpenInNewTab.default_stroke())
                        .disabled_when(move || busy)
                        .into(),
                    );
                }
                if volume.can_set_label() {
                    items.push(
                        self.menu_action("rename", move |browser| {
                            browser.rename_volume(rename.clone())
                        })
                        .icon(Icon::TextCursor)
                        .shortcut(Command::Rename.default_stroke())
                        .disabled_when(move || busy)
                        .into(),
                    );
                }
                if volume.mount_points.is_empty() {
                    let id = volume.id.clone();
                    let enabled = volume.mountable && !volume.locked && !busy;
                    items.push(
                        self.menu_action("device-mount", move |browser| {
                            browser.change_volume(volumes::DeviceAction::Mount(id.clone()))
                        })
                        .icon(Icon::Plug)
                        .shortcut(Command::MountVolume.default_stroke())
                        .disabled_when(move || !enabled)
                        .into(),
                    );
                } else if volume.can_unmount() {
                    let id = volume.id.clone();
                    items.push(
                        self.menu_action("device-unmount", move |browser| {
                            browser.change_volume(volumes::DeviceAction::Unmount(id.clone()))
                        })
                        .icon(Icon::Unplug)
                        .shortcut(Command::UnmountVolume.default_stroke())
                        .disabled_when(move || busy)
                        .icon(Icon::Unplug)
                        .disabled_when(move || busy)
                        .into(),
                    );
                }
                let snapshot = self.0.volumes.snapshot();
                if let Some(drive) = volume.drive.as_ref().and_then(|id| snapshot.drive(id)) {
                    if drive.can_eject {
                        let volume = volume.clone();
                        items.push(
                            self.menu_action("device-eject", move |browser| {
                                browser
                                    .change_volume(volumes::DeviceAction::Eject(volume.id.clone()))
                            })
                            .icon(Icon::Unplug)
                            .disabled_when(move || busy)
                            .into(),
                        );
                    }
                    if drive.can_power_off {
                        let volume = volume.clone();
                        items.push(
                            self.menu_action("device-power-off", move |browser| {
                                browser.change_volume(volumes::DeviceAction::PowerOff(
                                    volume.id.clone(),
                                ))
                            })
                            .into(),
                        );
                    }
                }
                items
            }
            Row::Tag(_) => self.place_entries(row),
            Row::Remote(_) => vec![
                self.command_item(Command::Open, Browser::open_selected),
                self.command_item(Command::OpenInNewTab, |browser| browser.open_selected_tab()),
                self.command_item(Command::Copy, |browser| browser.copy(false)),
                self.paste_item(),
                self.menu_action("remote-download", Browser::download_selected_remote)
                    .into(),
                self.command_item(Command::Rename, Browser::rename),
                self.command_item(Command::DeletePermanent, |browser| browser.delete(true)),
                menu_separator(),
                self.command_item(Command::Favorite, Browser::favorite),
                self.menu_action("remote-disconnect", Browser::disconnect_remote)
                    .into(),
            ],
            Row::Place(_) => self.place_entries(row),
            Row::File(_)
                if self.read(|state| {
                    matches!(state.tab().navigation.location(), Location::Tags(_))
                }) =>
            {
                self.place_entries(row)
            }
            Row::File(entry) => self.file_entries(entry),
        }
    }

    fn trash_entries(&self, selection: bool) -> Vec<MenuEntry> {
        let (count, busy, has_rows) = self.read(|state| {
            let tab = state.tab();
            (tab.navigation.view().selection.len(), state.operation.busy() || tab.navigation.is_loading(),
                matches!(&tab.contents, super::model::Contents::Trash(entries) if !entries.is_empty()))
        });
        let mut entries = Vec::new();
        if selection {
            let restore = if count == 1 {
                i18n::text("recycle-restore")
            } else {
                i18n::count("recycle-restore-selected", count)
            };
            for (label, icon, command, purge) in [
                (restore, Icon::Undo, Command::RestoreSelection, false),
                (
                    i18n::count("recycle-purge-selected", count),
                    Icon::Trash,
                    Command::PurgeSelection,
                    true,
                ),
            ] {
                let weak = self.downgrade();
                entries.push(
                    menu_item(label)
                        .icon(icon)
                        .shortcut(command.default_stroke())
                        .disabled_when(move || busy || count == 0)
                        .on_click(move || {
                            if let Some(browser) = weak.upgrade() {
                                browser.restore_trash(purge, false);
                            }
                        })
                        .into(),
                );
            }
            entries.push(menu_separator());
        }
        entries.extend([
            self.command_item(Command::Refresh, Browser::refresh),
            menu_separator(),
            self.menu_action("recycle-empty-action", |browser| {
                browser.restore_trash(true, true)
            })
            .icon(Icon::Trash)
            .shortcut(Command::EmptyTrash.default_stroke())
            .disabled_when(move || busy || !has_rows)
            .into(),
        ]);
        entries
    }

    pub(super) fn place_location(row: &Row) -> Option<Location> {
        match row {
            Row::Place(location) => Some(location.clone()),
            Row::File(entry) => Some(Location::from(entry.path.as_path())),
            Row::Tag(tag) => crate::entities::location::scheme::TagAddress::new(tag)
                .ok()
                .map(Location::Tags),
            _ => None,
        }
    }

    fn place_entries(&self, row: &Row) -> Vec<MenuEntry> {
        let Some(location) = Self::place_location(row) else {
            return Vec::new();
        };
        let local = location.local_path();
        let favorite = self.read(|state| state.places.is_favorite(&location));
        let recent =
            self.read(|state| matches!(state.tab().navigation.location(), Location::Recent));
        let target = location.clone();
        let new_tab = location.clone();
        let mut entries = vec![
            self.command_item(Command::Open, move |browser| browser.open(target.clone())),
            self.command_item(Command::OpenInNewTab, move |browser| {
                browser.new_tab(Some(new_tab.clone()))
            }),
        ];
        if local.is_some() {
            entries.push(
                self.menu_action("tags-show-parent", Browser::show_selected_parent)
                    .into(),
            );
        }
        entries.push(menu_separator());
        if local.is_some() && !recent {
            entries.push(self.command_item(Command::Copy, |browser| browser.copy(false)));
        }
        entries.push(self.command_item(Command::CopyPaths, Browser::copy_selected_paths));
        entries.push(menu_separator());
        let saved = location.clone();
        entries.push(
            self.menu_action(
                if favorite {
                    "place-favorite-remove"
                } else {
                    "place-favorite-add"
                },
                move |browser| {
                    browser.update(|state| state.places.toggle_favorite(saved.clone()));
                    browser.save_places();
                    browser.refresh();
                },
            )
            .shortcut(Command::Favorite.default_stroke())
            .into(),
        );
        if local.is_some() && self.0.preferences.snapshot().tags.enabled {
            entries.push(
                self.menu_action("tags-edit", Browser::edit_selected_tags)
                    .into(),
            );
        }
        if matches!(row, Row::Tag(_)) {
            let tag = row.clone();
            entries.push(
                self.menu_action("tags-delete", move |browser| {
                    browser.delete_tags(vec![tag.clone()])
                })
                .into(),
            );
        }
        if !recent {
            entries.push(menu_separator());
            entries.push(self.command_item(Command::Refresh, Browser::refresh));
        }
        entries
    }

    fn file_entries(&self, entry: &crate::entities::file::directory::Entry) -> Vec<MenuEntry> {
        use crate::entities::file::ui::FileIcon;
        let (selected, picker, busy) = self.read(|state| {
            (
                state.tab().selected(),
                state.picker.is_some(),
                state.operation.busy() || state.tab().navigation.is_loading(),
            )
        });
        let many = selected.len() > 1;
        let count = selected.len();
        let unavailable = selected
            .iter()
            .filter_map(Row::path)
            .any(|path| self.0.volumes.removal_in_progress(path));
        let directories = selected
            .iter()
            .all(|row| matches!(row, Row::File(file) if FileIcon::is_directory(file)));
        let action = |command: Command, plural: &'static str, run: fn(&Browser), disabled: bool| {
            let label = if many {
                i18n::count(plural, count)
            } else if command == Command::CopyPaths {
                i18n::text("copy-path")
            } else {
                i18n::text(command.label_id())
            };
            let weak = self.downgrade();
            let item = menu_item(label)
                .shortcut(command.default_stroke())
                .disabled_when(move || disabled || (command != Command::CopyPaths && unavailable))
                .on_click(move || {
                    if let Some(browser) = weak.upgrade() {
                        browser.0.owner.with(|| run(&browser));
                    }
                });
            match command_icon(command) {
                Some(icon) => item.icon(icon).into(),
                None => item.into(),
            }
        };
        let mut entries = Vec::new();
        if !many {
            entries.push(action(Command::Open, "open", Browser::open_selected, false));
            if !directories && !picker {
                entries.push(
                    self.menu_action("open-with", Browser::open_selected_with)
                        .icon(Icon::ExternalLink)
                        .disabled_when(move || unavailable)
                        .into(),
                );
            }
        }
        if directories {
            entries.push(action(
                Command::OpenInNewTab,
                "open-directories-new-tabs",
                Browser::open_selected_tab,
                false,
            ));
        }
        if !entries.is_empty() {
            entries.push(menu_separator());
        }
        entries.push(action(
            Command::Copy,
            "copy-selected-files",
            |browser| browser.copy(false),
            busy,
        ));
        entries.push(action(
            Command::Cut,
            "cut-selected-files",
            |browser| browser.copy(true),
            busy,
        ));
        if !many {
            let can_paste = self.clone();
            entries.push(
                self.menu_action("paste-files", Browser::paste)
                    .icon(Icon::Paste)
                    .shortcut(Command::Paste.default_stroke())
                    .disabled_when(move || {
                        busy || can_paste.read(|state| {
                            state
                                .tab()
                                .navigation
                                .path()
                                .is_none_or(|path| can_paste.0.volumes.removal_in_progress(path))
                        }) || !crate::shared::lib::clipboard::Clipboard::current().can_paste()
                    })
                    .into(),
            );
        }
        entries.push(menu_separator());
        entries.push(action(
            Command::CopyPaths,
            "copy-selected-paths",
            Browser::copy_selected_paths,
            selected
                .iter()
                .any(|row| row.path().is_some_and(|p| p.to_str().is_none())),
        ));
        entries.push(menu_separator());
        if !many {
            entries.push(action(
                Command::Rename,
                "rename",
                Browser::rename,
                busy || !cfg!(target_os = "linux") || entry.path.file_name().is_none(),
            ));
            if self.0.preferences.snapshot().tags.enabled {
                entries.push(
                    self.menu_action("tags-edit", Browser::edit_selected_tags)
                        .icon(Icon::Tag)
                        .disabled_when(move || busy || unavailable)
                        .into(),
                );
            }
            #[cfg(unix)]
            {
                let entry = entry.clone();
                entries.push(
                    self.menu_action("permissions-title", move |browser| {
                        browser.edit_permissions(entry.clone())
                    })
                    .icon(Icon::Lock)
                    .disabled_when(move || busy || unavailable)
                    .into(),
                );
            }
            entries.push(menu_separator());
        }
        entries.push(action(
            Command::Trash,
            "trash-selected",
            |browser| browser.delete(false),
            busy,
        ));
        entries.push(action(
            Command::DeletePermanent,
            "delete-selected",
            |browser| browser.delete(true),
            busy,
        ));
        if !many && self.0.preferences.snapshot().terminal_enabled() {
            let target = if directories {
                Some(entry.path.as_path())
            } else {
                entry.path.parent()
            };
            if let Some(target) = target {
                entries.push(menu_separator());
                if !picker {
                    let path = target.to_owned();
                    entries.push(
                        self.menu_action("files-terminal-open-here", move |browser| {
                            browser.new_terminal_at(path.clone())
                        })
                        .icon(Icon::Terminal)
                        .disabled_when(move || unavailable)
                        .into(),
                    );
                }
                let path = target.to_owned();
                entries.push(
                    self.menu_action("terminal-open-external", move |browser| {
                        browser.open_external_terminal(path.clone())
                    })
                    .icon(Icon::Terminal)
                    .disabled_when(move || unavailable)
                    .into(),
                );
            }
        }
        entries
    }

    fn open_external_terminal(&self, path: std::path::PathBuf) {
        self.blocking(
            "Open external terminal",
            move || {
                gds::service::desktop::Desktop::default()
                    .open_terminal(&path)
                    .map_err(|error| error.to_string())
            },
            |browser, result| {
                if let Err(error) = result {
                    browser.report_notice(error);
                }
            },
        );
    }
    pub(super) fn open_selected_tab(&self) {
        for row in self.read(|state| state.tab().selected()) {
            let location = match row {
                Row::File(entry) if crate::entities::file::ui::FileIcon::is_directory(&entry) => {
                    Location::from(entry.path.as_path())
                }
                Row::Place(location) => location,
                Row::Remote(entry) if entry.kind == gds::service::remote::EntryKind::Directory => {
                    Location::Remote(entry.address)
                }
                Row::Volume(volume) if !volume.mount_points.is_empty() => {
                    self.open_volume(&volume.id, true);
                    continue;
                }
                _ => continue,
            };
            self.new_tab(Some(location));
        }
    }
    fn open_selected_with(&self) {
        let paths = self.read(|state| {
            state
                .tab()
                .selected()
                .iter()
                .filter_map(|row| row.path().map(|path| path.to_owned()))
                .collect::<Vec<_>>()
        });
        for path in paths {
            self.open_file(path, gds::service::desktop::OpenMode::ChooseApplication);
        }
    }
    fn show_selected_parent(&self) {
        let paths = self.read(|state| {
            state
                .tab()
                .selected()
                .iter()
                .filter_map(|row| {
                    match row {
                        Row::Place(location) => location.local_path(),
                        _ => row.path(),
                    }
                    .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        });
        self.apply_desktop_command(gds::service::desktop::file_manager::Command {
            request: gds::service::desktop::file_manager::Request::ShowItems { paths },
            startup_id: String::new(),
        });
    }
    fn edit_selected_tags(&self) {
        let paths = self.read(|state| {
            state
                .tab()
                .selected()
                .iter()
                .filter_map(|row| {
                    match row {
                        Row::Place(location) => location.local_path(),
                        _ => row.path(),
                    }
                    .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        });
        for path in paths {
            self.edit_tags(path);
        }
    }
    pub(super) fn copy_selected_paths(&self) {
        use crate::features::file_operations::{Batch, Error};

        let (rows, collection) = self.read(|state| {
            let tab = state.tab();
            (
                tab.selected(),
                matches!(
                    tab.navigation.location(),
                    Location::Favorites | Location::Recent | Location::Tags(_)
                ),
            )
        });
        if collection {
            let result = rows
                .iter()
                .filter_map(Self::place_location)
                .map(|location| location.uri())
                .collect::<Result<Vec<_>, _>>();
            match result {
                Ok(uris) if !uris.is_empty() => self
                    .0
                    .owner
                    .with(|| set_clipboard(ClipboardKind::Clipboard, uris.join("\n"))),
                Ok(_) => {}
                Err(error) => self.report_notice(error.to_string()),
            }
            return;
        }
        let paths = rows
            .iter()
            .filter_map(|row| row.path().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        let text = if paths.is_empty() {
            rows.into_iter()
                .filter_map(|row| match row {
                    Row::Remote(entry) => Some(Ok(entry.address.uri().to_owned())),
                    Row::Tag(_) => {
                        Self::place_location(&row).map(|location| Ok(location.display_address()))
                    }
                    Row::Place(location) => Some(match location.local_path() {
                        Some(path) => path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
                            Error::NonUtf8Clipboard {
                                path: path.to_owned(),
                            }
                        }),
                        None => Ok(location.display_address()),
                    }),
                    _ => None,
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|addresses| addresses.join("\n"))
        } else {
            Batch::new(paths).and_then(|batch| batch.clipboard_text())
        };
        match text {
            Ok(text) if !text.is_empty() => self
                .0
                .owner
                .with(|| set_clipboard(ClipboardKind::Clipboard, text)),
            Ok(_) => {}
            Err(error) => self.report_notice(error.to_string()),
        }
    }
    pub(super) fn change_volume(&self, action: volumes::DeviceAction) {
        if self.read(|state| state.operation.busy()) {
            self.report_notice(i18n::text("remote-task-running"));
            return;
        }
        let weak = self.downgrade();
        self.0.owner.with(|| match self.0.volumes.operate(action) {
            Ok(task) => task.then(move |result| {
                if let Some(browser) = weak.upgrade() {
                    if let Err(error) = result {
                        browser.report_notice(error.to_string());
                    }
                    browser.refresh();
                }
            }),
            Err(error) => self.report_notice(error.to_string()),
        });
    }
    pub(super) fn show_help(&self) {
        self.open_page(super::model::Pane::Help);
    }
    pub(super) fn show_about(&self) {
        self.0.prompt.set(super::model::Prompt::About);
    }
    pub(super) fn column_menu(&self) -> Div {
        let browser = self.clone();
        el! {.div.[$(move||browser.column_controls()).dyn_view]}
    }
    fn column_controls(&self) -> Div {
        let columns = self.clone();
        let sort = self.clone();
        let shown = self.clone();
        let close = self.clone();
        let configurable = self.configurable_columns();
        let grid = self.read(|state| state.tab().mode == crate::entities::file::ViewMode::Grid);
        let columns_available = self.sortable_columns();
        let sortable = !columns_available.is_empty();
        let entries = self.sort_entries();
        el! { .div.{flex; gap: 8.0_f32}.[
            if sortable [
                .div.{relative}.[
                    "sort".IconButton::new.{
                        icon: Icon::Sort;
                        label: $(i18n::text("sort"));
                        keytip: $(Command::Sort.default_stroke());
                        on_click: move _ => $(sort.show_sort_menu());
                    },
                    $(menu(
                        move || matches!(shown.0.menus.surface.get(), Surface::Sort),
                        move || close.0.menus.close(),
                        entries,
                    )),
                ],
            ],
            if configurable [
                "configure-columns".IconButton::new.{
                    icon: Icon::Columns;
                    label: $(i18n::text("columns-configure"));
                    disabled: grid;
                    keytip: $(Command::Columns.default_stroke());
                    on_click: move _ => $(columns.configure_columns());
                },
            ],
        ] }
    }
    fn sort_entries(&self) -> Vec<MenuEntry> {
        self.sortable_columns()
            .into_iter()
            .map(|column| {
                let browser = self.clone();
                let checked = self.clone();
                menu_item(i18n::text(column.label_id()))
                    .checked_when(move || checked.read(|state| state.tab().sort.key == column))
                    .on_click(move || browser.sort_by(column))
                    .into()
            })
            .collect()
    }
    pub(super) fn show_sort_menu(&self) {
        if self.sortable_columns().is_empty() {
            return;
        }
        self.0.menus.surface.set(Surface::Sort);
    }
    pub(super) fn configure_columns(&self) {
        if !self.configurable_columns()
            || self.read(|state| state.tab().mode == crate::entities::file::ViewMode::Grid)
        {
            return;
        }
        let (location, layout) = self.read(|state| {
            (
                state.tab().navigation.location().clone(),
                state.tab().columns.clone(),
            )
        });
        self.0.owner.with(|| {
            self.0.menus.surface.set(Surface::Columns {
                location,
                layout: signal(layout),
                scope: signal(ColumnScope::Folder),
            })
        });
    }
    fn configurable_columns(&self) -> bool {
        self.read(|state| {
            matches!(
                state.tab().contents,
                super::model::Contents::Directory | super::model::Contents::Tags { .. }
            )
        })
    }
    pub(super) fn menu_overlay(&self) -> impl IntoView {
        let origin = self.clone();
        effect(move || {
            let valid = match origin.0.menus.surface.get() {
                Surface::Selection { target, .. } => target.still_current(&origin),
                Surface::Directory { target, .. } => target.still_current(&origin),
                Surface::Trash { tab, .. } => origin.read(|state| {
                    state.tab().id == tab
                        && matches!(state.tab().navigation.location(), Location::Trash)
                }),
                Surface::Tab { id, .. } => {
                    origin.read(|state| state.tabs.iter().any(|tab| tab.id == id))
                }
                _ => true,
            };
            if !valid {
                origin.0.menus.close();
            }
        });
        let visible = self.clone();
        let close = self.clone();
        let content = self.clone();
        let context = self.clone();
        let permissions = self.clone();
        let dismiss_permissions = self.clone();
        let tags = self.clone();
        let dismiss_tags = self.clone();
        el! { .div.[
            $(move || context.context_popup()).dyn_view,
            $(Dialog::show(
                move || matches!(visible.0.menus.surface.get(), Surface::Columns { .. }),
                move || close.0.menus.close(),
                move || content.columns_dialog(),
            )),
            $(Dialog::show_alert(
                {
                    let browser = permissions.clone();
                    move || matches!(browser.0.menus.surface.get(), Surface::Permissions(_))
                },
                move || dismiss_permissions.close_permissions(),
                move || permissions.permissions_dialog(),
            )),
            $(Dialog::show_alert(
                {
                    let browser = tags.clone();
                    move || matches!(browser.0.menus.surface.get(), Surface::TagEditor(_))
                },
                move || dismiss_tags.0.menus.close(),
                move || tags.tags_dialog(),
            )),
        ] }
    }
    fn context_popup(&self) -> View {
        let surface = self.0.menus.surface.get();
        let Some((at, entries)) = untrack(|| match surface {
            Surface::Selection { target, at } => self
                .read(|state| {
                    state
                        .tab()
                        .rows
                        .iter()
                        .find(|row| row.key() == target.row)
                        .cloned()
                })
                .map(|row| (at, self.selection_entries(&row))),
            Surface::Directory { target, at } => Some((at, self.directory_entries(target))),
            Surface::Trash { at, .. } => Some((at, self.trash_entries(false))),
            Surface::Tab { id, at } => self
                .read(|state| {
                    state
                        .tabs
                        .iter()
                        .find(|tab| tab.id == id)
                        .map(|tab| tab.pinned)
                })
                .map(|pinned| (at, self.tab_entries(id, pinned))),
            _ => None,
        }) else {
            return div().into_view();
        };
        let close = self.clone();
        ui::menu::menu_with_options(
            || true,
            move || close.0.menus.close(),
            ui::menu::MenuOptions::new().at(signal(Some(at))),
            entries,
        )
        .into_view()
    }
    fn columns_dialog(&self) -> View {
        let Surface::Columns {
            location,
            layout,
            scope,
        } = self.0.menus.surface.get()
        else {
            return div().into_view();
        };
        let browser = self.clone();
        let defaults = self.clone();
        let restore = location.clone();
        let cancel = self.clone();
        let apply = location.clone();
        let actions = el! { .DialogActions::default.[
            $(i18n::text("dialog-cancel")).button.{on_click: move _ => $(cancel.0.menus.close())},
            $(i18n::text("columns-apply")).button.{
                on_click: move _ => $(browser.apply_columns(
                    apply.clone(), Some(layout.get_untracked()), scope.get_untracked(),
                ));
            },
        ] };
        el! { .Dialog::default.{title: $(i18n::text("columns-title")); w: 520.0_f32; actions: actions}.[
            $(location.display_address()).text.{text_size: 12.0_f32; text_color: $(theme().text_muted)},
            $(select(scope, vec![
                (ColumnScope::Folder, i18n::text("columns-scope-folder")),
                (ColumnScope::Application, i18n::text("columns-scope-global")),
            ])),
            $(move || {
                let current = layout.get();
                let visible = current.columns().iter().copied().enumerate().map(|(index, column)| {
                    let enabled = Binding::new(
                        move || layout.with(|layout| layout.contains(column)),
                        move |visible| layout.update(|layout| { layout.set_visible(column, visible); }),
                    );
                    el! { .div.{flex; items_center; gap: 8.0_f32}.[
                        $(checkbox(enabled, i18n::text(column.label_id()))).{
                            flex_1;
                            disabled: move () => $(column.is_required());
                        },
                        "column-up".IconButton::new.{
                            icon: Icon::Up;
                            label: $(i18n::text("columns-move-up"));
                            disabled: $(index == 0);
                            on_click: move _ => $(layout.update(|layout| {
                                layout.move_to(column, index.saturating_sub(1));
                            }));
                        },
                        "column-down".IconButton::new.{
                            icon: Icon::ChevronDown;
                            label: $(i18n::text("columns-move-down"));
                            disabled: $(index + 1 == current.columns().len());
                            on_click: move _ => $(layout.update(|layout| {
                                layout.move_to(column, index + 1);
                            }));
                        },
                    ] }
                }).collect::<Vec<_>>();
                let hidden = Column::ALL.into_iter().filter(|column| !current.contains(*column)).map(|column| {
                    let value = Binding::new(
                        move || layout.with(|layout| layout.contains(column)),
                        move |visible| layout.update(|layout| { layout.set_visible(column, visible); }),
                    );
                    el! { $(checkbox(value, i18n::text(column.label_id()))) }
                }).collect::<Vec<_>>();
                el! { .div.{flex; flex_col; gap: 6.0_f32}.[..visible, ..hidden] }
            }).dyn_view,
            $(i18n::text("columns-restore")).button.{
                on_click: move _ => $(defaults.apply_columns(restore.clone(), None, ColumnScope::Folder));
            },
        ] }.into_view()
    }
    fn apply_columns(&self, location: Location, layout: Option<Layout>, scope: ColumnScope) {
        self.update(|state| {
            match (scope, layout) {
                (_, None) => state.places.clear_columns(location),
                (ColumnScope::Folder, Some(layout)) => state.places.set_columns(location, layout),
                (ColumnScope::Application, Some(layout)) => {
                    state.places.set_default_columns(layout);
                    state.places.clear_columns(location);
                }
            }
            for tab in &mut state.tabs {
                tab.columns = state
                    .places
                    .effective_columns(tab.navigation.location())
                    .clone();
                if !tab.columns.contains(tab.sort.key) {
                    tab.sort = Default::default();
                }
            }
        });
        self.save_places();
        self.0.menus.close();
        let tabs = self.read(|state| state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
        for id in tabs {
            self.reproject(id);
            self.enrich_directory(id);
        }
    }
}
