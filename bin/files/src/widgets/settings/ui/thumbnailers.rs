use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gds::service::file_inspection::{
    ExternalProbeReport, ExternalThumbnailers, SandboxError, ThumbnailerInfo,
};
use gds::ui::button::{ButtonProps, ButtonSize, ButtonVariant};
use gds::ui::disclosure::DisclosureProps;
use gds::ui::feedback::tooltip::HintTooltipProps;
use gds::ui::field::FieldLabelProps;
use gds::ui::settings::{
    SettingActionsProps, SettingControlProps, SettingDescriptionProps, SettingLabelProps,
    SettingRowProps,
};
use gds::ui::{IconName, IconProps, ToggleSwitchProps};
use gds::{app::Gds, i18n, service::task_manager};
use zgui::prelude::*;
use zgui::reactive::{
    LocalStorage, Owner as UiOwner, ReadSignal, RenderEffect, RwSignal, on_cleanup_local,
    spawn_local,
};
use zgui::view::install_stylesheet;
use zgui::{css, view};
use zgui_ui_primitives::Binding;

use super::{Preferences, View};
use crate::entities::preferences::ThumbnailerCommands;

const THUMBNAILERS_SHEET: &str = "gds-files-settings-thumbnailers";
const THUMBNAILERS_CSS: &str = css!(
    r#"
    .gds-settings-thumbnailers {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0;
    }

    .gds-settings-thumbnailers__title {
        font-size: var(--gds-text-md, 16px);
        font-weight: 600;
    }

    .gds-settings-thumbnailers__warning {
        display: flex;
        flex-direction: row;
        align-items: flex-start;
        gap: 10px;
        margin: 8px 0 4px;
        padding: 10px 12px;
        border-radius: var(--gds-radius-control, 6px);
        border: 1px solid color-mix(in srgb, var(--gds-warning) 40%, transparent);
        background: color-mix(in srgb, var(--gds-warning) 8%, transparent);
        width: 100%;
        min-width: 0;
        box-sizing: border-box;
    }

    .gds-settings-thumbnailers__warning-icon {
        padding-top: 2px;
        color: var(--gds-warning);
        flex: none;
    }

    .gds-settings-thumbnailers__warning-text {
        flex: 1 1 0%;
        min-width: 0;
        white-space: normal;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
    }

    .gds-settings-thumbnailers__probe {
        display: flex;
        flex-direction: row;
        align-items: flex-start;
        gap: 12px;
        padding: 12px 0;
    }

    .gds-settings-thumbnailers__status {
        flex: 1 1 0%;
        min-width: 0;
        white-space: normal;
        color: var(--gds-muted-foreground);
    }

    .gds-settings-thumbnailers__disclosure {
        margin-top: 8px;
        width: 100%;
        min-width: 0;
        border-radius: 8px;
        border: 1px solid var(--gds-border);
        background-color: var(--gds-control);
        color: var(--gds-control-foreground);
        overflow: hidden;
        box-sizing: border-box;
    }

    .gds-settings-thumbnailers__disclosure > .gds-disclosure__trigger {
        width: 100%;
        min-height: 40px;
        padding: 8px 14px;
        gap: 8px;
        border-radius: 0;
        box-sizing: border-box;
    }

    .gds-settings-thumbnailers__disclosure-header {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        flex: 1 1 0%;
        min-width: 0;
    }

    .gds-settings-thumbnailers__disclosure-title {
        font-size: var(--gds-text-sm, 14px);
        font-weight: 500;
        color: var(--gds-foreground);
        flex: none;
    }

    .gds-settings-thumbnailers__count {
        font-size: var(--gds-text-xs, 12px);
        font-variant-numeric: tabular-nums;
        padding: 1px 7px;
        border-radius: 999px;
        background-color: color-mix(in srgb, var(--gds-foreground) 10%, transparent);
        color: var(--gds-muted-foreground);
        flex: none;
    }

    .gds-settings-thumbnailers__categories {
        flex: 1 1 0%;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        text-align: right;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
    }

    .gds-settings-thumbnailers__rows {
        display: flex;
        flex-direction: column;
        width: 100%;
        min-width: 0;
        border-top: 1px solid var(--gds-border);
    }

    .gds-settings-thumbnailers__rows > *:not(:first-child) {
        border-top: 1px solid color-mix(in srgb, var(--gds-border) 65%, transparent);
    }

    .gds-settings-thumbnailers__rows > .zui-tooltip__trigger {
        display: flex;
        width: 100%;
        min-width: 0;
    }

    .gds-settings-thumbnailers__entry {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        width: 100%;
        min-height: 36px;
        padding: 7px 14px;
        min-width: 0;
        box-sizing: border-box;
    }

    .gds-settings-thumbnailers__entry:hover {
        background-color: var(--gds-hover);
    }

    .gds-settings-thumbnailers__entry-identity {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0;
        flex: 0 1 auto;
    }

    .gds-settings-thumbnailers__entry-name {
        font-size: var(--gds-text-sm, 14px);
        font-weight: 500;
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
    }

    .gds-settings-thumbnailers__entry-badge {
        font-size: var(--gds-text-xs, 12px);
        padding: 1px 6px;
        border-radius: 4px;
        background-color: color-mix(in srgb, var(--gds-primary) 14%, transparent);
        color: var(--gds-primary);
        flex: none;
    }

    .gds-settings-thumbnailers__entry-summary {
        flex: 1 1 0%;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        text-align: right;
    }
    "#
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum State {
    Checking(ThumbnailerCommands),
    Done {
        commands: ThumbnailerCommands,
        sandbox: Result<PathBuf, SandboxError>,
        discovered: Vec<ThumbnailerInfo>,
        mime_count: usize,
    },
}

#[derive(Clone)]
pub(super) struct Probe {
    state: RwSignal<State, LocalStorage>,
    refresh: RwSignal<u64, LocalStorage>,
    discovered_open: RwSignal<bool, LocalStorage>,
}

impl Probe {
    pub(super) fn new(preferences: ReadSignal<Preferences, LocalStorage>) -> Self {
        Self::new_with(preferences, |commands, cancel| async move {
            ExternalThumbnailers::probe_report(commands.bwrap.as_deref(), &cancel).await
        })
    }

    pub(super) fn new_with<F, Fut>(
        preferences: ReadSignal<Preferences, LocalStorage>,
        probe_fn: F,
    ) -> Self
    where
        F: Fn(ThumbnailerCommands, tokio_util::sync::CancellationToken) -> Fut + 'static,
        Fut: std::future::Future<Output = ExternalProbeReport> + Send + 'static,
    {
        let initial_commands = preferences.with_untracked(|p| p.thumbnailers.commands.clone());
        let state = RwSignal::new_local(State::Checking(initial_commands));
        let refresh = RwSignal::new_local(0_u64);
        let discovered_open = RwSignal::new_local(false);

        let parent_scope = Gds::current()
            .tasks()
            .owner_scope("Settings external thumbnailers");
        let ui_owner = UiOwner::current();
        let in_flight: Rc<RefCell<Option<task_manager::Owner>>> = Rc::new(RefCell::new(None));
        let generation: Rc<Cell<u64>> = Rc::new(Cell::new(0));
        let last_key: Rc<RefCell<Option<(u64, ThumbnailerCommands)>>> = Rc::new(RefCell::new(None));

        let in_flight_cleanup = Rc::clone(&in_flight);
        on_cleanup_local(move || {
            let previous = in_flight_cleanup.borrow_mut().take();
            drop(previous);
        });

        let fx = RenderEffect::new(move |_| {
            let seq = refresh.get();
            let commands = preferences.with(|p| p.thumbnailers.commands.clone());

            let unchanged = last_key
                .borrow()
                .as_ref()
                .is_some_and(|(prev_seq, prev_cmds)| *prev_seq == seq && *prev_cmds == commands);
            if unchanged {
                return;
            }
            *last_key.borrow_mut() = Some((seq, commands.clone()));

            let probe_gen = generation.get().wrapping_add(1);
            generation.set(probe_gen);

            let attempt_owner = parent_scope.named_child("Check external thumbnailers");
            let attempt_scope = (*attempt_owner).clone();
            let attempt_cancel = attempt_owner.token();
            let cancelled_previous = in_flight.borrow_mut().replace(attempt_owner);
            drop(cancelled_previous);

            state.set(State::Checking(commands.clone()));

            let future = probe_fn(commands.clone(), attempt_cancel);
            let task = attempt_scope.spawn_cancellable_named("Check external thumbnailers", future);
            let generation_check = Rc::clone(&generation);
            let in_flight_done = Rc::clone(&in_flight);
            let probe_commands = commands;

            let deliver = move || {
                spawn_local(async move {
                    let result = task.await;
                    if generation_check.get() != probe_gen {
                        return;
                    }
                    if preferences.with_untracked(|p| p.thumbnailers.commands != probe_commands) {
                        return;
                    }
                    let finished = in_flight_done.borrow_mut().take();
                    drop(finished);

                    let next = match result {
                        Ok(ExternalProbeReport {
                            discovered,
                            mime_count,
                            sandbox,
                        }) => State::Done {
                            commands: probe_commands,
                            sandbox,
                            discovered,
                            mime_count,
                        },
                        Err(error) => State::Done {
                            commands: probe_commands,
                            sandbox: Err(SandboxError::ProbeFailed(error.to_string())),
                            discovered: Vec::new(),
                            mime_count: 0,
                        },
                    };
                    state.set(next);
                });
            };

            if let Some(ref owner) = ui_owner {
                owner.with(deliver);
            } else {
                Gds::current().with(deliver);
            }
        });
        on_cleanup_local(move || drop(fx));

        Self {
            state,
            refresh,
            discovered_open,
        }
    }

    fn bwrap_ready(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with(|p| p.thumbnailers.commands.clone());
        self.state.with(|state| {
            matches!(
                state,
                State::Done {
                    commands,
                    sandbox: Ok(_),
                    ..
                } if *commands == current
            )
        })
    }

    fn bwrap_ready_untracked(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with_untracked(|p| p.thumbnailers.commands.clone());
        self.state.with_untracked(|state| {
            matches!(
                state,
                State::Done {
                    commands,
                    sandbox: Ok(_),
                    ..
                } if *commands == current
            )
        })
    }

    fn can_enable(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let use_bwrap = preferences.with(|p| p.thumbnailers.use_bwrap);
        !use_bwrap || self.bwrap_ready(preferences)
    }

    fn can_enable_untracked(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let use_bwrap = preferences.with_untracked(|p| p.thumbnailers.use_bwrap);
        !use_bwrap || self.bwrap_ready_untracked(preferences)
    }

    fn checking(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with(|p| p.thumbnailers.commands.clone());
        self.state
            .with(|state| matches!(state, State::Checking(checked) if *checked == current))
    }

    #[cfg(test)]
    fn checking_untracked(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> bool {
        let current = preferences.with_untracked(|p| p.thumbnailers.commands.clone());
        self.state.with_untracked(
            |state| matches!(state, State::Checking(checked) if *checked == current),
        )
    }

    fn status(&self, preferences: ReadSignal<Preferences, LocalStorage>) -> String {
        let (current, use_bwrap) =
            preferences.with(|p| (p.thumbnailers.commands.clone(), p.thumbnailers.use_bwrap));
        self.state.with(|state| match state {
            State::Checking(checked) if *checked == current => i18n::text("thumbnailers-checking"),
            State::Done {
                commands,
                sandbox,
                discovered,
                ..
            } if *commands == current => {
                if !use_bwrap {
                    if discovered.is_empty() {
                        i18n::text("thumbnailers-unsandboxed-empty")
                    } else {
                        gds::t!(
                            "thumbnailers-unsandboxed-ready",
                            count = discovered.len() as i64
                        )
                    }
                } else {
                    match sandbox {
                        Ok(_) if discovered.is_empty() => i18n::text("thumbnailers-ready-empty"),
                        Ok(_) => gds::t!("thumbnailers-ready", count = discovered.len() as i64),
                        Err(error) => format_sandbox_error(error),
                    }
                }
            }
            _ => i18n::text("thumbnailers-unavailable"),
        })
    }

    fn discovered(
        &self,
        preferences: ReadSignal<Preferences, LocalStorage>,
    ) -> Vec<ThumbnailerInfo> {
        let current = preferences.with(|p| p.thumbnailers.commands.clone());
        self.state.with(|state| match state {
            State::Done {
                commands,
                discovered,
                ..
            } if *commands == current => discovered.clone(),
            _ => Vec::new(),
        })
    }
}

const MAX_ENTRY_CATEGORIES: usize = 3;
const MAX_TOOLTIP_MIMES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MediaCategory {
    Video,
    Audio,
    Pdf,
    Office,
    Ebook,
    Comics,
    Document,
    Font,
    Image,
    Model3d,
    Other,
}

impl MediaCategory {
    fn i18n_key(self) -> &'static str {
        match self {
            Self::Video => "thumbnailers-cat-video",
            Self::Audio => "thumbnailers-cat-audio",
            Self::Pdf => "thumbnailers-cat-pdf",
            Self::Office => "thumbnailers-cat-office",
            Self::Ebook => "thumbnailers-cat-ebook",
            Self::Comics => "thumbnailers-cat-comics",
            Self::Document => "thumbnailers-cat-document",
            Self::Font => "thumbnailers-cat-font",
            Self::Image => "thumbnailers-cat-image",
            Self::Model3d => "thumbnailers-cat-3d",
            Self::Other => "thumbnailers-cat-other",
        }
    }

    fn label(self) -> String {
        i18n::text(self.i18n_key())
    }
}

fn classify_mime(mime: &str) -> MediaCategory {
    let mime = mime.trim().to_ascii_lowercase();
    if mime.starts_with("video/")
        || matches!(
            mime.as_str(),
            "application/mxf"
                | "application/vnd.ms-asf"
                | "application/vnd.rn-realmedia"
                | "application/x-matroska"
        )
    {
        return MediaCategory::Video;
    }
    if mime.starts_with("audio/") || mime == "application/x-flac" {
        return MediaCategory::Audio;
    }
    if matches!(
        mime.as_str(),
        "application/pdf"
            | "application/x-ext-pdf"
            | "application/x-bzpdf"
            | "application/x-gzpdf"
            | "application/x-xzpdf"
    ) {
        return MediaCategory::Pdf;
    }
    if mime.starts_with("application/vnd.oasis.opendocument.")
        || mime.starts_with("application/vnd.openxmlformats-officedocument.")
        || mime.starts_with("application/vnd.sun.xml.")
        || matches!(
            mime.as_str(),
            "application/msword"
                | "application/vnd.ms-excel"
                | "application/vnd.ms-powerpoint"
                | "application/rtf"
                | "text/rtf"
        )
    {
        return MediaCategory::Office;
    }
    if matches!(
        mime.as_str(),
        "application/epub+zip"
            | "application/x-mobipocket-ebook"
            | "application/vnd.amazon.mobi8-ebook"
            | "application/x-fictionbook+xml"
    ) {
        return MediaCategory::Ebook;
    }
    if mime.starts_with("application/vnd.comicbook")
        || matches!(
            mime.as_str(),
            "application/x-cb7"
                | "application/x-cbr"
                | "application/x-cbt"
                | "application/x-cbz"
                | "application/x-ext-cb7"
                | "application/x-ext-cbr"
                | "application/x-ext-cbt"
                | "application/x-ext-cbz"
        )
    {
        return MediaCategory::Comics;
    }
    if mime.starts_with("image/vnd.djvu")
        || matches!(
            mime.as_str(),
            "application/x-ext-djv"
                | "application/x-ext-djvu"
                | "application/illustrator"
                | "application/postscript"
                | "application/oxps"
                | "application/vnd.ms-xpsdocument"
        )
    {
        return MediaCategory::Document;
    }
    if mime.starts_with("font/")
        || mime.starts_with("application/x-font-")
        || mime == "application/vnd.ms-opentype"
    {
        return MediaCategory::Font;
    }
    if mime.starts_with("image/") || matches!(mime.as_str(), "application/ico" | "text/ico") {
        return MediaCategory::Image;
    }
    if mime.starts_with("model/") || mime == "application/x-blender" {
        return MediaCategory::Model3d;
    }
    MediaCategory::Other
}

fn exact_format_label(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/avif" => Some("AVIF"),
        "image/jxl" => Some("JPEG XL"),
        "image/heif" | "image/heic" => Some("HEIF"),
        "image/webp" => Some("WebP"),
        "image/svg+xml" | "image/svg+xml-compressed" => Some("SVG"),
        "application/epub+zip" => Some("EPUB"),
        "application/x-mobipocket-ebook" => Some("MOBI"),
        "application/vnd.amazon.mobi8-ebook" => Some("AZW3"),
        "application/pdf" => Some("PDF"),
        "application/x-blender" => Some("Blender"),
        _ => None,
    }
}

fn collect_categories<'a>(mimes: impl IntoIterator<Item = &'a str>) -> Vec<MediaCategory> {
    let mut categories: Vec<MediaCategory> = mimes.into_iter().map(classify_mime).collect();
    categories.sort();
    categories.dedup();
    if categories.len() > 1 {
        categories.retain(|category| *category != MediaCategory::Other);
    }
    categories
}

fn summarize_entry_formats(mime_types: &[String]) -> String {
    let sep = i18n::text("thumbnailers-list-sep");
    if !mime_types.is_empty() && mime_types.len() <= 3 {
        let mut exact = Vec::with_capacity(mime_types.len());
        let mut all_exact = true;
        for mime in mime_types {
            if let Some(label) = exact_format_label(mime) {
                if !exact.contains(&label) {
                    exact.push(label);
                }
            } else {
                all_exact = false;
                break;
            }
        }
        if all_exact && !exact.is_empty() {
            return exact.join(&sep);
        }
    }

    let categories = collect_categories(mime_types.iter().map(String::as_str));
    let summary = categories
        .into_iter()
        .take(MAX_ENTRY_CATEGORIES)
        .map(MediaCategory::label)
        .collect::<Vec<_>>()
        .join(&sep);
    if mime_types.len() > 1 {
        gds::t!(
            "thumbnailers-formats-count",
            summary = summary,
            count = mime_types.len() as i64
        )
    } else {
        summary
    }
}

fn summarize_all_categories(entries: &[ThumbnailerInfo]) -> String {
    let categories = collect_categories(
        entries
            .iter()
            .flat_map(|entry| entry.mime_types.iter().map(String::as_str)),
    );
    categories
        .into_iter()
        .map(MediaCategory::label)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn format_entry_tooltip(entry: &ThumbnailerInfo) -> String {
    let mimes = if entry.mime_types.len() <= MAX_TOOLTIP_MIMES {
        entry.mime_types.join(", ")
    } else {
        let shown = entry.mime_types[..MAX_TOOLTIP_MIMES].join(", ");
        let rest = entry.mime_types.len() - MAX_TOOLTIP_MIMES;
        gds::t!(
            "thumbnailers-tooltip-more",
            shown = shown,
            rest = rest as i64,
            total = entry.mime_types.len() as i64
        )
    };
    format!("{}\n{mimes}", entry.path.display())
}

fn format_sandbox_error(error: &SandboxError) -> String {
    match error {
        SandboxError::UnsupportedPlatform => i18n::text("thumbnailers-unsupported-platform"),
        SandboxError::NotFound(path) => {
            gds::t!("thumbnailers-not-found", path = path.display().to_string())
        }
        SandboxError::NotExecutable(path) => {
            gds::t!(
                "thumbnailers-not-executable",
                path = path.display().to_string()
            )
        }
        SandboxError::ProbeFailed(detail) => {
            gds::t!("thumbnailers-probe-failed", detail = detail.as_str())
        }
    }
}

impl View {
    pub(super) fn thumbnailers_section(&self) -> AnyView {
        install_stylesheet(THUMBNAILERS_SHEET, THUMBNAILERS_CSS);
        let probe_toggle = self.thumbnailers.clone();
        let probe_can_enable = self.thumbnailers.clone();
        let probe_checking = self.thumbnailers.clone();
        let probe_status = self.thumbnailers.clone();
        let probe_list = self.thumbnailers.clone();
        let probe_refresh = self.thumbnailers.refresh;
        let preferences = self.preferences;
        let view_enabled = self.clone();
        let view_sandbox = self.clone();
        let enabled_ref = NodeRef::new();
        let sandbox_ref = NodeRef::new();
        let list_open = self.thumbnailers.discovered_open;

        let enabled = Binding::controlled(
            Signal::derive_local(move || preferences.with(|p| p.thumbnailers.enabled)),
            move |enabled: bool| {
                if !enabled || probe_toggle.can_enable_untracked(preferences) {
                    view_enabled.apply(|p| p.thumbnailers.enabled = enabled);
                }
            },
        );
        let enabled_disabled = Signal::derive_local(move || {
            !preferences.with(|p| p.thumbnailers.enabled)
                && !probe_can_enable.can_enable(preferences)
        });
        let reset_enabled = self.reset(
            "thumbnailers-enabled",
            |p| p.thumbnailers.enabled,
            |p, v| p.thumbnailers.enabled = v,
        );

        let use_bwrap = Binding::controlled(
            Signal::derive_local(move || preferences.with(|p| p.thumbnailers.use_bwrap)),
            move |use_bwrap: bool| {
                if !use_bwrap || ExternalThumbnailers::BWRAP_SUPPORTED {
                    view_sandbox.apply(|p| p.thumbnailers.use_bwrap = use_bwrap);
                }
            },
        );
        let sandbox_disabled = Signal::derive_local(move || {
            !preferences.with(|p| p.thumbnailers.use_bwrap)
                && !ExternalThumbnailers::BWRAP_SUPPORTED
        });
        let reset_sandbox = self.reset(
            "thumbnailers-use-bwrap",
            |p| p.thumbnailers.use_bwrap,
            |p, v| p.thumbnailers.use_bwrap = v,
        );

        let commands = ExternalThumbnailers::BWRAP_SUPPORTED.then(|| {
            self.subform(
                |p| p.thumbnailers.commands.clone(),
                |p, v| p.thumbnailers.commands = v,
            )
            .view()
        });

        AnyView::new(view! {
            column(class = "gds-settings-thumbnailers") {
                text(class = "gds-settings-thumbnailers__title") {{move || i18n::text("thumbnailers-title")}}
                SettingRow {
                    SettingLabel {
                        FieldLabel(node_ref = enabled_ref) {{move || i18n::text("thumbnailers-enabled")}}
                        SettingDescription {{move || i18n::text("thumbnailers-enabled-help")}}
                    }
                    SettingActions {
                        {reset_enabled}
                        SettingControl {
                            ToggleSwitch(
                                checked = enabled,
                                disabled = enabled_disabled,
                                labelled_by = Some(enabled_ref),
                            )
                        }
                    }
                }
                SettingRow {
                    SettingLabel {
                        FieldLabel(node_ref = sandbox_ref) {{move || i18n::text("thumbnailers-use-bwrap")}}
                        SettingDescription {{move || {
                            if ExternalThumbnailers::BWRAP_SUPPORTED {
                                i18n::text("thumbnailers-use-bwrap-help")
                            } else {
                                i18n::text("thumbnailers-use-bwrap-unsupported-help")
                            }
                        }}}
                    }
                    SettingActions {
                        {reset_sandbox}
                        SettingControl {
                            ToggleSwitch(
                                checked = use_bwrap,
                                disabled = sandbox_disabled,
                                labelled_by = Some(sandbox_ref),
                            )
                        }
                    }
                }
                {move || {
                    (!preferences.with(|p| p.thumbnailers.use_bwrap)).then(|| {
                        let key = if ExternalThumbnailers::BWRAP_SUPPORTED {
                            "thumbnailers-unsandboxed-warning"
                        } else {
                            "thumbnailers-unsandboxed-warning-non-linux"
                        };
                        AnyView::new(view! {
                            row(class = "gds-settings-thumbnailers__warning") {
                                box(class = "gds-settings-thumbnailers__warning-icon") {
                                    Icon(name = IconName::TriangleAlert, size = 16.0)
                                }
                                text(class = "gds-settings-thumbnailers__warning-text") {
                                    {move || i18n::text(key)}
                                }
                            }
                        })
                    })
                }}
                row(class = "gds-settings-thumbnailers__probe") {
                    text(class = "gds-settings-thumbnailers__status") {{move || probe_status.status(preferences)}}
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        disabled = Signal::derive_local(move || probe_checking.checking(preferences)),
                        tooltip = Signal::derive_local(|| i18n::text("thumbnailers-detect")),
                        on:click = move |_| probe_refresh.update(|n| *n = n.wrapping_add(1)),
                    ) {{move || i18n::text("thumbnailers-detect")}}
                }
                {commands}
                {move || {
                    let entries = probe_list.discovered(preferences);
                    (!entries.is_empty()).then(|| {
                        let count = entries.len().to_string();
                        let header_entries = entries.clone();
                        let open_binding = Binding::controlled(
                            Signal::derive_local(move || list_open.get()),
                            move |next: bool| list_open.set(next),
                        );
                        AnyView::new(view! {
                            Disclosure(
                                open = open_binding,
                                show_icon = true,
                                class = "gds-settings-thumbnailers__disclosure",
                                header = move || AnyView::new(view! {
                                    row(class = "gds-settings-thumbnailers__disclosure-header") {
                                        text(class = "gds-settings-thumbnailers__disclosure-title") {
                                            {move || i18n::text("thumbnailers-discovered")}
                                        }
                                        text(class = "gds-settings-thumbnailers__count") {{count}}
                                        text(class = "gds-settings-thumbnailers__categories") {
                                            {move || summarize_all_categories(&header_entries)}
                                        }
                                    }
                                }),
                                content = move || {
                                    let rows = entries
                                        .into_iter()
                                        .map(|entry| {
                                            let name = entry.id.clone();
                                            let user_override = entry.user_override;
                                            let summary_mimes = entry.mime_types.clone();
                                            let tooltip = Signal::derive_local(move || {
                                                format_entry_tooltip(&entry)
                                            });
                                            AnyView::new(view! {
                                                HintTooltip(text = tooltip) {
                                                    row(class = "gds-settings-thumbnailers__entry") {
                                                        row(class = "gds-settings-thumbnailers__entry-identity") {
                                                            text(class = "gds-settings-thumbnailers__entry-name") {{name}}
                                                            {user_override.then(|| AnyView::new(view! {
                                                                text(class = "gds-settings-thumbnailers__entry-badge") {
                                                                    {move || i18n::text("thumbnailers-user-badge")}
                                                                }
                                                            }))}
                                                        }
                                                        text(class = "gds-settings-thumbnailers__entry-summary") {
                                                            {move || summarize_entry_formats(&summary_mimes)}
                                                        }
                                                    }
                                                }
                                            })
                                        })
                                        .collect::<Vec<_>>();
                                    AnyView::new(view! {
                                        column(class = "gds-settings-thumbnailers__rows") {
                                            {rows}
                                        }
                                    })
                                },
                            )
                        })
                    })
                }}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use gds::app::Gds;
    use gds::service::file_inspection::{ExternalProbeReport, SandboxError};
    use gds::service::task_manager::Supervisor;
    use tokio::sync::oneshot;
    use zgui::prelude::*;
    use zgui::reactive::{Mounted, RwSignal, TestWaker, flush, install, set_frame_waker};

    use super::{Probe, State};
    use crate::entities::preferences::{Preferences, ThumbnailerCommands};

    type BoxProbeFuture = Pin<Box<dyn Future<Output = ExternalProbeReport> + Send>>;

    struct DropSignal {
        dropped: Arc<AtomicUsize>,
        tx: Option<oneshot::Sender<()>>,
    }

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }
    }

    #[test]
    fn changing_bwrap_command_cancels_stale_inflight_probe_and_ignores_unrelated_edits() {
        let _ = install();
        set_frame_waker(Arc::new(TestWaker::default()));
        let supervisor = Supervisor::new(false).expect("start test supervisor");

        let started = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let (first_started_tx, first_started_rx) = oneshot::channel();
        let (first_dropped_tx, first_dropped_rx) = oneshot::channel();
        let (second_release_tx, second_release_rx) = oneshot::channel::<()>();
        let (second_done_tx, second_done_rx) = oneshot::channel();

        let dropped_first = Arc::clone(&dropped);
        let first_fut: BoxProbeFuture = Box::pin(async move {
            let _guard = DropSignal {
                dropped: dropped_first,
                tx: Some(first_dropped_tx),
            };
            let _ = first_started_tx.send(());
            std::future::pending::<ExternalProbeReport>().await
        });

        let second_fut: BoxProbeFuture = Box::pin(async move {
            let _ = second_release_rx.await;
            let _ = second_done_tx.send(());
            ExternalProbeReport {
                discovered: Vec::new(),
                mime_count: 0,
                sandbox: Err(SandboxError::NotFound(PathBuf::from("/usr/bin/bwrap"))),
            }
        });

        let queue: Arc<Mutex<VecDeque<BoxProbeFuture>>> =
            Arc::new(Mutex::new(VecDeque::from([first_fut, second_fut])));
        let started_probe = Arc::clone(&started);
        let queue_probe = Arc::clone(&queue);

        let root = Mounted::new();
        root.with(|| {
            let _gds = Gds::install(supervisor.scope());
            let prefs = RwSignal::new_local(Preferences::default());
            let probe = Probe::new_with(prefs.read_only(), move |_cmds, _cancel| {
                started_probe.fetch_add(1, Ordering::SeqCst);
                queue_probe
                    .lock()
                    .expect("queue lock")
                    .pop_front()
                    .expect("queued probe future")
            });
            flush();
            assert_eq!(started.load(Ordering::SeqCst), 1);
            assert!(probe.checking_untracked(prefs.read_only()));

            supervisor
                .scope()
                .runtime_handle()
                .expect("supervisor runtime")
                .block_on(first_started_rx)
                .expect("first probe started");

            // Unrelated preference edits must NOT restart the probe.
            prefs.update(|p| p.files.show_hidden = !p.files.show_hidden);
            flush();
            assert_eq!(started.load(Ordering::SeqCst), 1);

            // Editing bwrap command cancels the stale probe and launches a new one.
            let custom = PathBuf::from("/usr/bin/bwrap");
            prefs.update(|p| p.thumbnailers.commands.bwrap = Some(custom.clone()));
            flush();
            assert_eq!(started.load(Ordering::SeqCst), 2);
            supervisor
                .scope()
                .runtime_handle()
                .expect("supervisor runtime")
                .block_on(first_dropped_rx)
                .expect("first probe cancelled and dropped");
            assert_eq!(dropped.load(Ordering::SeqCst), 1);

            let _ = second_release_tx.send(());
            supervisor
                .scope()
                .runtime_handle()
                .expect("supervisor runtime")
                .block_on(second_done_rx)
                .expect("second probe done");

            for _ in 0..20 {
                flush();
                if !probe.checking_untracked(prefs.read_only()) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(
                probe.state.get_untracked(),
                State::Done {
                    commands: ThumbnailerCommands {
                        bwrap: Some(custom.clone()),
                    },
                    sandbox: Err(SandboxError::NotFound(custom)),
                    discovered: Vec::new(),
                    mime_count: 0,
                }
            );
            assert!(!probe.bwrap_ready_untracked(prefs.read_only()));
            // While use_bwrap is true on Linux and bwrap is missing, enabling is blocked.
            prefs.update(|p| p.thumbnailers.use_bwrap = true);
            assert!(!probe.can_enable_untracked(prefs.read_only()));
            // Explicitly opting out of bwrap unlocks enabling without restarting the probe.
            prefs.update(|p| p.thumbnailers.use_bwrap = false);
            flush();
            assert!(probe.can_enable_untracked(prefs.read_only()));
            assert_eq!(started.load(Ordering::SeqCst), 2);
        });
        root.unmount();
    }

    #[test]
    fn format_summaries_hide_raw_mime_soup_and_bound_tooltips() {
        let _ = install();
        set_frame_waker(Arc::new(TestWaker::default()));
        let supervisor = Supervisor::new(false).expect("start test supervisor");

        let root = Mounted::new();
        root.with(|| {
            let app = Gds::install(supervisor.scope());
            app.i18n().install_catalog(
                crate::shared::i18n::prepare(gds::i18n::Language::Chinese)
                    .expect("Chinese catalog"),
            );

            Signal::derive_local(move || {
                assert_eq!(
                    super::summarize_entry_formats(&["image/avif".into()]),
                    "AVIF"
                );
                assert_eq!(
                    super::summarize_entry_formats(&["image/heif".into(), "image/avif".into()]),
                    "HEIF、AVIF"
                );
                assert_eq!(
                    super::summarize_entry_formats(&["application/epub+zip".into()]),
                    "EPUB"
                );

                let video_mimes: Vec<String> = vec![
                    "video/mp4".into(),
                    "video/webm".into(),
                    "video/x-matroska".into(),
                    "application/ogg".into(),
                ];
                let video_summary = super::summarize_entry_formats(&video_mimes);
                assert_eq!(video_summary, "视频 · 4 种格式");
                assert!(!video_summary.contains("video/"));

                let papers_mimes: Vec<String> = vec![
                    "application/pdf".into(),
                    "application/x-cbz".into(),
                    "image/vnd.djvu".into(),
                    "image/tiff".into(),
                ];
                let papers_summary = super::summarize_entry_formats(&papers_mimes);
                assert_eq!(papers_summary, "PDF、漫画、文档 · 4 种格式");
                assert!(!papers_summary.contains("application/"));

                let many_mimes: Vec<String> = (0..12).map(|i| format!("audio/fmt-{i}")).collect();
                let entry = gds::service::file_inspection::ThumbnailerInfo {
                    id: "ffmpegthumbnailer-audio".into(),
                    path: PathBuf::from(
                        "/usr/share/thumbnailers/ffmpegthumbnailer-audio.thumbnailer",
                    ),
                    executable: PathBuf::from("/usr/bin/ffmpegthumbnailer"),
                    mime_types: many_mimes,
                    user_override: false,
                };
                let tooltip = super::format_entry_tooltip(&entry);
                assert_eq!(tooltip.lines().count(), 2);
                assert!(tooltip.contains("audio/fmt-0"));
                assert!(tooltip.contains("audio/fmt-4"));
                assert!(!tooltip.contains("audio/fmt-5"));
                assert!(tooltip.contains("12"));
            })
            .get_untracked();
        });
        root.unmount();
    }
}
