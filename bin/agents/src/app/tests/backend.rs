use crate::entities::backend::Profiles;
use crate::features::navigation::{current, manage};
use crate::pages::workspace::windows::{init_windows, open_for, register};
use crate::shared::config::{ConnectionProfile, Store};
use bas::service::tls::ConnectionOptions as Target;
use gpui_kit::{AppContext, BorrowAppContext, Context, TestAppContext};

#[gpui_kit::test]
fn profiles_persist_selection_without_overwriting_cli_override_or_other_sections(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().join("settings.json"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    cx.update(|cx| {
        crate::entities::agent::init(Target::default(), runtime.handle().clone(), cx);
        let profiles = cx.new(|_| Profiles::new(store.clone()));
        profiles.update(cx, |profiles, cx| {
            assert_eq!(
                profiles.startup_target(None).expect("default"),
                Target::default()
            );
            let target = profiles
                .put(
                    ConnectionProfile {
                        id: "remote".into(),
                        name: "Remote".into(),
                        note: None,
                        target: "tcp://192.0.2.1".parse().unwrap(),
                        tls: None,
                    },
                    cx,
                )
                .unwrap();
            assert_eq!(
                profiles.startup_target(None).expect("default"),
                Target::default(),
                "saving does not select"
            );
            profiles.select("remote", cx).expect("explicit selection");
            manage(target.clone(), cx);
            assert_eq!(current(cx), Some(target.clone()));
            assert_eq!(
                Profiles::new(store.clone())
                    .startup_target(None)
                    .expect("saved selection"),
                target
            );
            assert_eq!(
                profiles
                    .startup_target(Some(Target::default()))
                    .expect("explicit override"),
                Target::default()
            );
            let mut outside = store.load().unwrap();
            let appearance = outside.settings.appearance.clone();
            // A concurrent same-section change must not silently overwrite the selection.
            let mut frontend = outside.settings.frontend.clone();
            frontend.connections[0].name = "Changed externally".into();
            store.save_frontend(&mut outside, frontend).unwrap();
            assert!(profiles.select("local", cx).is_err());
            assert_eq!(current(cx), Some(target));
            profiles.reload(cx);
            let local = profiles.select("local", cx).unwrap();
            manage(local, cx);
            assert_eq!(current(cx), Some(Target::default()));
            // Other sections remain available to their own section-scoped writers.
            assert_eq!(store.load().unwrap().settings.appearance, appearance);
        });
    });
}

#[gpui_kit::test]
fn opening_a_registered_backend_window_does_not_retarget_management_or_replay_selection(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{IntoElement, Render, Window, component::Root, div, px, size};

    struct Empty;
    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    cx.update(gpui_kit::init);
    let first = cx.open_window(size(px(800.), px(600.)), |window, cx| {
        Root::new(cx.new(|_| Empty), window, cx)
    });
    let second = cx.open_window(size(px(800.), px(600.)), |window, cx| {
        Root::new(cx.new(|_| Empty), window, cx)
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let a = Target::default();
    let b: Target = bas::service::Target::Pipe("another-backend".into()).into();
    cx.update(|cx| {
        crate::entities::agent::init(b.clone(), runtime.handle().clone(), cx);
        init_windows(runtime.handle().clone(), "/unused".into(), false, cx);
        register(a.clone(), first, cx);
        register(b.clone(), second, cx);
        cx.update_global::<crate::entities::agent::Changes, _>(|changes, _| {
            changes.server = Some(b.clone());
            changes.select = Some("stale-agent-selection".into());
        });
        assert_eq!(
            open_for(a.clone(), cx).expect("reuse A").window_id(),
            first.window_id()
        );
        assert_eq!(
            current(cx),
            Some(b.clone()),
            "opening a pinned window is not management switching"
        );
        assert!(
            cx.global::<crate::entities::agent::Changes>()
                .select
                .is_none()
        );
        assert_eq!(
            open_for(b, cx).expect("reuse B").window_id(),
            second.window_id()
        );
        assert_eq!(
            open_for(a, cx).expect("A remains available").window_id(),
            first.window_id()
        );
    });
}
