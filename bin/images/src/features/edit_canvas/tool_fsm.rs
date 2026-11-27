//! Pure-Rust hybrid interaction FSM, text draft transactions, and non-destructive property summary.

use std::sync::Arc;

use gds::service::image::PixelRect;

use crate::{
    entities::document::{
        AnnotationId, AnnotationItem, AnnotationKind, DocumentState, RgbaColor, SpatialTransformOp,
    },
    shared::geometry::{Point2, Size2, ViewportTransform},
};

use super::{
    command::EditCommand,
    history::{CoalesceKey, EditHistory, StyleField},
    hit_test::{HandleKind, HitTarget, hit_test_canvas, hit_test_cycle_stack, marquee_hits},
    manipulate::{
        ReorderOp, ResizeModifiers, compute_duplicate_offset, reorder_annotations,
        resize_item_with_handle, snap_angle_15, translate_group,
    },
    selection::SelectionSet,
};

const DRAG_THRESHOLD_SCREEN_PX: f32 = 3.0;
const MIN_DRAW_DRAG_PX: f32 = 2.0;
const MIN_BRUSH_STEP_SCREEN_PX: f32 = 1.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, strum::EnumIter)]
pub enum ToolMode {
    #[default]
    Pointer,
    Pan,
    Crop,
    Rect,
    Ellipse,
    Arrow,
    Line,
    Brush,
    Text,
    Number,
    Mosaic,
    Blur,
}

impl ToolMode {
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Pointer => "tool-select",
            Self::Pan => "tool-pan",
            Self::Crop => "tool-crop",
            Self::Rect => "tool-rect",
            Self::Ellipse => "tool-ellipse",
            Self::Arrow => "tool-arrow",
            Self::Line => "tool-line",
            Self::Brush => "tool-brush",
            Self::Text => "tool-text",
            Self::Number => "tool-number",
            Self::Mosaic => "tool-mosaic",
            Self::Blur => "tool-blur",
        }
    }

    pub const fn element_id(self) -> &'static str {
        self.label_key()
    }

    pub const fn is_configurable(self) -> bool {
        matches!(
            self,
            Self::Rect
                | Self::Ellipse
                | Self::Arrow
                | Self::Line
                | Self::Brush
                | Self::Text
                | Self::Number
                | Self::Mosaic
                | Self::Blur
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolSettings {
    pub font_family: String,
    pub color: RgbaColor,
    pub stroke_width: f32,
    pub marker_diameter: f32,
    pub font_size: f32,
    pub mosaic_block_size: u32,
    pub blur_radius: u32,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            color: RgbaColor::RED,
            font_family: "sans-serif".into(),
            stroke_width: 3.0,
            marker_diameter: 32.0,
            font_size: 24.0,
            mosaic_block_size: 14,
            blur_radius: 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyValue<T> {
    Uniform(T),
    Mixed,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionStyleSummary {
    pub font_family: PropertyValue<String>,
    pub count: usize,
    pub color: PropertyValue<RgbaColor>,
    pub color_applicable: usize,
    pub stroke_width: PropertyValue<f32>,
    pub marker_diameter: PropertyValue<f32>,
    pub stroke_applicable: usize,
    pub font_size: PropertyValue<f32>,
    pub font_applicable: usize,
    pub mosaic_block_size: PropertyValue<u32>,
    pub mosaic_applicable: usize,
    pub blur_radius: PropertyValue<u32>,
    pub blur_applicable: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextEditTarget {
    New {
        position: Point2,
    },
    Existing {
        id: AnnotationId,
        original: AnnotationItem,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextDraft {
    pub family: String,
    pub target: TextEditTarget,
    pub color: RgbaColor,
    pub font_size: f32,
    pub preview_content: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl PointerModifiers {
    const fn to_resize(self) -> ResizeModifiers {
        ResizeModifiers {
            shift: self.shift,
            ctrl: self.ctrl,
            alt: self.alt,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum ActiveInteraction {
    #[default]
    Idle,
    PendingClickOrMove {
        hit_id: AnnotationId,
        start_vp: Point2,
        start_img: Point2,
        modifiers: PointerModifiers,
        selection_snapshot: SelectionSet,
        originals: Arc<[AnnotationItem]>,
        first_changed_idx: usize,
    },
    Moving {
        start_img: Point2,
        current_img: Point2,
        shift: bool,
        originals: Arc<[AnnotationItem]>,
        first_changed_idx: usize,
    },
    DuplicatingMove {
        start_img: Point2,
        current_img: Point2,
        shift: bool,
        originals: Arc<[AnnotationItem]>,
        clone_ids: Arc<[AnnotationId]>,
        first_changed_idx: usize,
    },
    Resizing {
        target_id: AnnotationId,
        handle: HandleKind,
        original: AnnotationItem,
        current_img: Point2,
        modifiers: PointerModifiers,
        first_changed_idx: usize,
    },
    Marquee {
        start_img: Point2,
        current_img: Point2,
        shift: bool,
        initial_selection: SelectionSet,
    },
    Creating {
        tool: ToolMode,
        start_img: Point2,
        current_img: Point2,
        shift: bool,
    },
    DrawingBrush {
        points: Vec<[f32; 2]>,
        last_vp: Point2,
        current_img: Point2,
    },
    EditingText(TextDraft),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerDownInput {
    pub vp_pt: Point2,
    pub img_pt: Point2,
    pub scale: f32,
    pub modifiers: PointerModifiers,
    pub click_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointerDownOutcome {
    None,
    SelectionChanged,
    NeedsTransientPreview,
    OpenTextEditor { initial_text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerMoveOutcome {
    None,
    ChromeOnly,
    NeedsTransientPreview { first_changed_idx: usize },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FinishOutcome {
    None,
    SelectionChanged,
    CropStaged(PixelRect),
    Committed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelOutcome {
    None,
    ClearedOverlay,
    NeedsRestoreComplete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleApplyOutcome {
    None,
    UpdatedTextDraft,
    Committed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyleTarget {
    Tool,
    Objects,
    TextDraft,
}

#[derive(Default)]
pub struct ToolFsm {
    pub tool: ToolMode,
    pub settings: ToolSettings,
    pub selection: SelectionSet,
    pub staged_crop: Option<PixelRect>,
    pub interaction: ActiveInteraction,
}

impl ToolFsm {
    pub const fn is_pointer_gesture(&self) -> bool {
        matches!(
            self.interaction,
            ActiveInteraction::PendingClickOrMove { .. }
                | ActiveInteraction::Moving { .. }
                | ActiveInteraction::DuplicatingMove { .. }
                | ActiveInteraction::Resizing { .. }
                | ActiveInteraction::Marquee { .. }
                | ActiveInteraction::Creating { .. }
                | ActiveInteraction::DrawingBrush { .. }
        )
    }

    pub const fn is_editing_text(&self) -> bool {
        matches!(self.interaction, ActiveInteraction::EditingText(_))
    }

    pub fn style_target(&self) -> Option<StyleTarget> {
        if self.is_editing_text() {
            Some(StyleTarget::TextDraft)
        } else if self.tool.is_configurable() {
            Some(StyleTarget::Tool)
        } else if self.tool == ToolMode::Pointer && !self.selection.is_empty() {
            Some(StyleTarget::Objects)
        } else {
            None
        }
    }

    pub fn set_tool(&mut self, tool: ToolMode, history: &mut EditHistory) -> CancelOutcome {
        let cancel_outcome = self.cancel_pointer_gesture(history);
        if matches!(tool, ToolMode::Pan | ToolMode::Crop)
            || (tool.is_configurable() && tool != self.tool)
        {
            self.selection.clear();
        }
        if tool != ToolMode::Crop {
            self.staged_crop = None;
        }
        self.tool = tool;
        history.break_coalesce();
        cancel_outcome
    }

    pub fn on_pointer_down(
        &mut self,
        doc: &DocumentState,
        history: &mut EditHistory,
        input: PointerDownInput,
    ) -> PointerDownOutcome {
        if self.tool == ToolMode::Pan {
            return PointerDownOutcome::None;
        }

        if self.tool == ToolMode::Crop {
            history.break_coalesce();
            self.staged_crop = None;
            self.interaction = ActiveInteraction::Creating {
                tool: ToolMode::Crop,
                start_img: input.img_pt,
                current_img: input.img_pt,
                shift: input.modifiers.shift,
            };
            return PointerDownOutcome::None;
        }

        let suppress_handles = self.staged_crop.is_some();
        let effective_selection = if suppress_handles {
            SelectionSet::empty()
        } else {
            self.selection.clone()
        };

        if doc.canvas_bounds().contains(input.img_pt)
            && let Some(hit) = hit_test_canvas(
                &doc.annotations,
                &effective_selection,
                input.img_pt,
                self.tool,
                input.scale,
            )
        {
            match hit {
                HitTarget::Handle { id, handle } => {
                    let Some(idx) = doc.annotations.iter().position(|item| item.id == id) else {
                        return PointerDownOutcome::None;
                    };
                    let original = doc.annotations[idx].clone();
                    history.break_coalesce();
                    self.interaction = ActiveInteraction::Resizing {
                        target_id: id,
                        handle,
                        original,
                        current_img: input.img_pt,
                        modifiers: input.modifiers,
                        first_changed_idx: idx,
                    };
                    return PointerDownOutcome::NeedsTransientPreview;
                }
                HitTarget::Body { id } => {
                    let Some(hit_idx) = doc.annotations.iter().position(|item| item.id == id)
                    else {
                        return PointerDownOutcome::None;
                    };
                    if input.click_count >= 2
                        && !input.modifiers.alt
                        && !input.modifiers.shift
                        && let Some(initial_text) = self.begin_edit_existing_text(id, doc, history)
                    {
                        return PointerDownOutcome::OpenTextEditor { initial_text };
                    }

                    let (selection_snapshot, selection_changed) =
                        if input.modifiers.shift || self.selection.contains(id) {
                            (self.selection.clone(), false)
                        } else if !input.modifiers.alt {
                            let changed = self.selection.set_single(id);
                            (self.selection.clone(), changed)
                        } else {
                            (SelectionSet::single(id), false)
                        };

                    let drag_ids = if selection_snapshot.contains(id) {
                        selection_snapshot.clone()
                    } else {
                        let mut next = selection_snapshot.clone();
                        next.add(id);
                        next
                    };
                    let originals: Arc<[AnnotationItem]> =
                        Arc::from(drag_ids.selected_items_ordered(&doc.annotations));
                    let first_changed_idx = drag_ids
                        .indices_ascending(&doc.annotations)
                        .into_iter()
                        .next()
                        .unwrap_or(hit_idx);

                    history.break_coalesce();
                    self.interaction = ActiveInteraction::PendingClickOrMove {
                        hit_id: id,
                        start_vp: input.vp_pt,
                        start_img: input.img_pt,
                        modifiers: input.modifiers,
                        selection_snapshot,
                        originals,
                        first_changed_idx,
                    };
                    return if selection_changed {
                        PointerDownOutcome::SelectionChanged
                    } else {
                        PointerDownOutcome::None
                    };
                }
            }
        }

        // Blank canvas hit:
        history.break_coalesce();
        match self.tool {
            ToolMode::Pointer => {
                let initial_selection = if input.modifiers.shift {
                    self.selection.clone()
                } else {
                    self.selection.clear();
                    SelectionSet::empty()
                };
                self.interaction = ActiveInteraction::Marquee {
                    start_img: input.img_pt,
                    current_img: input.img_pt,
                    shift: input.modifiers.shift,
                    initial_selection,
                };
                PointerDownOutcome::SelectionChanged
            }
            ToolMode::Text => {
                let pos = input.img_pt;
                self.interaction = ActiveInteraction::EditingText(TextDraft {
                    target: TextEditTarget::New { position: pos },
                    family: self.settings.font_family.clone(),
                    color: self.settings.color,
                    font_size: self.settings.font_size,
                    preview_content: String::new(),
                });
                PointerDownOutcome::OpenTextEditor {
                    initial_text: String::new(),
                }
            }
            ToolMode::Brush => {
                let mut points = Vec::with_capacity(32);
                if input.img_pt.x.is_finite() && input.img_pt.y.is_finite() {
                    points.push([input.img_pt.x, input.img_pt.y]);
                }
                self.interaction = ActiveInteraction::DrawingBrush {
                    points,
                    last_vp: input.vp_pt,
                    current_img: input.img_pt,
                };
                PointerDownOutcome::NeedsTransientPreview
            }
            tool => {
                self.interaction = ActiveInteraction::Creating {
                    tool,
                    start_img: input.img_pt,
                    current_img: input.img_pt,
                    shift: input.modifiers.shift,
                };
                PointerDownOutcome::None
            }
        }
    }

    pub fn on_pointer_move(
        &mut self,
        doc: &DocumentState,
        vp_pt: Point2,
        img_pt: Point2,
        modifiers: PointerModifiers,
    ) -> PointerMoveOutcome {
        match &mut self.interaction {
            ActiveInteraction::PendingClickOrMove {
                start_vp,
                start_img,
                modifiers: down_mods,
                originals,
                first_changed_idx,
                ..
            } => {
                let dist = (vp_pt.x - start_vp.x).hypot(vp_pt.y - start_vp.y);
                if dist < DRAG_THRESHOLD_SCREEN_PX {
                    return PointerMoveOutcome::None;
                }
                let start = *start_img;
                let shift = modifiers.shift;
                let originals_arc = Arc::clone(originals);
                if down_mods.alt || modifiers.alt {
                    let clone_base_id = doc.next_annotation_id.saturating_add(1_000_000);
                    let clone_ids: Arc<[AnnotationId]> = Arc::from(
                        (0..originals_arc.len())
                            .map(|idx| AnnotationId(clone_base_id.saturating_add(idx as u64)))
                            .collect::<Vec<_>>(),
                    );
                    let idx = doc.annotations.len();
                    self.interaction = ActiveInteraction::DuplicatingMove {
                        start_img: start,
                        current_img: img_pt,
                        shift,
                        originals: originals_arc,
                        clone_ids,
                        first_changed_idx: idx,
                    };
                    PointerMoveOutcome::NeedsTransientPreview {
                        first_changed_idx: idx,
                    }
                } else {
                    let idx = *first_changed_idx;
                    self.interaction = ActiveInteraction::Moving {
                        start_img: start,
                        current_img: img_pt,
                        shift,
                        originals: originals_arc,
                        first_changed_idx: idx,
                    };
                    PointerMoveOutcome::NeedsTransientPreview {
                        first_changed_idx: idx,
                    }
                }
            }
            ActiveInteraction::Moving {
                current_img,
                shift,
                first_changed_idx,
                ..
            }
            | ActiveInteraction::DuplicatingMove {
                current_img,
                shift,
                first_changed_idx,
                ..
            } => {
                *current_img = img_pt;
                *shift = modifiers.shift;
                PointerMoveOutcome::NeedsTransientPreview {
                    first_changed_idx: *first_changed_idx,
                }
            }
            ActiveInteraction::Resizing {
                current_img,
                modifiers: active_mods,
                first_changed_idx,
                ..
            } => {
                *current_img = img_pt;
                *active_mods = modifiers;
                PointerMoveOutcome::NeedsTransientPreview {
                    first_changed_idx: *first_changed_idx,
                }
            }
            ActiveInteraction::Marquee { current_img, .. } => {
                *current_img = img_pt;
                PointerMoveOutcome::ChromeOnly
            }
            ActiveInteraction::Creating {
                tool,
                current_img,
                shift,
                ..
            } => {
                *current_img = img_pt;
                *shift = modifiers.shift;
                if *tool == ToolMode::Crop {
                    PointerMoveOutcome::ChromeOnly
                } else {
                    PointerMoveOutcome::NeedsTransientPreview {
                        first_changed_idx: doc.annotations.len(),
                    }
                }
            }
            ActiveInteraction::DrawingBrush {
                points,
                last_vp,
                current_img,
            } => {
                *current_img = img_pt;
                if img_pt.x.is_finite()
                    && img_pt.y.is_finite()
                    && (points.is_empty()
                        || (vp_pt.x - last_vp.x).hypot(vp_pt.y - last_vp.y)
                            >= MIN_BRUSH_STEP_SCREEN_PX)
                {
                    points.push([img_pt.x, img_pt.y]);
                    *last_vp = vp_pt;
                }
                PointerMoveOutcome::NeedsTransientPreview {
                    first_changed_idx: doc.annotations.len(),
                }
            }
            ActiveInteraction::Idle | ActiveInteraction::EditingText(_) => PointerMoveOutcome::None,
        }
    }

    pub fn on_pointer_up(
        &mut self,
        doc: &mut DocumentState,
        history: &mut EditHistory,
        scale: f32,
    ) -> FinishOutcome {
        let current = std::mem::take(&mut self.interaction);
        match current {
            ActiveInteraction::Idle => FinishOutcome::None,
            ActiveInteraction::EditingText(draft) => {
                self.interaction = ActiveInteraction::EditingText(draft);
                FinishOutcome::None
            }
            ActiveInteraction::PendingClickOrMove {
                hit_id,
                start_img,
                modifiers,
                ..
            } => {
                history.break_coalesce();
                if modifiers.alt {
                    let stack = hit_test_cycle_stack(&doc.annotations, start_img, scale);
                    if !stack.is_empty() {
                        let next_id = self
                            .selection
                            .primary()
                            .and_then(|cur| stack.iter().position(|&id| id == cur))
                            .map_or(stack[0], |pos| stack[(pos + 1) % stack.len()]);
                        self.selection.set_single(next_id);
                    }
                } else if modifiers.shift {
                    self.selection.toggle(hit_id);
                } else {
                    self.selection.set_single(hit_id);
                }
                FinishOutcome::SelectionChanged
            }
            ActiveInteraction::Moving {
                start_img,
                current_img,
                shift,
                originals,
                ..
            } => {
                let (moved, dx, dy) = translate_group(
                    &originals,
                    current_img.x - start_img.x,
                    current_img.y - start_img.y,
                    shift,
                );
                if dx.abs() < 1.0 && dy.abs() < 1.0 {
                    return FinishOutcome::None;
                }
                history.apply(
                    EditCommand::UpdateAnnotations {
                        before: originals,
                        after: Arc::from(moved),
                    },
                    doc,
                );
                FinishOutcome::Committed
            }
            ActiveInteraction::DuplicatingMove {
                start_img,
                current_img,
                shift,
                originals,
                ..
            } => {
                let (moved, dx, dy) = translate_group(
                    &originals,
                    current_img.x - start_img.x,
                    current_img.y - start_img.y,
                    shift,
                );
                if dx.abs() < 1.0 && dy.abs() < 1.0 {
                    return FinishOutcome::None;
                }
                let mut clones = Vec::with_capacity(moved.len());
                let mut new_ids = Vec::with_capacity(moved.len());
                let mut number = next_number(doc);
                for mut item in moved {
                    renumber_copy(&mut item, &mut number);
                    let id = doc.allocate_annotation_id();
                    item.id = id;
                    new_ids.push(id);
                    clones.push(item);
                }
                history.apply(EditCommand::AddAnnotations(Arc::from(clones)), doc);
                self.selection = SelectionSet::from_ids(new_ids);
                FinishOutcome::Committed
            }
            ActiveInteraction::Resizing {
                original,
                handle,
                current_img,
                modifiers,
                ..
            } => {
                let updated =
                    resize_item_with_handle(&original, handle, current_img, modifiers.to_resize());
                if updated == original {
                    return FinishOutcome::None;
                }
                history.apply(
                    EditCommand::UpdateAnnotations {
                        before: Arc::from([original]),
                        after: Arc::from([updated]),
                    },
                    doc,
                );
                FinishOutcome::Committed
            }
            ActiveInteraction::Marquee {
                start_img,
                current_img,
                shift,
                initial_selection,
            } => {
                history.break_coalesce();
                let rect = ViewportTransform::normalize_drag_rect(start_img, current_img);
                let hits = rect
                    .intersect(doc.canvas_bounds())
                    .map(|area| marquee_hits(&doc.annotations, area))
                    .unwrap_or_default();
                if shift {
                    self.selection = initial_selection;
                    for id in hits {
                        self.selection.add(id);
                    }
                } else {
                    self.selection = SelectionSet::from_ids(hits);
                }
                FinishOutcome::SelectionChanged
            }
            ActiveInteraction::Creating {
                tool,
                start_img,
                current_img,
                shift,
            } => {
                let (out_w, out_h) = doc.output_dimensions();
                if tool == ToolMode::Crop {
                    let dx = (current_img.x - start_img.x).abs();
                    let dy = (current_img.y - start_img.y).abs();
                    if dx < MIN_DRAW_DRAG_PX && dy < MIN_DRAW_DRAG_PX {
                        return FinishOutcome::None;
                    }
                    let norm = ViewportTransform::normalize_drag_rect(start_img, current_img);
                    let Some(rect) = ViewportTransform::clamp_image_rect(
                        norm,
                        Size2::new(out_w as f32, out_h as f32),
                    ) else {
                        return FinishOutcome::None;
                    };
                    if rect.width < 2 || rect.height < 2 {
                        return FinishOutcome::None;
                    }
                    self.staged_crop = Some(rect);
                    return FinishOutcome::CropStaged(rect);
                }

                let preview_id = AnnotationId(doc.next_annotation_id);
                let Some(mut item) = build_annotation_from_drag(
                    tool,
                    start_img,
                    current_img,
                    shift,
                    &self.settings,
                    preview_id,
                    doc,
                ) else {
                    return FinishOutcome::None;
                };
                if item.dirty_bounds().intersect(doc.canvas_bounds()).is_none() {
                    return FinishOutcome::None;
                }
                let id = doc.allocate_annotation_id();
                item.id = id;
                history.apply(EditCommand::AddAnnotations(Arc::from([item])), doc);
                // Auto-select newly created item while preserving the active drawing tool.
                self.selection.set_single(id);
                FinishOutcome::Committed
            }
            ActiveInteraction::DrawingBrush {
                points,
                current_img,
                ..
            } => {
                let finalized = finish_brush_points(&points, current_img);
                if finalized.is_empty() {
                    return FinishOutcome::None;
                }
                let preview_id = AnnotationId(doc.next_annotation_id);
                let mut item = AnnotationItem {
                    id: preview_id,
                    kind: AnnotationKind::Brush {
                        points: finalized,
                        color: self.settings.color,
                        stroke_width: self.settings.stroke_width,
                    },
                };
                if item.dirty_bounds().intersect(doc.canvas_bounds()).is_none() {
                    return FinishOutcome::None;
                }
                let id = doc.allocate_annotation_id();
                item.id = id;
                history.apply(EditCommand::AddAnnotations(Arc::from([item])), doc);
                self.selection.set_single(id);
                FinishOutcome::Committed
            }
        }
    }

    pub fn begin_edit_existing_text(
        &mut self,
        id: AnnotationId,
        doc: &DocumentState,
        history: &mut EditHistory,
    ) -> Option<String> {
        let item = doc.annotations.iter().find(|entry| entry.id == id)?.clone();
        let AnnotationKind::Text {
            content,
            color,
            font_size,
            family,
            ..
        } = &item.kind
        else {
            return None;
        };
        let initial = content.clone();
        history.break_coalesce();
        self.selection.set_single(id);
        self.interaction = ActiveInteraction::EditingText(TextDraft {
            target: TextEditTarget::Existing {
                id,
                original: item.clone(),
            },
            color: *color,
            family: family.clone(),
            font_size: *font_size,
            preview_content: initial.clone(),
        });
        Some(initial)
    }

    pub fn update_text_draft_content(&mut self, value: String) -> bool {
        if let ActiveInteraction::EditingText(draft) = &mut self.interaction
            && draft.preview_content != value
        {
            draft.preview_content = value;
            return true;
        }
        false
    }

    pub fn commit_text_draft(
        &mut self,
        text: &str,
        doc: &mut DocumentState,
        history: &mut EditHistory,
    ) -> FinishOutcome {
        let ActiveInteraction::EditingText(draft) = std::mem::take(&mut self.interaction) else {
            return FinishOutcome::None;
        };
        let trimmed = text.trim();
        history.break_coalesce();

        match draft.target {
            TextEditTarget::New { position } => {
                if trimmed.is_empty() || !position.x.is_finite() || !position.y.is_finite() {
                    return FinishOutcome::None;
                }
                let id = AnnotationId(doc.next_annotation_id);
                let item = AnnotationItem {
                    id,
                    kind: AnnotationKind::Text {
                        position: [position.x, position.y],
                        content: trimmed.to_owned(),
                        color: draft.color,
                        family: draft.family.clone(),
                        font_size: draft.font_size,
                    },
                };
                if item.dirty_bounds().intersect(doc.canvas_bounds()).is_none() {
                    return FinishOutcome::None;
                }
                doc.allocate_annotation_id();
                history.apply(EditCommand::AddAnnotations(Arc::from([item])), doc);
                self.selection.set_single(id);
                FinishOutcome::Committed
            }
            TextEditTarget::Existing { id, original } => {
                if trimmed.is_empty() {
                    let Some(idx) = doc.annotations.iter().position(|entry| entry.id == id) else {
                        return FinishOutcome::None;
                    };
                    history.apply(
                        EditCommand::RemoveAnnotations {
                            removed: Arc::from([(idx, original)]),
                        },
                        doc,
                    );
                    self.selection.reconcile(&doc.annotations);
                    return FinishOutcome::Committed;
                }
                let position = match &original.kind {
                    AnnotationKind::Text { position, .. } => *position,
                    _ => return FinishOutcome::None,
                };
                let updated = AnnotationItem {
                    id,
                    kind: AnnotationKind::Text {
                        position,
                        content: trimmed.to_owned(),
                        color: draft.color,
                        family: draft.family.clone(),
                        font_size: draft.font_size,
                    },
                };
                if updated == original {
                    return FinishOutcome::None;
                }
                history.apply(
                    EditCommand::UpdateAnnotations {
                        before: Arc::from([original]),
                        after: Arc::from([updated]),
                    },
                    doc,
                );
                self.selection.set_single(id);
                FinishOutcome::Committed
            }
        }
    }

    pub fn cancel_pointer_gesture(&mut self, history: &mut EditHistory) -> CancelOutcome {
        if !self.is_pointer_gesture() {
            return CancelOutcome::None;
        }
        let prev = std::mem::take(&mut self.interaction);
        history.break_coalesce();
        match prev {
            ActiveInteraction::Marquee { .. }
            | ActiveInteraction::Creating {
                tool: ToolMode::Crop,
                ..
            } => CancelOutcome::ClearedOverlay,
            ActiveInteraction::PendingClickOrMove { .. } => CancelOutcome::None,
            _ => CancelOutcome::NeedsRestoreComplete,
        }
    }

    pub fn cancel_active(&mut self, history: &mut EditHistory) -> CancelOutcome {
        history.break_coalesce();
        if !matches!(self.interaction, ActiveInteraction::Idle) {
            let prev = std::mem::take(&mut self.interaction);
            return match prev {
                ActiveInteraction::Marquee { .. }
                | ActiveInteraction::Creating {
                    tool: ToolMode::Crop,
                    ..
                } => CancelOutcome::ClearedOverlay,
                ActiveInteraction::PendingClickOrMove { .. } => CancelOutcome::None,
                ActiveInteraction::EditingText(draft) => match draft.target {
                    TextEditTarget::New { .. } if draft.preview_content.trim().is_empty() => {
                        CancelOutcome::ClearedOverlay
                    }
                    _ => CancelOutcome::NeedsRestoreComplete,
                },
                _ => CancelOutcome::NeedsRestoreComplete,
            };
        }
        if self.staged_crop.take().is_some() {
            return CancelOutcome::ClearedOverlay;
        }
        CancelOutcome::None
    }

    pub fn delete_selected_or_crop(
        &mut self,
        doc: &mut DocumentState,
        history: &mut EditHistory,
    ) -> FinishOutcome {
        if !self.selection.is_empty() {
            let removed: Vec<(usize, AnnotationItem)> = doc
                .annotations
                .iter()
                .enumerate()
                .filter(|(_, item)| self.selection.contains(item.id))
                .map(|(idx, item)| (idx, item.clone()))
                .collect();
            self.selection.clear();
            history.break_coalesce();
            if removed.is_empty() {
                return FinishOutcome::SelectionChanged;
            }
            history.apply(
                EditCommand::RemoveAnnotations {
                    removed: Arc::from(removed),
                },
                doc,
            );
            return FinishOutcome::Committed;
        }
        if self.staged_crop.take().is_some() {
            history.break_coalesce();
            return FinishOutcome::SelectionChanged;
        }
        FinishOutcome::None
    }

    pub fn duplicate_selected(
        &mut self,
        doc: &mut DocumentState,
        history: &mut EditHistory,
    ) -> FinishOutcome {
        if self.selection.is_empty() {
            return FinishOutcome::None;
        }
        let originals = self.selection.selected_items_ordered(&doc.annotations);
        if originals.is_empty() {
            return FinishOutcome::None;
        }
        let (out_w, out_h) = doc.output_dimensions();
        let (dx, dy) = compute_duplicate_offset(&originals, out_w, out_h);
        let mut clones = Vec::with_capacity(originals.len());
        let mut new_ids = Vec::with_capacity(originals.len());
        let mut number = next_number(doc);
        for item in &originals {
            let mut moved = item.translated(dx, dy);
            renumber_copy(&mut moved, &mut number);
            let id = doc.allocate_annotation_id();
            moved.id = id;
            new_ids.push(id);
            clones.push(moved);
        }
        history.apply(EditCommand::AddAnnotations(Arc::from(clones)), doc);
        self.selection = SelectionSet::from_ids(new_ids);
        FinishOutcome::Committed
    }

    pub fn select_all(&mut self, doc: &DocumentState, history: &mut EditHistory) -> bool {
        if doc.annotations.is_empty() {
            return false;
        }
        let changed_tool = self.tool != ToolMode::Pointer;
        self.set_tool(ToolMode::Pointer, history);
        self.selection.select_all(&doc.annotations) || changed_tool
    }

    pub fn nudge_selection(
        &mut self,
        dx: f32,
        dy: f32,
        doc: &mut DocumentState,
        history: &mut EditHistory,
    ) -> bool {
        if self.selection.is_empty() || self.is_pointer_gesture() || self.is_editing_text() {
            return false;
        }
        let originals: Arc<[AnnotationItem]> =
            Arc::from(self.selection.selected_items_ordered(&doc.annotations));
        if originals.is_empty() {
            return false;
        }
        let (moved, cdx, cdy) = translate_group(&originals, dx, dy, false);
        if cdx.abs() < 1e-4 && cdy.abs() < 1e-4 {
            return false;
        }
        history.apply_coalesced(
            CoalesceKey::KeyboardNudge {
                selection_hash: self.selection.selection_hash(),
            },
            EditCommand::UpdateAnnotations {
                before: originals,
                after: Arc::from(moved),
            },
            doc,
        );
        true
    }

    pub fn reorder_selection(
        &mut self,
        op: ReorderOp,
        doc: &mut DocumentState,
        history: &mut EditHistory,
    ) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let Some(after_order) = reorder_annotations(&doc.annotations, &self.selection, op) else {
            return false;
        };
        let before_order: Arc<[AnnotationId]> = Arc::from(
            doc.annotations
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
        );
        history.apply(
            EditCommand::ReorderAnnotations {
                before_order,
                after_order: Arc::from(after_order),
            },
            doc,
        );
        true
    }

    pub fn apply_spatial_transform(
        &mut self,
        op: SpatialTransformOp,
        doc: &mut DocumentState,
        history: &mut EditHistory,
    ) {
        let cmd = EditCommand::build_spatial_transform(op, doc);
        history.apply(cmd, doc);
        self.selection.reconcile(&doc.annotations);
    }

    pub fn apply_staged_crop(
        &mut self,
        doc: &mut DocumentState,
        history: &mut EditHistory,
    ) -> bool {
        let Some(output_crop) = self.staged_crop.take() else {
            return false;
        };
        let (base_w, base_h) = doc.base_dimensions();
        let Some(base_crop) =
            doc.transform
                .map_output_rect_to_base_crop(output_crop, base_w, base_h)
        else {
            return false;
        };
        let cmd = EditCommand::build_crop(base_crop, output_crop, doc);
        history.apply(cmd, doc);
        self.selection.reconcile(&doc.annotations);
        true
    }

    pub fn selection_style_summary(
        &self,
        annotations: &[AnnotationItem],
    ) -> Option<SelectionStyleSummary> {
        let selected = self.selection.selected_items_ordered(annotations);
        if selected.is_empty() {
            return None;
        }
        let (color, color_applicable) =
            summarize_property(&selected, AnnotationItem::color, |a, b| a == b);
        let (stroke_width, stroke_applicable) =
            summarize_property(&selected, AnnotationItem::stroke_width, |a, b| {
                (a - b).abs() < 1e-3
            });
        let (marker_diameter, _) =
            summarize_property(&selected, AnnotationItem::marker_diameter, |a, b| {
                (a - b).abs() < 1e-3
            });
        let (font_size, font_applicable) =
            summarize_property(&selected, AnnotationItem::font_size, |a, b| {
                (a - b).abs() < 1e-3
            });
        let (mosaic_block_size, mosaic_applicable) =
            summarize_property(&selected, AnnotationItem::mosaic_block_size, |a, b| a == b);
        let (blur_radius, blur_applicable) =
            summarize_property(&selected, AnnotationItem::blur_radius, |a, b| a == b);

        let (font_family, _) =
            summarize_property(&selected, AnnotationItem::font_family, |a, b| a == b);
        Some(SelectionStyleSummary {
            marker_diameter,
            font_family,
            count: selected.len(),
            color,
            color_applicable,
            stroke_width,
            stroke_applicable,
            font_size,
            font_applicable,
            mosaic_block_size,
            mosaic_applicable,
            blur_radius,
            blur_applicable,
        })
    }

    pub fn apply_color(
        &mut self,
        color: RgbaColor,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
    ) -> StyleApplyOutcome {
        if let ActiveInteraction::EditingText(draft) = &mut self.interaction {
            draft.color = color;
            return StyleApplyOutcome::UpdatedTextDraft;
        }
        self.apply_style_change(
            StyleField::Color,
            doc,
            history,
            |settings| settings.color = color,
            |item| item.with_color(color),
        )
    }

    pub fn apply_stroke_width(
        &mut self,
        width: f32,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
    ) -> StyleApplyOutcome {
        self.apply_style_change(
            StyleField::StrokeWidth,
            doc,
            history,
            |settings| settings.stroke_width = width,
            |item| item.with_stroke_width(width),
        )
    }

    pub fn apply_marker_diameter(
        &mut self,
        diameter: f32,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
    ) -> StyleApplyOutcome {
        self.apply_style_change(
            StyleField::MarkerDiameter,
            doc,
            history,
            |settings| settings.marker_diameter = diameter,
            |item| item.with_marker_diameter(diameter),
        )
    }

    pub fn apply_font_family(
        &mut self,
        family: String,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
    ) -> StyleApplyOutcome {
        if let ActiveInteraction::EditingText(draft) = &mut self.interaction {
            draft.family = family;
            return StyleApplyOutcome::UpdatedTextDraft;
        }
        self.apply_style_change(
            StyleField::FontFamily,
            doc,
            history,
            |settings| settings.font_family.clone_from(&family),
            |item| item.with_font_family(&family),
        )
    }

    pub fn apply_font_size(
        &mut self,
        size: f32,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
    ) -> StyleApplyOutcome {
        if let ActiveInteraction::EditingText(draft) = &mut self.interaction {
            draft.font_size = size;
            return StyleApplyOutcome::UpdatedTextDraft;
        }
        self.apply_style_change(
            StyleField::FontSize,
            doc,
            history,
            |settings| settings.font_size = size,
            |item| item.with_font_size(size),
        )
    }

    pub fn apply_mosaic_block_size(
        &mut self,
        block_size: u32,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
    ) -> StyleApplyOutcome {
        self.apply_style_change(
            StyleField::MosaicBlockSize,
            doc,
            history,
            |settings| settings.mosaic_block_size = block_size,
            |item| item.with_mosaic_block_size(block_size),
        )
    }

    pub fn apply_blur_radius(
        &mut self,
        radius: u32,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
    ) -> StyleApplyOutcome {
        self.apply_style_change(
            StyleField::BlurRadius,
            doc,
            history,
            |settings| settings.blur_radius = radius,
            |item| item.with_blur_radius(radius),
        )
    }

    fn apply_style_change(
        &mut self,
        field: StyleField,
        doc: Option<&mut DocumentState>,
        history: &mut EditHistory,
        update_tool: impl FnOnce(&mut ToolSettings),
        transform: impl Fn(&AnnotationItem) -> Option<AnnotationItem>,
    ) -> StyleApplyOutcome {
        match self.style_target() {
            Some(StyleTarget::Tool) => {
                update_tool(&mut self.settings);
                return StyleApplyOutcome::None;
            }
            Some(StyleTarget::Objects) => {}
            _ => return StyleApplyOutcome::None,
        }
        let Some(doc) = doc else {
            return StyleApplyOutcome::None;
        };
        let mut before = Vec::new();
        let mut after = Vec::new();
        for item in self.selection.selected_items_ordered(&doc.annotations) {
            if let Some(updated) = transform(&item)
                && updated != item
            {
                before.push(item);
                after.push(updated);
            }
        }
        if after.is_empty() {
            return StyleApplyOutcome::None;
        }
        history.apply_coalesced(
            CoalesceKey::StyleChange {
                selection_hash: self.selection.selection_hash(),
                field,
            },
            EditCommand::UpdateAnnotations {
                before: Arc::from(before),
                after: Arc::from(after),
            },
            doc,
        );
        StyleApplyOutcome::Committed
    }

    /// Returns `(first_changed_idx, snapshot)` whenever the active interaction modifies canvas
    /// artwork that should render through the incremental `GestureSession` pipeline.
    pub fn transient_annotations(
        &self,
        doc: &DocumentState,
    ) -> Option<(usize, Arc<[AnnotationItem]>)> {
        match &self.interaction {
            ActiveInteraction::Moving {
                start_img,
                current_img,
                shift,
                originals,
                first_changed_idx,
            } => {
                let (moved, _, _) = translate_group(
                    originals,
                    current_img.x - start_img.x,
                    current_img.y - start_img.y,
                    *shift,
                );
                let mut next = doc.annotations.clone();
                for replacement in moved {
                    if let Some(slot) = next.iter_mut().find(|item| item.id == replacement.id) {
                        *slot = replacement;
                    }
                }
                Some((*first_changed_idx, Arc::from(next)))
            }
            ActiveInteraction::DuplicatingMove {
                start_img,
                current_img,
                shift,
                originals,
                clone_ids,
                first_changed_idx,
            } => {
                let (moved, _, _) = translate_group(
                    originals,
                    current_img.x - start_img.x,
                    current_img.y - start_img.y,
                    *shift,
                );
                let mut next = doc.annotations.clone();
                let mut number = next_number(doc);
                for (mut item, &clone_id) in moved.into_iter().zip(clone_ids.iter()) {
                    renumber_copy(&mut item, &mut number);
                    item.id = clone_id;
                    next.push(item);
                }
                Some((*first_changed_idx, Arc::from(next)))
            }
            ActiveInteraction::Resizing {
                target_id,
                handle,
                original,
                current_img,
                modifiers,
                first_changed_idx,
            } => {
                let updated =
                    resize_item_with_handle(original, *handle, *current_img, modifiers.to_resize());
                let mut next = doc.annotations.clone();
                if let Some(slot) = next.iter_mut().find(|item| item.id == *target_id) {
                    *slot = updated;
                }
                Some((*first_changed_idx, Arc::from(next)))
            }
            ActiveInteraction::Creating {
                tool,
                start_img,
                current_img,
                shift,
            } => {
                if *tool == ToolMode::Crop {
                    return None;
                }
                let idx = doc.annotations.len();
                let mut next = doc.annotations.clone();
                let preview_id = AnnotationId(doc.next_annotation_id.saturating_add(1_000_000));
                if let Some(item) = build_annotation_from_drag(
                    *tool,
                    *start_img,
                    *current_img,
                    *shift,
                    &self.settings,
                    preview_id,
                    doc,
                ) {
                    next.push(item);
                }
                Some((idx, Arc::from(next)))
            }
            ActiveInteraction::DrawingBrush {
                points,
                current_img,
                ..
            } => {
                let idx = doc.annotations.len();
                let mut next = doc.annotations.clone();
                let finalized = finish_brush_points(points, *current_img);
                if !finalized.is_empty() {
                    next.push(AnnotationItem {
                        id: AnnotationId(doc.next_annotation_id.saturating_add(1_000_000)),
                        kind: AnnotationKind::Brush {
                            points: finalized,
                            color: self.settings.color,
                            stroke_width: self.settings.stroke_width,
                        },
                    });
                }
                Some((idx, Arc::from(next)))
            }
            ActiveInteraction::EditingText(draft) => match &draft.target {
                TextEditTarget::New { position } => {
                    let trimmed = draft.preview_content.trim();
                    let mut next = doc.annotations.clone();
                    if !trimmed.is_empty() {
                        next.push(AnnotationItem {
                            id: AnnotationId(doc.next_annotation_id.saturating_add(1_000_000)),
                            kind: AnnotationKind::Text {
                                position: [position.x, position.y],
                                content: trimmed.to_owned(),
                                color: draft.color,
                                family: draft.family.clone(),
                                font_size: draft.font_size,
                            },
                        });
                    }
                    Some((doc.annotations.len(), Arc::from(next)))
                }
                TextEditTarget::Existing { id, original } => {
                    let idx = doc.annotations.iter().position(|item| item.id == *id)?;
                    let position = match &original.kind {
                        AnnotationKind::Text { position, .. } => *position,
                        _ => return None,
                    };
                    let mut next = doc.annotations.clone();
                    next[idx] = AnnotationItem {
                        id: *id,
                        kind: AnnotationKind::Text {
                            position,
                            content: draft.preview_content.trim().to_owned(),
                            color: draft.color,
                            family: draft.family.clone(),
                            font_size: draft.font_size,
                        },
                    };
                    Some((idx, Arc::from(next)))
                }
            },
            ActiveInteraction::Idle
            | ActiveInteraction::PendingClickOrMove { .. }
            | ActiveInteraction::Marquee { .. } => None,
        }
    }
}

fn summarize_property<T>(
    items: &[AnnotationItem],
    getter: impl Fn(&AnnotationItem) -> Option<T>,
    eq: impl Fn(&T, &T) -> bool,
) -> (PropertyValue<T>, usize) {
    let mut first: Option<T> = None;
    let mut mixed = false;
    let mut applicable = 0usize;
    for item in items {
        let Some(val) = getter(item) else {
            continue;
        };
        applicable += 1;
        match &first {
            None => first = Some(val),
            Some(prev) => {
                if !eq(prev, &val) {
                    mixed = true;
                }
            }
        }
    }
    let state = match (first, mixed) {
        (None, _) => PropertyValue::Unsupported,
        (Some(_), true) => PropertyValue::Mixed,
        (Some(val), false) => PropertyValue::Uniform(val),
    };
    (state, applicable)
}

fn build_annotation_from_drag(
    tool: ToolMode,
    start: Point2,
    current: Point2,
    shift: bool,
    settings: &ToolSettings,
    id: AnnotationId,
    doc: &DocumentState,
) -> Option<AnnotationItem> {
    if tool == ToolMode::Number {
        return Some(AnnotationItem {
            id,
            kind: AnnotationKind::Number {
                center: current,
                number: next_number(doc),
                radius: settings.marker_diameter * 0.5,
                color: settings.color,
            },
        });
    }
    let end = if shift && matches!(tool, ToolMode::Arrow | ToolMode::Line) {
        snap_angle_15(start, current)
    } else if shift && matches!(tool, ToolMode::Rect | ToolMode::Ellipse) {
        let dx = current.x - start.x;
        let dy = current.y - start.y;
        let side = dx.abs().max(dy.abs());
        Point2::new(
            start.x + side * if dx >= 0.0 { 1.0 } else { -1.0 },
            start.y + side * if dy >= 0.0 { 1.0 } else { -1.0 },
        )
    } else {
        current
    };

    let dx = (end.x - start.x).abs();
    let dy = (end.y - start.y).abs();
    if dx < MIN_DRAW_DRAG_PX && dy < MIN_DRAW_DRAG_PX {
        return None;
    }

    let kind = match tool {
        ToolMode::Rect => {
            let norm = ViewportTransform::normalize_drag_rect(start, end);
            if norm.width < 1.0 || norm.height < 1.0 {
                return None;
            }
            AnnotationKind::Rect {
                rect: norm,
                stroke: settings.color,
                stroke_width: settings.stroke_width,
                fill: None,
            }
        }
        ToolMode::Ellipse => {
            let norm = ViewportTransform::normalize_drag_rect(start, end);
            if norm.width < 1.0 || norm.height < 1.0 {
                return None;
            }
            AnnotationKind::Ellipse {
                rect: norm,
                stroke: settings.color,
                stroke_width: settings.stroke_width,
                fill: None,
            }
        }
        ToolMode::Arrow => AnnotationKind::Arrow {
            start: [start.x, start.y],
            end: [end.x, end.y],
            color: settings.color,
            stroke_width: settings.stroke_width,
        },
        ToolMode::Line => AnnotationKind::Line {
            start: [start.x, start.y],
            end: [end.x, end.y],
            color: settings.color,
            stroke_width: settings.stroke_width,
        },
        ToolMode::Mosaic => {
            let norm = ViewportTransform::normalize_drag_rect(start, end);
            if norm.width < 1. || norm.height < 1. {
                return None;
            }
            AnnotationKind::Mosaic {
                region: norm,
                block_size: settings.mosaic_block_size,
            }
        }
        ToolMode::Blur => {
            let norm = ViewportTransform::normalize_drag_rect(start, end);
            if norm.width < 1. || norm.height < 1. {
                return None;
            }
            AnnotationKind::GaussianBlur {
                region: norm,
                radius: settings.blur_radius,
            }
        }
        ToolMode::Pointer
        | ToolMode::Pan
        | ToolMode::Crop
        | ToolMode::Brush
        | ToolMode::Number
        | ToolMode::Text => {
            return None;
        }
    };

    Some(AnnotationItem { id, kind })
}

fn finish_brush_points(points: &[[f32; 2]], current_img: Point2) -> Vec<[f32; 2]> {
    let mut out = points.to_vec();
    if current_img.x.is_finite() && current_img.y.is_finite() {
        let should_push = out
            .last()
            .is_none_or(|last| (current_img.x - last[0]).hypot(current_img.y - last[1]) >= 0.25);
        if should_push {
            out.push([current_img.x, current_img.y]);
        }
    }
    out
}

fn next_number(doc: &DocumentState) -> u32 {
    doc.annotations
        .iter()
        .filter_map(|item| match item.kind {
            AnnotationKind::Number { number, .. } => Some(number),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}
fn renumber_copy(item: &mut AnnotationItem, next: &mut u32) {
    if let AnnotationKind::Number { number, .. } = &mut item.kind {
        *number = *next;
        *next = next.saturating_add(1);
    }
}
