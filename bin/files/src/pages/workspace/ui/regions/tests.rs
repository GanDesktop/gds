use gpui_kit::{App, Bounds, Edges, Pixels, Window, component::Root, px, test::TestWindowExt};

/// Exercise the real PageContent -> ListSurface -> DataTable clipping chain,
/// rather than a table with spare room around its focus decoration.
#[gpui_kit::test]
fn content_focus_is_complete_exclusive_and_does_not_move_the_table(
    cx: &mut gpui_kit::TestAppContext,
) {
    use super::super::*;
    cx.executor().allow_parking();
    let root = tempfile::tempdir().expect("focus fixture");
    for index in 0..40 {
        std::fs::create_dir(root.path().join(format!("folder-{index:02}")))
            .expect("fixture directory");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unpolled device runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        gds::appearance::theme::apply(&Default::default(), cx).expect("application theme");
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1100.), px(740.)), |window, cx| {
        let view = cx.new(|cx| {
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(Places::default()),
                    start: root.path().to_owned(),
                    home: root.path().to_owned(),
                    preferences: configuration::Preferences::default(),
                    startup_error: None,
                    picker: None,
                },
                window,
                cx,
            )
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.expect("workspace");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        let table_bounds = window.find("table").bounds();
        let region_bounds = window.find("content-region").bounds();
        let scroll = browser
            .read(cx)
            .active()
            .expect("directory tab")
            .scroll
            .clone();
        scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-80.)));
        window.render_frame(cx);
        let offset = scroll.0.borrow().base_handle.offset();

        window.press("alt-space", cx);
        window.render_frame(cx);
        assert!(browser.read(cx).window.focus.is_focused(window));
        assert_complete_outline(window, region_bounds, cx);
        assert_eq!(window.find("table").bounds(), table_bounds);
        assert_eq!(scroll.0.borrow().base_handle.offset(), offset);

        // The table is a separate Tab stop. Its focused descendant must not
        // paint a second outline around the entire content page.
        for _ in 0..32 {
            window.press("tab", cx);
            if browser
                .read(cx)
                .files
                .table
                .focus_handle(cx)
                .is_focused(window)
            {
                break;
            }
        }
        window.render_frame(cx);
        assert!(
            browser
                .read(cx)
                .files
                .table
                .focus_handle(cx)
                .is_focused(window)
        );
        assert_complete_outline(window, table_bounds, cx);
        assert!(!has_outline(window, region_bounds, cx));
        assert_eq!(window.find("table").bounds(), table_bounds);
        assert_eq!(scroll.0.borrow().base_handle.offset(), offset);

        // A real row click must pass through the decoration and keep selection
        // working, while removing keyboard-only page/table decoration.
        window.click_at("table", point(px(100.), px(60.)), cx);
        window.render_frame(cx);
        assert_eq!(browser.read(cx).selected_visible_paths().len(), 1);
        assert!(!has_outline(window, table_bounds, cx));
        assert!(!has_outline(window, region_bounds, cx));
        assert_eq!(window.find("table").bounds(), table_bounds);
        window.press("down", cx);
        window.render_frame(cx);
        assert_eq!(browser.read(cx).selected_visible_paths().len(), 1);
        assert_complete_outline(window, region_bounds, cx);

        // The inset beside the rows is real list blank space, not a control.
        window.click_at("content-region", point(px(10.), px(200.)), cx);
        window.render_frame(cx);
        assert!(browser.read(cx).selected_visible_paths().is_empty());
        assert!(!has_outline(window, region_bounds, cx));

        for (key, region) in [("alt-t", "tabs-region"), ("alt-v", "volumes-region")] {
            window.press(key, cx);
            window.render_frame(cx);
            assert_complete_outline(window, window.find(region).bounds(), cx);
        }
        window.press("alt-l", cx);
        window.press("down", cx);
        window.render_frame(cx);
        assert!(
            browser
                .read(cx)
                .sidebar
                .regions
                .locations
                .is_focused(window)
        );
        assert_complete_outline(window, window.find("locations-region").bounds(), cx);
        window.press("shift-tab", cx);
        window.render_frame(cx);
        assert!(!has_outline(
            window,
            window.find("locations-region").bounds(),
            cx
        ));
    })
    .expect("real Files focus navigation");
}

fn outline(window: &Window, bounds: Bounds<Pixels>, cx: &App) -> Vec<gpui_kit::Quad> {
    use gpui_kit::component::ActiveTheme;
    let scaled = bounds.scale(window.scale_factor());
    window
        .painted_quads()
        .into_iter()
        .filter(|quad| {
            quad.bounds == scaled
                && quad.border_color == cx.theme().ring
                && quad.border_widths.top > px(0.).scale(window.scale_factor())
        })
        .collect()
}

fn has_outline(window: &Window, bounds: Bounds<Pixels>, cx: &App) -> bool {
    !outline(window, bounds, cx).is_empty()
}

fn assert_complete_outline(window: &Window, bounds: Bounds<Pixels>, cx: &App) {
    use gpui_kit::{ScaledPixels, point};
    let quads = outline(window, bounds, cx);
    assert!(!quads.is_empty(), "focused target has a painted outline");
    let hairline = px(1.).scale(window.scale_factor());
    for quad in &quads {
        assert_eq!(quad.border_widths, Edges::all(hairline));
    }
    // GPUI splits border-only quads into narrow masked strips. Verify their
    // combined coverage along every edge, not just the unclipped quad bounds.
    let bounds = bounds.scale(window.scale_factor());
    let covered = |position| {
        quads
            .iter()
            .any(|quad| quad.content_mask.bounds.contains(&position))
    };
    for x in 0..bounds.size.width.0 as usize {
        let x = bounds.left() + ScaledPixels(x as f32 + 0.5);
        assert!(
            covered(point(x, bounds.top() + hairline / 2.)),
            "top edge clipped"
        );
        assert!(
            covered(point(x, bounds.bottom() - hairline / 2.)),
            "bottom edge clipped"
        );
    }
    for y in 0..bounds.size.height.0 as usize {
        let y = bounds.top() + ScaledPixels(y as f32 + 0.5);
        assert!(
            covered(point(bounds.left() + hairline / 2., y)),
            "left edge clipped"
        );
        assert!(
            covered(point(bounds.right() - hairline / 2., y)),
            "right edge clipped"
        );
    }
}
