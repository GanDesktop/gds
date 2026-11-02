use std::sync::Arc;

use crate::features::file_operations::commands::Command;
use gds::{i18n, service::desktop, ui::menu, ui::notifications};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{prelude::*, *};

use super::{Browser, DirectoryTarget, Selection};

#[derive(Clone, Copy)]
enum MenuAction {
    Open,
    OpenWith,
    OpenDirectories,
    Copy,
    Cut,
    Paste,
    CopyPaths,
    Rename,
    Trash,
    DeletePermanent,
    Terminal,
    InternalTerminal,
}

impl MenuAction {
    fn command(self) -> Option<Command> {
        match self {
            Self::Open => Some(Command::Open),
            Self::OpenWith => None,
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
            Self::Open | Self::OpenWith => Some(IconName::ExternalLink),
            Self::OpenDirectories => Some(IconName::Folder),
            Self::Copy => Some(IconName::Copy),
            Self::Cut => Some(IconName::Scissors),
            Self::Paste => Some(IconName::ClipboardPaste),
            Self::CopyPaths => Some(IconName::ClipboardCopy),
            Self::Rename => Some(IconName::TextCursor),
            Self::Trash | Self::DeletePermanent => Some(IconName::Trash),
            Self::Terminal | Self::InternalTerminal => Some(IconName::Terminal),
        }
    }
}

struct MenuItems {
    browser: WeakEntity<Browser>,
    selection: Arc<Selection>,
}

impl MenuItems {
    fn new(browser: WeakEntity<Browser>, selection: Arc<Selection>) -> Self {
        Self { browser, selection }
    }

    fn item(&self, action: MenuAction, label: SharedString) -> PopupMenuItem {
        let render = menu::row(label, action.command().map(Into::into));
        let on_click = Browser::entry_action(self.browser.clone(), self.selection.clone(), action);
        let icon = action.icon();
        el! {
            render.PopupMenuItem::element {
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
        view: WeakEntity<Browser>,
        target: Arc<DirectoryTarget>,
        cx: &App,
    ) -> PopupMenuItem {
        let render = menu::row(
            i18n::text(self.label_id(), cx),
            self.command().map(Into::into),
        );
        let on_click = Browser::directory_action(view, target, self);
        let icon = self.icon();
        el! {
            render.PopupMenuItem::element {
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
        let items = MenuItems::new(cx.entity().downgrade(), selection);
        let open = items.item(MenuAction::Open, i18n::text("open", cx));
        let open_with = (!directory && self.picker.is_none() && cfg!(target_os = "linux"))
            .then(|| items.item(MenuAction::OpenWith, i18n::text("open-with", cx)));
        let open_in_tab = directory.then(|| {
            items.item(
                MenuAction::OpenDirectories,
                i18n::text("open-in-new-tab", cx),
            )
        });
        let copy = items.item(MenuAction::Copy, i18n::text("copy-files", cx));
        let cut = items.item(MenuAction::Cut, i18n::text("cut-files", cx));
        let paste = items
            .item(MenuAction::Paste, i18n::text("paste-files", cx))
            .disabled(self.operations.in_progress || !crate::shared::lib::clipboard::can_paste(cx));
        let copy_path = items.item(MenuAction::CopyPaths, i18n::text("copy-path", cx));
        let rename = items
            .item(MenuAction::Rename, i18n::text("rename", cx))
            .disabled(self.operations.in_progress);
        let trash = items
            .item(MenuAction::Trash, i18n::text("move-to-trash", cx))
            .disabled(self.operations.in_progress);
        let delete = items
            .item(
                MenuAction::DeletePermanent,
                i18n::text("delete-permanently", cx),
            )
            .disabled(self.operations.in_progress);
        let internal_terminal =
            (self.picker.is_none() && self.settings.preferences.terminal_enabled()).then(|| {
                items.item(
                    MenuAction::InternalTerminal,
                    i18n::text("files-terminal-open-here", cx),
                )
            });
        let terminal = items.item(
            MenuAction::Terminal,
            i18n::text("terminal-open-external", cx),
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
        let items = MenuItems::new(cx.entity().downgrade(), selection);
        let open_directories = directories.then(|| {
            items.item(
                MenuAction::OpenDirectories,
                i18n::count("open-directories-new-tabs", count, cx),
            )
        });
        let copy = items.item(
            MenuAction::Copy,
            i18n::count("copy-selected-files", count, cx),
        );
        let cut = items.item(
            MenuAction::Cut,
            i18n::count("cut-selected-files", count, cx),
        );
        let copy_paths = items.item(
            MenuAction::CopyPaths,
            i18n::count("copy-selected-paths", count, cx),
        );
        let trash = items
            .item(MenuAction::Trash, i18n::count("trash-selected", count, cx))
            .disabled(self.operations.in_progress);
        let delete = items
            .item(
                MenuAction::DeletePermanent,
                i18n::count("delete-selected", count, cx),
            )
            .disabled(self.operations.in_progress);
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
        let new_folder = DirectoryAction::NewFolder
            .item(view.clone(), target.clone(), cx)
            .disabled(self.operations.in_progress);
        let paste = DirectoryAction::Paste
            .item(view.clone(), target.clone(), cx)
            .disabled(self.operations.in_progress || !crate::shared::lib::clipboard::can_paste(cx));
        let internal_terminal = self
            .picker
            .is_none()
            .then_some(())
            .filter(|_| self.settings.preferences.terminal_enabled())
            .map(|_| DirectoryAction::InternalTerminal.item(view.clone(), target.clone(), cx));
        let terminal = DirectoryAction::Terminal.item(view.clone(), target.clone(), cx);
        let refresh = DirectoryAction::Refresh.item(view, target, cx);
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
                match action {
                    MenuAction::Open => this.activate_path(selection.paths[0].clone(), cx),
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
