//! Capacity meter with separate ready, loading, and unavailable states.

use anti_webui::prelude::*;
use gds_uim::el;

const BAR_HEIGHT: f32 = 4.0;
const LABEL_HEIGHT: f32 = 17.0;
const GAP: f32 = 5.0;

#[derive(Clone, Copy)]
pub enum CapacityStatus {
    Loading,
    Unavailable,
    Used(f32),
}

pub struct CapacityBar {
    label: String,
    status: CapacityStatus,
    compact: Binding<bool>,
    selected: Binding<bool>,
}

impl CapacityBar {
    pub const HEIGHT: f32 = BAR_HEIGHT + LABEL_HEIGHT + GAP;

    pub fn new(label: impl Into<String>, status: CapacityStatus) -> Self {
        Self {
            label: label.into(),
            status,
            compact: Binding::new(|| false, |_| {}),
            selected: Binding::new(|| false, |_| {}),
        }
    }

    pub fn compact(mut self, compact: Binding<bool>) -> Self {
        self.compact = compact;
        self
    }

    pub fn selected(mut self, selected: Binding<bool>) -> Self {
        self.selected = selected;
        self
    }
}

impl IntoView for CapacityBar {
    fn into_view(self) -> View {
        let label = self.label;
        let compact = self.compact;
        let status = self.status;
        let selected = self.selected;
        let bar = match status {
            CapacityStatus::Loading => indeterminate_progress_bar(),
            CapacityStatus::Used(fraction) => progress_bar(move || fraction),
            CapacityStatus::Unavailable => div()
                .rounded(2.0_f32)
                .dyn_style(|style| style.bg(theme().surface_muted)),
        };
        el! { .div.{flex; flex_col; w_full; min_w: 0.0_f32; gap: GAP}.[
            bar.{h: BAR_HEIGHT; w_full},
            (move () => ${
                if compact.get() {
                    match status {
                        CapacityStatus::Used(fraction) => format!("{:.0}%", (1.0 - fraction) * 100.0),
                        _ => "—".to_owned(),
                    }
                } else { label.clone() }
            }).text.{h: LABEL_HEIGHT; line_height: $(LineHeight::Px(LABEL_HEIGHT)); ellipsis; whitespace_nowrap;
                dyn_style: move style => $(style
                    .text_color(if selected.get() {crate::app::Gds::current().appearance().theme().colors().selection_foreground} else {theme().text_muted})
                    .text_size(if compact.get() {10.0_f32} else {12.0_f32})
                    .text_align(if compact.get() {TextAlign::Center} else {TextAlign::Start}));
            },
        ] }.into_view()
    }
}
