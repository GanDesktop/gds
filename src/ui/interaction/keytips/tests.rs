use zgui::geom::{Css, CssPx, Point, Rect, Size};
use zgui::reactive::{Mounted, RwSignal, Signal, install, prelude::*};
use zgui::vocab::{Key, Modifiers, NamedKey};

use super::{
    EDGE_INSET, EDGE_MARGIN, HintPlacement, KeytipHost, KeytipRegistry, LabelSlot,
    allocate_dialog_scope_id, held_after_key_down, held_after_key_up, place_labels,
    push_modal_scope, rects_intersect, remove_modal_scope, scope_allows,
};
use crate::ui::shortcut::{HeldModifier, Shortcut};

fn window_content() -> Rect<CssPx, Css> {
    Rect::new(
        Point::new(CssPx(20.0), CssPx(20.0)),
        Size::new(CssPx(1240.0), CssPx(780.0)),
    )
}

fn slot(anchor: Rect<CssPx, Css>, width: f32, height: f32, placement: HintPlacement) -> LabelSlot {
    LabelSlot {
        anchor,
        size: Size::new(CssPx(width), CssPx(height)),
        placement,
    }
}

fn icon_button(x: f32, y: f32) -> Rect<CssPx, Css> {
    Rect::new(
        Point::new(CssPx(x), CssPx(y)),
        Size::new(CssPx(32.0), CssPx(32.0)),
    )
}

fn label_rects(slots: &[LabelSlot], content: Rect<CssPx, Css>) -> Vec<Rect<CssPx, Css>> {
    place_labels(slots, content)
        .into_iter()
        .zip(slots)
        .map(|(origin, s)| Rect::new(origin, s.size))
        .collect()
}

fn assert_clear_and_inside(rects: &[Rect<CssPx, Css>], content: Rect<CssPx, Css>) {
    let c_left = content.origin.x.0;
    let c_top = content.origin.y.0;
    let c_right = c_left + content.size.width.0;
    let c_bottom = c_top + content.size.height.0;

    for (idx, rect) in rects.iter().enumerate() {
        let r_left = rect.origin.x.0;
        let r_top = rect.origin.y.0;
        let r_right = r_left + rect.size.width.0;
        let r_bottom = r_top + rect.size.height.0;
        assert!(
            r_left >= c_left + EDGE_MARGIN
                && r_right <= c_right - EDGE_MARGIN
                && r_top >= c_top + EDGE_MARGIN
                && r_bottom <= c_bottom - EDGE_MARGIN,
            "label {idx} at {rect:?} left content {content:?}"
        );
        for other in &rects[idx + 1..] {
            assert!(
                !rects_intersect(*rect, *other),
                "label {idx} at {rect:?} overlaps {other:?}"
            );
        }
    }
}

#[test]
fn passive_ctrl_and_alt_hold_state_updates_immediately_on_press_and_release() {
    // Pressing Ctrl alone (even when backend sends modifiers = NONE on KeyDown) arms Ctrl immediately.
    assert_eq!(
        held_after_key_down(&Key::Named(NamedKey::Control), Modifiers::NONE),
        Some(HeldModifier::Ctrl)
    );
    // Holding Shift alongside Ctrl or Alt stays in the same modifier group.
    assert_eq!(
        held_after_key_down(&Key::Named(NamedKey::Shift), Modifiers::CONTROL),
        Some(HeldModifier::Ctrl)
    );
    assert_eq!(
        held_after_key_down(&Key::character("X"), Modifiers::ALT | Modifiers::SHIFT),
        Some(HeldModifier::Alt)
    );
    // Ctrl+Alt and AltGraph suppress hints immediately.
    assert_eq!(
        held_after_key_down(&Key::Named(NamedKey::Alt), Modifiers::CONTROL),
        None
    );
    assert_eq!(
        held_after_key_down(&Key::Named(NamedKey::AltGraph), Modifiers::NONE),
        None
    );
    // Releasing Ctrl while Shift remains held clears keytips immediately.
    assert_eq!(
        held_after_key_up(
            &Key::Named(NamedKey::Control),
            Modifiers::CONTROL | Modifiers::SHIFT
        ),
        None
    );
    // Releasing Shift while Alt remains held keeps Alt active.
    assert_eq!(
        held_after_key_up(
            &Key::Named(NamedKey::Shift),
            Modifiers::ALT | Modifiers::SHIFT
        ),
        Some(HeldModifier::Alt)
    );
}

#[test]
fn registry_and_modal_dialog_scope_suppress_workspace_hints() {
    install().ok();
    let scope = Mounted::new();
    scope.with(|| {
        let active = RwSignal::new_local(false);
        let _host = KeytipHost(Some(active), zgui::view::Children::from_view(()));
        let reg = KeytipRegistry::current().expect("keytip registry installed");
        assert_eq!(reg.held().get_untracked(), None);

        reg.set_held(Some(HeldModifier::Ctrl));
        assert_eq!(reg.held().get_untracked(), Some(HeldModifier::Ctrl));
        assert!(active.get_untracked());

        reg.stage(
            zgui::prelude::NodeRef::new(),
            Shortcut::parse("Ctrl+R").expect("Ctrl+R"),
            HintPlacement::Corner,
            Signal::stored_local(true),
        );

        assert!(scope_allows(&[], None));
        let modal_id = allocate_dialog_scope_id();
        push_modal_scope(Some(reg), modal_id);
        assert!(!reg.is_scope_active(None));
        assert!(reg.is_scope_active(Some(modal_id)));
        remove_modal_scope(Some(reg), modal_id);
        assert!(reg.is_scope_active(None));

        let child = scope.owner().child();
        let child_scope_id =
            child.with(|| crate::ui::feedback::dialog::DialogScope::enter().id().0);
        assert!(!reg.is_scope_active(None));
        assert!(reg.is_scope_active(Some(child_scope_id)));
        child.cleanup();
        assert!(reg.is_scope_active(None));

        reg.release();
        assert_eq!(reg.held().get_untracked(), None);
        assert!(!active.get_untracked());
    });
    scope.unmount();
}

#[test]
fn corner_trailing_trailing_before_and_bottom_leading_placements_avoid_collisions() {
    let content = window_content();

    // 1. Corner inside 32x32 icon button.
    let btn = icon_button(1064.0, 71.0);
    let corner_rects = label_rects(&[slot(btn, 20.0, 16.0, HintPlacement::Corner)], content);
    assert_clear_and_inside(&corner_rects, content);
    assert!(
        (corner_rects[0].origin.y.0 - (btn.origin.y.0 + EDGE_INSET)).abs() <= 1.0
            && ((corner_rects[0].origin.x.0 + corner_rects[0].size.width.0)
                - (btn.origin.x.0 + btn.size.width.0 - EDGE_INSET))
                .abs()
                <= 1.0
    );

    // 2. Trailing + TrailingBefore(reserved) + Corner close button on same row.
    let row = Rect::new(
        Point::new(CssPx(10.0), CssPx(65.0)),
        Size::new(CssPx(202.0), CssPx(44.0)),
    );
    let close = Rect::new(
        Point::new(CssPx(172.0), CssPx(71.0)),
        Size::new(CssPx(32.0), CssPx(32.0)),
    );
    let reserve = (row.origin.x.0 + row.size.width.0) - close.origin.x.0;
    let row_rects = label_rects(
        &[
            slot(close, 20.0, 16.0, HintPlacement::Corner),
            slot(row, 48.0, 16.0, HintPlacement::TrailingBefore(reserve)),
            slot(row, 24.0, 16.0, HintPlacement::TrailingBefore(reserve)),
        ],
        content,
    );
    assert_clear_and_inside(&row_rects, content);
    assert!(!rects_intersect(row_rects[1], close));
    assert!(!rects_intersect(row_rects[2], close));

    // 3. BottomLeadingCorner on collapsed row.
    let compact = Rect::new(
        Point::new(CssPx(24.0), CssPx(120.0)),
        Size::new(CssPx(38.0), CssPx(44.0)),
    );
    let bottom_rects = label_rects(
        &[slot(
            compact,
            18.0,
            16.0,
            HintPlacement::BottomLeadingCorner,
        )],
        content,
    );
    assert_clear_and_inside(&bottom_rects, content);
    assert!(rects_intersect(bottom_rects[0], compact));

    // 4. Dense overlapping targets never overlap or leave content.
    let dense: Vec<LabelSlot> = (0..18)
        .map(|_| slot(icon_button(300.0, 200.0), 38.0, 16.0, HintPlacement::Corner))
        .collect();
    let dense_rects = label_rects(&dense, content);
    assert_clear_and_inside(&dense_rects, content);
}

#[test]
fn rects_intersect_requires_positive_area_overlap() {
    let outer = Rect::new(
        Point::new(CssPx(0.0), CssPx(0.0)),
        Size::new(CssPx(10.0), CssPx(10.0)),
    );
    let zero_width_inside = Rect::new(
        Point::new(CssPx(5.0), CssPx(2.0)),
        Size::new(CssPx(0.0), CssPx(6.0)),
    );
    let touching_edge = Rect::new(
        Point::new(CssPx(10.0), CssPx(0.0)),
        Size::new(CssPx(10.0), CssPx(10.0)),
    );
    let overlapping = Rect::new(
        Point::new(CssPx(8.0), CssPx(8.0)),
        Size::new(CssPx(4.0), CssPx(4.0)),
    );
    assert!(!rects_intersect(zero_width_inside, outer));
    assert!(!rects_intersect(outer, zero_width_inside));
    assert!(!rects_intersect(outer, touching_edge));
    assert!(rects_intersect(outer, overlapping));
}
