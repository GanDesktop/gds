//! Shared capacity meter: the filled part is occupied or reserved; the
//! remaining track is available. Callers supply a localized byte summary.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::view::AttrName;
use zgui::vocab::Role;
use zgui::{component, css, view};

use crate::app::Gds;

pub const EXTRA_ROW_HEIGHT: f32 = 32.;

const CAPACITY_SHEET: &str = "gds-ui-capacity";

const CAPACITY_CSS: &str = css!(
    ".gds-capacity.gds-capacity {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: 5px;
        width: 100%;
        min-width: 0px;
        padding-bottom: 6px;
    }
    .gds-capacity.gds-capacity[data-embedded=\"true\"] {
        padding-bottom: 0px;
    }
    .gds-capacity.gds-capacity[data-compact=\"true\"] {
        width: 100%;
        min-width: 0px;
        align-items: stretch;
    }
    .gds-capacity.gds-capacity .gds-capacity__track {
        width: 100%;
        height: 4px;
        border-radius: 2px;
        border: none;
        background: color-mix(in srgb, var(--gds-foreground) 12%, transparent);
        overflow: hidden;
    }
    .gds-capacity.gds-capacity[data-selected=\"true\"] .gds-capacity__track {
        background: color-mix(in srgb, var(--gds-selection-foreground) 18%, transparent);
    }
    .gds-capacity.gds-capacity[data-unavailable=\"true\"] .gds-capacity__track {
        background: color-mix(in srgb, var(--gds-foreground) 10%, transparent);
    }
    .gds-capacity.gds-capacity .gds-capacity__fill {
        height: 100%;
        width: var(--gds-capacity-width, 0%);
        border-radius: 2px;
        background: var(--gds-primary);
        transform: translateX(0);
        transition: width var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }
    .gds-capacity.gds-capacity[data-selected=\"true\"] .gds-capacity__fill {
        background: var(--gds-primary);
    }
    .gds-capacity.gds-capacity[data-unavailable=\"true\"] .gds-capacity__fill {
        width: 0%;
        opacity: 0;
    }
    .gds-capacity.gds-capacity[data-loading=\"true\"] .gds-capacity__fill {
        width: 40%;
        animation: gds-capacity-indeterminate calc(1200ms * (1 - var(--gds-motion-reduced, 0))) var(--gds-motion-ease-standard, ease) infinite;
    }
    .gds-capacity.gds-capacity[data-loading=\"true\"][data-reduced-motion=\"true\"] .gds-capacity__fill {
        animation: none;
        transform: translateX(0);
    }
    .gds-capacity.gds-capacity .gds-capacity__label {
        height: 17px;
        line-height: 17px;
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .gds-capacity.gds-capacity[data-selected=\"true\"] .gds-capacity__label {
        color: var(--gds-selection-foreground);
    }
    .gds-capacity.gds-capacity[data-compact=\"true\"] .gds-capacity__label {
        display: flex;
        flex-direction: row;
        justify-content: center;
        align-items: center;
        font-size: calc(10px * var(--gds-font-scale, 1));
        font-variant-numeric: tabular-nums;
        text-align: center;
    }"
);

fn install_capacity_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    zgui::view::install_stylesheet(CAPACITY_SHEET, CAPACITY_CSS);
}

fn normalized_fraction(fraction: Option<f32>, unavailable: bool) -> Option<f32> {
    if unavailable {
        return None;
    }
    fraction
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 1.0))
}

fn compact_percent_text(fraction: Option<f32>, unavailable: bool) -> String {
    let Some(used) = normalized_fraction(fraction, unavailable) else {
        return "\u{2014}".to_owned();
    };
    let free = ((1.0 - used).clamp(0.0, 1.0) * 100.0).round();
    format!("{free:.0}%")
}

/// Displays a `4px` borderless capacity bar above a localized byte summary or compact `%` label.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn CapacityBar(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(into, default = Signal::stored_local(None))] fraction: Signal<Option<f32>, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] compact: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] embedded: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] unavailable: Signal<bool, LocalStorage>,
    #[prop(into, default = Signal::stored_local(false))] selected: Signal<bool, LocalStorage>,
    #[prop(optional)] node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_capacity_stylesheets();

    let meter_a11y = A11yBinding::new(Role::ProgressIndicator)
        .label(move || label.get())
        .step(move |a11y| {
            if let Some(used) = normalized_fraction(fraction.get(), unavailable.get()) {
                a11y.numeric_range(0.0, 1.0).numeric_value(f64::from(used))
            } else {
                a11y
            }
        });

    let own = Attrs::new()
        .attribute(AttrName::new("data-compact"), move || {
            compact.get().then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-embedded"), move || {
            embedded.get().then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-unavailable"), move || {
            unavailable.get().then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-loading"), move || {
            (!unavailable.get() && normalized_fraction(fraction.get(), false).is_none())
                .then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-reduced-motion"), move || {
            Gds::try_current()
                .is_some_and(|gds| gds.appearance().motion().reduced())
                .then(|| "true".to_owned())
        })
        .attribute(AttrName::new("data-selected"), move || {
            selected.get().then(|| "true".to_owned())
        })
        .a11y_from(meter_a11y);

    view! {
        column(
            class = "gds-capacity",
            node_ref = node_ref.unwrap_or_default(),
            {..own},
            class = class,
            {..attrs}
        ) {
            box(class = "gds-capacity__track") {
                box(
                    class = "gds-capacity__fill",
                    var:--gds-capacity-width = move || {
                        let pct = normalized_fraction(fraction.get(), unavailable.get())
                            .map_or(0.0, |v| v * 100.0);
                        Some(format!("{pct:.2}%"))
                    }
                )
            }
            text(class = "gds-capacity__label") {{move || {
                if compact.get() {
                    compact_percent_text(fraction.get(), unavailable.get())
                } else {
                    label.get()
                }
            }}}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zgui::app::Fonts;
    use zgui::geom::{DevicePx, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::runtime::AppError;
    use zgui::vocab::Role;
    use zgui_platform_headless::Harness;

    use super::{CapacityBarProps, compact_percent_text, normalized_fraction};
    use crate::app::Gds;
    use crate::service::task_manager::Scope;

    #[test]
    fn normalized_fraction_clamps_and_filters_non_finite() {
        assert_eq!(normalized_fraction(Some(-0.2), false), Some(0.0));
        assert_eq!(normalized_fraction(Some(0.42), false), Some(0.42));
        assert_eq!(normalized_fraction(Some(1.8), false), Some(1.0));
        assert_eq!(normalized_fraction(Some(f32::NAN), false), None);
        assert_eq!(normalized_fraction(Some(f32::INFINITY), false), None);
        assert_eq!(normalized_fraction(Some(0.5), true), None);
    }

    #[test]
    fn compact_percent_reports_remaining_percentage_or_em_dash() {
        assert_eq!(compact_percent_text(Some(0.25), false), "75%");
        assert_eq!(compact_percent_text(Some(0.0), false), "100%");
        assert_eq!(compact_percent_text(Some(1.0), false), "0%");
        assert_eq!(compact_percent_text(None, false), "\u{2014}");
        assert_eq!(compact_percent_text(Some(0.5), true), "\u{2014}");
    }

    #[test]
    fn capacity_bar_exposes_used_fraction_to_assistive_technology() {
        #[derive(Default)]
        struct Sink {
            atlas: zgui::atlas::MemorySink,
            target: Option<zgui::render::RenderTarget>,
        }

        impl zgui::render::Renderer for Sink {
            fn capabilities(&self) -> zgui::render::RenderCapabilities {
                zgui::render::RenderCapabilities::MINIMAL
            }
            fn configure(&mut self, target: zgui::render::RenderTarget) {
                self.target = Some(target);
            }
            fn target(&self) -> Option<zgui::render::RenderTarget> {
                self.target
            }
            fn register_external(
                &mut self,
                _: zgui::render::ExternalTexture,
            ) -> zgui::render::TextureHandle {
                zgui::render::TextureHandle(0)
            }
            fn release_external(&mut self, _: zgui::render::TextureHandle) {}
            fn memory(&self) -> zgui::render::MemoryReport {
                zgui::render::MemoryReport::default()
            }
            fn draw(
                &mut self,
                _: &zgui::scene::Scene,
                _: &zgui::bits::DamageSet,
            ) -> zgui::render::FrameOutcome {
                zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
            }
            fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
                &mut self.atlas
            }
        }

        let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
            let mut harness = Harness::new(handler);
            harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
                DevicePx(300.0),
                DevicePx(200.0),
            )));
            let surface = harness.platform().offscreens()[0].clone();
            surface.set_a11y_listener(true);
            harness.settle(16);

            let meter = surface
                .a11y_log()
                .into_iter()
                .flat_map(|update| update.nodes)
                .map(|(_, node)| node)
                .find(|node| node.role() == Role::ProgressIndicator)
                .expect("capacity meter in the accessibility tree");
            assert_eq!(meter.label(), Some("Drive"));
            assert_eq!(meter.numeric_value(), Some(0.25));

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_size(300.0, 200.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = Sink::default();
                zgui::render::Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                let gds = Gds::try_current().unwrap_or_else(|| Gds::install(Scope::default()));
                gds.appearance().motion().set_enabled(false);
                zgui::view! {
                    CapacityBar(label = "Drive", fraction = Signal::stored_local(Some(0.25)))
                }
            })
            .expect("headless capacity meter");
    }
}
