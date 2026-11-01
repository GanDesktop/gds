use super::*;
use gpui_kit::{SharedString, px};

fn details(count: usize) -> Vec<SharedString> {
    (0..count)
        .map(|index| format!("item {index}").into())
        .collect()
}

#[test]
fn report_details_never_exceed_the_visible_limit() {
    let items = details(DETAIL_LIMIT + 3);
    let (shown, remaining) = visible_details(&items);
    assert_eq!(shown.len(), DETAIL_LIMIT);
    assert_eq!(remaining, 3);

    let items = details(2);
    let (shown, remaining) = visible_details(&items);
    assert_eq!(shown.len(), 2);
    assert_eq!(remaining, 0);
}

#[test]
fn content_height_keeps_the_card_inside_a_short_window() {
    assert_eq!(content_height(px(820.)), px(MAX_CONTENT_HEIGHT));

    // A window with less room than the cap shrinks the text area instead of
    // letting the card run past the bottom edge.
    let short = content_height(px(320.));
    assert!(short < px(MAX_CONTENT_HEIGHT));
    let card_bottom = px(crate::appearance::tokens::TOOLBAR_HEIGHT + TOAST_TOP_GAP)
        + short
        + px(CARD_CHROME_HEIGHT);
    assert!(card_bottom <= px(320.));
}

#[test]
fn column_height_stops_at_the_bottom_inset() {
    let top = px(crate::appearance::tokens::TOOLBAR_HEIGHT + TOAST_TOP_GAP);
    let bottom = px(TOAST_INSET);
    assert_eq!(
        column_height(px(820.), top, bottom),
        px(820.) - top - bottom
    );

    // A window shorter than its own chrome gets no column instead of one that
    // runs past the bottom edge.
    assert_eq!(column_height(px(40.), top, bottom), px(0.));
}
