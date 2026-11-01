//! Shared controls, with section-revision persistence owned by Agents.

use crate::shared::config::{Document, Store};
use crate::shared::i18n::tr;
use crate::widgets::settings::ui::*;
use gds::ui::terminal::{Preferences, PreferencesEditor};

pub(in crate::widgets::settings) struct Page {
    store: Store,
    document: Option<Document>,
    editor: Entity<PreferencesEditor>,
    pub message: String,
    _subscription: Subscription,
}

impl Page {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = crate::shared::config::store(cx);
        let (document, message) = match store.load() {
            Ok(document) => (Some(document), String::new()),
            Err(error) => (None, error.to_string()),
        };
        let value = document
            .as_ref()
            .map(|document| document.settings.terminal.clone())
            .unwrap_or_default();
        let editor = cx.new(|cx| PreferencesEditor::new(value, window, cx));
        let subscription = cx.subscribe_in(&editor, window, Self::on_changed);
        Self {
            store,
            document,
            editor,
            message,
            _subscription: subscription,
        }
    }

    fn on_changed(
        &mut self,
        _: &Entity<PreferencesEditor>,
        event: &gds::ui::settings::form::Changed<Preferences>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .document
            .as_mut()
            .ok_or_else(|| tr!("settings-not-loaded").to_owned())
            .and_then(|document| {
                self.store
                    .save_terminal(document, event.value.clone())
                    .map_err(|error| error.to_string())
            });
        self.message = match result {
            Ok(()) => {
                cx.set_global(crate::shared::config::terminal::Current(
                    event.value.clone(),
                ));
                tr!("settings-saved").into()
            }
            // Keep the shared editor's draft intact after a conflict or managed-file denial.
            Err(error) => error,
        };
        cx.notify();
    }
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("local-terminal-settings")
            .test_support()
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .child(self.editor.clone())
    }
}
