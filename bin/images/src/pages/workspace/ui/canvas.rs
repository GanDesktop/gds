//! Pixel presentation, pointer capture and asynchronous gesture composition.

use super::*;
use crate::{
    entities::document::AnnotationKind,
    features::edit_canvas::{
        ActiveInteraction, HandleKind, HoverTarget, PointerDownInput, PointerDownOutcome,
        PointerModifiers, PointerMoveOutcome, ToolFsm, ToolMode, handles_for_item,
        hover_test_canvas,
    },
    shared::geometry::Rect2,
};
use zgui::{
    geom::{Css, CssPx, Point},
    view::{Either, events},
    vocab::{Modifiers, PointerButton, Role},
};
use zgui_ui::SpinnerProps;

mod render;
mod tile;
pub(super) use render::Compositor;

const MIN_SCALE_FACTOR: f32 = 0.01;
const DOUBLE_CLICK_MAX_MS: u128 = 400;
const DOUBLE_CLICK_MAX_DISTANCE_PX: f32 = 4.0;
const WHEEL_LINE_PX: f32 = 32.0;
const WHEEL_ZOOM_SENSITIVITY: f32 = 0.005;
const CHECKERBOARD_TILE_PX: f32 = 12.0;
const HANDLE_SIZE_PX: f32 = 6.0;
const OVERLAY_HANDLE_MARGIN_PX: f32 = 8.0;

enum PointerUpOutcome {
    None,
    PanEnded,
    Committed,
}

impl Page {
    fn point(self, pos: Point<CssPx, Css>) -> Point2 {
        let node = self.window().viewport;
        let offset = node.window_bounds().map(|b| b.origin).unwrap_or_default();
        let scale = node.scale().max(MIN_SCALE_FACTOR);
        Point2::new(pos.x.0 - offset.x.0 / scale, pos.y.0 - offset.y.0 / scale)
    }

    fn down(self, pt: Point2, mods: Modifiers, button: Option<PointerButton>, double: bool) {
        self.flush_text();
        self.focus();
        let outcome = self.change(|s| {
            let doc = s.doc.as_ref()?;
            if button == Some(PointerButton::Middle) || s.tool.tool == ToolMode::Pan {
                s.pan = Some(pt);
                return None;
            }
            s.gesture += 1;
            Some(s.tool.on_pointer_down(
                doc,
                &mut s.history,
                PointerDownInput {
                    vp_pt: pt,
                    img_pt: s.transform.viewport_to_image(pt),
                    scale: s.transform.scale,
                    modifiers: modifiers(mods),
                    click_count: if double { 2 } else { 1 },
                },
            ))
        });
        match outcome {
            Some(PointerDownOutcome::OpenTextEditor { initial_text }) => {
                self.open_text_editor(initial_text);
            }
            Some(PointerDownOutcome::NeedsTransientPreview) => {
                self.render();
            }
            Some(PointerDownOutcome::None | PointerDownOutcome::SelectionChanged) | None => {}
        }
        self.changed();
        self.repaint();
    }

    fn moved(self, pt: Point2, mods: Modifiers) {
        let outcome = self.change(|s| {
            if let Some(last) = s.pan {
                s.pan = Some(pt);
                s.transform.pan_by(pt.x - last.x, pt.y - last.y);
                return PointerMoveOutcome::ChromeOnly;
            }
            let Some(doc) = &s.doc else {
                return PointerMoveOutcome::None;
            };
            s.tool
                .on_pointer_move(doc, pt, s.transform.viewport_to_image(pt), modifiers(mods))
        });
        match outcome {
            PointerMoveOutcome::NeedsTransientPreview { .. } => {
                self.window().overlay.update(|v| *v += 1);
                self.render();
            }
            PointerMoveOutcome::ChromeOnly if self.read(|s| s.pan.is_some()) => self.repaint(),
            PointerMoveOutcome::ChromeOnly => self.window().overlay.update(|v| *v += 1),
            PointerMoveOutcome::None => {}
        }
    }

    fn up(self, pt: Point2, mods: Modifiers) {
        let outcome = self.change(|s| {
            if s.pan.take().is_some() {
                return PointerUpOutcome::PanEnded;
            }
            let Some(doc) = &mut s.doc else {
                return PointerUpOutcome::None;
            };
            if !s.tool.is_pointer_gesture() {
                return PointerUpOutcome::None;
            }
            s.tool
                .on_pointer_move(doc, pt, s.transform.viewport_to_image(pt), modifiers(mods));
            s.tool.on_pointer_up(doc, &mut s.history, s.transform.scale);
            s.gesture += 1;
            PointerUpOutcome::Committed
        });
        match outcome {
            PointerUpOutcome::None => {}
            PointerUpOutcome::PanEnded => self.changed(),
            PointerUpOutcome::Committed => {
                self.render();
                self.changed();
                self.repaint();
            }
        }
    }

    pub(super) fn flush_text(self) {
        let Some(value) = self.text_editor_value() else {
            return;
        };
        self.change(|s| {
            if let Some(doc) = &mut s.doc {
                s.tool.commit_text_draft(&value, doc, &mut s.history);
            }
            s.gesture += 1;
        });
        self.close_text_editor();
        self.render();
        self.changed();
    }
}

fn cursor_at(page: Page, target: HoverTarget) -> &'static str {
    page.read(|s| {
        if s.doc.is_none() {
            return "default";
        }
        if s.pan.is_some() {
            return "grabbing";
        }
        match target {
            HoverTarget::Handle { handle, .. } => match handle {
                HandleKind::N | HandleKind::S => "ns-resize",
                HandleKind::E | HandleKind::W => "ew-resize",
                HandleKind::Ne | HandleKind::Sw => "nesw-resize",
                HandleKind::Nw | HandleKind::Se => "nwse-resize",
                HandleKind::LineStart | HandleKind::LineEnd => "crosshair",
            },
            HoverTarget::Body { .. } => "move",
            HoverTarget::None => match s.tool.tool {
                ToolMode::Pan => "grab",
                ToolMode::Text => "text",
                ToolMode::Pointer => "default",
                _ => "crosshair",
            },
        }
    })
}

fn modifiers(mods: Modifiers) -> PointerModifiers {
    PointerModifiers {
        shift: mods.shift(),
        alt: mods.alt(),
        ctrl: mods.primary(),
    }
}

fn px(value: f32) -> String {
    format!("{value}px")
}

fn outline(rect: Rect2, class: &'static str) -> AnyView {
    AnyView::new(view! {
        box(
            class = class,
            style:left = px(rect.x),
            style:top = px(rect.y),
            style:width = px(rect.width),
            style:height = px(rect.height),
        )
    })
}

/// Draw two paths for the visible part of the image, regardless of its zoomed dimensions.
fn checkerboard(page: Page) -> AnyView {
    use zgui::canvas::{Brush, ShapeBuilder};
    use zgui::elements::kurbo::{BezPath, Rect, Shape};

    let prefs = page.window().prefs.state();
    let image_rect = Memo::new(move |_| {
        page.window().paint.track();
        page.read(|s| {
            let doc = s.doc.as_ref()?;
            let (w, h) = doc.output_dimensions();
            doc.decoded.metadata.technical.has_alpha.then(|| {
                s.transform
                    .image_rect_to_viewport(Rect2::new(0.0, 0.0, w as f32, h as f32))
            })
        })
    });
    AnyView::new(
        zgui::elements::canvas()
            .class("images-checker")
            .draw(move |cx| {
                if !prefs.with(|s| s.checkerboard_backdrop) {
                    return;
                }
                let Some(image) = image_rect.get() else {
                    return;
                };
                let Some(area) =
                    image.intersect(Rect2::new(0.0, 0.0, cx.size.width.0, cx.size.height.0))
                else {
                    return;
                };
                let palette = Gds::current().appearance().theme().colors();
                let rect = |r: Rect2| {
                    Rect::new(r.x.into(), r.y.into(), r.right().into(), r.bottom().into())
                };
                cx.scene.push(
                    ShapeBuilder::new(rect(area).to_path(0.1))
                        .fill(Brush::Solid(palette.surface_container_lowest))
                        .build(),
                );
                let mut squares = BezPath::new();
                let x0 = ((area.x - image.x) / CHECKERBOARD_TILE_PX).floor() as i32;
                let y0 = ((area.y - image.y) / CHECKERBOARD_TILE_PX).floor() as i32;
                let x1 = ((area.right() - image.x) / CHECKERBOARD_TILE_PX).ceil() as i32;
                let y1 = ((area.bottom() - image.y) / CHECKERBOARD_TILE_PX).ceil() as i32;
                for row in y0..y1 {
                    for col in (x0 + (row + x0).rem_euclid(2)..x1).step_by(2) {
                        let square = Rect2::new(
                            image.x + col as f32 * CHECKERBOARD_TILE_PX,
                            image.y + row as f32 * CHECKERBOARD_TILE_PX,
                            CHECKERBOARD_TILE_PX,
                            CHECKERBOARD_TILE_PX,
                        );
                        if let Some(r) = square.intersect(area) {
                            squares.extend(rect(r).path_elements(0.1));
                        }
                    }
                }
                cx.scene.push(
                    ShapeBuilder::new(squares)
                        .fill(Brush::Solid(palette.surface_container_highest))
                        .build(),
                );
            }),
    )
}

fn image_tiles(page: Page, surfaces: &RefCell<tile::Tiles>) -> Vec<AnyView> {
    page.window().paint.track();
    let size = page.window().size.get();
    let viewport = Rect2::new(0.0, 0.0, size.width, size.height);
    page.read(|s| {
        let mut views = Vec::new();
        let mut surfaces = surfaces.borrow_mut();
        surfaces.resize(s.frame.as_ref().map_or(0, |frame| frame.tiles.len()));
        let Some(doc) = &s.doc else {
            return views;
        };
        let Some(frame) = &s.frame else {
            return views;
        };
        if frame.width == 0 || frame.height == 0 {
            return views;
        }
        let tf = s.transform;
        let (width, height) = doc.output_dimensions();
        let sx = tf.scale * width as f32 / frame.width as f32;
        let sy = tf.scale * height as f32 / frame.height as f32;
        for (index, (tile, image)) in frame.tiles.iter().enumerate() {
            let rect = Rect2::new(
                tf.pan_x + tile.x as f32 * sx,
                tf.pan_y + tile.y as f32 * sy,
                tile.width as f32 * sx,
                tile.height as f32 * sy,
            );
            // Keep each visible surface in its own retained view slot when panning.
            views.push(if rect.intersect(viewport).is_some() {
                surfaces.show(page, index, image, rect)
            } else {
                surfaces.hide(index)
            });
        }
        views
    })
}

fn active_crop_rect(tool: &ToolFsm) -> Option<Rect2> {
    if let ActiveInteraction::Creating {
        tool: ToolMode::Crop,
        start_img,
        current_img,
        ..
    } = tool.interaction
    {
        return Some(ViewportTransform::normalize_drag_rect(
            start_img,
            current_img,
        ));
    }
    tool.staged_crop
        .map(|r| Rect2::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32))
}

fn crop_overlay(vp: Rect2, size: Size2) -> Vec<AnyView> {
    let x0 = vp.x.clamp(0.0, size.width);
    let x1 = vp.right().clamp(x0, size.width);
    let y0 = vp.y.clamp(0.0, size.height);
    let y1 = vp.bottom().clamp(y0, size.height);
    let mut views = Vec::with_capacity(5);
    for band in [
        Rect2::new(0.0, 0.0, size.width, y0),
        Rect2::new(0.0, y1, size.width, size.height - y1),
        Rect2::new(0.0, y0, x0, y1 - y0),
        Rect2::new(x1, y0, size.width - x1, y1 - y0),
    ] {
        views.push(outline(band, "images-scrim"));
    }
    views.push(outline(vp, "images-outline"));
    views
}

fn push_selection_overlays(
    views: &mut Vec<AnyView>,
    tool: &ToolFsm,
    items: &[AnnotationItem],
    tf: ViewportTransform,
) {
    if tool.selection.len() == 1
        && let Some(id) = tool.selection.primary()
        && let Some(item) = items.iter().find(|item| item.id == id)
    {
        match &item.kind {
            AnnotationKind::Line { start, end, .. } | AnnotationKind::Arrow { start, end, .. } => {
                let a = tf.image_to_viewport(Point2::new(start[0], start[1]));
                let b = tf.image_to_viewport(Point2::new(end[0], end[1]));
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                views.push(AnyView::new(view! {
                    box(
                        class = "images-line-selection",
                        style:left = px(a.x),
                        style:top = px(a.y),
                        style:width = px(dx.hypot(dy)),
                        style:transform = format!("rotate({}rad)", dy.atan2(dx)),
                    )
                }));
            }
            _ => views.push(outline(
                tf.image_rect_to_viewport(item.visual_bounds()),
                "images-outline",
            )),
        }
        if tool.tool == ToolMode::Pointer {
            let half = HANDLE_SIZE_PX * 0.5;
            for (_, point) in handles_for_item(item) {
                let at = tf.image_to_viewport(point);
                views.push(outline(
                    Rect2::new(at.x - half, at.y - half, HANDLE_SIZE_PX, HANDLE_SIZE_PX),
                    "images-handle",
                ));
            }
        }
    } else if let Some(rect) = tool.selection.union_bounds(items) {
        for item in items.iter().filter(|i| tool.selection.contains(i.id)) {
            views.push(outline(
                tf.image_rect_to_viewport(item.visual_bounds()),
                "images-outline images-member",
            ));
        }
        views.push(outline(
            tf.image_rect_to_viewport(rect),
            "images-outline images-dashed",
        ));
    }
}

fn overlays(page: Page, hover: Memo<HoverTarget>) -> Vec<AnyView> {
    let w = page.window();
    w.paint.track();
    w.overlay.track();
    // Read `hover` before borrowing `State` so Memo recomputation never re-borrows `p.read`.
    let hover_target = hover.get();
    let size = w.size.get();
    page.read(|s| {
        let Some(doc) = &s.doc else {
            return Vec::new();
        };
        let tf = s.transform;
        if let Some(crop_rect) = active_crop_rect(&s.tool) {
            return crop_overlay(tf.image_rect_to_viewport(crop_rect), size);
        }

        let mut canvas_views = Vec::new();
        let mut marquee_view = None;
        match s.tool.interaction {
            ActiveInteraction::Marquee {
                start_img,
                current_img,
                ..
            } => {
                let vp = tf.image_rect_to_viewport(ViewportTransform::normalize_drag_rect(
                    start_img,
                    current_img,
                ));
                marquee_view = Some(outline(vp, "images-outline images-marquee"));
            }
            ActiveInteraction::Creating {
                tool: ToolMode::Blur | ToolMode::Mosaic,
                start_img,
                current_img,
                ..
            } => {
                let vp = tf.image_rect_to_viewport(ViewportTransform::normalize_drag_rect(
                    start_img,
                    current_img,
                ));
                canvas_views.push(outline(vp, "images-outline"));
            }
            _ => {}
        }

        let items = s.preview_objects.as_deref().unwrap_or(&doc.annotations);
        if matches!(s.tool.interaction, ActiveInteraction::Idle)
            && let HoverTarget::Body {
                id,
                discover_redaction: true,
            } = hover_target
            && let Some(item) = items.iter().find(|item| item.id == id)
        {
            canvas_views.push(outline(
                tf.image_rect_to_viewport(item.visual_bounds()),
                "images-outline images-dashed",
            ));
        }

        push_selection_overlays(&mut canvas_views, &s.tool, items, tf);

        let mut views = Vec::new();
        let viewport = Rect2::new(0.0, 0.0, size.width, size.height);
        if !canvas_views.is_empty()
            && let Some(clip) = tf
                .image_rect_to_viewport(doc.canvas_bounds())
                .expand(OVERLAY_HANDLE_MARGIN_PX)
                .intersect(viewport)
        {
            views.push(AnyView::new(view! {
                box(
                    class = "images-overlay-clip",
                    style:left = px(clip.x),
                    style:top = px(clip.y),
                    style:width = px(clip.width),
                    style:height = px(clip.height),
                ) {
                    box(
                        class = "images-overlay-origin",
                        style:left = px(-clip.x),
                        style:top = px(-clip.y),
                        style:width = px(size.width),
                        style:height = px(size.height),
                    ) {
                        {canvas_views}
                    }
                }
            }));
        }
        if let Some(marquee) = marquee_view {
            views.push(marquee);
        }
        views
    })
}

#[component]
pub(super) fn CanvasView(page: Page) -> impl IntoView {
    let node = page.window().viewport;
    let has_doc = Memo::new(move |_| {
        page.window().version.track();
        page.read(|s| s.doc.is_some())
    });
    let label = move || {
        page.window().version.track();
        page.read(|s| {
            s.doc
                .as_ref()
                .map(|d| d.decoded.origin.display_name())
                .unwrap_or_else(|| text("empty-title"))
        })
    };
    let surfaces = RefCell::new(tile::Tiles::default());
    let pointer = RwSignal::new_local(None::<Point2>);
    let hover = Memo::new(move |_| {
        let Some(pt) = pointer.get() else {
            return HoverTarget::None;
        };
        let w = page.window();
        w.version.track();
        w.overlay.track();
        w.paint.track();
        page.read(|s| {
            s.doc.as_ref().map_or(HoverTarget::None, |doc| {
                let point = s.transform.viewport_to_image(pt);
                if !doc.canvas_bounds().contains(point) {
                    return HoverTarget::None;
                }
                hover_test_canvas(
                    s.preview_objects.as_deref().unwrap_or(&doc.annotations),
                    &s.tool.selection,
                    point,
                    s.tool.tool,
                    s.transform.scale,
                )
            })
        })
    });
    let previous_click = Cell::new(None::<(std::time::Instant, Point2)>);
    let active_button_down = Rc::new(Cell::new(None::<PointerButton>));
    let active_button_up = Rc::clone(&active_button_down);
    let active_button_cancel = Rc::clone(&active_button_down);

    view! {
        box(
            node_ref = node,
            class = "images-viewport",
            style:cursor = move || {
                page.window().version.track();
                let target = hover.get();
                Some(cursor_at(page, target).to_owned())
            },
            tabindex = Focus::Sequential,
            a11y:role = move || if has_doc.get() { Role::Image } else { Role::Region },
            a11y:label = label,
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                page.dismiss_popovers();
                if active_button_down.get().is_some() {
                    if page.read(|s| s.pan.is_some() || s.tool.is_pointer_gesture()) {
                        return;
                    }
                    active_button_down.set(None);
                }
                if ev.button != Some(PointerButton::Primary) {
                    previous_click.set(None);
                }
                let Some(button) = ev.button.filter(|button| {
                    matches!(button, PointerButton::Primary | PointerButton::Middle)
                }) else {
                    return;
                };
                if !ev.primary
                    || page.read(|s| s.doc.is_none())
                    || page.window().busy.get_untracked()
                    || page.window().loading.get_untracked()
                {
                    return;
                }
                let pt = page.point(ev.position);
                let double = if button == PointerButton::Primary {
                    let now = std::time::Instant::now();
                    let matched = previous_click.get().is_some_and(|(time, at)| {
                        now.duration_since(time).as_millis() < DOUBLE_CLICK_MAX_MS
                            && (at.x - pt.x).hypot(at.y - pt.y) < DOUBLE_CLICK_MAX_DISTANCE_PX
                    });
                    if matched {
                        previous_click.set(None);
                    } else {
                        previous_click.set(Some((now, pt)));
                    }
                    matched
                } else {
                    false
                };
                active_button_down.set(Some(button));
                page.down(pt, ev.modifiers, Some(button), double);
                ev.capture_pointer();
                ev.prevent_default();
            },
            on:pointer_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
                let pt = page.point(ev.position);
                pointer.set(Some(pt));
                page.moved(pt, ev.modifiers);
            },
            on:pointer_leave = move |_| pointer.set(None),
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                let Some(pressed) = active_button_up.get() else {
                    return;
                };
                if !ev.primary || ev.button != Some(pressed) {
                    return;
                }
                active_button_up.set(None);
                page.up(page.point(ev.position), ev.modifiers);
                ev.release_pointer();
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                if active_button_cancel.take().is_some() {
                    ev.release_pointer();
                }
                page.cancel_gesture();
            },
            on:wheel = move |ev: &mut EventCx<'_, events::Wheel>| {
                let delta = ev.delta.to_pixels(CssPx(WHEEL_LINE_PX));
                if delta.height.0.abs() < f32::EPSILON || page.read(|s| s.doc.is_none()) {
                    return;
                }
                let pt = page.point(ev.position);
                page.change(|s| {
                    s.transform
                        .zoom_around_cursor((-delta.height.0 * WHEEL_ZOOM_SENSITIVITY).exp(), pt);
                });
                page.zoom_changed();
                ev.prevent_default();
            },
        ) {
            {checkerboard(page)}
            {move || image_tiles(page, &surfaces)}
            {move || overlays(page, hover)}
            {move || (!has_doc.get()).then(|| view! { EmptyState(page = page) }.into_view())}
            {move || {
                (page.window().loading.get() && has_doc.get()).then(|| {
                    view! {
                        row(class = "images-loading-pill", a11y:role = Role::Status) {
                            Spinner()
                            text { {text("loading-image")} }
                        }
                    }
                })
            }}
        }
    }
}

fn clipboard_banner(page: Page) -> Option<AnyView> {
    use crate::shared::clipboard::ClipboardOfferKind;
    use gds::ui::{IconButtonProps, IconName, IconProps, button::ButtonProps};

    page.window().version.track();
    let offer = page.window().clipboard_offer.borrow().clone()?;
    let message = match offer {
        ClipboardOfferKind::LocalImageFile(path) => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            format!("{} {name}", text("clipboard-banner-file"))
        }
        ClipboardOfferKind::RawImageMime(_) => text("clipboard-banner-image"),
    };
    Some(AnyView::new(view! {
        row(class = "images-banner") {
            Icon(name = IconName::Clipboard, size = 18.0) {}
            text { {message} }
            Button(on:click = move |_| {
                page.window().clipboard_offer.replace(None);
                page.changed();
                page.open_clipboard();
            }) {
                {text("clipboard-banner-open")}
            }
            IconButton(
                icon = IconName::X,
                label = text("close"),
                on:click = move |_| {
                    page.window().clipboard_offer.replace(None);
                    page.changed();
                },
            )
        }
    }))
}

#[component]
fn EmptyState(page: Page) -> impl IntoView {
    use super::commands::Command;
    use gds::ui::{
        IconName, IconProps,
        button::{ButtonProps, ButtonVariant},
    };

    let loading = page.window().loading;
    move || {
        if loading.get() {
            Either::Left(view! {
                column(class = "images-empty") {
                    Spinner()
                    text { {text("loading-image")} }
                }
            })
        } else {
            Either::Right(view! {
                column(class = "images-empty") {
                    {move || clipboard_banner(page)}
                    Icon(name = IconName::Image, size = 48.0) {}
                    text(class = "images-empty-title") { {text("empty-title")} }
                    text(class = "images-empty-help") { {text("empty-subtitle")} }
                    row(class = "images-empty-actions") {
                        Button(on:click = move |_| page.action(Command::Open)) {
                            {Command::Open.label()}
                        }
                        Button(
                            variant = ButtonVariant::Outline,
                            on:click = move |_| page.action(Command::Paste),
                        ) {
                            {Command::Paste.label()}
                        }
                    }
                }
            })
        }
    }
}
