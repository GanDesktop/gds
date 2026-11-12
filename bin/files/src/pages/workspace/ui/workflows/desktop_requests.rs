//! Apply desktop requests through the workspace's existing tabs and selection.
use super::*;
use gds::service::desktop::file_manager::{Command as DesktopCommand, Request};
use gds::ui::window::TitleBar;
use gpui_kit::component::ActiveTheme;
use std::sync::Once;

static PROPERTIES_KEY_BINDINGS: Once = Once::new();

struct InspectedEntry {
    path: PathBuf,
    result: std::io::Result<directory::Entry>,
}

impl Browser {
    pub fn apply_desktop_command(
        &mut self,
        command: DesktopCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picker.is_some() {
            return;
        }
        let show_properties = matches!(&command.request, Request::ShowProperties { .. });
        let targets = match command.request {
            Request::NewWindow { directory } => Target::folders(vec![directory]),
            Request::ShowFolders { directories } => Target::folders(directories),
            Request::ShowItems { paths } | Request::ShowProperties { paths } => {
                Target::items(paths)
            }
        };
        if show_properties {
            let paths = targets
                .iter()
                .flat_map(|target| target.selection.iter().cloned())
                .collect();
            self.open_desktop_properties(paths, cx);
        }
        self.pages.clear();
        for target in targets {
            let existing = self
                .tabs
                .items
                .iter()
                .find(|slot| slot.path() == Some(target.directory.as_path()))
                .map(|slot| slot.id);
            if let Some(id) = existing {
                self.switch_tab(id, window, cx);
            } else {
                self.open_tab(target.directory, false, window, cx);
            }
            let id = self.tabs.active_id;
            if let Some(slot) = self.tab_slot_mut(id) {
                slot.clear_filter();
                slot.reveal_hidden = target.selection.iter().any(|path| {
                    path.file_name()
                        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
                });
                slot.pending_reveal = Some(target.selection);
            }
            self.sync_filter_input(window, cx);
            self.refresh_tab(id, cx);
            if show_properties {
                self.locations
                    .places
                    .set_details(self.current_location(), true);
            }
        }
        self.window.focus.focus(window, cx);
        if command.startup_id.is_empty() {
            window.activate_window();
        } else if let Err(error) = window.activate_with_token(&command.startup_id) {
            tracing::warn!(%error, "cannot activate Files with the supplied token");
            window.request_attention();
        }
        cx.notify();
    }

    fn open_desktop_properties(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Read file properties", move || {
                paths
                    .into_iter()
                    .map(|path| {
                        let result = directory::Entry::inspect(&path);
                        InspectedEntry { path, result }
                    })
                    .collect::<Vec<_>>()
            });
        cx.spawn(async move |this, cx| {
            let entries = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                let entries = match entries {
                    Ok(entries) => entries,
                    Err(error) => {
                        notifications::error(error.to_string(), cx);
                        return;
                    }
                };
                for item in entries {
                    let entry = match item.result {
                        Ok(entry) => entry,
                        Err(error) => {
                            notifications::error(format!("{}: {error}", item.path.display()), cx);
                            continue;
                        }
                    };
                    let inspections = this.files.inspections.clone();
                    let title = entry.display_name.clone();
                    let tags_enabled = this.settings.preferences.tags.enabled;
                    let options = WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                            None,
                            size(px(420.), px(640.)),
                            cx,
                        ))),
                        app_id: Some("io.github.GanDesktop.GdsFiles".into()),
                        kind: WindowKind::Floating,
                        ..ui::window::window_options()
                    };
                    if let Err(error) = gpui_kit::open_window(options, cx, move |window, cx| {
                        window.set_window_title(&title);
                        cx.new(|cx| Properties::new(entry, inspections, tags_enabled, window, cx))
                    }) {
                        notifications::error(error.to_string(), cx);
                    }
                }
            }) {
                tracing::warn!(%error, "Files closed before properties were read");
            }
        })
        .detach();
    }
}

/// A workspace destination, computed when applying a desktop request to the UI.
struct Target {
    directory: PathBuf,
    selection: Vec<PathBuf>,
}

impl Target {
    fn folders(directories: Vec<PathBuf>) -> Vec<Self> {
        let mut targets: Vec<Self> = Vec::new();
        for directory in directories {
            if !targets.iter().any(|target| target.directory == directory) {
                targets.push(Self {
                    directory,
                    selection: Vec::new(),
                });
            }
        }
        targets
    }

    fn items(paths: Vec<PathBuf>) -> Vec<Self> {
        let mut targets: Vec<Self> = Vec::new();
        for path in paths {
            let directory = path.parent().unwrap_or(&path).to_owned();
            if let Some(target) = targets
                .iter_mut()
                .find(|target| target.directory == directory)
            {
                if !target.selection.contains(&path) {
                    target.selection.push(path);
                }
            } else {
                targets.push(Self {
                    directory,
                    selection: vec![path],
                });
            }
        }
        targets
    }
}

gpui_kit::actions!(gds_files_properties, [CloseProperties]);

struct Properties {
    title: SharedString,
    view: Entity<preview::View>,
    focus: FocusHandle,
}

impl Properties {
    fn new(
        entry: directory::Entry,
        inspections: Entity<ui::file_inspection::Cache>,
        tags_enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = entry.display_name.clone().into();
        let view = cx.new(|cx| preview::View::new(inspections, cx));
        view.update(cx, |view, cx| {
            view.set_tags_enabled(tags_enabled, cx);
            view.show_entry(entry, cx);
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        PROPERTIES_KEY_BINDINGS.call_once(|| {
            cx.bind_keys([
                KeyBinding::new("escape", CloseProperties, Some("FileProperties")),
                KeyBinding::new("ctrl-w", CloseProperties, Some("FileProperties")),
            ]);
        });
        Self { title, view, focus }
    }

    fn close(&mut self, _: &CloseProperties, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }
}

impl Render for Properties {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gds_uim::el;
        let title = self.title.clone();
        let view = self.view.clone();
        let close = cx.listener(Self::close);
        el! {
            .div {
                size_full; flex; flex_col; min_h_0; key_context: "FileProperties";
                track_focus: $&self.focus; on_action: close;
                bg: $cx.theme().background; text_color: $cx.theme().foreground;
            } [
                .TitleBar::default { } [title],
                view,
            ]
        }
    }
}

#[cfg(test)]
mod tests;
