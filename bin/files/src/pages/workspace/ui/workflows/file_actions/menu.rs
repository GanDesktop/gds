use std::sync::Arc;

use crate::shared::app::FilesExt as _;
use crate::shared::ui::actions::Command;
use gds::app::AppExt as _;
use gds::{i18n, service::desktop, ui::menu, ui::notifications};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{prelude::*, *};

use super::{Browser, DirectoryTarget, Selection};

#[derive(Clone, Copy)]
enum MenuAction {
    Tags,
    Open,
    OpenWith,
    OpenDirectories,
    Copy,
    Cut,
    Paste,
    CopyPaths,
    Rename,
    #[cfg(unix)]
    Permissions,
    Trash,
    DeletePermanent,
    Terminal,
    InternalTerminal,
}

impl MenuAction {
    fn enabled(self, browser: &Browser, selection: &Selection, cx: &App) -> bool {
        if selection.paths.is_empty() || !selection.still_selected(browser) {
            return false;
        }
        let available = selection
            .paths
            .iter()
            .all(|path| browser.device_path_accepts_task(path, cx));
        let idle = !browser.operations.in_progress
            && !browser.collections.trash.is_busy()
            && browser.active().is_some_and(|slot| !slot.is_loading());
        match self {
            Self::Tags => {
                available
                    && idle
                    && selection.paths.len() == 1
                    && browser.settings.preferences.tags.enabled
            }
            Self::CopyPaths => selection.paths.iter().all(|path| path.to_str().is_some()),
            Self::Copy | Self::Cut | Self::Trash | Self::DeletePermanent => available && idle,
            Self::Paste => {
                idle && browser.device_path_accepts_task(&selection.directory, cx)
                    && cx.gds_ro().files().clipboard().can_paste()
            }
            #[cfg(unix)]
            Self::Permissions => available && idle && selection.paths.len() == 1,
            Self::Rename => {
                available
                    && idle
                    && cfg!(target_os = "linux")
                    && selection.paths.len() == 1
                    && selection.paths[0]
                        .file_name()
                        .is_some_and(|name| name.to_str().is_some())
            }
            Self::InternalTerminal => {
                available
                    && browser.picker.is_none()
                    && browser.settings.preferences.terminal_enabled()
            }
            Self::Terminal => available && browser.settings.preferences.terminal_enabled(),
            Self::Open | Self::OpenWith | Self::OpenDirectories => available,
        }
    }

    fn command(self) -> Option<Command> {
        match self {
            Self::Tags => None,
            Self::Open => Some(Command::Open),
            Self::OpenWith => None,
            #[cfg(unix)]
            Self::Permissions => None,
            Self::OpenDirectories => Some(Command::OpenInNewTab),
            Self::Copy => Some(Command::Copy),
            Self::Cut => Some(Command::Cut),
            Self::Paste => Some(Command::Paste),
            Self::CopyPaths => Some(Command::CopyPaths),
            Self::Rename => Some(Command::Rename),
            Self::Trash => Some(Command::Trash),
            Self::DeletePermanent => Some(Command::DeletePermanent),
            Self::Terminal | Self::InternalTerminal => None,
        }
    }

    fn icon(self) -> Option<IconName> {
        match self {
            Self::Tags => Some(IconName::Tag),
            Self::Open | Self::OpenWith => Some(IconName::ExternalLink),
            Self::OpenDirectories => Some(IconName::Folder),
            Self::Copy => Some(IconName::Copy),
            Self::Cut => Some(IconName::Scissors),
            Self::Paste => Some(IconName::ClipboardPaste),
            Self::CopyPaths => Some(IconName::ClipboardCopy),
            Self::Rename => Some(IconName::TextCursor),
            #[cfg(unix)]
            Self::Permissions => Some(IconName::Lock),
            Self::Trash | Self::DeletePermanent => Some(IconName::Trash),
            Self::Terminal | Self::InternalTerminal => Some(IconName::Terminal),
        }
    }
}

struct MenuItems<'a> {
    state: &'a Browser,
    browser: WeakEntity<Browser>,
    selection: Arc<Selection>,
}

impl<'a> MenuItems<'a> {
    fn new(state: &'a Browser, browser: WeakEntity<Browser>, selection: Arc<Selection>) -> Self {
        Self {
            state,
            browser,
            selection,
        }
    }

    fn item(&self, action: MenuAction, label: SharedString, cx: &App) -> PopupMenuItem {
        let enabled = action.enabled(self.state, &self.selection, cx);
        let item = menu::item(label, action.command().map(Into::into), enabled);
        let on_click = Browser::entry_action(self.browser.clone(), self.selection.clone(), action);
        let icon = action.icon();
        el! {
            item {
                on_click: on_click;
                @if (let Some(icon) = icon) icon: icon; @end;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DirectoryAction {
    NewFolder,
    Paste,
    Terminal,
    InternalTerminal,
    Refresh,
}

impl DirectoryAction {
    fn enabled(self, browser: &Browser, target: &DirectoryTarget, cx: &App) -> bool {
        if !target.still_current(browser) {
            return false;
        }
        let available = browser.device_path_accepts_task(&target.path, cx);
        let idle = !browser.operations.in_progress
            && !browser.collections.trash.is_busy()
            && browser.active().is_some_and(|slot| !slot.is_loading());
        match self {
            Self::NewFolder => available && idle,
            Self::Paste => available && idle && cx.gds_ro().files().clipboard().can_paste(),
            Self::InternalTerminal => {
                available
                    && browser.picker.is_none()
                    && browser.settings.preferences.terminal_enabled()
            }
            Self::Terminal => available && browser.settings.preferences.terminal_enabled(),
            Self::Refresh => true,
        }
    }

    fn command(self) -> Option<Command> {
        match self {
            Self::NewFolder => Some(Command::NewFolder),
            Self::Paste => Some(Command::Paste),
            Self::Terminal | Self::InternalTerminal => None,
            Self::Refresh => Some(Command::Refresh),
        }
    }

    fn label_id(self) -> &'static str {
        match self {
            Self::NewFolder => "new-folder",
            Self::Paste => "paste-files",
            Self::Terminal => "terminal-open-external",
            Self::InternalTerminal => "files-terminal-open-here",
            Self::Refresh => "refresh",
        }
    }

    fn icon(self) -> Option<IconName> {
        match self {
            Self::NewFolder => Some(IconName::FolderPlus),
            Self::Paste => Some(IconName::ClipboardPaste),
            Self::Terminal | Self::InternalTerminal => Some(IconName::Terminal),
            Self::Refresh => Some(IconName::RefreshCw),
        }
    }

    fn item(
        self,
        browser: &Browser,
        view: WeakEntity<Browser>,
        target: Arc<DirectoryTarget>,
        cx: &App,
    ) -> PopupMenuItem {
        let enabled = self.enabled(browser, &target, cx);
        let item = menu::item(
            i18n::text(self.label_id(), cx),
            self.command().map(Into::into),
            enabled,
        );
        let on_click = Browser::directory_action(view, target, self);
        let icon = self.icon();
        el! {
            item {
                on_click: on_click;
                @if (let Some(icon) = icon)
                    icon: icon;
                @end;
            }
        }
    }
}

impl Browser {
    pub(super) fn single_entry_menu(
        &self,
        menu: PopupMenu,
        selection: Arc<Selection>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let directory = selection.directories_only;
        let items = MenuItems::new(self, cx.entity().downgrade(), selection);
        let open = items.item(MenuAction::Open, i18n::text("open", cx), cx);
        let open_with = (!directory && self.picker.is_none() && cfg!(target_os = "linux"))
            .then(|| items.item(MenuAction::OpenWith, i18n::text("open-with", cx), cx));
        let open_in_tab = directory.then(|| {
            items.item(
                MenuAction::OpenDirectories,
                i18n::text("open-in-new-tab", cx),
                cx,
            )
        });
        let copy = items.item(MenuAction::Copy, i18n::text("copy-files", cx), cx);
        let cut = items.item(MenuAction::Cut, i18n::text("cut-files", cx), cx);
        let paste = items.item(MenuAction::Paste, i18n::text("paste-files", cx), cx);
        let copy_path = items.item(MenuAction::CopyPaths, i18n::text("copy-path", cx), cx);
        let rename = items.item(MenuAction::Rename, i18n::text("rename", cx), cx);
        let tags = self
            .settings
            .preferences
            .tags
            .enabled
            .then(|| items.item(MenuAction::Tags, i18n::text("tags-edit", cx), cx));
        #[cfg(unix)]
        let permissions = Some(items.item(
            MenuAction::Permissions,
            i18n::text("permissions-title", cx),
            cx,
        ));
        #[cfg(not(unix))]
        let permissions: Option<PopupMenuItem> = None;
        let trash = items.item(MenuAction::Trash, i18n::text("move-to-trash", cx), cx);
        let delete = items.item(
            MenuAction::DeletePermanent,
            i18n::text("delete-permanently", cx),
            cx,
        );
        let internal_terminal =
            (self.picker.is_none() && self.settings.preferences.terminal_enabled()).then(|| {
                items.item(
                    MenuAction::InternalTerminal,
                    i18n::text("files-terminal-open-here", cx),
                    cx,
                )
            });
        let terminal = items.item(
            MenuAction::Terminal,
            i18n::text("terminal-open-external", cx),
            cx,
        );
        el! {
            menu {
                item: open;
                @if (let Some(item) = open_with) item: item; @end;
                @if (let Some(item) = open_in_tab) item: item; @end;
                separator;
                item: copy; item: cut;
                item: paste;
                separator;
                item: copy_path;
                separator;
                item: rename;
                @if (let Some(item) = tags) item: item; @end;
                @if (let Some(item) = permissions) item: item; @end;
                separator;
                item: trash; item: delete;
                separator;
                @if (let Some(item) = internal_terminal) item: item; @end;
                @if (self.settings.preferences.terminal_enabled()) item: terminal; @end;
            }
        }
    }

    pub(super) fn multi_entry_menu(
        &self,
        menu: PopupMenu,
        selection: Arc<Selection>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let count = selection.paths.len();
        let directories = selection.directories_only;
        let items = MenuItems::new(self, cx.entity().downgrade(), selection);
        let open_directories = directories.then(|| {
            items.item(
                MenuAction::OpenDirectories,
                i18n::count("open-directories-new-tabs", count, cx),
                cx,
            )
        });
        let copy = items.item(
            MenuAction::Copy,
            i18n::count("copy-selected-files", count, cx),
            cx,
        );
        let cut = items.item(
            MenuAction::Cut,
            i18n::count("cut-selected-files", count, cx),
            cx,
        );
        let copy_paths = items.item(
            MenuAction::CopyPaths,
            i18n::count("copy-selected-paths", count, cx),
            cx,
        );
        let trash = items.item(
            MenuAction::Trash,
            i18n::count("trash-selected", count, cx),
            cx,
        );
        let delete = items.item(
            MenuAction::DeletePermanent,
            i18n::count("delete-selected", count, cx),
            cx,
        );
        el! {
            menu {
                @if (let Some(item) = open_directories) item: item; separator; @end;
                item: copy; item: cut;
                separator;
                item: copy_paths;
                separator;
                item: trash; item: delete;
            }
        }
    }

    pub(super) fn directory_menu(
        &self,
        menu: PopupMenu,
        target: Arc<DirectoryTarget>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let view = cx.entity().downgrade();
        let new_folder = DirectoryAction::NewFolder.item(self, view.clone(), target.clone(), cx);
        let paste = DirectoryAction::Paste.item(self, view.clone(), target.clone(), cx);
        let internal_terminal = self
            .picker
            .is_none()
            .then_some(())
            .filter(|_| self.settings.preferences.terminal_enabled())
            .map(|_| {
                DirectoryAction::InternalTerminal.item(self, view.clone(), target.clone(), cx)
            });
        let terminal = DirectoryAction::Terminal.item(self, view.clone(), target.clone(), cx);
        let refresh = DirectoryAction::Refresh.item(self, view, target, cx);
        el! { menu { item: new_folder; separator; item: paste; separator;
            @if (let Some(item) = internal_terminal) item: item; @end;
            @if (self.settings.preferences.terminal_enabled()) item: terminal; @end; item: refresh;
        } }
    }

    fn directory_action(
        view: WeakEntity<Self>,
        target: Arc<DirectoryTarget>,
        action: DirectoryAction,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            let Some(view) = view.upgrade() else {
                return;
            };
            view.update(cx, |this, cx| {
                if !target.still_current(this) {
                    notifications::info(i18n::text("location-changed", cx), cx);
                    cx.notify();
                    return;
                }
                if !action.enabled(this, &target, cx) {
                    return;
                }
                match action {
                    DirectoryAction::NewFolder => {
                        this.request_new_folder(target.path.clone(), window, cx)
                    }
                    DirectoryAction::Paste => this.paste_files(target.path.clone(), window, cx),
                    DirectoryAction::Terminal => {
                        this.open_external_terminal(target.path.clone(), cx)
                    }
                    DirectoryAction::InternalTerminal => {
                        this.open_internal_terminal(target.path.clone(), window, cx)
                    }
                    DirectoryAction::Refresh => this.refresh_current(cx),
                }
            });
        }
    }

    fn entry_action(
        view: WeakEntity<Self>,
        selection: Arc<Selection>,
        action: MenuAction,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
        move |_, window, cx| {
            let Some(view) = view.upgrade() else {
                return;
            };
            view.update(cx, |this, cx| {
                if !selection.still_selected(this) {
                    notifications::info(i18n::text("selection-changed", cx), cx);
                    cx.notify();
                    return;
                }
                if !action.enabled(this, &selection, cx) {
                    return;
                }
                match action {
                    MenuAction::Open => this.activate_path(selection.paths[0].clone(), cx),
                    MenuAction::Tags => this.edit_tags(selection.paths[0].clone(), window, cx),
                    MenuAction::OpenWith => {
                        let path = selection.paths[0].clone();
                        if this.device_path_available(&path, cx) {
                            this.open_path(path, desktop::OpenMode::ChooseApplication, cx);
                        }
                    }
                    MenuAction::OpenDirectories => this.open_directories(&selection, window, cx),
                    MenuAction::Copy => this.copy_files(false, window, cx),
                    MenuAction::Cut => this.copy_files(true, window, cx),
                    MenuAction::Paste => this.paste_files(selection.directory.clone(), window, cx),
                    MenuAction::CopyPaths => this.copy_selected_paths(&selection.paths, cx),
                    MenuAction::Rename => this.begin_rename(selection.paths[0].clone(), window, cx),
                    #[cfg(unix)]
                    MenuAction::Permissions => {
                        this.edit_permissions(selection.paths[0].clone(), window, cx)
                    }
                    MenuAction::Trash => this.request_trash_selection(window, cx),
                    MenuAction::DeletePermanent => this.request_delete_selection(window, cx),
                    MenuAction::Terminal | MenuAction::InternalTerminal => {
                        let Some(target) = selection.terminal_target() else {
                            return;
                        };
                        if matches!(action, MenuAction::InternalTerminal) {
                            this.open_internal_terminal(target, window, cx);
                        } else {
                            this.open_external_terminal(target, cx);
                        }
                    }
                }
            });
        }
    }
}
