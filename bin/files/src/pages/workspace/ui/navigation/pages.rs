//! Secondary pages keep the file workspace and the previous focus intact.
use super::*;
use gds::ui::page::stack::Entry;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pages::workspace) enum Page {
    Settings,
    Help,
}

impl Browser {
    pub(in crate::pages::workspace) fn show_page(
        &mut self,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.pages.iter().position(|entry| entry.content == page) {
            while self.pages.len() > index + 1 {
                self.back_page(window, cx);
            }
            return;
        }
        if page == Page::Settings {
            let preferences = self.settings.preferences.clone();
            let presentation = self.sidebar.presentation;
            self.settings.view.update(cx, |view, cx| {
                view.set_preferences(preferences, window, cx);
                view.set_sidebar_presentation(presentation, cx);
            });
        }
        self.pages.push(Entry {
            content: page,
            return_focus: window.focused(cx),
            subscription: None,
        });

        self.discard_marquee();
        self.menus.open = None;
        self.menus.sort = None;
        self.toolbar.finish_path_edit();
        self.window.root_focus.focus(window, cx);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn back_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(page) = self.pages.pop() {
            page.restore_focus(window, cx);
            self.sync_terminal_cwd(cx);
            cx.notify();
        }
    }
}
