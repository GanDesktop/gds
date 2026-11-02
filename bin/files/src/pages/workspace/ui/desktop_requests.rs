//! Apply desktop requests through the workspace's existing tabs and selection.
use super::*;
use gds::service::desktop::file_manager::{Command as DesktopCommand, Operation};
use gpui_kit::component::ActiveTheme;

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
        if command.operation == Operation::Properties {
            self.open_desktop_properties(&command, cx);
        }
        self.settings.open = false;
        for target in command.targets {
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
                slot.filter.clear();
                slot.reveal_hidden = target.selection.iter().any(|path| {
                    path.file_name()
                        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
                });
                slot.pending_reveal = Some(target.selection);
            }
            self.sync_filter_input(window, cx);
            self.refresh_tab(id, cx);
            if command.operation == Operation::Properties {
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
    fn open_desktop_properties(&mut self, command: &DesktopCommand, cx: &mut Context<Self>) {
        let paths = command
            .targets
            .iter()
            .flat_map(|target| target.selection.iter().cloned())
            .collect::<Vec<_>>();
        let work = cx.background_executor().spawn(async move {
            paths
                .into_iter()
                .map(|path| {
                    let result = directory::Entry::inspect(&path);
                    (path, result)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let entries = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                for (path, result) in entries {
                    let entry = match result {
                        Ok(entry) => entry,
                        Err(error) => {
                            notifications::error(format!("{}: {error}", path.display()), cx);
                            continue;
                        }
                    };
                    let thumbnails = this.files.thumbnails.clone();
                    let title = entry.display_name.clone();
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
                        cx.new(|cx| Properties::new(entry, thumbnails, window, cx))
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

gpui_kit::actions!(gds_files_properties, [CloseProperties]);

struct Properties {
    title: SharedString,
    view: Entity<preview::View>,
    focus: FocusHandle,
}

impl Properties {
    fn new(
        entry: directory::Entry,
        thumbnails: Entity<ui::thumbnail::Cache>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = entry.display_name.clone().into();
        let view = cx.new(|cx| preview::View::new(thumbnails, cx));
        view.update(cx, |view, cx| view.show_entry(entry, cx));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        cx.bind_keys([
            KeyBinding::new("escape", CloseProperties, Some("FileProperties")),
            KeyBinding::new("ctrl-w", CloseProperties, Some("FileProperties")),
        ]);
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
                .ui::window::TitleBar::default { } [title],
                view,
            ]
        }
    }
}
