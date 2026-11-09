//! Let native text input start a search while the file list owns focus.

use super::*;
use gpui_kit::component::input::{MoveDown, MoveUp};
use std::ops::Range;

impl Browser {
    pub(in crate::pages::workspace::ui) fn on_search_down(
        &mut self,
        _: &MoveDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enter_search_results(selection::Motion::First, window, cx);
    }

    pub(in crate::pages::workspace::ui) fn on_search_up(
        &mut self,
        _: &MoveUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enter_search_results(selection::Motion::Last, window, cx);
    }

    fn enter_search_results(
        &mut self,
        motion: selection::Motion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.toolbar.filter.input.clone();
        if !self.pages.is_empty()
            || (self.collection().is_some() && self.place_collection().is_none())
            || window.has_active_dialog(cx)
            || !input.read(cx).focus_handle(cx).is_focused(window)
            || input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            cx.propagate();
            return;
        }
        if let Some(collection) = self.place_collection() {
            self.move_place_cursor(collection, motion, cx);
            self.window.focus.focus(window, cx);
        } else {
            self.move_file_cursor(motion, false, window, cx);
        }
        cx.stop_propagation();
    }

    fn accepts_search_input(&self, window: &mut Window, cx: &mut App) -> bool {
        self.settings.preferences.files.auto_focus_search
            && (self.file_selection_focused(window, cx) || self.place_collection().is_some())
            && self.list_keys_available(window, cx)
    }

    pub(in crate::pages::workspace::ui) fn search_input_handler(
        &self,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let browser = cx.entity();
        canvas(
            |_, _, _| {},
            move |bounds, (), window, cx| {
                if browser.update(cx, |browser, cx| browser.accepts_search_input(window, cx))
                    && let Some(focus) = window.focused(cx)
                {
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, browser.clone()),
                        cx,
                    );
                }
            },
        )
        .absolute()
        .inset_0()
    }
}

// Delegate the native UTF-16 ranges and composition state to the actual field.
// A raw key listener would lose IME preedit and keyboard-layout text.
impl EntityInputHandler for Browser {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.toolbar.filter.input.update(cx, |input, cx| {
            input.text_for_range(range, adjusted, window, cx)
        })
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.toolbar.filter.input.update(cx, |input, cx| {
            input.selected_text_range(ignore_disabled, window, cx)
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.toolbar
            .filter
            .input
            .update(cx, |input, cx| input.marked_text_range(window, cx))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toolbar
            .filter
            .input
            .update(cx, |input, cx| input.unmark_text(window, cx));
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.filter.input.update(cx, |input, cx| {
            if !text.is_empty() {
                input.focus(window, cx);
            }
            input.replace_text_in_range(range, text, window, cx);
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.filter.input.update(cx, |input, cx| {
            if !text.is_empty() {
                input.focus(window, cx);
            }
            input.replace_and_mark_text_in_range(range, text, selected, window, cx);
        });
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.toolbar.filter.input.update(cx, |input, cx| {
            input.bounds_for_range(range, bounds, window, cx)
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.toolbar.filter.input.update(cx, |input, cx| {
            input.character_index_for_point(point, window, cx)
        })
    }
}
