use zgui::prelude::AnyView;

use crate::{
    entities::location::volumes,
    pages::workspace::chrome::tests::{click_button, run_browser_harness},
    shared::i18n,
};

#[test]
fn compact_task_button_opens_the_list_and_cancels_one_task() {
    let temp = tempfile::tempdir().expect("temporary start directory");
    run_browser_harness(
        temp.path(),
        (640.0, 480.0),
        volumes::Snapshot::default(),
        |browser| AnyView::new(browser.view()),
        |harness, browser| {
            assert_eq!(
                browser.width_tier_untracked(),
                gds::ui::window::WidthTier::Compact
            );
            let files = browser
                .begin_background_task("files", "Copying documents")
                .expect("files task");
            let trash = browser
                .begin_background_task("trash", "Emptying Trash")
                .expect("trash task");
            harness.settle(8);

            click_button(harness, &i18n::text_untracked("tasks-title"));
            click_button(harness, &i18n::text_untracked("operation-cancel"));
            assert_ne!(files.is_cancelled(), trash.is_cancelled());
        },
    );
}
