//! Window chrome, filmstrip, filename entry and confirmation surfaces.
use gds::{
    appearance::tokens,
    ui::{
        FrameFooterProps, FrameProps, FrameToolbarProps, FrameWorkspaceProps, IconButtonProps,
        IconName,
        about::{AboutDialogProps, Application},
        button::{ApplicationMenuButtonProps, ButtonProps, ButtonSize, ButtonVariant},
        dialog::{ConfirmDialogProps, DialogDecision},
        drag::{self, ExternalFileDrop, FileDropOptions, FileDropTarget},
        field::{InputFieldProps, TextareaFieldProps},
        menu::{AnchoredMenuProps, MenuModel, render_entries},
        notifications::NotificationHostProps,
        settings::{SettingsFooterProps, SubpageToolbarTitleProps},
        sidebar::{
            SidebarPresentation, SidebarState, use_sidebar_presentation,
            use_sidebar_presentation_widths,
        },
        thumbnail::ThumbnailProps,
        window::WidthTier,
    },
};
use zgui::{component, css, reactive::UnsyncCallback, view, vocab::Role};
use zgui_ui_primitives::{Align, Binding, Placement, Side};

use super::{canvas::CanvasViewProps, commands::Command, inspector::InspectorPanelProps, *};

const WORKSPACE_CSS: &str = css!(
    r#"
    .images-root, .images-body, .images-center {
        display: flex;
        flex: 1 1 0%;
        min-width: 0;
        min-height: 0;
        width: 100%;
        height: 100%;
        overflow: hidden;
    }
    .images-root, .images-center {
        flex-direction: column;
    }
    .images-center {
        position: relative;
    }
    .images-body, .images-toolbar, .images-tools, .images-footer {
        display: flex;
        flex-direction: row;
    }
    .images-toolbar {
        align-items: center;
        width: 100%;
        height: 100%;
        flex: 1 1 0%;
        min-width: 0;
        gap: 4px;
    }
    .images-title {
        flex: 1 1 0%;
        min-width: 0;
        margin: 0 6px;
    }
    .images-location {
        width: 100%;
        height: 34px;
        align-items: center;
        border: 1px solid var(--gds-border);
        border-radius: 6px;
        overflow: hidden;
    }
    .images-filename {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-width: 0;
        padding: 0 8px;
        height: 100%;
        justify-content: center;
        cursor: pointer;
        font-weight: 500;
    }
    .images-filename, .images-entry-label text, .images-value {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .images-tools, .images-footer {
        align-items: center;
        gap: 4px;
    }
    .images-tools {
        flex: none;
    }
    .images-zoom, .images-position, .images-muted {
        font-size: var(--gds-text-xs);
        color: var(--gds-muted-foreground);
    }
    .images-zoom {
        min-width: 38px;
        text-align: center;
        font-weight: 500;
    }
    .images-position {
        width: 100%;
        text-align: center;
    }
    .images-separator {
        width: 1px;
        height: 14px;
        margin: 0 2px;
        background-color: var(--gds-border);
    }

    .images-filmstrip {
        width: 100%;
        height: 100%;
        justify-content: center;
        align-items: center;
        padding: 8px;
        gap: 6px;
        overflow: hidden;
    }
    .images-filmstrip-track {
        width: 100%;
        gap: 4px;
    }
    .images-entry {
        display: flex;
        width: 100%;
        height: 54px;
        flex: none;
        align-items: center;
        flex-direction: row;
        gap: 8px;
        padding: 0 4px;
        border: 1px solid transparent;
        border-radius: 6px;
        cursor: pointer;
        overflow: hidden;
    }
    .images-entry:hover {
        background-color: var(--gds-hover);
    }
    .images-entry.images-active {
        border-color: var(--gds-primary);
        background-color: var(--gds-selection);
    }
    .images-rail .images-entry {
        height: 42px;
        padding: 0;
        justify-content: center;
    }
    .images-entry-label {
        gap: 2px;
        flex: 1;
        min-width: 0;
        overflow: hidden;
        font-size: var(--gds-text-xs);
        font-weight: 500;
    }
    .images-hidden {
        display: none;
    }
    .images-thumb {
        width: 38px;
        height: 38px;
        flex: none;
        border-radius: 4px;
        overflow: hidden;
        object-fit: cover;
        background-color: var(--gds-surface-container);
    }
    .images-thumb .gds-thumbnail__img {
        object-fit: cover;
    }

    .images-viewport {
        position: relative;
        overflow: hidden;
        flex: 1 1 0%;
        width: 100%;
        min-height: 0;
    }
    .images-checker {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
        pointer-events: none;
    }
    .images-tile, .images-outline, .images-handle, .images-scrim {
        position: absolute;
        pointer-events: none;
    }
    .images-outline, .images-handle {
        border: 1px solid var(--gds-primary);
    }
    .images-scrim {
        background-color: rgba(0, 0, 0, 0.533);
    }
    .images-marquee {
        border-style: dashed;
        background-color: color-mix(in srgb, var(--gds-primary) 14%, transparent);
    }
    .images-handle {
        background-color: white;
    }
    .images-dashed {
        border-style: dashed;
    }
    .images-member {
        opacity: 0.65;
    }
    .images-line-selection {
        position: absolute;
        pointer-events: none;
        border-top: 1.5px dashed var(--gds-primary);
        transform-origin: 0 0;
    }
    .images-loading-pill {
        position: absolute;
        bottom: 16px;
        left: 16px;
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        padding: 6px 12px;
        border: 1px solid var(--gds-border);
        border-radius: 6px;
        background-color: var(--gds-panel);
        color: var(--gds-panel-foreground);
        font-size: var(--gds-text-xs);
        pointer-events: none;
    }
    .images-empty {
        width: 100%;
        height: 100%;
        align-items: center;
        justify-content: center;
        gap: 12px;
        padding: 24px;
    }
    .images-empty .gds-icon {
        color: var(--gds-muted-foreground);
    }
    .images-empty-title {
        font-size: var(--gds-text-md);
        font-weight: 600;
    }
    .images-empty-help {
        text-align: center;
        max-width: 360px;
        color: var(--gds-muted-foreground);
    }
    .images-empty-actions, .images-banner {
        align-items: center;
        gap: 8px;
    }
    .images-banner {
        padding: 6px;
        border: 1px solid var(--gds-border);
        border-radius: 6px;
        background-color: var(--gds-panel);
    }
    .images-crop-actions {
        position: absolute;
        align-items: center;
        padding: 4px;
        gap: 6px;
        border: 1px solid var(--gds-border);
        border-radius: 6px;
        background-color: var(--gds-panel);
    }
    .images-text-editor {
        position: absolute;
        width: 248px;
        max-width: calc(100% - 16px);
        max-height: calc(100% - 16px);
        padding: 0;
        gap: 0;
        overflow: hidden;
        border: 1px solid var(--gds-outline);
        border-radius: 8px;
        color: var(--gds-panel-foreground);
        background-color: var(--gds-panel);
        box-shadow: 0 3px 12px rgba(0, 0, 0, 0.22);
    }
    .images-text-editor .zui-field.gds-textarea-field {
        width: 100%;
        height: 58px;
        min-height: 0;
        flex: 1 1 auto;
        padding: 8px 10px;
        border: 0;
        border-radius: 0;
        color: var(--gds-panel-foreground);
        background-color: var(--gds-panel);
    }
    .images-text-actions {
        flex: none;
        justify-content: flex-end;
        align-items: center;
        gap: 6px;
        padding: 5px 7px;
        border-top: 1px solid var(--gds-border);
        background-color: var(--gds-surface-container);
    }

    .images-inspector {
        height: 100%;
        flex: none;
        position: relative;
        min-width: 0;
        overflow: hidden;
        border-left: 1px solid var(--gds-border);
    }
    .images-inspector-scroll {
        height: 100%;
        width: 100%;
        overflow-y: auto;
    }
    .images-inspector-inner {
        padding: 12px 10px 12px 12px;
        gap: 10px;
    }
    .images-tool-section {
        --gds-icon-button-size: 24px;
        flex-direction: row;
        align-items: center;
        gap: 1px;
    }
    .images-tool-section + .images-tool-section {
        border-top: 1px solid var(--gds-border);
        margin-top: 6px;
        padding-top: 6px;
    }
    .images-inspector-group {
        border-top: 1px solid var(--gds-border);
        padding-top: 10px;
        gap: 6px;
    }
    .images-fields {
        gap: 0;
    }
    .images-inspector-group, .images-heading {
        font-size: var(--gds-text-xs);
        line-height: 18px;
    }
    .images-heading {
        font-weight: 600;
        color: var(--gds-muted-foreground);
    }
    .images-picker-popover, .images-size-popover {
        padding: 10px;
        gap: 8px;
        background-color: var(--gds-panel);
        border: 1px solid var(--gds-border);
        border-radius: 8px;
        box-shadow: 0 6px 20px rgba(0, 0, 0, 0.24);
    }
    .images-picker-popover {
        width: 208px;
    }
    .images-size-popover {
        width: 162px;
    }
    .images-style-panel {
        height: 140px;
        flex: 0 0 140px;
        min-height: 140px;
        box-sizing: border-box;
        overflow: hidden;
    }
    .images-style-panel > .images-heading {
        flex: none;
    }
    .images-style-scroll {
        flex: 1 1 0%;
        width: 100%;
        min-height: 0;
        overflow-y: auto;
    }
    .images-object-actions .images-muted {
        margin-left: auto;
    }
    .images-style-empty {
        font-size: var(--gds-text-xs);
        color: var(--gds-muted-foreground);
    }
    .images-field-row {
        width: 100%;
        height: 28px;
        min-width: 0;
        flex: none;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        font-size: var(--gds-text-xs);
        font-weight: 400;
        line-height: 18px;
        --gds-control-height: 26px;
    }
    .images-field-label {
        flex: none;
        white-space: nowrap;
        color: var(--gds-muted-foreground);
    }
    .images-style-options {
        align-items: center;
        gap: 2px;
        flex: none;
    }
    .images-size-input {
        align-items: center;
        gap: 6px;
    }
    .images-size-input .gds-input-field {
        min-width: 0;
        flex: 1;
    }
    .images-chip {
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        height: 24px;
        min-width: 20px;
        padding: 0 3px;
        border: 1px solid var(--gds-border);
        border-radius: 4px;
        cursor: pointer;
        font-size: var(--gds-text-xs);
        white-space: nowrap;
    }
    .images-chip:hover {
        background-color: var(--gds-hover);
    }
    .images-custom-size {
        min-width: 42px;
        padding: 0 4px;
        gap: 2px;
    }
    .images-chip-selected {
        background-color: var(--gds-selection);
        border-color: var(--gds-primary);
    }
    .images-palette-row {
        gap: 0;
    }
    .images-swatch-button {
        width: 18px;
        height: 24px;
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        cursor: pointer;
    }
    .images-swatch {
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        width: 14px;
        height: 14px;
        background-color: var(--gds-content);
        border: 1px solid var(--gds-border);
        border-radius: 50%;
    }
    .images-swatch-button:hover .images-swatch {
        border-color: var(--gds-foreground);
    }
    .images-swatch-selected .images-swatch, .images-swatch-button:focus-visible .images-swatch {
        outline: 2px solid var(--gds-foreground);
        outline-offset: 1px;
    }
    .images-swatch-wheel {
        border: 0;
        background-color: transparent;
    }
    .images-swatch-wheel-paint {
        width: 100%;
        height: 100%;
        pointer-events: none;
    }
    .images-swatch-button:hover .images-swatch-wheel {
        outline: 1px solid var(--gds-foreground);
        outline-offset: 1px;
    }
    .images-swatch-selected .images-swatch-wheel,
    .images-swatch-button:focus-visible .images-swatch-wheel {
        outline: 2px solid var(--gds-foreground);
        outline-offset: 1px;
    }
    .images-value {
        min-width: 0;
    }
    .images-color-controls {
        gap: 6px;
    }
    .images-current-color {
        width: 16px;
        height: 16px;
        border: 1px solid var(--gds-border);
        border-radius: 3px;
        flex: none;
    }
    .images-wheel {
        align-self: center;
        width: 160px;
        max-width: 100%;
        height: 160px;
        flex: none;
        cursor: crosshair;
    }
    .images-wheel:focus-visible {
        outline: 2px solid var(--gds-primary);
    }
    .images-wheel-paint {
        width: 100%;
        height: 100%;
        pointer-events: none;
    }

    .images-inspector .zui-button.gds-icon-button[data-tool][data-toggled="true"] {
        background-color: var(--gds-primary);
        color: var(--gds-primary-foreground);
    }
    .images-inspector-slot {
        flex: none;
        overflow: hidden;
        height: 100%;
    }
    .images-overlay-clip, .images-overlay-origin {
        position: absolute;
        pointer-events: none;
    }
    .images-overlay-clip {
        overflow: hidden;
    }
    .images-field-row .gds-search-choice-button {
        flex: 0 1 auto;
        min-width: 0;
        max-width: 100%;
        height: 24px;
    }
    "#
);

fn action_label(p: Page, command: Command) -> String {
    if command == Command::Inspector {
        text(if p.window().prefs.state().with(|v| v.inspector_open) {
            "action-hide-inspector"
        } else {
            "action-show-inspector"
        })
    } else {
        command.label()
    }
}

pub(super) fn button(p: Page, c: Command) -> AnyView {
    let w = p.window();
    let vs = w.version;
    let cat = w.catalog_version;
    let busy = w.busy;
    let loading = w.loading;
    let prefs = w.prefs.state();
    let disabled = Signal::derive_local(move || {
        vs.track();
        cat.track();
        busy.track();
        loading.track();
        !p.enabled(c) || (c == Command::Delete && p.read(|s| s.tool.selection.is_empty()))
    });
    let toggled = match c {
        Command::Filmstrip => Some(Signal::derive_local(move || {
            prefs.with(|s| s.filmstrip_open)
        })),
        Command::Inspector => Some(Signal::derive_local(move || {
            prefs.with(|s| s.inspector_open)
        })),
        Command::Save => Some(Signal::derive_local(move || {
            vs.track();
            p.dirty()
        })),
        Command::Fit | Command::Actual | Command::Width | Command::Height => {
            Some(Signal::derive_local(move || {
                use crate::shared::geometry::FitMode;
                p.window().paint.track();
                p.read(|s| match c {
                    Command::Fit => s.transform.fit_mode == FitMode::FitToWindow,
                    Command::Actual => {
                        s.transform.fit_mode == FitMode::ActualPixels
                            || (s.transform.fit_mode == FitMode::Custom
                                && (s.transform.scale - 1.).abs() < 0.005)
                    }
                    Command::Width => s.transform.fit_mode == FitMode::FillWidth,
                    _ => s.transform.fit_mode == FitMode::FillHeight,
                })
            }))
        }
        _ => None,
    };
    AnyView::new(view! {
        IconButton(
            icon = c.icon(),
            label = Signal::derive_local(move || action_label(p, c)),
            disabled = disabled,
            toggled = toggled,
            shortcut = c.shortcut(),
            on:click = move |_| p.action(c),
        )
    })
}

#[component]
fn AppMenu(page: Page) -> impl IntoView {
    use Command::*;
    let w = page.window();
    let open = page.popover_binding(ActivePopover::AppMenu);
    let active = Signal::derive_local(move || open.get().unwrap_or(false));
    let model = Signal::derive_local(move || {
        w.version.track();
        w.busy.track();
        w.loading.track();
        let mut m = MenuModel::new();
        let push_item = |m: &mut MenuModel, cmd: Command| {
            m.item(
                cmd.label(),
                Some(cmd.icon()),
                cmd.shortcut(),
                !page.enabled(cmd),
                cmd == Trash,
                UnsyncCallback::new(move |()| {
                    open.write(false);
                    page.action(cmd);
                }),
            );
        };
        for group in [
            &[Open, OpenFolder, Paste][..],
            &[Save, SaveAs, Copy],
            &[Rename, Reveal, OpenWith, Trash],
        ] {
            if !m.entries.is_empty() {
                m.separator();
            }
            for &cmd in group {
                push_item(&mut m, cmd);
            }
        }
        m.separator();
        push_item(&mut m, Settings);
        if gds::ui::logs::is_enabled() {
            push_item(&mut m, Logs);
        }
        push_item(&mut m, About);
        m
    });
    let anchor = NodeRef::new();
    view! {
        box {
            ApplicationMenuButton(
                node_ref = anchor,
                application = Application::try_cargo("gds-images"),
                label = text("action-app-menu"),
                active = active,
                on:click = move |_| open.write(!open.get_untracked().unwrap_or(false)),
            ) {}
            AnchoredMenu(
                anchor = anchor,
                open = open,
                placement = Placement::new(Side::Bottom, Align::Start),
            ) {
                {render_entries(model.get().entries)}
            }
        }
    }
}

#[component]
fn RenameInput(page: Page, value: RwSignal<String>, node: NodeRef) -> impl IntoView {
    let fx = RenderEffect::new(move |prev: Option<bool>| {
        let mounted = node.get().is_some();
        if mounted && prev != Some(true) {
            node.focus();
            node.select_all();
        }
        mounted
    });
    on_cleanup_local(move || drop(fx));
    let binding = Binding::controlled(
        Signal::derive_local(move || value.get()),
        move |next: String| value.set(next),
    );
    view! {
        InputField(
            node_ref = node,
            value = binding,
            label = text("action-edit-location"),
            placeholder = text("path-placeholder"),
            on:focus_out = move |_| page.close_rename(),
        )
    }
}

#[component]
fn TextAnnotationEditor(page: Page, value: RwSignal<String>, node: NodeRef) -> impl IntoView {
    let fx = RenderEffect::new(move |prev: Option<bool>| {
        let mounted = node.get().is_some();
        if mounted && prev != Some(true) {
            node.focus();
            node.select_all();
        }
        mounted
    });
    on_cleanup_local(move || drop(fx));
    let binding = Binding::controlled(
        Signal::derive_local(move || value.get()),
        move |next: String| {
            value.set(next.clone());
            page.change(|state| {
                state.tool.update_text_draft_content(next);
            });
            page.render();
        },
    );
    let editor = NodeRef::new();
    let measured = editor.observe_border_size();
    let position = Memo::new(move |_| {
        let w = page.window();
        w.paint.track();
        let viewport = w.size.get();
        let lines = value.with(|v| v.split('\n').count() as f32);
        let (width, height) = measured
            .get()
            .map(|size| {
                let scale = editor.scale().max(0.01);
                (size.width.0 / scale, size.height.0 / scale)
            })
            .unwrap_or((248., 112.));
        page.read(|s| {
            use crate::features::edit_canvas::{ActiveInteraction, TextEditTarget};
            let ActiveInteraction::EditingText(draft) = &s.tool.interaction else {
                return Point2::new(0., 0.);
            };
            let anchor = match &draft.target {
                TextEditTarget::New { position } => *position,
                TextEditTarget::Existing { original, .. } => {
                    let bounds = original.visual_bounds();
                    Point2::new(bounds.x, bounds.y)
                }
            };
            let anchor = s.transform.image_to_viewport(anchor);
            let gap = 8.;
            let (x, y) = if anchor.y >= height + 2. * gap {
                (anchor.x, anchor.y - height - gap)
            } else if anchor.x >= width + 2. * gap {
                (anchor.x - width - gap, anchor.y)
            } else {
                (
                    anchor.x,
                    anchor.y + lines * draft.font_size * s.transform.scale * 1.25 + gap,
                )
            };
            Point2::new(
                x.clamp(gap, (viewport.width - width - gap).max(gap)),
                y.clamp(gap, (viewport.height - height - gap).max(gap)),
            )
        })
    });
    let editing_existing = page.read(|s| {
        matches!(
            &s.tool.interaction,
            crate::features::edit_canvas::ActiveInteraction::EditingText(draft)
                if matches!(
                    &draft.target,
                    crate::features::edit_canvas::TextEditTarget::Existing { .. }
                )
        )
    });
    let apply_label = text(if editing_existing {
        "tool-finish-text"
    } else {
        "tool-commit-text"
    });
    view! {
        column(
            class = "images-text-editor",
            node_ref = editor,
            style:left = move || Some(format!("{}px", position.get().x)),
            style:top = move || Some(format!("{}px", position.get().y)),
        ) {
            TextareaField(
                node_ref = node,
                value = binding,
                label = text("tool-text"),
                placeholder = text("tool-text-placeholder"),
                style:font-family = move || {
                    page.window().version.track();
                    page.read(|s| match &s.tool.interaction {
                        crate::features::edit_canvas::ActiveInteraction::EditingText(draft) => {
                            Some(draft.family.clone())
                        }
                        _ => None,
                    })
                },
            )
            row(class = "images-text-actions") {
                Button(
                    size = ButtonSize::Sm,
                    variant = ButtonVariant::Ghost,
                    on:click = move |_| page.escape(),
                ) {{text("tool-cancel-text")}}
                Button(
                    size = ButtonSize::Sm,
                    on:click = move |_| {
                        page.flush_text();
                        page.focus();
                    },
                ) {{apply_label}}
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct MemoryRow {
    name: String,
    subtitle: String,
    url: String,
}

#[component]
fn Filmstrip(page: Page, presentation: Signal<SidebarPresentation, LocalStorage>) -> impl IntoView {
    let w = page.window();
    let cache = w.cache.clone();
    let window = try_use_window();
    let window_height = Signal::derive_local(move || {
        window
            .as_ref()
            .map(|win| win.size().get().height.0)
            .unwrap_or(600.)
    });
    let rail = Memo::new(move |_| presentation.get().collapsed());
    let capacity = Memo::new(move |_| {
        let row_height = if rail.get() { 42. } else { 54. };
        ((window_height.get() - tokens::TOOLBAR_HEIGHT - tokens::FOOTER_HEIGHT - 84.)
            / (row_height + 4.))
            .floor()
            .max(1.) as usize
    });
    let memory_row = Memo::new(move |_| {
        page.window().version.track();
        page.read(|s| {
            let doc = s.doc.as_ref()?;
            let thumb = s.memory_thumbnail.as_ref()?;
            let (width, height) = doc.output_dimensions();
            Some(MemoryRow {
                name: doc.decoded.origin.display_name(),
                subtitle: format!("{} · {width}×{height}", doc.format().label()),
                url: thumb.url(),
            })
        })
    });
    let has_rows = Memo::new(move |_| {
        page.window().catalog_version.track();
        memory_row.get().is_some() || page.read(|s| !s.catalog.entries.is_empty())
    });
    let scroll_delta = Cell::new(0.0_f32);
    let track_rows = move || {
        page.window().catalog_version.track();
        let cap = capacity.get();
        let memory = memory_row.get();
        let (entries, current) = page.read(|s| {
            let (_, slice) = s.catalog.visible_window(cap);
            (slice.to_vec(), s.catalog.current_path().cloned())
        });
        let mut rows: Vec<AnyView> = entries
            .into_iter()
            .map(|entry| {
                let path = entry.path;
                let name = entry.meta.display_name.to_string();
                let subtitle = entry.meta.subtitle.to_string();
                let active = current.as_ref() == Some(&path);
                let source = workflows::inspection_source(&path, &entry.meta);
                AnyView::new(view! {
                    control(
                        class = "images-entry",
                        class:images-active = active,
                        a11y:role = Role::Button,
                        a11y:label = name.clone(),
                        tabindex = Focus::Sequential,
                        on:click = move |_| page.open_path(path.clone()),
                    ) {
                        box(class = "images-thumb") {
                            Thumbnail(cache = cache.clone(), source = Signal::stored_local(source))
                        }
                        column(
                            class = "images-entry-label",
                            class:images-hidden = move || rail.get(),
                            style:opacity = move || Some(format!("{:.3}", presentation.get().label_opacity())),
                        ) {
                            text {{name}}
                            text(class = "images-muted") {{subtitle}}
                        }
                    }
                })
            })
            .collect();
        if let Some(MemoryRow {
            name,
            subtitle,
            url,
        }) = memory
        {
            rows.push(AnyView::new(view! {
                row(class = "images-entry images-active") {
                    image(class = "images-thumb", src = Some(url))
                    column(
                        class = "images-entry-label",
                        class:images-hidden = move || rail.get(),
                        style:opacity = move || Some(format!("{:.3}", presentation.get().label_opacity())),
                    ) {
                        text {{name}}
                        text(class = "images-muted") {{subtitle}}
                    }
                }
            }));
        }
        rows
    };

    view! {
        column(
            class = "images-filmstrip",
            class:images-rail = move || rail.get(),
            on:wheel = move |ev: &mut EventCx<'_, zgui::view::events::Wheel>| {
                let dy = ev.delta.to_pixels(zgui::geom::CssPx(48.)).height.0;
                let last = scroll_delta.get();
                let next = if last.signum() == dy.signum() {
                    last + dy
                } else {
                    dy
                };
                scroll_delta.set(next);
                if next.abs() >= 40.
                    && !page.window().loading.get_untracked()
                    && !page.window().confirm_open.get_untracked()
                {
                    scroll_delta.set(0.);
                    page.action(if next > 0. {
                        Command::Next
                    } else {
                        Command::Prev
                    });
                }
                ev.prevent_default();
            },
        ) {
            {move || has_rows.get().then(|| button(page, Command::Prev))}
            column(class = "images-filmstrip-track") {
                {track_rows}
            }
            {move || has_rows.get().then(|| button(page, Command::Next))}
        }
    }
}

#[component]
fn CropActionBar(page: Page) -> impl IntoView {
    move || {
        page.window().version.track();
        page.window().paint.track();
        let vp = page.read(|s| {
            s.tool.staged_crop.map(|r| {
                s.transform
                    .image_rect_to_viewport(crate::shared::geometry::Rect2::new(
                        r.x as f32,
                        r.y as f32,
                        r.width as f32,
                        r.height as f32,
                    ))
            })
        });
        vp.map(|rect| {
            let size = page.window().size.get();
            let x = rect.x.clamp(8., (size.width - 210.).max(8.));
            let y = if rect.y < 38. {
                (rect.y + 6.).clamp(6., (size.height - 40.).max(6.))
            } else {
                (rect.y - 36.).max(6.)
            };
            view! {
                row(
                    class = "images-crop-actions",
                    style:left = format!("{x}px"),
                    style:top = format!("{y}px"),
                ) {
                    Button(
                        size = ButtonSize::Sm,
                        icon = IconName::Check,
                        on:click = move |_| page.action(Command::Crop),
                    ) {{text("tool-apply-crop")}}
                    Button(
                        size = ButtonSize::Sm,
                        variant = ButtonVariant::Ghost,
                        icon = IconName::X,
                        on:click = move |_| page.escape(),
                    ) {{text("tool-clear-crop")}}
                }
            }
        })
    }
}

#[component]
fn WorkspaceBody(page: Page) -> impl IntoView {
    let w = page.window();
    let prefs = w.prefs.state();
    let right = use_sidebar_presentation_widths(
        Signal::derive_local(move || {
            SidebarState::from_collapsed(!prefs.with(|s| s.inspector_open))
        }),
        inspector::WIDTH,
        0.,
    );
    let visible = Memo::new(move |_| right.get().width() > 0.);
    let drops = drag::file_drop_target_attrs(FileDropTarget {
        active: Signal::stored_local(false),
        options: FileDropOptions::copy_only(),
        can_accept: Some(Rc::new(move || !page.window().busy.get_untracked())),
        on_hover_change: None,
        on_reject: None,
        on_drop: Some(UnsyncCallback::new(move |drop: ExternalFileDrop| {
            if let Some(path) = drop
                .paths
                .into_iter()
                .find(|p| gds::service::image::source::Format::from_path(p).is_some())
            {
                page.open_path(path);
                if let Some(h) = drop.handle {
                    h.complete();
                }
            } else if let Some(h) = drop.handle {
                h.reject();
            }
        })),
    });
    let inline_edit = w.inline_edit;
    let text_editor = move || match inline_edit.get() {
        InlineEdit::Text { value, node } => AnyView::new(view! {
            TextAnnotationEditor(page = page, value = value, node = node)
        }),
        InlineEdit::None | InlineEdit::Rename { .. } => AnyView::new(()),
    };
    view! {
        row(class = "images-body", {..drops}) {
            column(class = "images-center") {
                CanvasView(page = page) {}
                {text_editor}
                CropActionBar(page = page) {}
            }
            box(
                class = "images-inspector-slot",
                style:width = move || Some(format!("{}px", right.get().width())),
            ) {
                {move || visible.get().then(|| view! { InspectorPanel(page = page) }.into_view())}
            }
        }
    }
}

#[component]
fn WorkspaceToolbar(page: Page, micro: Signal<bool, LocalStorage>) -> impl IntoView {
    use Command::*;
    let w = page.window();
    let settings = w.settings;
    let inline_edit = w.inline_edit;
    let prefs = w.prefs.state();
    move || {
        if settings.get() {
            return zgui::view::Either::Left(view! {
                row(class = "images-toolbar") {
                    {move || (!micro.get()).then(|| {
                        view! {
                            IconButton(
                                icon = Filmstrip.icon(),
                                label = Filmstrip.label(),
                                shortcut = Filmstrip.shortcut(),
                                toggled = Signal::derive_local(move || prefs.with(|s| s.filmstrip_open)),
                                on:click = move |_| page.action(Filmstrip),
                            )
                        }
                        .into_view()
                    })}
                    SubpageToolbarTitle(label = text("settings-title"))
                }
            });
        }
        let title = move || {
            if let InlineEdit::Rename { value, node } = inline_edit.get() {
                return AnyView::new(
                    view! { RenameInput(page = page, value = value, node = node) },
                );
            }
            page.window().version.track();
            let Some(name) = page.read(|s| s.doc.as_ref().map(|d| d.decoded.origin.display_name()))
            else {
                return AnyView::new(());
            };
            let label = if page.dirty() {
                format!("{name} *")
            } else {
                name
            };
            let local = page.path().is_some();
            AnyView::new(view! {
                row(class = "images-location") {
                    control(
                        class = "images-filename",
                        a11y:role = Role::Label,
                        on:click = move |_| {
                            if page.enabled(Rename) {
                                page.action(Rename);
                            }
                        },
                    ) {{label}}
                    {local.then(|| (button(page, Reveal), button(page, Rename)))}
                }
            })
        };
        zgui::view::Either::Right(view! {
            row(class = "images-toolbar") {
                {button(page, Filmstrip)}
                {button(page, Save)}
                {button(page, Copy)}
                {button(page, Undo)}
                {button(page, Redo)}
                box(class = "images-title") {{title}}
                {button(page, Inspector)}
            }
        })
    }
}

#[component]
pub(super) fn WorkspaceView(page: Page) -> impl IntoView {
    use Command::*;
    let w = page.window();
    let settings = w.settings;
    zgui::view::install_stylesheet("gds-images", WORKSPACE_CSS);
    let prefs = w.prefs.state();
    let expanded = Signal::derive_local(move || prefs.with(|s| s.filmstrip_open));
    let screen = try_use_window();
    let is_micro = Signal::derive_local(move || {
        screen.as_ref().is_some_and(|win| {
            let width = win.size().get().width.0;
            width > 0.0 && WidthTier::from_width(width) == WidthTier::Micro
        })
    });
    let sidebar_presentation = use_sidebar_presentation(Signal::derive_local(move || {
        SidebarState::from_collapsed(is_micro.get() || !expanded.get())
    }));
    let sidebar_width = Signal::derive_local(move || sidebar_presentation.get().width());
    let settings_page = crate::widgets::settings::View::new(w.prefs.clone());
    let navigation = settings_page.clone();
    let sidebar = AnyView::new(move || {
        if settings.get() {
            navigation.navigation(sidebar_presentation)
        } else {
            AnyView::new(view! { Filmstrip(page = page, presentation = sidebar_presentation) })
        }
    });
    let leading = AnyView::new(move || {
        if settings.get() {
            AnyView::new(view! {
                IconButton(
                    icon = IconName::ArrowLeft,
                    label = text("action-back"),
                    shortcut = gds::ui::Shortcut::parse("Escape"),
                    on:click = move |_| page.action(Settings),
                )
            })
        } else {
            AnyView::new(view! { AppMenu(page = page) })
        }
    });
    let toolbar = AnyView::new(view! { WorkspaceToolbar(page = page, micro = is_micro) });
    let controls = AnyView::new(move || {
        page.window().version.track();
        if settings.get() || page.read(|s| s.doc.is_none()) {
            return AnyView::new(());
        }
        let zoom = move || {
            page.window().paint.track();
            format!("{:.0}%", page.read(|s| s.transform.scale) * 100.)
        };
        AnyView::new(view! {
            row(class = "images-tools") {
                {button(page, ZoomOut)}
                text(class = "images-zoom") {{zoom}}
                {button(page, ZoomIn)}
                box(class = "images-separator") {}
                {button(page, Fit)}
                {button(page, Actual)}
                {button(page, Width)}
                {button(page, Height)}
            }
        })
    });
    let facts = move || {
        page.window().version.track();
        if settings.get() {
            return String::new();
        }
        page.read(|s| {
            let Some(doc) = &s.doc else {
                return text("empty-subtitle");
            };
            let (width, height) = doc.output_dimensions();
            let size = s
                .metadata
                .as_ref()
                .and_then(|m| m.bundle.technical.file_bytes)
                .map(|bytes| format!(" · {}", crate::entities::catalog::format_byte_size(bytes)))
                .unwrap_or_default();
            format!("{} · {width} × {height} px{size}", doc.format().label())
        })
    };
    let position = move || {
        page.window().catalog_version.track();
        if settings.get() {
            return String::new();
        }
        page.read(|s| {
            s.catalog
                .position_display()
                .map(|(n, total)| format!("{n} / {total}"))
                .unwrap_or_default()
        })
    };
    let sidebar_status = AnyView::new(view! { text(class = "images-position") {{position}} });
    let footer = AnyView::new(view! {
        row(class = "images-footer") {
            {move || {
                if settings.get() {
                    AnyView::new(view! { SettingsFooter(label = text("settings-auto-apply")) })
                } else {
                    AnyView::new(view! {
                        text(
                            class = "images-muted",
                            a11y:role = Role::Label,
                            a11y:label = facts,
                        ) {{facts}}
                    })
                }
            }}
        }
    });
    let confirm_open = w.confirm_open;
    let confirm_kind = w.confirm_kind;
    let confirm_generation = w.confirm_generation;
    let about = w.about;
    let confirm_title = Signal::derive_local(move || match confirm_kind.get() {
        ConfirmKind::Trash(_) => text("action-trash"),
        ConfirmKind::Discard => text("dialog-discard-title"),
    });
    let confirm_description = Signal::derive_local(move || match confirm_kind.get() {
        ConfirmKind::Trash(path) => path.display().to_string(),
        ConfirmKind::Discard => text("dialog-discard-changes"),
    });
    let confirm_label = Signal::derive_local(move || match confirm_kind.get() {
        ConfirmKind::Trash(_) => text("action-trash"),
        ConfirmKind::Discard => text("dialog-discard-confirm"),
    });
    let cancel_label = Signal::derive_local(move || match confirm_kind.get() {
        ConfirmKind::Trash(_) => text("dialog-trash-cancel"),
        ConfirmKind::Discard => text("dialog-discard-cancel"),
    });
    let decide = UnsyncCallback::new(move |decision| {
        let pending = page.window().pending.borrow_mut().take();
        if decision == DialogDecision::Confirm
            && let Some(prompt) = pending
        {
            page.proceed(prompt.action);
        }
        page.focus();
    });
    let root = NodeRef::new();
    let initial = RenderEffect::new(move |previous: Option<(bool, bool)>| {
        let route = (root.get().is_some(), settings.get());
        if route.0 && !route.1 && Some(route) != previous {
            root.focus();
        }
        route
    });
    on_cleanup_local(move || drop(initial));
    view! {
        NotificationHost {
            Frame(sidebar_width = sidebar_width) {
                column(
                    node_ref = root,
                    tabindex = Focus::Programmatic,
                    class = "images-root",
                    on:key_down = move |event| page.key(event),
                ) {
                    FrameToolbar(menu = leading, controls = toolbar)
                    FrameWorkspace(sidebar = sidebar) {
                        {move || {
                            if settings.get() {
                                settings_page.content()
                            } else {
                                AnyView::new(view! { WorkspaceBody(page = page) })
                            }
                        }}
                    }
                    FrameFooter(sidebar_status = sidebar_status, status = footer, controls = controls)
                }
            }
            ConfirmDialog(
                open = confirm_open,
                generation = confirm_generation,
                description = confirm_description,
                title = confirm_title,
                confirm_label = confirm_label,
                cancel_label = cancel_label,
                destructive = true,
                on_decision = decide,
            )
            AboutDialog(open = about, application = Application::cargo("gds-images"))
        }
    }
}
