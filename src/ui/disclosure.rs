//! Decorative chevron driven by its host's disclosure state.
use super::Icon;
use anti_webui::prelude::*;
use gds_uim::el;

pub struct DisclosureIcon {
    size: f32,
    progress: Binding<f32>,
}

impl DisclosureIcon {
    pub fn new(progress: Binding<f32>) -> Self {
        Self {
            size: crate::appearance::tokens::SIDEBAR_ICON_SIZE,
            progress,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl IntoView for DisclosureIcon {
    fn into_view(self) -> View {
        let progress = self.progress;
        el! { .div.{
            flex; items_center; justify_center; size: self->size; flex_none;
            dyn_style: move s => $(s.rotate(90.0 * progress.get()));
        }.[ $(Icon::ChevronRight.view(self.size)) ] }
        .into_view()
    }
}
