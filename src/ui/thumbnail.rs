//! Reusable still preview backed by the shared file inspection cache and icon fallback.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::view::ChildrenFn;
use zgui::{component, css, view};

use super::file_inspection::{Cache, State};
use crate::app::Gds;
use crate::service::file_inspection::{Kind, Request, Source};
use crate::ui::icon::{IconName, IconProps};

const SHEET: &str = css!(
    r#"
    .gds-thumbnail.gds-thumbnail {
        width: 100%;
        height: 100%;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        position: relative;
        overflow: hidden;
    }

    .gds-thumbnail__img {
        width: 100%;
        height: 100%;
        object-fit: contain;
    }

    .gds-thumbnail__fallback {
        width: 100%;
        height: 100%;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 8px;
        color: var(--gds-muted-foreground);
    }

    .gds-thumbnail__status {
        width: 100%;
        padding: 0px 8px;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        text-align: center;
        overflow: hidden;
        overflow-wrap: anywhere;
    }
    "#
);

/// Reusable view builder invoked whenever [`Thumbnail`] renders fallback artwork.
#[derive(Clone)]
pub struct ViewFn(ChildrenFn);

impl ViewFn {
    #[must_use]
    pub fn new<V: IntoView + 'static>(build: impl Fn() -> V + 'static) -> Self {
        Self(ChildrenFn::new(move || AnyView::new(build())))
    }

    #[must_use]
    pub fn render(&self) -> AnyView {
        self.0.view()
    }
}

impl<F, V> From<F> for ViewFn
where
    F: Fn() -> V + 'static,
    V: IntoView + 'static,
{
    fn from(build: F) -> Self {
        Self::new(build)
    }
}

impl From<ChildrenFn> for ViewFn {
    fn from(children: ChildrenFn) -> Self {
        Self(children)
    }
}

impl From<ViewFn> for ChildrenFn {
    fn from(view: ViewFn) -> Self {
        view.0
    }
}

fn fallback_icon(kind: Kind) -> IconName {
    match kind {
        Kind::Image => IconName::Image,
        Kind::Video => IconName::FileVideoCamera,
        Kind::Audio => IconName::FileMusic,
        Kind::Directory => IconName::Folder,
        _ => IconName::File,
    }
}

fn status_label(key: &'static str, fallback: &'static str) -> String {
    Gds::try_current().map_or_else(|| fallback.to_owned(), |gds| gds.i18n().text(key))
}

/// Renders a media thumbnail for `source` strictly from cached [`zgui_image::ImageBytes::url`],
/// falling back to `fallback` or a typed [`Icon`] without bypassing `file_inspection` limits.
#[component]
pub fn Thumbnail(
    cache: Cache,
    #[prop(into)] source: Signal<Source, LocalStorage>,
    #[prop(into, optional)] fallback: Option<ViewFn>,
    #[prop(default = false)] show_status: bool,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    if zgui::view::current_host().is_some() {
        crate::ui::styles::install_ui_stylesheets();
        zgui::view::install_stylesheet("gds-ui-thumbnail", SHEET);
    }

    view! {
        box(class = "gds-thumbnail", class = class, {..attrs}) {
            {move || {
                let src = source.get();
                let snap = cache.snapshot(&src, Request::Thumbnail);
                if let State::Ready(bytes) = snap.thumbnail {
                    return AnyView::new(view! {
                        image(class = "gds-thumbnail__img", src = {Some(bytes.url())})
                    });
                }

                let artwork = match (&fallback, &snap.thumbnail) {
                    (Some(custom), _) => custom.render(),
                    (None, State::TooLarge) => {
                        AnyView::new(view! { Icon(name = IconName::ImageOff, size = 32.0) })
                    }
                    (None, _) => {
                        AnyView::new(view! { Icon(name = fallback_icon(src.kind), size = 32.0) })
                    }
                };

                let status = show_status
                    .then(|| match snap.thumbnail {
                        State::NotRequested | State::Unavailable => None,
                        State::Loading => {
                            Some(status_label("gds-thumbnail-loading", "Loading preview…"))
                        }
                        State::TooLarge => Some(status_label(
                            "gds-thumbnail-too-large",
                            "This image is too large to preview",
                        )),
                        State::Failed(_) => {
                            Some(status_label("gds-thumbnail-failed", "Preview unavailable"))
                        }
                        State::Ready(_) => unreachable!("ready thumbnails return above"),
                    })
                    .flatten();

                AnyView::new(view! {
                    column(class = "gds-thumbnail__fallback") {
                        {artwork}
                        {status.map(|label| view! { text(class = "gds-thumbnail__status") {{label}} }.into_view())}
                    }
                })
            }}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use zgui::app::Fonts;
    use zgui::geom::{DevicePx, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::runtime::AppError;
    use zgui_platform_headless::Harness;

    use super::{ThumbnailProps, ViewFn};
    use crate::service::file_inspection::{Kind, Source};
    use crate::ui::file_inspection::{Cache, Options};

    #[test]
    fn unrequested_and_unavailable_thumbnails_omit_status_and_render_custom_fallback() {
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
                DevicePx(200.0),
                DevicePx(200.0),
            )));
            if let Some(surface) = harness.platform().offscreens().first() {
                surface.set_a11y_listener(true);
            }
            harness.settle(8);

            let surface = harness.platform().offscreens()[0].clone();
            let mut nodes = HashMap::new();
            for update in surface.a11y_log() {
                for (id, node) in update.nodes {
                    nodes.insert(id, node);
                }
            }

            let texts: Vec<String> = nodes
                .values()
                .filter_map(|n| n.value().or_else(|| n.label()).map(str::to_owned))
                .collect();
            assert!(
                texts.iter().any(|t| t == "Custom fallback artwork"),
                "expected custom fallback view in {texts:?}"
            );
            assert!(
                !texts.iter().any(|t| t == "Preview unavailable"),
                "NotRequested/Unavailable must not show 'Preview unavailable' in {texts:?}"
            );
            assert!(
                nodes.values().all(|n| format!("{:?}", n.role()) != "Image"),
                "NotRequested/Unavailable with text fallbacks must mount zero Image nodes"
            );

            harness.shut_down();
            Ok(())
        };

        zgui::app()
            .with_fonts(Fonts::shipped_only())
            .with_size(200.0, 200.0)
            .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
                let mut sink = Sink::default();
                zgui::render::Renderer::configure(&mut sink, target);
                Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
            }))
            .run_on(driver, move || {
                let disabled_cache = Cache::new(Options {
                    capacity: 0,
                    workers: 0,
                    ..Options::default()
                });
                let image_src = Source {
                    path: PathBuf::from("/tmp/unrequested-huge.png"),
                    kind: Kind::Image,
                    size: Some(64),
                    modified: None,
                };
                let folder_src = Source {
                    path: PathBuf::from("/tmp/folder"),
                    kind: Kind::Directory,
                    size: None,
                    modified: None,
                };
                zgui::view! {
                    column {
                        Thumbnail(
                            cache = disabled_cache.clone(),
                            source = image_src,
                            show_status = true,
                            fallback = ViewFn::new(|| zgui::view! { text { "Custom fallback artwork" } }),
                        )
                        Thumbnail(
                            cache = disabled_cache,
                            source = folder_src,
                            show_status = true,
                            fallback = ViewFn::new(|| zgui::view! { text { "Unavailable folder fallback" } }),
                        )
                    }
                }
            })
            .expect("headless Thumbnail test ran");
    }
}
