//! Shared content appearance for file rows, grid tiles, and drag previews.

use gpui_kit::App;

use super::{AppExt as _, FilesExt as _, drag_drop};
use crate::entities::file::directory::Entry;

const HIDDEN_OPACITY: f32 = 0.65;
const GHOSTED_OPACITY: f32 = 0.35;

#[derive(Clone, Copy)]
pub(in crate::pages::workspace) struct EntryAppearance {
    pub opacity: f32,
    pub cut: bool,
}

impl EntryAppearance {
    pub(in crate::pages::workspace) fn of(entry: &Entry, cx: &App) -> Self {
        let cut = cx.gds_ro().files().clipboard().is_cut(&entry.path);
        // Resolve all causes together so hidden, cut, and dragged never multiply.
        let opacity = if cut || drag_drop::is_source(&entry.path, cx) {
            GHOSTED_OPACITY
        } else if entry.is_hidden {
            HIDDEN_OPACITY
        } else {
            1.
        };
        Self { opacity, cut }
    }

    pub(in crate::pages::workspace) fn preview(self) -> Self {
        Self {
            opacity: 1.,
            ..self
        }
    }
}
