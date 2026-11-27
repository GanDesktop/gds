//! Pure workspace menu action helpers (tab opening, clipboard path copy, volume/terminal/page actions).

use std::path::PathBuf;

use gds::i18n;
use zgui::prelude::*;

use super::super::terminals::TerminalDock;
use crate::{
    entities::{
        file::ui::FileIcon,
        location::{Location, scheme::TagAddress, volumes},
    },
    features::file_operations::{Batch, Error},
    pages::workspace::{
        navigation::Navigator,
        operations::{DeviceOps, Dispatch, Lifecycle, LocalOps},
        projection::{Rows, UiState},
        state::{
            context::Context,
            menus::{DirectoryAction, DirectoryTarget},
            model::{Pane, Prompt, Row},
        },
    },
    shared::lib::clipboard::Clipboard,
};

pub(super) fn place_location(row: &Row) -> Option<Location> {
    match row {
        Row::Place(location) => Some(location.clone()),
        Row::File(entry) => Some(Location::from(entry.path.as_path())),
        Row::Tag(tag) => TagAddress::new(tag).ok().map(Location::Tags),
        _ => None,
    }
}

pub(super) fn open_selected_tab(ctx: &Context) {
    let devices = DeviceOps(ctx);
    let nav = Navigator(ctx);
    for row in ctx.read_untracked(|state| state.tab().selected()) {
        let location = match row {
            Row::File(entry) if FileIcon::is_directory(&entry) => {
                Location::from(entry.path.as_path())
            }
            Row::Place(location) => location,
            Row::Remote(entry) if entry.kind == gds::service::remote::EntryKind::Directory => {
                Location::Remote(entry.address)
            }
            Row::Volume(volume) if !volume.mount_points.is_empty() => {
                devices.open_volume(&volume.id, true);
                continue;
            }
            _ => continue,
        };
        nav.new_tab(Some(location));
    }
}

pub(super) fn open_selected_with(ctx: &Context) {
    let paths = ctx.read_untracked(|state| {
        state
            .tab()
            .selected()
            .iter()
            .filter_map(|row| row.path().map(ToOwned::to_owned))
            .collect::<Vec<_>>()
    });
    let lifecycle = Lifecycle(ctx);
    for path in paths {
        lifecycle.open_file(path, gds::service::desktop::OpenMode::ChooseApplication);
    }
}

pub(super) fn show_selected_parent(ctx: &Context) {
    if ctx.read_untracked(|state| state.picker.is_some()) {
        return;
    }
    let paths = ctx.read_untracked(|state| {
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
    let mut targets: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for path in paths {
        let directory = path.parent().unwrap_or(&path).to_owned();
        if let Some((_, selection)) = targets.iter_mut().find(|(dir, _)| *dir == directory) {
            if !selection.contains(&path) {
                selection.push(path);
            }
        } else {
            targets.push((directory, vec![path]));
        }
    }
    ctx.show_files();
    let nav = Navigator(ctx);
    let rows = Rows(ctx);
    for (directory, selection) in targets {
        let existing = ctx.read_untracked(|state| {
            state
                .tabs
                .iter()
                .position(|tab| tab.navigation.path() == Some(directory.as_path()))
        });
        if let Some(index) = existing {
            nav.select_tab(index);
        } else {
            nav.new_tab(Some(Location::from(directory.as_path())));
        }
        let reveal_dotfiles = selection.iter().any(|path| {
            path.file_name()
                .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
        });
        ctx.update(|state| {
            let tab = state.tab_mut();
            tab.navigation.view_mut().filter.clear();
            tab.reveal_hidden = reveal_dotfiles;
        });
        ctx.0.filter.set(String::new());
        nav.reveal_paths(selection);
        rows.sync_address();
        nav.refresh();
    }
    ctx.focus_content();
}

fn write_clipboard_text(text: String) {
    if let Some(clipboard) = try_use_clipboard() {
        clipboard.set_text(ClipboardKind::Standard, text);
    }
}

fn copy_collection_uris(ctx: &Context, rows: &[Row]) {
    let result = rows
        .iter()
        .filter_map(place_location)
        .map(|location| location.uri())
        .collect::<Result<Vec<_>, _>>();
    match result {
        Ok(uris) if !uris.is_empty() => write_clipboard_text(uris.join("\n")),
        Ok(_) => {}
        Err(error) => ctx.report_notice(error.to_string()),
    }
}

pub(super) fn copy_selected_paths(ctx: &Context) {
    let (rows, collection) = ctx.read_untracked(|state| {
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
        copy_collection_uris(ctx, &rows);
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
                Row::Tag(_) => place_location(&row).map(|location| Ok(location.display_address())),
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
        Ok(text) if !text.is_empty() => write_clipboard_text(text),
        Ok(_) => {}
        Err(error) => ctx.report_notice(error.to_string()),
    }
}

pub(super) fn change_volume(ctx: &Context, action: volumes::DeviceAction) {
    DeviceOps(ctx).request_device_action(action);
}

pub(super) fn show_help(ctx: &Context) {
    UiState(ctx).open_page(Pane::Help);
}

pub(super) fn show_about(ctx: &Context) {
    ctx.open_prompt(Prompt::About);
}

pub(super) fn open_external_terminal(ctx: &Context, path: PathBuf) {
    ctx.blocking(
        "Open external terminal",
        move || {
            gds::service::desktop::Desktop::default()
                .open_terminal(&path)
                .map_err(|error| error.to_string())
        },
        |ctx, result| {
            if let Err(error) = result {
                ctx.report_notice(error);
            }
        },
    );
}

pub(super) fn directory_action_available(
    ctx: &Context,
    action: DirectoryAction,
    target: &DirectoryTarget,
) -> bool {
    ctx.read_untracked(|state| target.matches_state(state))
        && match action {
            DirectoryAction::NewFolder | DirectoryAction::Paste => {
                !target.removal_in_progress(&ctx.0.volumes)
                    && !ctx.read_untracked(|state| {
                        state.operation.busy() || state.tab().navigation.is_loading()
                    })
                    && (!matches!(action, DirectoryAction::Paste)
                        || Clipboard::current().can_paste_untracked())
            }
            DirectoryAction::InternalTerminal => {
                !target.removal_in_progress(&ctx.0.volumes)
                    && ctx.0.preferences.snapshot().terminal_enabled()
                    && ctx.read_untracked(|state| state.picker.is_none())
            }
            DirectoryAction::Terminal => {
                !target.removal_in_progress(&ctx.0.volumes)
                    && ctx.0.preferences.snapshot().terminal_enabled()
            }
            DirectoryAction::Refresh => true,
        }
}

pub(super) fn run_directory_action(
    ctx: &Context,
    action: DirectoryAction,
    target: DirectoryTarget,
) {
    let Some(path) = ctx.read_untracked(|state| target.current_path(state)) else {
        ctx.report_notice(i18n::text_untracked("location-changed"));
        return;
    };
    if !directory_action_available(ctx, action, &target) {
        return;
    }
    match action {
        DirectoryAction::NewFolder => LocalOps(ctx).new_folder(),
        DirectoryAction::Paste => Dispatch(ctx).paste(),
        DirectoryAction::InternalTerminal => TerminalDock(ctx).new_terminal_at(path),
        DirectoryAction::Terminal => open_external_terminal(ctx, path),
        DirectoryAction::Refresh => Navigator(ctx).refresh(),
    }
}
