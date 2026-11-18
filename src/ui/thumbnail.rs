//! A still preview using the shared inspection cache and a freshly owned fallback.
use super::{
    Icon,
    file_inspection::{Cache, State},
};
use crate::service::file_inspection::{Kind, Request, Source};
use anti_webui::prelude::*;
use gds_uim::el;
pub struct Thumbnail {
    base: Div,
    cache: Cache,
    source: Source,
    fallback: Option<Box<dyn Fn() -> View>>,
    show_status: bool,
}
impl Thumbnail {
    pub fn new(cache: Cache, source: Source) -> Self {
        Self {
            base: el! { .div.{ flex; flex_col; items_center; justify_center; size_full; overflow_hidden } },
            cache,
            source,
            fallback: None,
            show_status: false,
        }
    }
    pub fn fallback<V: IntoView>(mut self, factory: impl Fn() -> V + 'static) -> Self {
        self.fallback = Some(Box::new(move || factory().into_view()));
        self
    }
    pub fn show_status(mut self, show: bool) -> Self {
        self.show_status = show;
        self
    }
}
impl Styled for Thumbnail {
    fn style_mut(&mut self) -> &mut Style {
        self.base.style_mut()
    }
}
impl IntoView for Thumbnail {
    fn into_view(self) -> View {
        let Self {
            base,
            cache,
            source,
            fallback,
            show_status,
        } = self;
        let icon = match source.kind {
            Kind::Image => Icon::Image,
            Kind::Audio => Icon::Audio,
            Kind::Video => Icon::Video,
            _ => Icon::File,
        };
        let contents = dyn_view(move || {
            match cache.request(source.clone(), Request::Thumbnail).thumbnail {
                State::Ready(bitmap) => el! { (move () => $(Some(bitmap.clone()))).bitmap_view.{ size_full; flex_none; object_fit: ObjectFit::Contain } }.into_view(),
                state => {
                    let status = match state { State::Loading => "gds-thumbnail-loading", State::TooLarge => "gds-thumbnail-too-large", State::Failed(_) => "gds-thumbnail-failed", _ => "" };
                    let fallback = fallback.as_ref().map_or_else(|| icon.view(32.0).into_view(), |factory| factory());
                    el! { .div.{ flex; flex_col; items_center; justify_center; gap: 8.0_f32; size_full }.[
                        fallback,
                        if $(show_status && !status.is_empty()) [(move () => status.crate::i18n::text).text.{
                            px: 8.0_f32; w_full; text_size: 12.0_f32; text_align: TextAlign::Center;
                        }],
                    ] }.into_view()
                }
            }
        });
        base.child(contents).into_view()
    }
}
