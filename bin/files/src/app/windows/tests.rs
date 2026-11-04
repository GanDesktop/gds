use gpui_kit::{TestAppContext, test::TestWindowExt};

#[gpui_kit::test]
fn closing_first_window_keeps_other_workspace_and_desktop_requests_usable(cx: &mut TestAppContext) {
    use super::*;
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace fixture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let (manager, first, second) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
        let mut preferences = configuration::Preferences::default();
        preferences.appearance.source = gds::appearance::theme::Source::Builtin;
        configuration::publish(preferences.clone(), cx);
        let manager = Windows::install(runtime.handle().clone(), directory.path().to_owned(), cx);
        let mut open = || {
            manager.update(cx, |manager, cx| {
                manager
                    .open(
                        LaunchOptions {
                            runtime: runtime.handle().clone(),
                            places: Ok(Places::default()),
                            start: directory.path().to_owned(),
                            home: directory.path().to_owned(),
                            preferences: preferences.clone(),
                            startup_error: None,
                            picker: None,
                        },
                        cx,
                    )
                    .expect("workspace window")
            })
        };
        let first = open();
        let second = open();
        (manager, first, second)
    });
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(cx.windows().len(), 2));
    cx.update_window(first.0, |_, window, _| window.remove_window())
        .expect("close first window");
    drop(first);
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(cx.windows().len(), 1);
        let mut preferences = configuration::current(cx);
        preferences.appearance.rounded_corners = true;
        configuration::publish(preferences, cx);
        #[cfg(target_os = "linux")]
        manager.update(cx, |manager, cx| {
            manager.desktop_command(
                gds::service::desktop::file_manager::Command {
                    operation: gds::service::desktop::file_manager::Operation::Folders,
                    targets: vec![],
                    startup_id: String::new(),
                },
                cx,
            )
        });
    });
    cx.run_until_parked();
    cx.update_window(second.0, |_, window, cx| {
        window.activate_window();
        window.click("application-menu", cx);
        assert!(window.try_find("New window").is_some());
        assert_eq!(cx.windows().len(), 1);
    })
    .expect("remaining window still handles commands");
}
