//! Editing tools, selection style and document properties.

mod style;

pub(super) use style::{CustomSizes, Dimension};

use gds::{
    i18n::text_untracked as text,
    ui::{
        IconButtonProps, IconName, Shortcut,
        button::{ButtonProps, ButtonSize, ButtonVariant},
    },
};
use zgui::{prelude::*, reactive::RenderEffect};

use super::{ActivePopover, Page, chrome::button, commands::Command};
use crate::{
    entities::{catalog::format_byte_size, document::AnnotationId, metadata::MetadataState},
    features::edit_canvas::{StyleTarget, ToolMode},
};
use style::StyleControlsProps;

pub(super) const WIDTH: f32 = 216.0;

#[derive(Clone, PartialEq, Eq)]
enum StyleScope {
    Tool(ToolMode),
    Objects(Vec<AnnotationId>),
    TextDraft(Vec<AnnotationId>),
}

impl StyleScope {
    const fn target(&self) -> StyleTarget {
        match self {
            Self::Tool(_) => StyleTarget::Tool,
            Self::Objects(_) => StyleTarget::Objects,
            Self::TextDraft(_) => StyleTarget::TextDraft,
        }
    }

    const fn heading_key(&self) -> &'static str {
        match self {
            Self::Tool(_) => "inspector-tool-style",
            Self::Objects(_) | Self::TextDraft(_) => "inspector-object-style",
        }
    }
}

fn tool_icon(mode: ToolMode) -> IconName {
    match mode {
        ToolMode::Pointer => IconName::MousePointer,
        ToolMode::Pan => IconName::Hand,
        ToolMode::Crop => IconName::Crop,
        ToolMode::Rect => IconName::Square,
        ToolMode::Ellipse => IconName::Circle,
        ToolMode::Arrow => IconName::ArrowUpRight,
        ToolMode::Line => IconName::Minus,
        ToolMode::Brush => IconName::Pencil,
        ToolMode::Text => IconName::Type,
        ToolMode::Number => IconName::CircleNumber1,
        ToolMode::Mosaic => IconName::Grid2x2,
        ToolMode::Blur => IconName::Droplet,
    }
}

fn tool_shortcut(mode: ToolMode) -> Option<Shortcut> {
    Shortcut::parse(match mode {
        ToolMode::Pointer => "1",
        ToolMode::Pan => "2",
        ToolMode::Crop => "3",
        ToolMode::Rect => "4",
        ToolMode::Ellipse => "5",
        ToolMode::Arrow => "6",
        ToolMode::Line => "7",
        ToolMode::Brush => "B",
        ToolMode::Text => "8",
        ToolMode::Mosaic => "9",
        ToolMode::Blur => "0",
        ToolMode::Number => "N",
    })
}

#[component]
fn SectionHeading(#[prop(into)] label: Signal<String, LocalStorage>) -> impl IntoView {
    view! {
        text(
            class = "images-heading",
            a11y:role = Role::Heading,
            a11y:label = move || label.get(),
        ) {{move || label.get()}}
    }
}

fn fact_row(key: &'static str, value: String) -> AnyView {
    let label = text(key);
    AnyView::new(view! {
        row(class = "images-field-row") {
            text(
                class = "images-field-label",
                a11y:role = Role::Label,
                a11y:label = label.clone(),
            ) {{label}}
            text(class = "images-value") {{value}}
        }
    })
}

fn build_fact_rows(
    dimensions: Option<(u32, u32)>,
    metadata: Option<&MetadataState>,
) -> Vec<AnyView> {
    let mut rows = Vec::new();
    if let Some(metadata) = metadata {
        let technical = &metadata.bundle.technical;
        let exif = &metadata.bundle.exif;
        let profile = technical.icc_profile_name.clone().unwrap_or_else(|| {
            if technical.has_icc_profile {
                "ICC".to_owned()
            } else {
                "sRGB".to_owned()
            }
        });
        let technical_fields = [
            ("field-format", Some(technical.format.label().to_owned())),
            (
                "field-dimensions",
                dimensions.map(|(width, height)| format!("{width} × {height} px")),
            ),
            (
                "field-file-size",
                Some(
                    technical
                        .file_bytes
                        .map_or_else(|| "—".to_owned(), format_byte_size),
                ),
            ),
            ("field-color-type", Some(technical.color_type.clone())),
            ("field-color-profile", Some(profile)),
            (
                "field-orientation",
                (technical.orientation > 1).then(|| technical.orientation.to_string()),
            ),
        ];
        for (key, value) in technical_fields {
            if let Some(value) = value {
                rows.push(fact_row(key, value));
            }
        }
        if !exif.is_empty() {
            rows.push(AnyView::new(view! {
                SectionHeading(label = text("inspector-camera"))
            }));
            let camera = [exif.camera_make.as_deref(), exif.camera_model.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            let exposure = [exif.exposure_time.as_deref(), exif.f_number.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            let exif_fields = [
                ("field-camera", (!camera.is_empty()).then_some(camera)),
                ("field-lens", exif.lens_model.clone()),
                ("field-iso", exif.iso.map(|iso| iso.to_string())),
                ("field-exposure", (!exposure.is_empty()).then_some(exposure)),
                ("field-focal-length", exif.focal_length.clone()),
            ];
            for (key, value) in exif_fields {
                if let Some(value) = value {
                    rows.push(fact_row(key, value));
                }
            }
        }
    }
    if dimensions.is_none() {
        rows.push(AnyView::new(view! {
            text(class = "images-muted") {{text("empty-title")}}
        }));
    }
    rows
}

#[component]
fn FactsList(page: Page) -> impl IntoView {
    let rows = move || {
        page.window().version.track();
        page.read(|state| {
            let dimensions = state.doc.as_ref().map(|doc| doc.output_dimensions());
            build_fact_rows(dimensions, state.metadata.as_ref())
        })
    };
    view! {
        column(class = "images-fields") {
            {rows}
        }
    }
}

#[component]
fn ToolButton(page: Page, mode: ToolMode) -> impl IntoView {
    let toggled = Signal::derive_local(move || {
        page.window().version.track();
        page.read(|state| state.tool.tool == mode)
    });
    let disabled = Signal::derive_local(move || {
        page.window().version.track();
        page.window().busy.track();
        !page.enabled(Command::Copy)
    });
    view! {
        IconButton(
            attr:data-tool = mode.element_id(),
            icon = tool_icon(mode),
            label = text(mode.label_key()),
            shortcut = tool_shortcut(mode),
            toggled = toggled,
            disabled = disabled,
            on:click = move |_| page.select_tool(mode),
        )
    }
}

#[component]
pub(super) fn InspectorPanel(page: Page) -> impl IntoView {
    // Keep the controls mounted while their values change. A new editing target closes
    // its predecessor's popovers and discards any uncommitted numeric entry.
    let target = Memo::new(move |_| {
        page.window().version.track();
        page.read(|state| {
            let doc = state.doc.as_ref()?;
            let selected_ids = || {
                doc.annotations
                    .iter()
                    .filter(|item| state.tool.selection.contains(item.id))
                    .map(|item| item.id)
                    .collect()
            };
            Some(match state.tool.style_target()? {
                StyleTarget::Tool => StyleScope::Tool(state.tool.tool),
                StyleTarget::Objects => StyleScope::Objects(selected_ids()),
                StyleTarget::TextDraft => StyleScope::TextDraft(selected_ids()),
            })
        })
    });
    let inspector_popovers = [
        page.popover_binding(ActivePopover::ColorSelector),
        page.popover_binding(ActivePopover::CustomSize(Dimension::Stroke)),
        page.popover_binding(ActivePopover::CustomSize(Dimension::Marker)),
        page.popover_binding(ActivePopover::CustomSize(Dimension::Font)),
        page.popover_binding(ActivePopover::CustomSize(Dimension::Mosaic)),
        page.popover_binding(ActivePopover::CustomSize(Dimension::Blur)),
    ];
    let dismiss_inspector_popovers = move || {
        for popover in inspector_popovers {
            if popover.get_untracked() == Some(true) {
                popover.write(false);
            }
        }
    };
    let dismiss_on_target_change =
        RenderEffect::new(move |previous: Option<Option<StyleScope>>| {
            let current = target.get();
            if previous.is_some() && previous.as_ref() != Some(&current) {
                dismiss_inspector_popovers();
            }
            current
        });
    on_cleanup_local(move || {
        drop(dismiss_on_target_change);
        dismiss_inspector_popovers();
    });

    let selection_count = Memo::new(move |_| {
        page.window().version.track();
        page.read(|state| state.tool.selection.len())
    });
    let count_label = move || {
        gds::t_untracked!(
            "inspector-selection-count",
            count = selection_count.get() as i64
        )
    };
    let style_heading = Signal::derive_local(move || {
        let key = target
            .get()
            .as_ref()
            .map_or("inspector-tool-style", StyleScope::heading_key);
        text(key)
    });
    let styles = move || match target.get() {
        Some(scope) => Either::Left(
            view! {
                StyleControls(page = page, target = scope.target())
            }
            .into_view(),
        ),
        None => Either::Right(view! {
            text(class = "images-style-empty") {{text("inspector-style-empty")}}
        }),
    };
    let strip_disabled = Signal::derive_local(move || {
        page.window().version.track();
        page.window().busy.track();
        !page.enabled(Command::Strip)
    });

    view! {
        box(class = "images-inspector", style:width = format!("{WIDTH}px")) {
            scroll(class = "images-inspector-scroll") {
                column(class = "images-inspector-inner") {
                    column(class = "images-tool-sections") {
                        row(
                            class = "images-tool-section",
                            a11y:role = Role::Group,
                            a11y:label = text("tools-selection"),
                        ) {
                            ToolButton(page = page, mode = ToolMode::Pointer)
                            ToolButton(page = page, mode = ToolMode::Pan)
                        }
                        row(
                            class = "images-tool-section",
                            a11y:role = Role::Group,
                            a11y:label = text("tools-image"),
                        ) {
                            {button(page, Command::Ccw)}
                            {button(page, Command::Cw)}
                            {button(page, Command::FlipH)}
                            {button(page, Command::FlipV)}
                            ToolButton(page = page, mode = ToolMode::Crop)
                            ToolButton(page = page, mode = ToolMode::Mosaic)
                            ToolButton(page = page, mode = ToolMode::Blur)
                        }
                        row(
                            class = "images-tool-section",
                            a11y:role = Role::Group,
                            a11y:label = text("tools-objects"),
                        ) {
                            ToolButton(page = page, mode = ToolMode::Arrow)
                            ToolButton(page = page, mode = ToolMode::Rect)
                            ToolButton(page = page, mode = ToolMode::Ellipse)
                            ToolButton(page = page, mode = ToolMode::Line)
                            ToolButton(page = page, mode = ToolMode::Brush)
                            ToolButton(page = page, mode = ToolMode::Text)
                            ToolButton(page = page, mode = ToolMode::Number)
                        }
                        row(
                            class = "images-tool-section images-object-actions",
                            a11y:role = Role::Group,
                            a11y:label = text("tools-object-actions"),
                        ) {
                            {button(page, Command::Duplicate)}
                            {button(page, Command::Forward)}
                            {button(page, Command::Backward)}
                            {button(page, Command::Delete)}
                            {move || {
                                (selection_count.get() > 0).then(|| {
                                    view! {
                                        text(
                                            class = "images-muted",
                                            a11y:role = Role::Label,
                                            a11y:label = count_label,
                                        ) {{count_label}}
                                    }
                                })
                            }}
                        }
                    }
                    column(class = "images-inspector-group images-style-panel") {
                        SectionHeading(label = style_heading)
                        scroll(class = "images-style-scroll") {
                            {styles}
                        }
                    }
                    column(class = "images-inspector-group") {
                        SectionHeading(label = text("inspector-properties"))
                        FactsList(page = page)
                    }
                    column(class = "images-inspector-group") {
                        SectionHeading(label = text("inspector-metadata"))
                        Button(
                            size = ButtonSize::Sm,
                            variant = ButtonVariant::Outline,
                            icon = IconName::Shield,
                            disabled = strip_disabled,
                            on:click = move |_| page.action(Command::Strip),
                        ) {{text("inspector-strip-privacy")}}
                    }
                }
            }
        }
    }
}
