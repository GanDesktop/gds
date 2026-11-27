//! Native cross-window drag preview rendering via `zgui`'s offscreen view pipeline.

use zgui::geom::{CssPx, Edges, Point};
use zgui::prelude::{DragIcon, IntoView, ViewSnapshotOptions, WindowHandle};

use super::{CARD_WIDTH, POINTER_GAP};

/// Maximum number of individual items enumerated inside a multi-file drag card before showing `…`.
pub const MAX_PREVIEW_ITEMS: usize = 3;

/// Transparent margin around a [`DragCardSurface`](super::DragCardSurface) snapshot so its CSS
/// `box-shadow` (`0 8px 24px rgba(0, 0, 0, 0.24)`) renders cleanly without bitmap edge clipping.
const CARD_SHADOW_PADDING: Edges<CssPx> =
    Edges::new(CssPx(20.0), CssPx(24.0), CssPx(32.0), CssPx(24.0));

/// Maximum logical height reserved while measuring a drag card view offscreen, in CSS pixels.
const CARD_MAX_HEIGHT: f32 = 320.0;

/// Renders a real `zgui` drag card view offscreen on `window` into a HiDPI native [`DragIcon`].
///
/// The snapshot inherits `window`'s installed stylesheets (`--gds-*` theme tokens, typography and
/// component CSS), active colour scheme, font collection and HiDPI scale, and automatically
/// offsets the hotspot so the card's border box starts [`POINTER_GAP`] CSS pixels to the
/// bottom-right of the pointer.
#[must_use]
pub fn render_drag_card_icon<V: IntoView>(
    window: &WindowHandle,
    view: impl FnOnce() -> V,
) -> Option<DragIcon> {
    let options = ViewSnapshotOptions::new()
        .with_max_size(
            CARD_WIDTH + CARD_SHADOW_PADDING.left.0 + CARD_SHADOW_PADDING.right.0,
            CARD_MAX_HEIGHT,
        )
        .with_padding(CARD_SHADOW_PADDING);
    match window.render_view(options, view) {
        Ok(rendered) => {
            Some(rendered.into_drag_icon(Point::new(CssPx(-POINTER_GAP), CssPx(-POINTER_GAP))))
        }
        Err(error) => {
            tracing::debug!(%error, "offscreen drag card snapshot unavailable");
            None
        }
    }
}
