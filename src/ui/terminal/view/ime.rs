//! IME surrounding text is only the local preedit. Screen output and submitted
//! commands are never exposed as an editable document or retained as input history.
use super::View;
use gpui_kit::*;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn utf8_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (offset, ch) in text.char_indices() {
        if units + ch.len_utf16() > utf16 {
            return offset;
        }
        units += ch.len_utf16();
    }
    text.len()
}
pub(super) fn byte_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf8_offset(text, range.start);
    start..utf8_offset(text, range.end).max(start)
}

impl EntityInputHandler for View {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = byte_range(&self.preedit, range);
        *actual = Some(
            self.preedit[..bytes.start].encode_utf16().count()
                ..self.preedit[..bytes.end].encode_utf16().count(),
        );
        Some(self.preedit[bytes].to_owned())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.preedit_selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.preedit.is_empty()).then(|| 0..self.preedit.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.preedit.clear();
        self.preedit_selection = 0..0;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let committed = if let Some(range) = range.filter(|_| !self.preedit.is_empty()) {
            let bytes = byte_range(&self.preedit, range);
            self.preedit.replace_range(bytes, text);
            std::mem::take(&mut self.preedit)
        } else {
            text.to_owned()
        };
        self.preedit.clear();
        self.preedit_selection = 0..0;
        self.input(committed.as_bytes(), cx);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bytes = range
            .map(|range| byte_range(&self.preedit, range))
            .unwrap_or(0..self.preedit.len());
        let prefix_units = self.preedit[..bytes.start].encode_utf16().count();
        self.preedit.replace_range(bytes, text);
        let length = self.preedit.encode_utf16().count();
        self.preedit_selection = selected
            .map(|range| {
                (prefix_units + range.start).min(length)..(prefix_units + range.end).min(length)
            })
            .unwrap_or(length..length);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (cursor, _) = self.snapshot.cursor?;
        let row = cursor.line.0 + self.snapshot.display_offset as i32;
        if row < 0 {
            return None;
        }
        let geometry = self.geometry;
        let bytes = byte_range(&self.preedit, range);
        let start = geometry.cell.width * self.preedit[..bytes.start].width() as f32;
        let end = geometry.cell.width * self.preedit[..bytes.end].width() as f32;
        let column = geometry.composition_column(cursor.column.0, &self.preedit);
        Some(Bounds::new(
            geometry.bounds.origin
                + point(
                    geometry.cell.width * column as f32 + start,
                    geometry.cell.height * row as f32,
                ),
            size((end - start).max(px(1.)), geometry.cell.height),
        ))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.bounds_for_range(0..0, self.geometry.bounds, window, cx)?;
        let column = (f32::from(point.x - bounds.left()) / f32::from(self.geometry.cell.width))
            .max(0.) as usize;
        let mut cells = 0;
        let mut index = 0;
        for grapheme in self.preedit.graphemes(true) {
            if cells + grapheme.width().max(1) > column {
                break;
            }
            cells += grapheme.width().max(1);
            index += grapheme.encode_utf16().count();
        }
        Some(index)
    }
}

#[cfg(test)]
mod tests;
