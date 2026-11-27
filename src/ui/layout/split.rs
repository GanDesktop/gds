//! A vertical split whose preferred height survives temporary space constraints.

mod handle;
mod state;
#[cfg(test)]
mod tests;

pub use handle::{SplitHandle, SplitHandleProps};
pub use state::{Options, State, constrained_height};

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback};
use zgui::{component, css, view};

pub const DIVIDER_HEIGHT: f32 = 9.;
pub const KEY_STEP: f32 = 12.;

const SPLIT_SHEET: &str = "gds-ui-split";

const SPLIT_CSS: &str = css!(
    ".gds-split {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }
    .gds-split__leading {
        display: flex;
        flex-direction: column;
        flex: 1 1 0px;
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }
    .gds-split__trailing {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        height: var(--gds-split-size, 220px);
        min-width: 0px;
        min-height: 0px;
        overflow: hidden;
    }
    .gds-split__handle-slot {
        position: relative;
        z-index: 10;
        width: 100%;
        height: 1px;
        flex-shrink: 0;
    }
    .gds-split__trigger.gds-split__trigger {
        position: absolute;
        inset: 0px;
        display: block;
        width: 100%;
        height: 1px;
    }
    .gds-split__divider-bg {
        position: absolute;
        inset: 0px;
        pointer-events: none;
    }
    .gds-split__passive-line {
        position: absolute;
        left: 0px;
        right: 0px;
        top: 0px;
        height: 1px;
        background-color: var(--gds-border);
        pointer-events: none;
    }
    .gds-split__handle {
        position: absolute;
        left: 0px;
        right: 0px;
        top: -4px;
        height: 9px;
        display: flex;
        align-items: center;
        justify-content: center;
        background-color: transparent;
        cursor: ns-resize;
    }
    .gds-split__line {
        position: absolute;
        left: 0px;
        right: 0px;
        top: 4px;
        height: 1px;
        background-color: var(--gds-border);
        transition: background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-split__handle:hover .gds-split__line,
    .gds-split__handle[data-dragging=\"true\"] .gds-split__line {
        background-color: var(--gds-primary);
    }
    .gds-split__handle:focus-visible {
        outline: none;
    }
    .gds-split__handle:focus-visible .gds-split__line {
        outline: 1px solid var(--gds-primary);
        outline-offset: 0px;
        background-color: var(--gds-primary);
    }"
);

pub(super) fn install_split_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(SPLIT_SHEET, SPLIT_CSS);
}

#[derive(Clone)]
pub(super) struct SplitContext {
    pub(super) container: NodeRef,
    pub(super) label: Option<String>,
    pub(super) on_commit: Option<UnsyncCallback<f32>>,
}

/// Vertical split pane hosting leading/trailing views separated by a draggable handle, supporting
/// continuous `160ms` `bottom_progress` reveal, `collapsed_bottom_height`, `bottom_maximized_progress`,
/// and custom `divider_background` column continuations.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn VerticalSplit(
    state: State,
    #[prop(optional)] on_commit: Option<UnsyncCallback<f32>>,
    #[prop(into, optional)] label: Option<String>,
    #[prop(optional)] divider_background: Option<ChildrenFn>,
    #[prop(into, optional)] bottom_progress: Option<Signal<f32, LocalStorage>>,
    #[prop(default = 0.0)] collapsed_bottom_height: f32,
    #[prop(into, optional)] bottom_maximized_progress: Option<Signal<f32, LocalStorage>>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    leading: ChildrenFn,
    trailing: ChildrenFn,
) -> impl IntoView {
    install_split_stylesheets();
    let container = NodeRef::new();
    provide_local_context(SplitContext {
        container,
        label,
        on_commit,
    });

    let collapsed = state.collapsed();
    let width = state.width();
    let progress = Signal::derive_local(move || match bottom_progress {
        Some(sig) => sig.get().clamp(0.0, 1.0),
        None => {
            if collapsed.get() {
                0.0
            } else {
                1.0
            }
        }
    });
    let maximized = Signal::derive_local(move || {
        bottom_maximized_progress.map_or(0.0, |sig| sig.get().clamp(0.0, 1.0))
    });
    let resizable = Signal::derive_local(move || {
        progress.get() >= 1.0 - 1e-4 && maximized.get() <= 1e-4 && !collapsed.get()
    });
    let show_divider =
        Signal::derive_local(move || progress.get() > 1e-4 && maximized.get() < 1.0 - 1e-4);

    let split_size = move || {
        let p = progress.get();
        let m = maximized.get();
        let target_h = if collapsed.get() && bottom_progress.is_none() {
            collapsed_bottom_height
        } else {
            collapsed_bottom_height + (width.get() - collapsed_bottom_height) * p
        };
        let max_mix = (m * p).clamp(0.0, 1.0);
        if max_mix <= 1e-4 {
            Some(format!("{target_h:.2}px"))
        } else {
            let base_weight = 1.0 - max_mix;
            Some(format!(
                "calc({:.2}px * {base_weight:.4} + 100% * {max_mix:.4})",
                target_h
            ))
        }
    };

    view! {
        column(
            node_ref = container,
            class = "gds-split",
            attr:data-collapsed = move || (progress.get() <= 1e-4).then(|| "true".to_owned()),
            class = class,
            {..attrs}
        ) {
            box(class = "gds-split__leading") {{leading.view()}}
            Show(when = move || show_divider.get()) {
                {{
                    let bg = divider_background.clone();
                    view! {
                        box(class = "gds-split__handle-slot") {
                            Show(when = move || !resizable.get()) {
                                box(class = "gds-split__passive-line", a11y:hidden = true) {}
                            }
                            {bg.map(|render_bg| AnyView::new(view! {
                                box(class = "gds-split__divider-bg", a11y:hidden = true) {
                                    {render_bg.view()}
                                }
                            }))}
                            Show(when = move || resizable.get()) {
                                SplitHandle(state = state)
                            }
                        }
                    }
                }}
            }
            box(
                class = "gds-split__trailing",
                var:--gds-split-size = split_size
            ) {{trailing.view()}}
        }
    }
}

/// Optional change payload emitted when split dragging commits a height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Changed(pub f32);
