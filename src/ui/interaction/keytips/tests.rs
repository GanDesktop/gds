use std::rc::Rc;

use gpui_kit::{Bounds, KeyBinding, Modifiers, point, px, size};

use super::{
    EDGE_INSET, EDGE_MARGIN, HeldModifier, HintPlacement, LabelSlot, Pixels, Shortcut, Staged,
    place_labels, visible_binding,
};
use crate::ui::ShortcutBadge;

fn bounds(x: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(0.)), size(px(10.), px(10.)))
}

fn shortcut() -> Shortcut {
    crate::ui::actions::Command::FocusNotifications.into()
}

fn binding(keystrokes: &'static str) -> KeyBinding {
    KeyBinding::new(keystrokes, crate::ui::actions::FocusNotifications, None)
}

/// The content area of a 1280x820 window with Kit's 20px client frame.
fn window_content() -> Bounds<Pixels> {
    Bounds::new(point(px(20.), px(20.)), size(px(1240.), px(780.)))
}

fn slot(anchor: Bounds<Pixels>, width: f32, height: f32, placement: HintPlacement) -> LabelSlot {
    LabelSlot {
        anchor,
        size: size(px(width), px(height)),
        placement,
    }
}

fn icon_button(x: f32, y: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(32.), px(32.)))
}

fn label_rects(slots: &[LabelSlot], content: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    place_labels(slots, content)
        .into_iter()
        .zip(slots)
        .map(|(origin, slot)| Bounds::new(origin, slot.size))
        .collect()
}

fn assert_clear_and_inside(rects: &[Bounds<Pixels>], content: Bounds<Pixels>) {
    for (index, rect) in rects.iter().enumerate() {
        assert!(
            rect.left() >= content.left() + EDGE_MARGIN
                && rect.right() <= content.right() - EDGE_MARGIN
                && rect.top() >= content.top() + EDGE_MARGIN
                && rect.bottom() <= content.bottom() - EDGE_MARGIN,
            "label {index} at {rect:?} left the content area {content:?}"
        );
        for other in &rects[index + 1..] {
            assert!(
                !rect.intersects(other),
                "label {index} at {rect:?} covers {other:?}"
            );
        }
    }
}

#[test]
fn hints_are_kept_while_a_group_is_held_and_dropped_on_release() {
    let mut staged = Staged::default();
    let mounted = Rc::new(());

    assert!(!staged.stage(
        "a".into(),
        shortcut(),
        bounds(0.),
        HintPlacement::Corner,
        Rc::downgrade(&mounted)
    ));
    assert_eq!(staged.hints.len(), 1);
    assert!(staged.set_held(Some(HeldModifier::Ctrl)));
    assert!(!staged.set_held(Some(HeldModifier::Ctrl)));
    // Switching groups keeps what the controls staged, so they do not have to
    // restage for the other modifier.
    assert!(staged.set_held(Some(HeldModifier::Alt)));
    assert_eq!(staged.hints.len(), 1);
    assert!(staged.set_held(None));
    assert!(staged.hints.is_empty());
}

/// Only one group is armed at a time, and a mixed press arms neither: AltGr and
/// Ctrl+Alt must not light up labels that belong to the other modifier.
#[test]
fn a_held_group_is_one_modifier_and_never_a_mix() {
    let modifiers = |control, alt, shift, platform| Modifiers {
        control,
        alt,
        shift,
        platform,
        function: false,
    };

    assert_eq!(
        HeldModifier::from_modifiers(modifiers(true, false, false, false)),
        Some(HeldModifier::Ctrl)
    );
    assert_eq!(
        HeldModifier::from_modifiers(modifiers(false, true, false, false)),
        Some(HeldModifier::Alt)
    );
    assert_eq!(
        HeldModifier::from_modifiers(modifiers(true, false, true, false)),
        Some(HeldModifier::Ctrl)
    );
    assert_eq!(
        HeldModifier::from_modifiers(modifiers(false, true, true, false)),
        Some(HeldModifier::Alt)
    );
    assert_eq!(
        HeldModifier::from_modifiers(modifiers(true, true, false, false)),
        None
    );
    assert_eq!(
        HeldModifier::from_modifiers(modifiers(false, false, false, true)),
        None
    );
    assert_eq!(
        HeldModifier::from_modifiers(modifiers(false, false, true, false)),
        None
    );
    assert_eq!(HeldModifier::from_modifiers(Modifiers::none()), None);
}

#[test]
fn a_restaged_control_asks_for_a_frame_only_when_it_moved() {
    let mut staged = Staged::default();
    let mounted = Rc::new(());
    staged.set_held(Some(HeldModifier::Ctrl));

    assert!(staged.stage(
        "a".into(),
        shortcut(),
        bounds(0.),
        HintPlacement::Corner,
        Rc::downgrade(&mounted)
    ));
    assert!(!staged.stage(
        "a".into(),
        shortcut(),
        bounds(0.),
        HintPlacement::Corner,
        Rc::downgrade(&mounted)
    ));
    assert!(staged.stage(
        "a".into(),
        shortcut(),
        bounds(20.),
        HintPlacement::Corner,
        Rc::downgrade(&mounted)
    ));
    assert_eq!(staged.hints.len(), 1);
}

#[test]
fn a_control_that_left_the_tree_stops_contributing() {
    let mut staged = Staged::default();
    let mounted = Rc::new(());
    staged.stage(
        "a".into(),
        shortcut(),
        bounds(0.),
        HintPlacement::Corner,
        Rc::downgrade(&mounted),
    );
    staged
        .hints
        .retain(|_, hint| hint.mounted.strong_count() > 0);
    assert_eq!(staged.hints.len(), 1);

    drop(mounted);
    staged
        .hints
        .retain(|_, hint| hint.mounted.strong_count() > 0);
    assert!(staged.hints.is_empty());
}

/// A label spells the combination with the same key names the keycaps use and
/// drops only the modifier the press already shows.
#[test]
fn a_hint_label_keeps_only_the_keys_the_press_does_not_show() {
    let label = |keystrokes: &'static str, held| {
        ShortcutBadge::path_label(std::slice::from_ref(&binding(keystrokes)), held)
            .map(|label| label.to_string())
    };

    assert_eq!(label("ctrl-r", HeldModifier::Ctrl).as_deref(), Some("R"));
    assert_eq!(
        label("ctrl-shift-tab", HeldModifier::Ctrl).as_deref(),
        Some("Shift+Tab")
    );
    assert_eq!(label("alt-a", HeldModifier::Alt).as_deref(), Some("A"));
    assert_eq!(label("alt-left", HeldModifier::Alt).as_deref(), Some("←"));
    // A hint never shows the other group's prefix as a leftover.
    assert_eq!(
        label("ctrl-shift-a", HeldModifier::Ctrl).as_deref(),
        Some("Shift+A")
    );
}

/// A label belongs to the group that was pressed: a key without that modifier
/// (`F5`, `Delete`) is not advertised, and neither is one from the other group.
#[test]
fn a_label_only_answers_its_own_modifier_group() {
    let refresh = binding("ctrl-r");
    let confirm = binding("alt-a");
    let delete = binding("delete");
    let next_tab = binding("ctrl-shift-tab");

    assert!(visible_binding(true, Some(refresh.clone()), HeldModifier::Ctrl).is_some());
    assert!(visible_binding(true, Some(refresh.clone()), HeldModifier::Alt).is_none());
    assert!(visible_binding(true, Some(confirm.clone()), HeldModifier::Alt).is_some());
    assert!(visible_binding(true, Some(confirm), HeldModifier::Ctrl).is_none());
    assert!(visible_binding(true, Some(next_tab), HeldModifier::Ctrl).is_some());
    assert!(visible_binding(true, Some(delete.clone()), HeldModifier::Ctrl).is_none());
    assert!(visible_binding(true, Some(delete), HeldModifier::Alt).is_none());
}

/// A binding on its own is not a working shortcut: a global command stays bound
/// inside a dialog while its handler sits outside the dialog, and a multi-key
/// sequence is not something one modifier press can advertise.
#[test]
fn only_a_handled_single_stroke_action_gets_a_label() {
    let single = binding("ctrl-b");
    let sequence = binding("ctrl-k ctrl-b");

    assert!(visible_binding(true, Some(single.clone()), HeldModifier::Ctrl).is_some());
    assert!(visible_binding(false, Some(single), HeldModifier::Ctrl).is_none());
    assert!(visible_binding(true, Some(sequence), HeldModifier::Ctrl).is_none());
    assert!(visible_binding(true, None, HeldModifier::Ctrl).is_none());
}

/// An icon control carries its label as a badge in its own top trailing corner,
/// inside the control, so the two read as one piece.
#[test]
fn an_icon_control_keeps_its_label_in_its_top_trailing_corner() {
    let content = window_content();
    let button = icon_button(1064., 71.);
    let slots = vec![slot(button, 20., 16., HintPlacement::Corner)];

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    let rect = rects[0];
    assert!(
        (rect.top() - (button.top() + EDGE_INSET)).abs() <= px(1.),
        "the label is not inside the control's top edge: {rect:?}"
    );
    assert!(
        (rect.right() - (button.right() - EDGE_INSET)).abs() <= px(1.),
        "the label does not end inside the control's trailing edge: {rect:?}"
    );
    assert!(rect.intersects(&button));
}

/// Five 32px toolbar buttons with corner labels: every label stays on its own
/// button, none covers another and none drifts away from its control.
#[test]
fn adjacent_toolbar_labels_stay_on_their_buttons() {
    let content = window_content();
    let buttons = (0..5)
        .map(|index| icon_button(1064. + index as f32 * 40., 71.))
        .collect::<Vec<_>>();
    let widths = [20., 20., 20., 20., 20.];
    let slots = buttons
        .iter()
        .zip(widths)
        .map(|(button, width)| slot(*button, width, 16., HintPlacement::Corner))
        .collect::<Vec<_>>();

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    for (rect, button) in rects.iter().zip(&buttons) {
        assert!(
            rect.intersects(button),
            "label {rect:?} is not on its own button {button:?}"
        );
    }
    let rows = rects.iter().map(|rect| rect.top()).collect::<Vec<_>>();
    assert!(
        rows.iter().all(|row| *row == rows[0]),
        "corner labels fit one row: {rows:?}"
    );
}

/// A control at a window edge keeps its label on screen and on its own corner.
#[test]
fn labels_clamp_at_the_window_edges() {
    let content = window_content();
    let top_right = icon_button(1228., 11.);
    let bottom_right = icon_button(1228., 758.);
    let bottom_left = icon_button(20., 758.);
    let slots = vec![
        slot(top_right, 20., 16., HintPlacement::Corner),
        slot(bottom_right, 20., 16., HintPlacement::Corner),
        slot(bottom_left, 20., 16., HintPlacement::Corner),
    ];

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    for (rect, anchor) in rects
        .iter()
        .zip([top_right, bottom_right, bottom_left].iter())
    {
        assert!(
            rect.intersects(anchor),
            "label {rect:?} left its control at the window edge: {anchor:?}"
        );
    }
}

/// Settings navigation cycles pages with Ctrl+Tab; those labels belong inside
/// their own rows and must not cover the next row.
#[test]
fn row_labels_stay_inside_their_own_row() {
    let content = window_content();
    let rows = (0..5)
        .map(|index| {
            Bounds::new(
                point(px(10.), px(65. + index as f32 * 42.)),
                size(px(202.), px(38.)),
            )
        })
        .collect::<Vec<_>>();
    let slots = vec![
        slot(rows[0], 52., 16., HintPlacement::Trailing),
        slot(rows[3], 64., 16., HintPlacement::Trailing),
    ];

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    for (rect, anchor) in rects.iter().zip([rows[0], rows[3]].iter()) {
        assert!(
            rect.top() >= anchor.top() && rect.bottom() <= anchor.bottom(),
            "label {rect:?} does not stay inside row {anchor:?}"
        );
        assert!(rect.right() <= anchor.right());
    }
}

/// A row that has its own control at the trailing edge keeps its label in front
/// of that control, so the row's key never covers the control's corner badge.
#[test]
fn a_row_label_stops_before_the_control_at_its_trailing_edge() {
    let content = window_content();
    let row = Bounds::new(point(px(10.), px(65.)), size(px(202.), px(44.)));
    let close = Bounds::new(point(px(172.), px(71.)), size(px(32.), px(32.)));
    let reserve = row.right() - close.left();
    let slots = vec![
        slot(close, 20., 16., HintPlacement::Corner),
        slot(row, 50., 16., HintPlacement::TrailingBefore(reserve)),
    ];

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    let (badge, hint) = (rects[0], rects[1]);
    assert!(
        badge.intersects(&close),
        "the badge left its control {badge:?}"
    );
    assert!(
        !hint.intersects(&close),
        "the row hint {hint:?} covers the trailing control {close:?}"
    );
    assert!(
        hint.top() >= row.top() && hint.bottom() <= row.bottom(),
        "the row hint {hint:?} left its row {row:?}"
    );
    assert!(
        (f32::from(hint.right()) - f32::from(close.left())).abs() <= 1.,
        "the row hint {hint:?} does not end in front of the control {close:?}"
    );
}

/// A row whose two directions land on the same tab shows both keys side by side
/// instead of stacking one over the other.
#[test]
fn two_labels_for_one_row_sit_side_by_side() {
    let content = window_content();
    let row = Bounds::new(point(px(10.), px(65.)), size(px(202.), px(44.)));
    let close = Bounds::new(point(px(172.), px(71.)), size(px(32.), px(32.)));
    let reserve = row.right() - close.left();
    let slots = vec![
        slot(row, 48., 16., HintPlacement::TrailingBefore(reserve)),
        slot(row, 24., 16., HintPlacement::TrailingBefore(reserve)),
    ];

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    for rect in &rects {
        assert!(
            rect.top() >= row.top() && rect.bottom() <= row.bottom(),
            "label {rect:?} left its row {row:?}"
        );
        assert!(
            rect.right() <= close.left(),
            "label {rect:?} reaches the trailing control {close:?}"
        );
    }
}

/// A row narrower than one label in front of its trailing control still shows
/// the key on the row instead of dropping it or spilling it far away.
#[test]
fn a_narrow_row_keeps_its_target_label_near_it() {
    let content = window_content();
    let row = Bounds::new(point(px(10.), px(65.)), size(px(58.), px(38.)));
    let reserve = px(40.);
    let slots = vec![slot(row, 48., 16., HintPlacement::TrailingBefore(reserve))];

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    assert!(
        rects[0].intersects(&row),
        "a narrow row cannot hold its label, but it must stay on it: {:?}",
        rects[0]
    );
}

/// A collapsed row is narrower than its label; the label stays a badge on the
/// row itself instead of spilling over the row's contents.
#[test]
fn a_row_too_narrow_for_its_label_keeps_its_corner_badge() {
    let content = window_content();
    let narrow = Bounds::new(point(px(10.), px(65.)), size(px(58.), px(38.)));
    let slots = vec![slot(narrow, 71., 16., HintPlacement::Corner)];

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
    assert!(
        rects[0].intersects(&narrow),
        "a narrow row cannot hold its label, but it must stay on it: {:?}",
        rects[0]
    );
}

/// The layout survives the largest text scale in a window at the minimum size:
/// labels are taller and wider and still never cover each other.
#[test]
fn a_short_window_at_the_largest_text_size_keeps_labels_apart() {
    let content = Bounds::new(point(px(20.), px(20.)), size(px(680.), px(440.)));
    let mut slots = (0..5)
        .map(|index| {
            slot(
                icon_button(380. + index as f32 * 44., 300.),
                [84., 78., 84., 84., 46.][index],
                23.,
                HintPlacement::Corner,
            )
        })
        .collect::<Vec<_>>();
    slots.push(slot(
        icon_button(380., 420.),
        84.,
        23.,
        HintPlacement::Corner,
    ));
    slots.push(slot(
        Bounds::new(point(px(10.), px(65.)), size(px(202.), px(38.))),
        100.,
        23.,
        HintPlacement::Trailing,
    ));

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
}

/// Even when a window cannot give every label room beside its control, none of
/// them covers another one or leaves the window.
#[test]
fn more_labels_than_rows_still_never_overlap() {
    let content = Bounds::new(point(px(20.), px(20.)), size(px(680.), px(440.)));
    let slots = (0..18)
        .map(|_| slot(icon_button(300., 200.), 38., 16., HintPlacement::Corner))
        .collect::<Vec<_>>();

    let rects = label_rects(&slots, content);
    assert_clear_and_inside(&rects, content);
}
