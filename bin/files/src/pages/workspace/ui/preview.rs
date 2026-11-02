//! Optional right-hand preview of the single selected entry.
//!
//! The panel carries the entry's name, shared metadata and a bounded,
//! header-only probe of its content. It deliberately shows no location bar:
//! the file's own path is not repeated here. A symlink's target is different
//! information and is shown as its own row.

mod header;
mod info;

use gds::ui::thumbnail::{Cache, Thumbnail};

use gds::appearance::tokens::{self, TOOLTIP_DELAY};
use gds::i18n;
use gds::ui::page::{PageHeader, PageTitle};
use gds::ui::time::Timestamp;
use gds_uim::el;
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

use super::{Browser, table::EntryPresentation};
use crate::entities::file::directory::{ContentCategory, Entry, EntryKind};
use header::{
    AudioFacts, DirectoryFacts, FileFacts, ImageFacts, Probe, ProbeHint, TextFacts, VideoFacts,
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
pub(super) enum Target {
    Empty,
    Multiple(usize),
    Item(Item),
}

#[derive(Clone, Debug)]
pub(super) struct Item {
    entry: Entry,
}

impl Item {
    fn from_entry(entry: &Entry) -> Self {
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

/// Header facts, loaded on the background executor.
enum Facts {
    None,
    Loading,
    Ready(Probe),
}

/// What one background read produced.
enum LoadOutcome {
    Probe(Probe),
    Failed(String),
}

pub(super) struct View {
    target: Target,
    thumbnails: Entity<Cache>,
    facts: Facts,
    generation: u64,
}

impl View {
    pub(super) fn new(thumbnails: Entity<Cache>, cx: &mut Context<Self>) -> Self {
        cx.observe(&thumbnails, |_, _, cx| cx.notify()).detach();
        Self {
            target: Target::Empty,
            thumbnails,
            facts: Facts::None,
            generation: 0,
        }
    }

    pub(super) fn show_entry(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.sync(Target::Item(Item::from_entry(&entry)), cx);
    }

    /// Point the preview at a new selection. Reading happens on the background
    /// executor; stale completions are ignored by generation.
    pub(super) fn sync(&mut self, target: Target, cx: &mut Context<Self>) {
        if self.target == target {
            return;
        }
        let same_source = matches!(
            (&self.target, &target),
            (Target::Item(previous), Target::Item(next)) if previous.same_source(next)
        );
        self.target = target;
        if same_source {
            cx.notify();
            return;
        }
        self.generation += 1;
        self.facts = Facts::None;
        self.load(cx);
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Target::Item(item) = &self.target else {
            return;
        };
        self.facts = Facts::Loading;
        let generation = self.generation;
        let entry = item.entry.clone();
        let hint = probe_hint(&entry);
        let work = cx.background_executor().spawn(async move {
            match entry.kind {
                EntryKind::Directory => LoadOutcome::Probe(header::probe_directory(&entry.path)),
                EntryKind::SymbolicLink => match header::probe_symlink(&entry.path) {
                    Some(probe) => LoadOutcome::Probe(probe),
                    None => LoadOutcome::Failed(format!(
                        "Could not read the link target of {}",
                        entry.path.display()
                    )),
                },
                _ => match header::read_prefix(&entry.path, header::PREFIX_BYTES) {
                    Ok(bytes) => LoadOutcome::Probe(header::probe_file(entry.size, hint, &bytes)),
                    Err(error) => LoadOutcome::Failed(format!(
                        "Header read failed for {}: {error}",
                        entry.path.display()
                    )),
                },
            }
        });
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            if let Err(error) = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match outcome {
                    LoadOutcome::Probe(probe) => this.facts = Facts::Ready(probe),
                    LoadOutcome::Failed(message) => {
                        tracing::warn!("{message}");
                        this.facts = Facts::None;
                    }
                }
                cx.notify();
            }) {
                tracing::debug!(%error, "preview closed before metadata completed");
            }
        })
        .detach();
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.target = Target::Empty;
        self.generation += 1;
        self.facts = Facts::None;
        cx.notify();
    }

    fn placeholder(&self, icon: IconName, message: SharedString, cx: &App) -> AnyElement {
        el! {
            .EmptyState::default { min_h_0; gap: PANEL_GAP.px;
                text_color: $cx.theme().muted_foreground; text_xs; text_center;
                px: PANEL_PADDING.px; py: PANEL_PADDING.px; overflow_hidden;
            } [
                .EmptyMedia::default [icon.Icon::new { size: ICON_SIZE.px; flex_shrink_0 }],
                .EmptyDescription::default { w_full; min_w_0; text_xs; text_center } [message],
            ]
        }
        .into_any_element()
    }

    fn media(&self, cx: &App) -> Option<AnyElement> {
        let Target::Item(item) = &self.target else {
            return None;
        };
        let source = crate::features::file_preview::source(&item.entry)?;
        let media = Thumbnail::new(self.thumbnails.clone(), source).show_status(true);
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
        if let Facts::Ready(probe) = &self.facts {
            rows.extend(self.probe_rows(probe, cx));
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
        let kind = EntryPresentation::type_label(&item.entry, cx);
        rows.push(meta_row(i18n::text("type", cx), kind, cx));
        if let Some(format) = self.format_label(item) {
            rows.push(meta_row(i18n::text("details-format", cx), format, cx));
        }
        if item.entry.kind == EntryKind::File {
            let size: SharedString = EntryPresentation::size(&item.entry).into();
            rows.push(meta_row(i18n::text("size", cx), size, cx));
        }
        if item.entry.details.owner.is_some() {
            let owner = EntryPresentation::owner(&item.entry, cx).text;
            rows.push(meta_row(i18n::text("owner", cx), owner, cx));
        }
        if let Some(group) = item.entry.details.group.as_ref() {
            let group: SharedString = group.label().into_owned().into();
            rows.push(meta_row(i18n::text("group", cx), group, cx));
        }
        if let Some(permissions) = item.entry.details.permissions.as_ref() {
            let value: SharedString = info::permissions(permissions).into();
            rows.push(meta_row(i18n::text("permissions", cx), value, cx));
        }
        rows.push(meta_timestamp(
            i18n::text("modified", cx),
            Timestamp::new("details-modified", item.entry.modified),
            cx,
        ));
        rows.push(meta_timestamp(
            i18n::text("accessed", cx),
            Timestamp::new("details-accessed", item.entry.details.accessed),
            cx,
        ));
        if let Some(created) = item.entry.details.created {
            rows.push(meta_timestamp(
                i18n::text("details-created", cx),
                Timestamp::new("details-created", Some(created)),
                cx,
            ));
        }
        rows
    }

    /// Concrete format from the probe, falling back to the listing's content.
    fn format_label(&self, item: &Item) -> Option<SharedString> {
        if let Facts::Ready(Probe::File(FileFacts::Image(facts))) = &self.facts {
            return Some(facts.format.label().into());
        }
        if let Facts::Ready(Probe::File(FileFacts::Audio(facts))) = &self.facts {
            return Some(facts.format.label().into());
        }
        if let Facts::Ready(Probe::File(FileFacts::Video(facts))) = &self.facts {
            return Some(facts.format.label().into());
        }
        item.entry
            .details
            .display_content()
            .and_then(|content| content.label())
            .map(SharedString::from)
    }

    fn probe_rows(&self, probe: &Probe, cx: &App) -> Vec<DescriptionItem> {
        match probe {
            Probe::File(FileFacts::Image(facts)) => image_rows(facts, cx),
            Probe::File(FileFacts::Audio(facts)) => audio_rows(facts, cx),
            Probe::File(FileFacts::Video(facts)) => video_rows(facts, cx),
            Probe::File(FileFacts::Text(facts)) => text_rows(facts, cx),
            Probe::File(FileFacts::Unknown) => Vec::new(),
            Probe::Directory(facts) => directory_rows(facts, cx),
            Probe::Symlink(facts) => vec![meta_row(
                i18n::text("details-target", cx),
                facts.target.display().to_string().into(),
                cx,
            )],
        }
    }
}

fn image_rows(facts: &ImageFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = Vec::new();
    if let Some(size) = facts.size {
        rows.push(meta_row(
            i18n::text("details-dimensions", cx),
            info::dimensions(size).into(),
            cx,
        ));
        rows.push(meta_row(
            i18n::text("details-aspect", cx),
            info::aspect_ratio(size).into(),
            cx,
        ));
    }
    if let Some(bit_depth) = facts.bit_depth.filter(|depth| *depth > 0) {
        rows.push(meta_row(
            i18n::text("details-bit-depth", cx),
            bit_depth.to_string().into(),
            cx,
        ));
    }
    if let Some(alpha) = facts.alpha {
        let value = i18n::text(if alpha { "details-yes" } else { "details-no" }, cx);
        rows.push(meta_row(i18n::text("details-alpha", cx), value, cx));
    }
    rows
}

fn audio_rows(facts: &AudioFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = Vec::new();
    if let Some(duration) = facts.duration {
        rows.push(meta_row(
            i18n::text("details-duration", cx),
            info::duration(duration).into(),
            cx,
        ));
    }
    if let Some(bitrate) = facts.bitrate.filter(|bitrate| *bitrate > 0) {
        rows.push(meta_row(
            i18n::text("details-bitrate", cx),
            info::bitrate(bitrate).into(),
            cx,
        ));
    }
    if let Some(rate) = facts.sample_rate.filter(|rate| *rate > 0) {
        rows.push(meta_row(
            i18n::text("details-sample-rate", cx),
            info::sample_rate(rate).into(),
            cx,
        ));
    }
    if let Some(channels) = facts.channels.filter(|channels| *channels > 0) {
        rows.push(meta_row(
            i18n::text("details-channels", cx),
            channels.to_string().into(),
            cx,
        ));
    }
    if let Some(bit_depth) = facts.bit_depth.filter(|depth| *depth > 0) {
        rows.push(meta_row(
            i18n::text("details-bit-depth", cx),
            bit_depth.to_string().into(),
            cx,
        ));
    }
    rows
}

fn video_rows(facts: &VideoFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = Vec::new();
    if let Some(duration) = facts.duration {
        rows.push(meta_row(
            i18n::text("details-duration", cx),
            info::duration(duration).into(),
            cx,
        ));
    }
    if let Some(size) = facts.dimensions {
        rows.push(meta_row(
            i18n::text("details-dimensions", cx),
            info::dimensions(size).into(),
            cx,
        ));
    }
    if let Some(codec) = facts.codec.as_ref() {
        rows.push(meta_row(
            i18n::text("details-codec", cx),
            codec.as_str().into(),
            cx,
        ));
    }
    rows
}

fn text_rows(facts: &TextFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = vec![meta_row(
        i18n::text("details-encoding", cx),
        facts.encoding.label().into(),
        cx,
    )];
    if let Some(line_endings) = facts.line_endings {
        let value = match info::line_endings(line_endings) {
            LineEndingLabel::Literal(text) => SharedString::from(text),
            LineEndingLabel::Message(id) => i18n::text(id, cx),
        };
        rows.push(meta_row(i18n::text("details-line-endings", cx), value, cx));
    }
    rows
}

fn directory_rows(facts: &DirectoryFacts, cx: &App) -> Vec<DescriptionItem> {
    let mut rows = vec![
        meta_row(
            i18n::text("details-folders", cx),
            info::count(facts.folders, facts.capped).into(),
            cx,
        ),
        meta_row(
            i18n::text("details-files", cx),
            info::count(facts.files, facts.capped).into(),
            cx,
        ),
    ];
    if facts.links > 0 {
        rows.push(meta_row(
            i18n::text("details-links", cx),
            info::count(facts.links, facts.capped).into(),
            cx,
        ));
    }
    rows
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name: SharedString = match &self.target {
            Target::Item(item) => item.entry.display_name.clone().into(),
            _ => SharedString::default(),
        };
        let name_hint = text_hint(name.clone());
        let content = match &self.target {
            Target::Empty => self.placeholder(IconName::Info, i18n::text("preview-empty", cx), cx),
            Target::Multiple(count) => self.placeholder(
                IconName::Files,
                i18n::count("preview-multiple", *count, cx),
                cx,
            ),
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

/// Content-category hints for the weak signatures inside the probe.
fn probe_hint(entry: &Entry) -> ProbeHint {
    match entry
        .details
        .display_content()
        .map(|content| content.category)
    {
        Some(ContentCategory::Text | ContentCategory::Code | ContentCategory::Document) => {
            ProbeHint {
                text: true,
                audio: false,
            }
        }
        Some(ContentCategory::Audio) => ProbeHint {
            text: false,
            audio: true,
        },
        _ => ProbeHint::default(),
    }
}

fn meta_row(label: SharedString, value: SharedString, cx: &App) -> DescriptionItem {
    let value_id: ElementId = format!("detail-value:{label}").into();
    let hint = (value.chars().count() > LONG_VALUE_CHARS).then(|| text_hint(value.clone()));
    let value = el! {
        .div {
            # value_id; min_w_0; truncate; text_xs; text_ellipsis;
            @if (let Some(hint) = hint) tooltip_show_delay: TOOLTIP_DELAY; tooltip: hint; @end;
        } [value]
    };
    meta_timestamp(label, value, cx)
}

fn meta_timestamp(label: SharedString, value: impl IntoElement, cx: &App) -> DescriptionItem {
    let label = el! {
        .div { text_xs; text_color: $cx.theme().muted_foreground } [label]
    };
    let value = el! { .div { min_w_0; text_xs; overflow_hidden } [value] };
    DescriptionItem::new(label.into_any_element()).value(value.into_any_element())
}

fn text_hint(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView {
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}

impl Browser {
    pub(super) fn preview_target(&self) -> Target {
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
                .entries
                .iter()
                .find(|entry| entry.path == *path)
                .map_or(Target::Empty, |entry| Target::Item(Item::from_entry(entry))),
            paths => Target::Multiple(paths.len()),
        }
    }

    /// Recompute the preview from current selection state during render.
    pub(super) fn sync_preview(&mut self, cx: &mut Context<Self>) {
        if !self.details_visible() || self.settings.open || self.collection().is_some() {
            return;
        }
        let target = self.preview_target();
        self.files
            .preview
            .update(cx, |preview, cx| preview.sync(target, cx));
    }
}

#[cfg(test)]
mod tests;
