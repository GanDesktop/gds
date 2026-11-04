//! File identity shared by drag previews and removable status lists.
use super::file_icon::FileIcon;
use gds_uim::{component, el};
use gpui_kit::{IntoElement, assets::IconName, prelude::*, *};

const ICON_SIZE: f32 = 16.;
const GAP: f32 = 8.;

#[component]
#[derive(IntoElement)]
pub(super) struct FileLabel {
    #[attr]
    icon: FileIcon,
    #[attr(into)]
    name: SharedString,
}

impl Default for FileLabel {
    fn default() -> Self {
        Self {
            icon: FileIcon::plain(IconName::File),
            name: SharedString::default(),
        }
    }
}

impl RenderOnce for FileLabel {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        el! {
            .div { flex; items_center; gap: GAP.px; min_w_0; line_height: $(relative(1.5)); } [
                $(self.icon.render(px(ICON_SIZE), window.text_style().color)),
                .div { min_w_0; truncate; } [$(self.name)],
            ]
        }
    }
}
