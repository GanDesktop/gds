//! Properties and content inspection share the existing filesystem services.
mod info;
pub(super) mod tags;
use super::{menus::Surface, model::Row, ui::Browser};
use crate::{
    entities::{
        file::{
            directory::{Entry, EntryKind},
            ui::{EntryPresentation, FileArtwork, FileIcon},
        },
        preferences,
    },
    features::file_operations::permissions,
};
use anti_webui::prelude::*;
use gds::{
    appearance::tokens::{ICON_BUTTON_SIZE, ICON_SIZE},
    i18n,
    service::{
        file_inspection::{FileFacts, Metadata, Request},
        task_manager::Scope,
    },
    ui::{
        Frame, Icon,
        dialog::{DecisionActions, Dialog, DialogDecision},
        file_inspection::{Cache, State},
        page::PageHeader,
        thumbnail::Thumbnail,
        time::Timestamp,
        tooltip::tooltip,
    },
};
use gds_uim::el;
use std::path::PathBuf;
use tags::TagEditor;
use tokio_util::sync::CancellationToken;

const PREVIEW_HEIGHT: f32 = 168.0;
const PANEL_PADDING: f32 = 16.0;
const METADATA_GAP: f32 = 10.0;
const METADATA_LABEL_WIDTH: f32 = 76.0;
const LONG_VALUE_CHARS: usize = 28;

#[derive(Clone)]
pub(crate) struct Properties {
    path: PathBuf,
    entry: Signal<Result<Option<Entry>, String>>,
    cache: Cache,
    scope: Scope,
    owner: Owner,
    notice: Signal<Option<String>>,
    permissions: Signal<bool>,
}
impl Properties {
    pub(crate) fn new(path: PathBuf, scope: Scope) -> Self {
        let owner = scope.named_child("File properties");
        let cancel = owner.token();
        on_cleanup(move || cancel.cancel());
        let scope = (*owner).clone();
        // The native owner bounds callbacks; the task owner bounds filesystem work.
        on_cleanup(move || drop(owner));
        let cache = Cache::new(
            scope.clone(),
            preferences::Service::current()
                .snapshot()
                .files
                .preview
                .cache_options(),
        );
        let view = Self {
            path,
            entry: signal(Ok(None)),
            cache,
            scope,
            owner: Owner::current().expect("properties owner"),
            notice: signal(None),
            permissions: signal(false),
        };
        view.reload();
        view
    }
    fn from_entry(entry: Entry, cache: Cache, scope: Scope) -> Self {
        Self {
            path: entry.path.clone(),
            entry: signal(Ok(Some(entry))),
            cache,
            scope,
            owner: Owner::current().expect("properties owner"),
            notice: signal(None),
            permissions: signal(false),
        }
    }
    pub(crate) fn open(path: PathBuf, scope: Scope) {
        open_window(
            WindowOptions::new()
                .title(i18n::text("details-toggle"))
                .size(520, 680),
            move || {
                let view = Properties::new(path, scope);
                let frame = Frame::without_sidebar();
                let toolbar = frame.toolbar(text(move || i18n::text("details-toggle")), div());
                let content = frame.workspace(div(), view);
                el! {frame.[toolbar,content]}
            },
        );
    }
    fn work<T: Send + 'static>(
        &self,
        name: &'static str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
        done: impl FnOnce(Result<T, String>) + 'static,
    ) {
        let (task, complete) = Task::pending();
        self.scope
            .spawn_blocking_named(name, work)
            .on_complete(&self.scope, move |result| {
                complete.complete(
                    result
                        .map_err(|error| error.to_string())
                        .and_then(std::convert::identity),
                )
            });
        task.then(done);
    }
    fn reload(&self) {
        let path = self.path.clone();
        let entry = self.entry;
        self.owner.with(|| {
            self.work(
                "Read file properties",
                move || Entry::inspect(&path).map_err(|error| error.to_string()),
                move |result| entry.set(result.map(Some)),
            )
        });
    }
    fn row(label: &'static str, value: impl IntoView) -> Div {
        el! {
            .div.{flex; w_full; min_w: 0.0_f32; gap: 8.0_f32; text_size: 12.0_f32;
                line_height: $(LineHeight::Px(18.0));
            }.[
                (move () => label.i18n::text).text.{
                    w: METADATA_LABEL_WIDTH; flex_none; line_clamp: 2; overflow_wrap_anywhere;
                    dyn_style: style => $(style.text_color(theme().text_muted));
                },
                .div.{flex_1; min_w: 0.0_f32; overflow_hidden}.[value],
            ]
        }
    }
    fn metadata_text(value: String) -> Div {
        let hint = (value.chars().count() > LONG_VALUE_CHARS).then(|| value.clone());
        let label = el! { .div.{min_w: 0.0_f32; w_full}.[value.text.{ellipsis; w_full}] };
        if let Some(hint) = hint {
            tooltip(label, hint)
        } else {
            label
        }
    }
    fn text_row(label: &'static str, value: String) -> Div {
        Self::row(label, Self::metadata_text(value))
    }
    fn format_label<'a>(entry: &'a Entry, metadata: Option<&'a Metadata>) -> Option<&'a str> {
        match metadata {
            Some(Metadata::File(FileFacts::Image(facts))) => Some(facts.format.label()),
            Some(Metadata::File(FileFacts::Audio(facts))) => Some(facts.format.label()),
            Some(Metadata::File(FileFacts::Video(facts))) => Some(facts.format.label()),
            _ => entry
                .details
                .display_content()
                .and_then(|content| content.label()),
        }
    }
    fn base(&self, entry: &Entry, metadata: Option<&Metadata>) -> Vec<Div> {
        let mut rows = vec![Self::text_row("type", EntryPresentation::type_label(entry))];
        if let Some(format) = Self::format_label(entry, metadata) {
            rows.push(Self::text_row("details-format", format.to_owned()));
        }
        if entry.kind == EntryKind::File {
            rows.push(Self::text_row("size", EntryPresentation::size(entry)));
        }
        if entry.details.owner.is_some() {
            rows.push(Self::text_row(
                "owner",
                EntryPresentation::owner(entry).text,
            ));
        }
        if let Some(group) = entry.details.group.as_ref() {
            rows.push(Self::text_row("group", group.label().into_owned()));
        }
        if let Some(permissions) = entry.details.permissions.as_ref() {
            let open = self.permissions;
            rows.push(
                Self::text_row("permissions", info::permissions(permissions))
                    .focusable()
                    .cursor(CursorIcon::Pointer)
                    .access(|a| a.role(Role::Button).label(i18n::text("permissions-title")))
                    .on_click(move |_| open.update(|open| *open = !*open)),
            );
        }
        rows.push(Self::row("modified", Timestamp::new(entry.modified)));
        rows.push(Self::row(
            "accessed",
            Timestamp::new(entry.details.accessed),
        ));
        if entry.details.created.is_some() {
            rows.push(Self::row(
                "details-created",
                Timestamp::new(entry.details.created),
            ));
        }
        rows
    }
    fn facts(metadata: &Metadata) -> Vec<Div> {
        let mut fields: Vec<(&'static str, String)> = Vec::new();
        match metadata {
            Metadata::File(FileFacts::Image(facts)) => {
                if let Some(size) = facts.size {
                    fields.push(("details-dimensions", info::dimensions(size)));
                    if let Some(ratio) = info::aspect_ratio(size) {
                        fields.push(("details-aspect", ratio));
                    }
                }
                if let Some(depth) = facts.bit_depth {
                    fields.push(("details-bit-depth", depth.to_string()));
                }
                if let Some(alpha) = facts.alpha {
                    fields.push((
                        "details-alpha",
                        i18n::text(if alpha { "details-yes" } else { "details-no" }),
                    ));
                }
            }
            Metadata::File(FileFacts::Audio(facts)) => {
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
                    if let Some(value) = value {
                        fields.push((label, value));
                    }
                }
                if let Some(duration) = facts.duration {
                    fields.push(("details-duration", info::duration(duration)));
                }
                if let Some(rate) = facts.sample_rate {
                    fields.push(("details-sample-rate", info::sample_rate(rate)));
                }
                if let Some(channels) = facts.channels {
                    fields.push(("details-channels", channels.to_string()));
                }
                if let Some(depth) = facts.bit_depth {
                    fields.push(("details-bit-depth", depth.to_string()));
                }
                if let Some(rate) = facts.bitrate {
                    fields.push(("details-bitrate", info::bitrate(rate)));
                }
            }
            Metadata::File(FileFacts::Video(facts)) => {
                if let Some(duration) = facts.duration {
                    fields.push(("details-duration", info::duration(duration)));
                }
                if let Some(size) = facts.dimensions {
                    fields.push(("details-dimensions", info::dimensions(size)));
                }
                if let Some(codec) = &facts.codec {
                    fields.push(("details-codec", codec.clone()));
                }
            }
            Metadata::File(FileFacts::Text(facts)) => {
                fields.push(("details-encoding", facts.encoding.label().into()));
                if let Some(endings) = facts.line_endings {
                    fields.push((
                        "details-line-endings",
                        match info::line_endings(endings) {
                            info::LineEndingLabel::Literal(value) => value.into(),
                            info::LineEndingLabel::Message(id) => i18n::text(id),
                        },
                    ));
                }
            }
            Metadata::File(FileFacts::Unknown) => {}
            Metadata::Directory(facts) => {
                fields.push(("details-folders", info::count(facts.folders, facts.capped)));
                fields.push(("details-files", info::count(facts.files, facts.capped)));
                if facts.links > 0 {
                    fields.push(("details-links", info::count(facts.links, facts.capped)));
                }
            }
            Metadata::Symlink(facts) => {
                fields.push(("details-target", facts.target.display().to_string()))
            }
        }
        fields
            .into_iter()
            .map(|(label, value)| Self::text_row(label, value))
            .collect()
    }
    fn tag_summary(&self) -> Div {
        let value = signal(None);
        let path = self.path.clone();
        self.work(
            "Read file tags",
            move || gds::service::file_tags::read(&path).map_err(|error| error.to_string()),
            move |result| value.set(Some(result)),
        );
        let content = dyn_view(move || match value.get() {
            Some(Ok(tags)) if !tags.is_empty() => {
                let chips = tags.into_iter().map(|tag| {
                    let hint = tag.clone();
                    tooltip(el! { .div.{max_w: 100.0_f32.pct; min_w: 0.0_f32; px: 6.0_f32; rounded: 3.0_f32;
                        dyn_style: style => $(style.bg(theme().surface_muted));
                    }.[tag.text.{min_w: 0.0_f32; ellipsis; whitespace_nowrap}] }, hint)
                });
                el! { .div.{flex; flex_wrap; gap: 4.0_f32; min_w: 0.0_f32}.[..chips] }.into_view()
            }
            Some(Ok(_)) => Self::metadata_text(i18n::text("tags-none")).into_view(),
            Some(Err(error)) => Self::metadata_text(error).into_view(),
            None => Self::metadata_text(i18n::text("tags-loading")).into_view(),
        });
        Self::row("tags-title", content)
    }
    fn content(&self, entry: Entry) -> Div {
        let source = entry.inspection_source();
        let cache = self.cache.clone();
        let metadata = memo(move || cache.request(source.clone(), Request::Metadata).metadata);
        let permissions = self.permissions;
        let thumbnail = entry.thumbnail_source();
        let name = entry.display_name.clone();
        let title_hint = name.clone();
        let editor = self.clone();
        let tag_summary = self.clone();
        let properties = self.clone();
        let can_edit = matches!(entry.kind, EntryKind::File | EntryKind::Directory);
        let directory = entry.kind == EntryKind::Directory;
        let notice = self.notice;
        let details = move || {
            let state = metadata.get();
            let facts = match &state {
                State::Ready(facts) => Some(facts.as_ref()),
                _ => None,
            };
            let mut rows = properties.base(&entry, facts);
            match state {
                State::Ready(facts) => rows.extend(Self::facts(&facts)),
                State::Failed(error) => rows.push(Self::metadata_text(error)),
                _ => (),
            }
            el! { .div.{flex; flex_col; w_full; min_w: 0.0_f32; gap: METADATA_GAP}.[..rows] }
        };
        let permissions_view = move || {
            permissions
                .get()
                .then(|| untrack(|| PermissionEditor::new(editor.clone(), directory).into_view()))
        };
        let tags_view = show(
            move || preferences::Service::current().snapshot().tags.enabled,
            move || tag_summary.tag_summary(),
        );
        let artwork = thumbnail.map(|source| {
            el! {
                .div.{h: PREVIEW_HEIGHT; w_full; flex_none; overflow_hidden; rounded: 8.0_f32; border: 1.0_f32;
                    dyn_style: style => $(style.border_color(theme().border).bg(theme().surface_muted));
                }.[
                    $(Thumbnail::new(self.cache.clone(),source).show_status(true)),
                ]
            }
        });
        let title = tooltip(
            el! {
                .div.{flex; flex_1; min_w: 0.0_f32; h: ICON_BUTTON_SIZE; items_center}.[
                    name.text.{ellipsis; w_full; min_w: 0.0_f32; font_weight: FontWeight::SEMIBOLD;
                        dyn_style: style => $(style.text_size(gds::app::Gds::current().appearance().typography().scaled(ICON_SIZE)));
                    },
                ]
            },
            title_hint,
        );
        el! {
            .div.{flex; flex_col; flex_1; w_full; min_h: 0.0_f32; min_w: 0.0_f32; overflow_hidden}.[
                .PageHeader::default.[title],
                .div.{flex; flex_col; flex_1; w_full; min_h: 0.0_f32; min_w: 0.0_f32;
                    overflow_y_scroll; p: PANEL_PADDING; gap: PANEL_PADDING;
                }.[
                    ..artwork,
                    .div.{flex; flex_col; flex_none; w_full; min_w: 0.0_f32; gap: METADATA_GAP}.[
                        if can_edit [tags_view],
                        details.dyn_view,
                        (move () => $(notice.get().map(Self::metadata_text))).dyn_view,
                        if can_edit [permissions_view.dyn_view],
                    ],
                ],
            ]
        }
    }
}
impl IntoView for Properties {
    fn into_view(self) -> View {
        dyn_view(move || match self.entry.get() {
            Ok(Some(entry)) => untrack(|| self.content(entry)),
            Ok(None) => el! {.div.{p: 16.0_f32}.["…".text]},
            Err(error) => el! {.div.{p: 16.0_f32}.[error.text]},
        })
    }
}

#[derive(Clone)]
enum PermissionState {
    Loading,
    Ready(u32),
    Saving(u32),
    Failed(String),
}
#[derive(Clone)]
pub(super) struct PermissionEditor {
    properties: Properties,
    state: Signal<PermissionState>,
    recursive: Signal<bool>,
    directory: bool,
    cancel: CancellationToken,
}
impl PermissionEditor {
    pub(super) fn cancel(&self) {
        self.cancel.cancel();
    }

    fn new(properties: Properties, directory: bool) -> Self {
        let cancel = properties.scope.token().child_token();
        let editor = Self {
            properties,
            state: signal(PermissionState::Loading),
            recursive: signal(false),
            directory,
            cancel,
        };
        let state = editor.state;
        let path = editor.properties.path.clone();
        editor.properties.work(
            "Read permissions",
            move || permissions::read(&path).map_err(|error| error.to_string()),
            move |result| {
                state.set(match result {
                    Ok(mode) => PermissionState::Ready(mode),
                    Err(error) => PermissionState::Failed(error),
                })
            },
        );
        editor
    }
    fn save(&self, saved: impl FnOnce() + 'static) {
        let PermissionState::Ready(mode) = self.state.get_untracked() else {
            return;
        };
        self.state.set(PermissionState::Saving(mode));
        let path = self.properties.path.clone();
        let recursive = self.recursive.get_untracked();
        let cancel = self.cancel.clone();
        let editor = self.clone();
        self.properties.work(
            "Save permissions",
            move || {
                Ok(permissions::apply_cancellable(
                    &path, mode, recursive, &cancel,
                ))
            },
            move |result| {
                if editor.cancel.is_cancelled() {
                    return;
                }
                editor.state.set(PermissionState::Ready(mode));
                editor.properties.notice.set(Some(match result {
                    Ok(report) if report.failures.is_empty() => {
                        saved();
                        i18n::text("permissions-saved")
                    }
                    Ok(report) => report
                        .failures
                        .into_iter()
                        .map(|failure| format!("{}: {}", failure.path.display(), failure.error))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Err(error) => error,
                }));
                editor.properties.reload();
            },
        );
    }
    fn permission(&self, label: &'static str, mask: u32) -> Div {
        let state = self.state;
        let binding = Binding::new(
            move || match state.get() {
                PermissionState::Ready(mode) | PermissionState::Saving(mode) => mode & mask != 0,
                _ => false,
            },
            move |checked| {
                state.update(|state| {
                    if let PermissionState::Ready(mode) = state {
                        if checked {
                            *mode |= mask;
                        } else {
                            *mode &= !mask;
                        }
                    }
                })
            },
        );
        el! {
            $(checkbox(binding,move||i18n::text(label))).{
                disabled:move()=>$(!matches!(state.get(),PermissionState::Ready(_)));
            }
        }
    }
    fn group(&self, label: &'static str, shift: u32) -> Div {
        let checks = [
            ("permissions-read", 4u32),
            ("permissions-write", 2),
            ("permissions-execute", 1),
        ]
        .into_iter()
        .map(|(label, bit)| el! { .div.{flex_1; min_w: 0.0_f32}.[ $(self.permission(label, bit << shift)) ] });
        el! {
            .div.{flex; flex_col; gap: 12.0_f32; pb: 16.0_f32; border_b: 1.0_f32;
                dyn_style: style => $(style.border_color(theme().border));
            }.[
                (move()=>label.i18n::text).text.{font_weight:FontWeight::SEMIBOLD},
                .div.{flex; items_center; gap: 16.0_f32}.[..checks],
            ]
        }
    }
    fn message(&self) -> String {
        match self.state.get() {
            PermissionState::Loading => i18n::text("permissions-loading"),
            PermissionState::Saving(_) => i18n::text("permissions-saving"),
            PermissionState::Failed(error) => error,
            _ => self.properties.notice.get().unwrap_or_default(),
        }
    }
}
impl PermissionEditor {
    fn fields(&self) -> Div {
        let recursive = self.recursive;
        let state = self.state;
        let message = self.clone();
        el! {
            .div.{flex;flex_col;gap:16.0_f32; min_w: 0.0_f32}.[
                $(self.group("permissions-owner",6)),
                $(self.group("permissions-group",3)),
                $(self.group("permissions-others",0)),
                if self->directory [
                    $(checkbox(recursive,move||i18n::text("permissions-recursive"))).{
                        disabled: move () => $(!matches!(state.get(), PermissionState::Ready(_)));
                    },
                    $(show(move || recursive.get(), || el! {
                        $(i18n::text("permissions-recursive-hint")).text.{text_size: 12.0_f32;
                            dyn_style: style => $(style.text_color(theme().text_muted));
                        }
                    })),
                ],
                $(move || message.message()).text.{text_size: 12.0_f32},
            ]
        }
    }
}
impl IntoView for PermissionEditor {
    fn into_view(self) -> View {
        let state = self.state;
        el! { .div.{flex; flex_col; gap: 10.0_f32}.[
            $(self.fields()),
            $(i18n::text("permissions-save")).button.{
                disabled: move () => $(!matches!(state.get(), PermissionState::Ready(_)));
                on_click: move _ => $(self.save(|| {}));
            },
        ] }
        .into_view()
    }
}

impl Browser {
    pub(super) fn edit_tags(&self, path: PathBuf) {
        if self.read(|state| state.operation.busy()) || !self.0.preferences.snapshot().tags.enabled
        {
            return;
        }
        self.0.owner.with(|| {
            self.0
                .menus
                .surface
                .set(Surface::TagEditor(TagEditor::new(self, path)));
        });
    }

    pub(super) fn tags_dialog(&self) -> View {
        let Surface::TagEditor(editor) = self.0.menus.surface.get_untracked() else {
            return div().into_view();
        };
        let browser = self.downgrade();
        let target = editor.clone();
        let save = move || {
            let browser = browser.clone();
            target.save(move || {
                if let Some(browser) = browser.upgrade() {
                    browser.0.menus.close();
                    browser.refresh_tag_tabs();
                    browser.report_success(i18n::text("tags-saved"));
                }
            });
        };
        let enabled = editor.clone();
        let cancel = self.clone();
        let decisions = el! { .DecisionActions::default.[
            (move () => $(cancel.0.menus.close())).DialogDecision::cancel.{
                label: $(i18n::text("operation-cancel"));
            },
            save.DialogDecision::confirm.{
                label: $(i18n::text("permissions-save"));
                disabled_when: move () => $(!enabled.available());
            },
        ] };
        let name = editor
            .path()
            .file_name()
            .unwrap_or(editor.path().as_os_str())
            .to_string_lossy()
            .into_owned();
        let view = el! { .Dialog::default.{w: 520.0_f32;
            title: $(i18n::text("tags-editor-title"));
            decisions: decisions;
        }.[
            name.text.{ellipsis; min_w: 0.0_f32; whitespace_nowrap},
            $(editor.fields()),
        ] }
        .into_view();
        request_frame(move |_| editor.focus());
        view
    }

    pub(super) fn edit_permissions(&self, entry: Entry) {
        if self.read(|state| state.operation.busy()) {
            return;
        }
        let directory = entry.kind == EntryKind::Directory;
        self.0.owner.with(|| {
            let properties =
                Properties::from_entry(entry, self.0.inspections.clone(), self.scope());
            self.0
                .menus
                .surface
                .set(Surface::Permissions(PermissionEditor::new(
                    properties, directory,
                )));
        });
    }

    pub(super) fn close_permissions(&self) {
        if let Surface::Permissions(editor) = self.0.menus.surface.get_untracked() {
            editor.cancel();
            self.0.menus.close();
        }
    }

    pub(super) fn permissions_dialog(&self) -> View {
        let Surface::Permissions(editor) = self.0.menus.surface.get_untracked() else {
            return div().into_view();
        };
        let browser = self.downgrade();
        let path = editor.properties.path.clone();
        let state = editor.state;
        let confirm_editor = editor.clone();
        let save = move || {
            let browser = browser.clone();
            let path = path.clone();
            confirm_editor.save(move || {
                if let Some(browser) = browser.upgrade() {
                    browser.close_permissions();
                    browser.refresh_affected(&[path]);
                    browser.report_success(i18n::text("permissions-saved"));
                }
            });
        };
        let cancel = self.clone();
        let decisions = el! { .DecisionActions::default.[
            (move () => &cancel.{close_permissions}).DialogDecision::cancel.{
                label: $(i18n::text("operation-cancel"));
            },
            save.DialogDecision::confirm.{
                label: $(i18n::text("permissions-save"));
                disabled_when: move () => $(!matches!(state.get(), PermissionState::Ready(_)));
            },
        ] };
        let path = editor.properties.path.display().to_string();
        el! { .Dialog::default.{w: 520.0_f32;
            title: $(i18n::text("permissions-title"));
            decisions: decisions;
        }.[
            .div.{flex; items_center; gap: 8.0_f32; min_w: 0.0_f32}.[
                $(Icon::Lock.view(16.0_f32)), path.text.{ellipsis; min_w: 0.0_f32; whitespace_nowrap},
            ],
            $(editor.fields()),
        ] }.into_view()
    }

    pub(super) fn row_artwork_with_selection(
        &self,
        row: &Row,
        size: f32,
        selected: Binding<f32>,
    ) -> View {
        el! { .div.{size: size; flex_none}.[ $(self.artwork(row, size, selected)) ] }.into_view()
    }
    pub(super) fn artwork(&self, row: &Row, size: f32, selected: Binding<f32>) -> View {
        let Row::File(entry) = row else {
            let icon = row.icon();
            return el! { .div.{size:size;flex_none;dyn_style:move s=>${
                let palette = gds::app::Gds::current().appearance().theme().colors();
                s.text_color(palette.muted_foreground.mix(palette.selection_foreground, selected.get()))
            }}.[$(icon.view(size))] }.into_view();
        };
        let icon = FileIcon::for_entry(entry);
        let source = entry.thumbnail_source();
        let directory = FileIcon::is_directory(entry);
        let fallback = move || {
            dyn_view(move || {
                let foreground = gds::app::Gds::current()
                    .appearance()
                    .theme()
                    .colors()
                    .selection_foreground;
                let color = FileIcon::category_color(directory).mix(foreground, selected.get());
                FileArtwork::default().icon(icon).size(size).color(color)
            })
        };
        match source {
            Some(source) => Thumbnail::new(self.0.inspections.clone(), source)
                .fallback(fallback)
                .into_view(),
            _ => fallback().into_view(),
        }
    }
    pub(super) fn toggle_details(&self) {
        let visible = !self.0.details.get_untracked();
        self.0.details.set(visible);
        self.update(|state| {
            let location = state.tab().navigation.location().clone();
            state.places.set_details(location, visible);
        });
        self.save_places();
    }
    pub(super) fn details_view(&self) -> Div {
        #[derive(Clone, PartialEq)]
        enum Target {
            Entry(Entry),
            Path(PathBuf),
            Empty,
            Multiple(usize),
        }
        let browser = self.clone();
        let target = memo(move || {
            browser.read(|state| {
                let rows = state.tab().selected();
                match rows.as_slice() {
                    [Row::File(entry)] => Target::Entry(entry.clone()),
                    [Row::Place(location)] => location
                        .local_path()
                        .map_or(Target::Empty, |path| Target::Path(path.to_owned())),
                    [] | [_] => Target::Empty,
                    _ => Target::Multiple(rows.len()),
                }
            })
        });
        let cache = self.0.inspections.clone();
        let scope = self.scope();
        let details = move || match target.get() {
            Target::Entry(entry) => {
                untrack(|| Properties::from_entry(entry, cache.clone(), scope.clone()).into_view())
            }
            Target::Path(path) => untrack(|| Properties::new(path, scope.clone()).into_view()),
            Target::Empty => {
                Self::preview_placeholder(Icon::Info, i18n::text("preview-empty")).into_view()
            }
            Target::Multiple(count) => {
                Self::preview_placeholder(Icon::Copy, i18n::count("preview-multiple", count))
                    .into_view()
            }
        };
        el! {
            .div.{
                w:280.0_f32; flex_none; flex; flex_col; min_h:0.0_f32; border_l:1.0_f32;
                dyn_style:s=>$(s.border_color(theme().border));
            }.[details.dyn_view]
        }
    }

    fn preview_placeholder(icon: Icon, message: String) -> Div {
        el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32;
            justify_center; items_center; p: 16.0_f32; gap: 10.0_f32; text_align: TextAlign::Center;
            dyn_style: style => $(style.text_color(theme().text_muted));
        }.[ $(icon.view(30.0_f32)), message.text.{text_size: 12.0_f32} ] }
    }
}
