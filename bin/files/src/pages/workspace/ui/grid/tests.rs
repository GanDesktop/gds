use super::{GAP, Geometry, view};
use gpui_kit::{Bounds, point, px, size};

#[test]
fn grid_hit_testing_respects_gaps_partial_rows_and_scrolling() {
    let grid = Geometry::new(700., 1.);
    assert!(grid.columns > 1);
    let bounds = Bounds::new(
        point(px(30.), px(70.)),
        size(px(700. + view::TABLE_INSET * 2.), px(600.)),
    );
    let count = grid.columns + 1;
    let first = grid.tile(0, bounds.origin, px(0.));
    assert_eq!(grid.hit(bounds, first.center(), px(0.), count), Some(0));
    let gap = point(first.right() + px(GAP / 2.), first.center().y);
    assert_eq!(grid.hit(bounds, gap, px(0.), count), None);
    let unfilled = grid.tile(grid.columns + 1, bounds.origin, px(0.));
    assert_eq!(grid.hit(bounds, unfilled.center(), px(0.), count), None);
    let shifted = grid.tile(grid.columns, bounds.origin, px(-grid.row_height));
    assert_eq!(
        grid.hit(bounds, shifted.center(), px(-grid.row_height), count),
        Some(grid.columns)
    );
}

#[test]
fn grid_keyboard_motion_moves_by_columns_and_clamps_the_last_row() {
    let grid = Geometry::new(700., 1.);
    let count = grid.columns * 2 + 1;
    assert_eq!(grid.navigate("down", None, count), Some(0));
    assert_eq!(
        grid.navigate("down", Some(1), count),
        Some(1 + grid.columns)
    );
    assert_eq!(
        grid.navigate("down", Some(1 + grid.columns), count),
        Some(count - 1)
    );
    assert_eq!(
        grid.navigate("up", Some(count - 1), count),
        Some(grid.columns)
    );
    assert_eq!(
        grid.navigate("right", Some(count - 1), count),
        Some(count - 1)
    );
    assert_eq!(grid.navigate("home", Some(3), count), Some(0));
    assert_eq!(grid.navigate("end", Some(0), count), Some(count - 1));
    assert_eq!(grid.navigate("down", None, 0), None);
}
