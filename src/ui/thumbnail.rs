//! Reusable still preview backed by the shared file inspection cache.
use super::file_inspection::{Cache, State};
use crate::{
    i18n::t,
    service::file_inspection::{Kind, Request, Source},
};
use gds_uim::el;
use gpui_kit::{
    IntoElement,
    assets::IconName,
    component::{ActiveTheme, Icon},
    prelude::*,
    *,
};

#[derive(IntoElement)]
pub struct Thumbnail {
    base: Div,
    cache: Entity<Cache>,
    source: Source,
    fallback: Option<AnyElement>,
    show_status: bool,
}

impl Thumbnail {
    pub fn new(cache: Entity<Cache>, source: Source) -> Self {
        Self {
            base: div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .overflow_hidden(),
            cache,
            source,
            fallback: None,
            show_status: false,
        }
    }

    pub fn fallback(mut self, element: impl IntoElement) -> Self {
        self.fallback = Some(element.into_any_element());
        self
    }

    pub fn show_status(mut self, show: bool) -> Self {
        self.show_status = show;
        self
    }

    fn placeholder(icon: IconName, message: Option<SharedString>, color: Hsla) -> AnyElement {
        el! {
            .div { size_full; flex; flex_col; items_center; justify_center; gap_2; text_color: color; } [
                icon.Icon::new { size: 32.0.px; },
                @if (let Some(message) = message)
                    .div { px_2; text_xs; text_center; } [message],
                @end,
            ]
        }.into_any_element()
    }
}

impl Styled for Thumbnail {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Thumbnail {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.cache.update(cx, |cache, cx| {
            cache
                .request(self.source.clone(), Request::Thumbnail, cx)
                .thumbnail
        });
        let icon = match self.source.kind {
            Kind::Video => IconName::FileVideoCamera,
            Kind::Audio => IconName::FileMusic,
            Kind::Image => IconName::Image,
            _ => IconName::File,
        };
        if let State::Ready(image) = state {
            let color = cx.theme().muted_foreground;
            let failed = self.show_status.then(|| t!(@cx, "gds-thumbnail-failed"));
            let loading = self.show_status.then(|| t!(@cx, "gds-thumbnail-loading"));
            return self.base.child(
                img(image)
                    .object_fit(ObjectFit::Contain)
                    .size_full()
                    .with_loading(move || Self::placeholder(icon, loading.clone(), color))
                    .with_fallback(move || {
                        Self::placeholder(IconName::ImageOff, failed.clone(), color)
                    }),
            );
        }
        let fallback = self
            .fallback
            .unwrap_or_else(|| Icon::new(icon).size(px(32.)).into_any_element());
        let status = match state {
            State::NotRequested | State::Unavailable => SharedString::default(),
            State::Loading => t!(@cx, "gds-thumbnail-loading"),
            State::TooLarge => t!(@cx, "gds-thumbnail-too-large"),
            State::Failed(_) => t!(@cx, "gds-thumbnail-failed"),
            State::Ready(_) => unreachable!("ready thumbnails return their image"),
        };
        el! {
            $(self.base) { gap_2; text_color: $(cx.theme().muted_foreground); } [
                fallback,
                @if (self.show_status && !status.is_empty())
                    .div { px_2; text_xs; text_center; w_full; overflow_hidden; } [status],
                @end,
            ]
        }
    }
}
