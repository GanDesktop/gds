use super::*;
use gds::app::AppExt as _;
use gpui_kit::{ScrollDelta, TestAppContext, point, px, size, test::TestWindowExt};

struct TestConfiguration {
    _directory: tempfile::TempDir,
}
impl gpui_kit::Global for TestConfiguration {}

pub(super) fn init(cx: &mut App) {
    gpui_kit::init(cx);
    cx.gds_mut().tasks().init_test(false);
    if !cx.has_global::<crate::shared::config::Configuration>() {
        let directory = tempfile::tempdir().expect("frontend settings");
        crate::shared::config::init(Some(directory.path().join("frontend.json")), cx);
        cx.set_global(TestConfiguration {
            _directory: directory,
        });
    }
    crate::shared::lib::appearance::init_default(cx);
}

fn configure(page: &mut Agents, registrations: Vec<AgentRegistration>, cx: &mut Context<Agents>) {
    page.accept_configuration(bas::service::pb::AgentConfiguration {
        profile: "remote-fixture".into(),
        revision: "revision-1".into(),
        registrations_json: serde_json::to_vec(&registrations).expect("RPC configuration"),
    })
    .expect("valid remote configuration");
    page.apply_catalog(
        bas::management::Catalog {
            entries: vec![],
            warning: None,
            settings_path: None,
            can_install: false,
        },
        cx,
    );
}

#[gpui_kit::test]
fn pending_requests_and_offline_save_preserve_the_draft_without_backend_file_access(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().expect("backend settings");
    let backend = directory.path().join("daemon.json");
    std::fs::write(&backend, b"server-owned opaque configuration").expect("backend source");
    let before = std::fs::read(&backend).expect("backend bytes");
    let spec = AgentSpec::acp("custom", Command::new("/fixture/agent", []));
    cx.update(|cx| {
        init(cx);
        crate::app::init(cx);
    });
    let mut page = None;
    let window = cx.open_window(size(px(760.), px(680.)), |window, cx| {
        let view = cx.new(|cx| Agents::new(window, cx));
        view.update(cx, |page, cx| {
            configure(
                page,
                vec![AgentSpec::embedded().into(), spec.clone().into()],
                cx,
            )
        });
        page = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let page = page.expect("page");
    cx.update_window(window.into(), |_, window, cx| {
        page.update(cx, |page, cx| {
            page.edit(spec.clone(), Some(spec.id.clone()), window, cx);
            page.form.program.update(cx, |input, cx| {
                input.set_value("/fixture/changed", window, cx)
            });
            page.configuration_task =
                Some(cx.spawn(async |_, _| std::future::pending::<()>().await));
        });
        click(window, "agents-save", cx);
        click(window, "agents-cancel", cx);
        assert!(page.read(cx).dirty(cx), "in-flight requests retain drafts");
        page.update(cx, |page, _| {
            page.configuration_task.take();
        });
        click(window, "agents-save", cx);
        assert!(
            page.read(cx).dirty(cx),
            "offline save never falls back to local persistence"
        );
        assert_eq!(std::fs::read(&backend).expect("server document"), before);
    })
    .expect("configuration guards");
}

#[gpui_kit::test]
fn remote_registration_drafts_preserve_environment_and_reject_replaced_ownership(
    cx: &mut TestAppContext,
) {
    cx.update(init);
    let mut spec = AgentSpec::acp("custom", Command::new("/fixture/agent", []));
    spec.env.insert("KEEP".into(), "original".into());
    cx.open_window(size(px(760.), px(680.)), |window, cx| {
        let page = cx.new(|cx| Agents::new(window, cx));
        page.update(cx, |page, cx| {
            configure(
                page,
                vec![AgentSpec::embedded().into(), spec.clone().into()],
                cx,
            );
            page.edit(spec.clone(), Some(spec.id.clone()), window, cx);
            page.form
                .program
                .update(cx, |input, cx| input.set_value("/fixture/new", window, cx));
            let candidate = page.candidate(cx).expect("edited registration");
            assert_eq!(
                candidate[1].as_custom().expect("custom").env["KEEP"],
                "original"
            );
            assert!(!page.can_remove(&AgentSpec::embedded().into(), cx));
            configure(
                page,
                vec![AgentRegistration::Managed(bas::config::ManagedAgent {
                    id: spec.id.clone(),
                    installation: bas::config::InstallationRef {
                        path: "/remote/bas-installation.json".into(),
                        sha256: "0".repeat(64),
                    },
                })],
                cx,
            );
            assert!(
                page.candidate(cx).is_err(),
                "a stale custom edit cannot replace a managed registration"
            );
        });
        gpui_kit::component::Root::new(page, window, cx)
    });
}

#[gpui_kit::test]
fn switching_management_preserves_backend_drafts_and_notifications(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("settings");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let first: bas::service::tls::ConnectionOptions =
        "192.0.2.1:4554".parse().expect("first target");
    let second: bas::service::tls::ConnectionOptions =
        "192.0.2.2:4554".parse().expect("second target");
    cx.update(|cx| {
        init(cx);
        crate::shared::config::init(Some(directory.path().join("settings.json")), cx);
        crate::entities::agent::init(
            first.clone(),
            gds::service::task_manager::Scope::with_runtime(&runtime, false),
            cx,
        );
    });
    cx.open_window(size(px(760.), px(680.)), |window, cx| {
        let original = shared(window, cx);
        original.update(cx, |page, cx| {
            page.form
                .id
                .update(cx, |input, cx| input.set_value("unsaved", window, cx));
            page.editor = Editor::Add(AgentSpec::acp("", Command::new("", [])));
            page.query = "original query".into();
        });
        crate::entities::agent::init(
            second.clone(),
            gds::service::task_manager::Scope::with_runtime(&runtime, false),
            cx,
        );
        let other = shared(window, cx);
        assert_ne!(original.entity_id(), other.entity_id());
        assert!(other.read(cx).query.is_empty());
        original.update(cx, |page, cx| {
            assert_eq!(page.target(), Some(first.clone()));
            assert_eq!(
                page.check_service.as_ref().expect("pinned check").address(),
                first
            );
            page.notify_agent_change(Some("ready".into()), cx);
        });
        assert_eq!(
            cx.global::<crate::entities::agent::Changes>().server,
            Some(first.clone())
        );
        crate::entities::agent::init(
            first.clone(),
            gds::service::task_manager::Scope::with_runtime(&runtime, false),
            cx,
        );
        let restored = shared(window, cx);
        assert_eq!(original.entity_id(), restored.entity_id());
        assert_eq!(
            restored.read(cx).form.id.read(cx).value().to_string(),
            "unsaved"
        );
        assert_eq!(restored.read(cx).query, "original query");
        assert!(restored.read(cx).dirty(cx));
        gpui_kit::component::Root::new(original, window, cx)
    });
}

pub(super) fn managed_fixture(directory: &std::path::Path, spec: &AgentSpec) -> AgentRegistration {
    let path = directory.join(bas::config::INSTALLATION_FILE);
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "spec": spec,
    }))
    .expect("descriptor JSON");
    std::fs::write(&path, &bytes).expect("managed descriptor");
    AgentRegistration::Managed(bas::config::ManagedAgent {
        id: spec.id.clone(),
        installation: bas::config::InstallationRef::from_bytes(path, &bytes).expect("reference"),
    })
}

/// Reach controls with native wheel events, without bypassing viewport hit testing.
fn click(window: &mut Window, id: &'static str, cx: &mut App) {
    for _ in 0..24 {
        window.render_frame(cx);
        let target = window.find(id);
        let bounds = target.bounds();
        let viewport = window.find("agents-sections").bounds();
        if target.visible()
            && ((bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom())
                || id == "agents-reload"
                || id == "agents-add-custom")
        {
            window.click(id, cx);
            return;
        }
        let delta = if bounds.top() < viewport.top() {
            180.
        } else {
            -180.
        };
        window.scroll(
            "agents-sections",
            ScrollDelta::Pixels(point(px(0.), px(delta))),
            cx,
        );
    }
    panic!("control {id} was not reachable by scrolling the settings viewport");
}
