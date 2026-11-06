use crate::ui::container::container_traits;
use gds_uim::el;
use gpui_kit::{IntoElement, prelude::*, *};

/// A picker column; its host owns the title and decision footer.
#[derive(IntoElement)]
pub struct Picker {
    base: Div,
}
impl Default for Picker {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; min_w_0; min_h_0; w_full; gap_2; text_size: crate::appearance::tokens::SMALL; } },
        }
    }
}
container_traits!(Picker);
impl RenderOnce for Picker {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

/// Compact navigation or filter controls with fixed, shared spacing.
#[derive(IntoElement)]
pub struct PickerToolbar {
    base: Div,
}
impl Default for PickerToolbar {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; items_center; gap_1; min_w_0; flex_shrink_0; } },
        }
    }
}
container_traits!(PickerToolbar);
impl RenderOnce for PickerToolbar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

/// The flexible location slot, shared by breadcrumbs and path editing.
#[derive(IntoElement)]
pub struct PickerLocation {
    base: Div,
}
impl Default for PickerLocation {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; items_center; flex_1; min_w_0; overflow_hidden; } },
        }
    }
}
container_traits!(PickerLocation);
impl RenderOnce for PickerLocation {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

/// The only scrolling region. Navigation and decisions remain outside it.
#[derive(IntoElement)]
pub struct PickerContent {
    base: Div,
}
impl Default for PickerContent {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; flex_col; flex_1; min_w_0; min_h_0; overflow_hidden; } },
        }
    }
}
container_traits!(PickerContent);
impl RenderOnce for PickerContent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

/// A stable status row for the current choice and loading progress.
#[derive(IntoElement)]
pub struct PickerStatus {
    base: Div,
}
impl Default for PickerStatus {
    fn default() -> Self {
        Self {
            base: el! { .div { flex; items_center; gap_2; min_w_0; flex_shrink_0; h: crate::appearance::tokens::ICON_BUTTON_SIZE.px; } },
        }
    }
}
container_traits!(PickerStatus);
impl RenderOnce for PickerStatus {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
