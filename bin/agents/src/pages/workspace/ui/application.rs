use super::*;
use crate::shared::i18n::tr;
use gpui_kit::component::menu::PopupMenuItem;

fn application_item(
    label: &'static str,
    action: Option<Box<dyn gpui_kit::Action>>,
) -> PopupMenuItem {
    action_item(label, action, None)
}

pub(super) fn action_item(
    label: &'static str,
    action: Option<Box<dyn gpui_kit::Action>>,
    context: Option<&'static str>,
) -> PopupMenuItem {
    let shortcut = action
        .as_ref()
        .map(|action| gds::ui::Shortcut::new(action.boxed_clone(), context));
    let item = PopupMenuItem::element(gds::ui::menu::row(label, shortcut));
    match action {
        Some(action) => item.action(action),
        None => item,
    }
}

impl Workspace {
    pub(super) fn application_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let settings = cx.entity().downgrade();
        let about = settings.clone();
        let history = settings.clone();
        let connected = self.backend.connected;
        let application = gds::ui::about::Application::cargo(env!("CARGO_BIN_NAME"));
        self.menu_button(
            Button::application_menu("app-menu", application.icon)
                .accessibility_label(tr!("app-menu"))
                .tooltip(tr!("app-menu")),
            3,
            false,
            move |menu, window, _| {
                let parent = window.window_handle();
                let settings = settings.clone();
                let about = about.clone();
                let history = history.clone();
                menu.min_w(px(240.))
                    .item(
                        shortcuts::menu_item(
                            tr!("agent-history-title"),
                            Some(Box::new(shortcuts::OpenSessionManager)),
                        )
                        .icon(crate::widgets::history::ICON)
                        .disabled(!connected)
                        .on_click(move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ = history.update(cx, |this, cx| {
                                    this.on_open_session_manager(
                                        &shortcuts::OpenSessionManager,
                                        window,
                                        cx,
                                    );
                                });
                            });
                        }),
                    )
                    .item(
                        application_item(
                            tr!("settings-title"),
                            Some(Box::new(crate::shared::ui::dialogs::OpenSettings)),
                        )
                        .on_click(move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ =
                                    settings.update(cx, |this, cx| this.open_settings(window, cx));
                            });
                        }),
                    )
                    .item(application_item(tr!("app-about"), None).on_click(
                        move |_, window, cx| {
                            menus::in_parent(parent, window, cx, |window, cx| {
                                let _ = about
                                    .update(cx, |this, cx| this.open_about_from_menu(window, cx));
                            });
                        },
                    ))
            },
            cx,
        )
    }

    fn open_about_from_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        self.record_settings_target(cx);
        let owner = cx.entity().downgrade();
        let parent = self.window.handle;
        cx.defer(move |cx| {
            if !crate::features::navigation::target_page(parent, cx) {
                return;
            }
            if let Err(error) = crate::features::navigation::open_about(cx) {
                let _ = owner.update(cx, |this, cx| {
                    this.selected.error = Some(tr!("app-about-error", error = error.to_string()));
                    cx.notify();
                });
            }
        });
    }
}
