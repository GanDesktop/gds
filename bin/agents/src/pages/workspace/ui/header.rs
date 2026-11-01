use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::ClickEvent;

use super::*;

impl Workspace {
    fn title_slot(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.renaming_header() {
            self.render_title_editor(cx).into_any_element()
        } else {
            el! {
                .div {
                    # "rename-session";
                    test_support;
                    flex_1;
                    min_w_0;
                    truncate;
                    text_size: crate::shared::ui::SMALL;
                    cursor_pointer;
                    hoverable_tooltip: $(shortcuts::tooltip(tr!("session-rename"), shortcuts::RenameThread));
                    on_click: $(cx.listener(Self::on_header_rename));
                } [
                    $(self.current().map(title).unwrap_or_else(|| self.draft_title())),
                ]
            }
            .into_any_element()
        };
        el! {
            .div {
                # "thread-title-slot";
                test_support;
                flex_1;
                min_w_0;
                h_8;
                flex;
                items_center;
                gap_1;
            } [
                content,
            ]
        }
    }

    pub(super) fn archive_notice(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        self.current()
            .filter(|session| session.archived)
            .map(|session| {
                div()
                    .px_5()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().child(tr!("session-archived-help")))
                    .child(self.archive_button(session, "restore-archived", cx))
                    .into_any_element()
            })
    }

    pub(super) fn header_row(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        el! {
            .div {
                # "conversation-header-row";
                test_support;
                w_full;
                flex;
                flex_shrink_0;
                h: layout::HEADER_HEIGHT.px;
                items_center;
                gap_1;
                when: $(cfg!(target_os = "macos") && !self.window.sidebar_visible), {
                    pl: layout::MACOS_WINDOW_CONTROLS_WIDTH.px;
                };
            } [
                ("sidebar-toggle", IconName::PanelLeft).layout::icon_button {
                    tooltip_with_shortcut: tr!("sidebar-toggle"), $(shortcuts::shortcut(ToggleSidebar));
                    on_click: $(cx.listener(Self::on_sidebar_toggle));
                },
                $(self.workspace_button(cx)),
                ("/", cx).caption,
                $(self.title_slot(cx)),
                $(self.mode_button(cx)),
            ]
        }
    }

    pub(super) fn window_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> crate::shared::ui::window_controls::WindowControls {
        crate::shared::ui::window_controls::WindowControls::default()
            .on_maximize(cx.listener(Self::on_window_zoom))
            .on_close_window(cx.listener(Self::on_window_close))
    }

    fn on_header_rename(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_rename(window, cx);
    }

    fn on_sidebar_toggle(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_sidebar(window, cx);
    }

    fn on_window_zoom(&mut self, _: &ClickEvent, window: &mut Window, _: &mut Context<Self>) {
        if !window.is_maximized() {
            self.window.size = window.bounds().size;
        }
        window.zoom_window();
    }

    fn on_window_close(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.close_window(window, cx);
    }
}
