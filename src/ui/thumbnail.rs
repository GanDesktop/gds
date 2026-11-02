//! Shared asynchronous media cache and a reusable still-preview component.
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use gds_uim::el;
use gpui_kit::{
    IntoElement,
    assets::IconName,
    component::{ActiveTheme, Icon},
    prelude::*,
    *,
};

pub use crate::service::thumbnail::{Kind, Source};
use crate::{i18n::t, service::thumbnail::Error};

const CAPACITY: usize = 256;
const BYTE_BUDGET: usize = 32 * 1024 * 1024;
const WORKERS: usize = 2;

#[derive(Clone)]
pub enum State {
    Loading,
    Ready(Arc<Image>),
    TooLarge,
    Failed(SharedString),
}

struct Entry {
    state: State,
    touched: u64,
    ticket: u64,
    bytes: usize,
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<Source, Entry>,
    pending: VecDeque<Source>,
    running: usize,
    clock: u64,
    bytes: usize,
}

impl Cache {
    pub fn request(&mut self, source: Source, cx: &mut Context<Self>) -> State {
        if self.clock == 0 {
            cx.on_release(|cache, cx| {
                for (_, entry) in cache.entries.drain() {
                    if let State::Ready(image) = entry.state {
                        image.remove_asset(cx);
                    }
                }
            })
            .detach();
        }
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(&source) {
            entry.touched = self.clock;
            return entry.state.clone();
        }
        self.entries.insert(
            source.clone(),
            Entry {
                state: State::Loading,
                touched: self.clock,
                ticket: self.clock,
                bytes: 0,
            },
        );
        self.pending.push_back(source);
        self.trim(cx);
        self.pump(cx);
        State::Loading
    }

    fn trim(&mut self, cx: &mut App) {
        while self.entries.len() > CAPACITY || self.bytes > BYTE_BUDGET {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.bytes;
                if let State::Ready(image) = entry.state {
                    image.remove_asset(cx);
                }
            }
        }
        self.pending
            .retain(|source| self.entries.contains_key(source));
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        while self.running < WORKERS {
            let Some(source) = self.pending.pop_front() else {
                break;
            };
            let Some(ticket) = self.entries.get(&source).map(|entry| entry.ticket) else {
                continue;
            };
            self.running += 1;
            let work_source = source.clone();
            let work = cx
                .background_executor()
                .spawn(async move { work_source.load() });
            cx.spawn(async move |cache, cx| {
                let result = work.await;
                if let Err(error) = cache.update(cx, |cache, cx| {
                    cache.running -= 1;
                    if let Some(entry) = cache.entries.get_mut(&source).filter(|entry| entry.ticket == ticket) {
                        entry.state = match result {
                            Ok(thumbnail) => {
                                entry.bytes = thumbnail.bytes.len();
                                cache.bytes += entry.bytes;
                                State::Ready(Arc::new(Image::from_bytes(ImageFormat::Png, thumbnail.bytes)))
                            },
                            Err(Error::TooLarge) => State::TooLarge,
                            Err(error) => {
                                tracing::debug!(path = %source.path.display(), %error, "media thumbnail unavailable");
                                State::Failed(error.to_string().into())
                            }
                        };
                    }
                    cache.trim(cx);
                    cache.pump(cx);
                    cx.notify();
                }) {
                    tracing::debug!(%error, "thumbnail owner closed before completion");
                }
            }).detach();
        }
    }
}

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
        let state = self
            .cache
            .update(cx, |cache, cx| cache.request(self.source.clone(), cx));
        let icon = if self.source.kind == Kind::Video {
            IconName::FileVideoCamera
        } else {
            IconName::Image
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
            State::Loading => t!(@cx, "gds-thumbnail-loading"),
            State::TooLarge => t!(@cx, "gds-thumbnail-too-large"),
            State::Failed(_) => t!(@cx, "gds-thumbnail-failed"),
            State::Ready(_) => unreachable!("ready thumbnails return their image"),
        };
        el! {
            $(self.base) { gap_2; text_color: $(cx.theme().muted_foreground); } [
                fallback,
                @if (self.show_status)
                    .div { px_2; text_xs; text_center; w_full; overflow_hidden; } [status],
                @end,
            ]
        }
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
