//! Optional right-hand preview of the single selected entry.
//!
//! The panel carries the entry's name, shared metadata and content properties.
//! Content metadata and thumbnails share a background cache. There is no location bar:
//! the file's own path is not repeated here. A symlink's target is different
//! information and is shown as its own row.

use gds::app::AppExt as _;

mod info;
mod metadata;
use metadata::{MetadataRow, MetadataTags};

use gds::ui::file_inspection::{Cache, State};
use gds::ui::thumbnail::Thumbnail;
use std::sync::Arc;

use gds::appearance::tokens::{self, TOOLTIP_DELAY};
use gds::i18n;
use gds::ui::page::{PageHeader, PageTitle};
use gds::ui::time::Timestamp;
use gds_uim::{component, el};
use gpui_kit::IntoElement;
use gpui_kit::assets::IconName;
use gpui_kit::component::empty::{Empty as EmptyState, EmptyDescription, EmptyMedia};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::component::{
    Sizable,
    description_list::{DescriptionItem, DescriptionList},
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Browser;
use crate::entities::file::{
    directory::{Entry, EntryKind},
    ui::EntryPresentation,
};
use gds::service::file_inspection::{
    AudioFacts, DirectoryFacts, FileFacts, ImageFacts, Metadata, Request, TextFacts, VideoFacts,
};
use info::LineEndingLabel;

const MEDIA_HEIGHT: f32 = 168.;
const PANEL_GAP: f32 = 10.;
const PANEL_PADDING: f32 = 16.;
const META_LABEL_WIDTH: f32 = 76.;
const MEDIA_RADIUS: f32 = 8.;
const ICON_SIZE: f32 = 30.;
/// Values longer than this get a tooltip so a truncated row stays readable.
const LONG_VALUE_CHARS: usize = 28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) enum Target {
    Empty,
    Multiple(usize),
    Item(Item),
}

#[derive(Clone, Debug)]
pub(in crate::pages::workspace) struct Item {
    entry: Entry,
}

impl Item {
    pub(in crate::pages::workspace) fn from_entry(entry: &Entry) -> Self {
        Self {
            entry: entry.clone(),
        }
    }

    /// Whether the entry's bytes and timestamps are unchanged, so the previous
    /// probe and thumbnail can stay. Permission or ownership changes keep them.
    fn same_source(&self, other: &Self) -> bool {
        self.entry.path == other.entry.path
            && self.entry.kind == other.entry.kind
            && self.entry.size == other.entry.size
            && self.entry.modified == other.entry.modified
    }
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.same_source(other) && self.entry.details == other.entry.details
    }
}

impl Eq for Item {}

pub(in crate::pages::workspace) struct View {
    target: Target,
    inspections: Entity<Cache>,
    tasks: Option<gds::service::task_manager::Scope>,
    tags_enabled: bool,
    tags: Option<Result<Vec<String>, String>>,
    tags_task: Option<Task<()>>,
}

impl View {
    pub(in crate::pages::workspace) fn new(
        inspections: Entity<Cache>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&inspections, |_, _, cx| cx.notify()).detach();
        Self {
            target: Target::Empty,
            inspections,
            tasks: None,
            tags_enabled: false,
            tags: None,
            tags_task: None,
        }
    }

    pub(in crate::pages::workspace) fn set_tasks(
        &mut self,
        tasks: gds::service::task_manager::Scope,
    ) {
        if self.tasks.is_none() {
            self.tasks = Some(tasks);
        }
    }

    pub(in crate::pages::workspace) fn show_entry(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.sync(Target::Item(Item::from_entry(&entry)), cx);
    }

    /// Point the preview at a selection; the shared cache owns background work.
    pub(in crate::pages::workspace) fn sync(&mut self, target: Target, cx: &mut Context<Self>) {
        if self.target == target {
            return;
        }
        self.target = target;
        self.request_tags(cx);
        self.request(cx);
        cx.notify();
    }

    pub(in crate::pages::workspace) fn set_tags_enabled(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if self.tags_enabled != enabled {
            self.tags_enabled = enabled;
            self.request_tags(cx);
            cx.notify();
        }
    }

    fn request_tags(&mut self, cx: &mut Context<Self>) {
        self.tags_task = None;
        self.tags = None;
        if !self.tags_enabled {
            return;
        }
        let Target::Item(item) = &self.target else {
            return;
        };
        if !matches!(item.entry.kind, EntryKind::File | EntryKind::Directory) {
            return;
        }
        let path = item.entry.path.clone();
        let scope = self
            .tasks
            .clone()
            .unwrap_or_else(|| cx.gds().tasks().scope());
        let work = scope.spawn_blocking_named("Read file tags", move || {
            gds::service::file_tags::read(&path).map_err(|e| e.to_string())
        });
        self.tags_task = Some(cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(|error| error.to_string())
                .and_then(std::convert::identity);
            let _ = this.update(cx, |this, cx| {
                this.tags = Some(result);
                cx.notify();
            });
        }));
    }

    fn request(&self, cx: &mut Context<Self>) {
        let Target::Item(item) = &self.target else {
            return;
        };
        let source = item.entry.inspection_source();
        let request = if source.kind.supports_thumbnail() {
            Request::Both
        } else {
            Request::Metadata
        };
        self.inspections.update(cx, |cache, cx| {
            cache.request(source, request, cx);
        });
    }

    fn metadata(&self, cx: &App) -> Option<Arc<Metadata>> {
        let Target::Item(item) = &self.target else {
            return None;
        };
        let snapshot = self
            .inspections
            .read(cx)
            .get(&item.entry.inspection_source())?;
        match snapshot.metadata {
            State::Ready(metadata) => Some(metadata),
            _ => None,
        }
    }

    pub(in crate::pages::workspace) fn clear(&mut self, cx: &mut Context<Self>) {
        self.target = Target::Empty;
        self.tags_task = None;
        self.tags = None;
        cx.notify();
    }

    fn media(&self, cx: &App) -> Option<AnyElement> {
        let Target::Item(item) = &self.target else {
            return None;
        };
        let source = item.entry.thumbnail_source()?;
        let media = el! { ($(self.inspections.clone()), source).Thumbnail::new {
            show_status: true;
        } };
        Some(
            el! {
                .div { h: MEDIA_HEIGHT.px; w_full; flex_shrink_0;
                    rounded: MEDIA_RADIUS.px; border_1;
                    border_color: $(cx.theme().border); bg: $(cx.theme().secondary);
                    overflow_hidden;
                } [media]
            }
            .into_any_element(),
        )
    }

    fn item(&self, item: &Item, cx: &App) -> AnyElement {
        let mut rows = self.base_rows(item, cx);
        if let Some(metadata) = self.metadata(cx) {
            rows.extend(self.probe_rows(&metadata, cx));
        }
        el! {
            .div { flex; flex_col; flex_1; min_h_0; min_w_0; w_full; overflow_hidden } [
                .div {
                    # "details-body"; flex_1; min_h_0; w_full; overflow_y_scroll;
                    flex; flex_col; p: PANEL_PADDING.px; gap: PANEL_PADDING.px;
                } [
                    @if (let Some(media) = self.media(cx))
                        media,
                    @end,
                    .div { flex_shrink_0; w_full; min_w_0 } [
                        .DescriptionList::new { columns: 1; bordered: false; large;
                            label_width: $px(META_LABEL_WIDTH + MEDIA_RADIUS); children: rows;
                        },
                    ],
                ],
            ]
        }
        .into_any_element()
    }

    /// Name-adjacent metadata shared by every entry kind.
    fn base_rows(&self, item: &Item, cx: &App) -> Vec<DescriptionItem> {
        let mut rows = Vec::new();
        if self.tags_enabled && matches!(item.entry.kind, EntryKind::File | EntryKind::Directory) {
            if let Some(Ok(tags)) = &self.tags
                && !tags.is_empty()
            {
                rows.push(
                    el! { .MetadataRow::default {
                        label: $(i18n::text("tags-title", cx));
                        value: $(el! { .MetadataTags::default { tags: $(tags.clone()); } });
                    } }
                    .into(),
                );
            } else {
                let text: SharedString = match &self.tags {
                    Some(Ok(tags)) if !tags.is_empty() => tags.join(", ").into(),
                    Some(Ok(_)) => i18n::text("tags-none", cx),
                    Some(Err(error)) => error.clone().into(),
                    None => i18n::text("tags-loading", cx),
                };
                rows.push(el! { .MetadataRow::default { label: $(i18n::text("tags-title", cx)); text: text; } }.into());
            }
        }
        let kind = EntryPresentation::type_label(&item.entry, cx);
        rows.push(
            el! { .MetadataRow::default { label: $(i18n::text("type", cx)); text: kind; } }.into(),
        );
        if let Some(format) = self.format_label(item, cx) {
            rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-format", cx)); text: format; } }.into());
        }
        if item.entry.kind == EntryKind::File {
            let size: SharedString = EntryPresentation::size(&item.entry).into();
            rows.push(
                el! { .MetadataRow::default { label: $(i18n::text("size", cx)); text: size; } }
                    .into(),
            );
        }
        if item.entry.details.owner.is_some() {
            let owner = EntryPresentation::owner(&item.entry, cx).text;
            rows.push(
                el! { .MetadataRow::default { label: $(i18n::text("owner", cx)); text: owner; } }
                    .into(),
            );
        }
        if let Some(group) = item.entry.details.group.as_ref() {
            let group: SharedString = group.label().into_owned().into();
            rows.push(
                el! { .MetadataRow::default { label: $(i18n::text("group", cx)); text: group; } }
                    .into(),
            );
        }
        if let Some(permissions) = item.entry.details.permissions.as_ref() {
            let value: SharedString = info::permissions(permissions).into();
            rows.push(el! { .MetadataRow::default { label: $(i18n::text("permissions", cx)); text: value; } }.into());
        }
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("modified", cx)); value: $(Timestamp::new("details-modified", item.entry.modified)); } }.into());
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("accessed", cx)); value: $(Timestamp::new("details-accessed", item.entry.details.accessed)); } }.into());
        if let Some(created) = item.entry.details.created {
            rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-created", cx)); value: $(Timestamp::new("details-created", Some(created))); } }.into());
        }
        rows
    }

    /// Concrete format from the probe, falling back to the listing's content.
    fn format_label(&self, item: &Item, cx: &App) -> Option<SharedString> {
        if let Some(Metadata::File(FileFacts::Image(facts))) = self.metadata(cx).as_deref() {
            return Some(facts.format.label().into());
        }
        if let Some(Metadata::File(FileFacts::Audio(facts))) = self.metadata(cx).as_deref() {
            return Some(facts.format.label().into());
        }
        if let Some(Metadata::File(FileFacts::Video(facts))) = self.metadata(cx).as_deref() {
            return Some(facts.format.label().into());
        }
        item.entry
            .details
            .display_content()
            .and_then(|content| content.label())
            .map(SharedString::from)
    }

    fn probe_rows(&self, probe: &Metadata, cx: &App) -> Vec<DescriptionItem> {
        match probe {
            Metadata::File(FileFacts::Image(facts)) => image_rows(facts, cx),
            Metadata::File(FileFacts::Audio(facts)) => audio_rows(facts, cx),
            Metadata::File(FileFacts::Video(facts)) => video_rows(facts, cx),
            Metadata::File(FileFacts::Text(facts)) => text_rows(facts, cx),
            Metadata::File(FileFacts::Unknown) => Vec::new(),
            Metadata::Directory(facts) => directory_rows(facts, cx),
            Metadata::Symlink(facts) => vec![el! { .MetadataRow::default { label: $(i18n::text("details-target", cx)); text: $(facts.target.display().to_string().into()); } }.into()],
        }
    }
}

fn image_rows(facts: &ImageFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = Vec::new();
    if let Some(size) = facts.size {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-dimensions", cx)); text: $(info::dimensions(size).into()); } }.into());
        if let Some(ratio) = info::aspect_ratio(size) {
            rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-aspect", cx)); text: $(ratio.into()); } }.into());
        }
    }
    if let Some(bit_depth) = facts.bit_depth.filter(|depth| *depth > 0) {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-bit-depth", cx)); text: $(bit_depth.to_string().into()); } }.into());
    }
    if let Some(alpha) = facts.alpha {
        let value = i18n::text(if alpha { "details-yes" } else { "details-no" }, cx);
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-alpha", cx)); text: value; } }.into());
    }
    rows
}

fn audio_rows(facts: &AudioFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = Vec::new();
    for (label, value) in [
        ("details-title", facts.tags.title.clone()),
        ("details-artist", facts.tags.artist.clone()),
        ("details-album", facts.tags.album.clone()),
        ("details-genre", facts.tags.genre.clone()),
        (
            "details-year",
            facts.tags.year.map(|value| value.to_string()),
        ),
        (
            "details-track",
            facts.tags.track.map(|value| value.to_string()),
        ),
    ] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            rows.push(el! { .MetadataRow::default { label: $(i18n::text(label, cx)); text: $(value.into()); } }.into());
        }
    }
    if let Some(duration) = facts.duration {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-duration", cx)); text: $(info::duration(duration).into()); } }.into());
    }
    if let Some(bitrate) = facts.bitrate.filter(|bitrate| *bitrate > 0) {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-bitrate", cx)); text: $(info::bitrate(bitrate).into()); } }.into());
    }
    if let Some(rate) = facts.sample_rate.filter(|rate| *rate > 0) {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-sample-rate", cx)); text: $(info::sample_rate(rate).into()); } }.into());
    }
    if let Some(channels) = facts.channels.filter(|channels| *channels > 0) {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-channels", cx)); text: $(channels.to_string().into()); } }.into());
    }
    if let Some(bit_depth) = facts.bit_depth.filter(|depth| *depth > 0) {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-bit-depth", cx)); text: $(bit_depth.to_string().into()); } }.into());
    }
    rows
}

fn video_rows(facts: &VideoFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = Vec::new();
    if let Some(duration) = facts.duration {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-duration", cx)); text: $(info::duration(duration).into()); } }.into());
    }
    if let Some(size) = facts.dimensions {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-dimensions", cx)); text: $(info::dimensions(size).into()); } }.into());
    }
    if let Some(codec) = facts.codec.as_ref() {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-codec", cx)); text: $(codec.as_str().into()); } }.into());
    }
    rows
}

fn text_rows(facts: &TextFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = vec![el! { .MetadataRow::default { label: $(i18n::text("details-encoding", cx)); text: $(facts.encoding.label().into()); } }.into()];
    if let Some(line_endings) = facts.line_endings {
        let value = match info::line_endings(line_endings) {
            LineEndingLabel::Literal(text) => SharedString::from(text),
            LineEndingLabel::Message(id) => i18n::text(id, cx),
        };
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-line-endings", cx)); text: value; } }.into());
    }
    rows
}

fn directory_rows(facts: &DirectoryFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = vec![
        el! { .MetadataRow::default { label: $(i18n::text("details-folders", cx)); text: $(info::count(facts.folders, facts.capped).into()); } }.into(),
        el! { .MetadataRow::default { label: $(i18n::text("details-files", cx)); text: $(info::count(facts.files, facts.capped).into()); } }.into(),
    ];
    if facts.links > 0 {
        rows.push(el! { .MetadataRow::default { label: $(i18n::text("details-links", cx)); text: $(info::count(facts.links, facts.capped).into()); } }.into());
    }
    rows
}

#[component]
#[derive(IntoElement)]
struct PreviewPlaceholder {
    #[attr]
    icon: IconName,
    #[attr(into)]
    message: SharedString,
}

impl Default for PreviewPlaceholder {
    fn default() -> Self {
        Self {
            icon: IconName::Info,
            message: SharedString::default(),
        }
    }
}

impl RenderOnce for PreviewPlaceholder {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! {
            .EmptyState::default { min_h_0; gap: PANEL_GAP.px;
                text_color: $(cx.theme().muted_foreground); text_xs; text_center;
                px: PANEL_PADDING.px; py: PANEL_PADDING.px; overflow_hidden;
            } [
                .EmptyMedia::default [$(self.icon).Icon::new { size: ICON_SIZE.px; flex_shrink_0; }],
                .EmptyDescription::default { w_full; min_w_0; text_xs; text_center; } [$(self.message)],
            ]
        }
    }
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name: SharedString = match &self.target {
            Target::Item(item) => item.entry.display_name.clone().into(),
            _ => SharedString::default(),
        };
        let name_hint = text_hint(name.clone());
        let content = match &self.target {
            Target::Empty => el! { .PreviewPlaceholder::default { message: $(i18n::text("preview-empty", cx)); } }.into_any_element(),
            Target::Multiple(count) => el! { .PreviewPlaceholder::default {
                icon: IconName::Files; message: $(i18n::count("preview-multiple", *count, cx));
            } }.into_any_element(),
            Target::Item(item) => self.item(item, cx),
        };
        el! {
            .div { flex_1; min_h_0; min_w_0; w_full; flex; flex_col; overflow_hidden } [
                .PageHeader::default [
                    .div { h: tokens::ICON_BUTTON_SIZE.px; flex; items_center; w_full; min_w_0; } [
                        .PageTitle::default { # "preview-name"; text_size: tokens::ICON_SIZE.px; tooltip: name_hint; } [name],
                    ],
                ],
                content,
            ]
        }
    }
}

fn text_hint(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView {
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}

impl Browser {
    fn sync_place_preview(&mut self, cx: &mut Context<Self>) {
        let location = self.selected_place();
        let enabled = self.settings.preferences.tags.enabled;
        let tasks = self.window.tasks.clone();
        self.files.preview.update(cx, |view, cx| {
            view.set_tasks(tasks);
            view.set_tags_enabled(enabled, cx);
        });
        if let Some(entry) = location
            .as_ref()
            .and_then(|location| self.tag_entry(location))
            .cloned()
        {
            self.collections.preview_task = None;
            self.collections.preview_location = location;
            self.files
                .preview
                .update(cx, |view, cx| view.show_entry(entry, cx));
            return;
        }
        if self.collections.preview_location == location && location.is_some() {
            return;
        }
        self.collections.preview_task = None;
        self.collections.preview_location = location.clone();
        self.files
            .preview
            .update(cx, |view, cx| view.sync(Target::Empty, cx));
        let Some(path) =
            location.and_then(|location| location.local_path().map(std::path::Path::to_path_buf))
        else {
            return;
        };
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Inspect preview target", move || Entry::inspect(&path));
        self.collections.preview_task = Some(cx.spawn(async move |this, cx| {
            let entry = work
                .await
                .map_err(std::io::Error::other)
                .and_then(std::convert::identity);
            let _ = this.update(cx, |this, cx| {
                match entry {
                    Ok(entry) => {
                        this.files
                            .preview
                            .update(cx, |view, cx| view.show_entry(entry, cx));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "cannot inspect preview target");
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(in crate::pages::workspace) fn preview_target(&self) -> Target {
        let Some(slot) = self.active() else {
            return Target::Empty;
        };
        let tab = &slot.navigation;
        let Some(directory) = tab.directory() else {
            return Target::Empty;
        };
        let selected = &tab.view().selected;
        match selected.as_slice() {
            [] => Target::Empty,
            [path] => directory
                .entry_for_path(path)
                .map_or(Target::Empty, |entry| Target::Item(Item::from_entry(entry))),
            paths => Target::Multiple(paths.len()),
        }
    }

    fn preview_needs_refresh(&self, cx: &App) -> bool {
        if !self.details_visible() || !self.pages.is_empty() {
            return false;
        }
        let tags_enabled = self.settings.preferences.tags.enabled;
        let preview = self.files.preview.read(cx);
        if preview.tags_enabled != tags_enabled || preview.tasks.is_none() {
            return true;
        }
        if self.place_collection().is_some() {
            let location = self.selected_place();
            if let Some(entry) = location
                .as_ref()
                .and_then(|location| self.tag_entry(location))
            {
                return self.collections.preview_location != location
                    || preview.target != Target::Item(Item::from_entry(entry));
            }
            return self.collections.preview_location != location
                || (location.is_none() && preview.target != Target::Empty);
        }
        self.collections.preview_location.is_some()
            || self.collections.preview_task.is_some()
            || preview.target != self.preview_target()
    }

    pub(in crate::pages::workspace) fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        if !self.details_visible() || !self.pages.is_empty() {
            return;
        }
        if self.place_collection().is_some() {
            self.sync_place_preview(cx);
            return;
        }
        self.collections.preview_location = None;
        self.collections.preview_task = None;
        let target = self.preview_target();
        let tags_enabled = self.settings.preferences.tags.enabled;
        let tasks = self.window.tasks.clone();
        self.files.preview.update(cx, |preview, cx| {
            preview.set_tasks(tasks);
            preview.set_tags_enabled(tags_enabled, cx);
            preview.sync(target, cx);
        });
    }

    pub(in crate::pages::workspace) fn sync_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview_needs_refresh(cx) {
            let this = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = this.update(cx, |this, cx| this.refresh_preview(cx));
            });
        }
    }
}
