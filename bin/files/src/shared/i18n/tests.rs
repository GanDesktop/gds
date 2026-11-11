use super::*;

#[test]
fn application_and_shared_catalogs_have_no_conflicting_keys() {
    let english =
        prepare(Language::English).expect("English Files and shared messages must compose");
    assert_ne!(
        english
            .text("logs-tab-tasks")
            .expect("shared log tab label"),
        "logs-tab-tasks"
    );
    assert_ne!(
        english.text("new-folder").expect("Files new-folder label"),
        "new-folder"
    );

    let chinese =
        prepare(Language::Chinese).expect("Chinese Files and shared messages must compose");
    assert_ne!(
        chinese
            .text("logs-tab-tasks")
            .expect("shared log tab label"),
        "logs-tab-tasks"
    );
    assert_ne!(
        chinese.text("new-folder").expect("Files new-folder label"),
        "new-folder"
    );
}
