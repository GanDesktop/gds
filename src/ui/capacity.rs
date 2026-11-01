//! Shared capacity meter: the filled part is occupied or reserved; the
//! remaining track is available. Callers supply a localized byte summary.

use gds_uim::{component, el};
use gpui_kit::{
    component::{ActiveTheme, Sizable, progress::Progress},
    prelude::*,
    *,
};

const GAP: f32 = 5.;
const PADDING_BOTTOM: f32 = 6.;
pub const EXTRA_ROW_HEIGHT: f32 = 32.;

#[component]
#[derive(IntoElement)]
pub struct CapacityBar {
    id: ElementId,
    #[attr(into)]
    label: SharedString,
    fraction: Option<f32>,
    #[attr]
    compact: bool,
    unavailable: bool,
    embedded: bool,
    #[attr]
    selected: bool,
}

impl CapacityBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: SharedString::default(),
            fraction: None,
            compact: false,
            unavailable: false,
            embedded: false,
            selected: false,
        }
    }

    pub fn value(mut self, fraction: f32) -> Self {
        self.fraction = fraction.is_finite().then(|| fraction.clamp(0., 1.));
        self
    }

    pub fn unavailable(mut self) -> Self {
        self.unavailable = true;
        self
    }

    /// Let the containing cell supply spacing around the entire meter/label.
    pub fn embedded(mut self) -> Self {
        self.embedded = true;
        self
    }
}

impl RenderOnce for CapacityBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let bar = if self.unavailable {
            el! { .div { w_full; h: 4.0.px; rounded: 2.0.px; bg: $(cx.theme().muted) } }
                .into_any_element()
        } else {
            Progress::new(self.id)
                .xsmall()
                .value(self.fraction.unwrap_or_default() * 100.)
                .loading(self.fraction.is_none())
                .accessibility_label(self.label.clone())
                .into_any_element()
        };
        el! {
            .div { w_full; min_w_0; flex; flex_col; gap: GAP.px;
                @if (!self.embedded) pb: PADDING_BOTTOM.px; @end;
            } [
                bar,
                .div { text_xs; text_color: $(if self.selected { cx.theme().accent_foreground } else { cx.theme().muted_foreground }); truncate;
                    @if (self.compact) invisible; @end;
                } [$(self.label)],
            ]
        }
    }
}
