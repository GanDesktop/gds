use super::*;
use crate::shared::lib::appearance as theme;
use bas::workspace::schema::{self, ServerDirectoryPath, schemars::JsonSchema};
use gpui_kit::{TestAppContext, size, test::TestWindowExt};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(crate = "schema::schemars")]
#[allow(dead_code)]
enum Parameters {
    Existing {
        directory: ServerDirectoryPath,
    },
    New {
        image: String,
        #[schemars(range(min = 1))]
        cpu: u32,
    },
}

#[gpui_kit::test]
fn branch_selection_renders_fields_and_invalid_parameters_stay_in_the_form(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::init_default(cx);
    });
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut panel = None;
    let window = cx.open_window(size(px(700.), px(500.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut panel = Panel {
                service: Service {
                    target: bas::service::Target::Local.into(),
                    local: false,
                    runtime: runtime.handle().clone(),
                },
                active: true,
                dialog_open: false,
                native_field: None,
                providers: vec![pb::WorkspaceProvider {
                    id: "test".into(),
                    title: "Test".into(),
                    schema_version: 1,
                    schema_json: serde_json::to_vec(&schema::generate::<Parameters>())
                        .expect("schema"),
                }],
                selected: 0,
                form: None,
                inputs: BTreeMap::new(),
                values: BTreeMap::new(),
                search: cx.new(|cx| InputState::new(window, cx)),
                picker: None,
                busy: false,
                error: None,
                opened: None,
            };
            panel.select_provider(0, window, cx);
            panel
        });
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.expect("panel");
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(panel.read(cx).inputs.contains_key("/directory"));
        window.click("branch-#-1", cx);
        window.render_frame(cx);
        assert!(!panel.read(cx).inputs.contains_key("/directory"));
        let image = panel.read(cx).inputs["/image"].clone();
        image.update(cx, |input, cx| input.set_value("image", window, cx));
        let cpu = panel.read(cx).inputs["/cpu"].clone();
        cpu.update(cx, |input, cx| input.set_value("0", window, cx));
        panel.update(cx, |panel, cx| panel.show_dialog(window, cx));
        window.render_frame(cx);
        window.click("dialog-confirm", cx);
        assert!(panel.read(cx).error.is_some());
        assert!(!panel.read(cx).busy);
        cpu.update(cx, |input, cx| input.set_value("2", window, cx));
        assert_eq!(
            panel.read(cx).parameters(true, cx).expect("valid"),
            json!({"mode":"new","image":"image","cpu":2})
        );
    })
    .expect("window");
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(crate = "schema::schemars")]
#[allow(dead_code)]
struct FolderParameters {
    directory: ServerDirectoryPath,
}

#[gpui_kit::test]
fn local_folder_requests_existing_chooser_without_a_dialog_and_cancel_finishes(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::init_default(cx);
    });
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut panel = None;
    let window = cx.open_window(size(px(700.), px(500.)), |window, cx| {
        let view = cx.new(|cx| {
            let mut panel = Panel {
                service: Service {
                    target: bas::service::Target::Local.into(),
                    local: true,
                    runtime: runtime.handle().clone(),
                },
                active: true,
                dialog_open: false,
                native_field: None,
                providers: vec![pb::WorkspaceProvider {
                    id: "any-provider-with-a-folder".into(),
                    title: "Folder".into(),
                    schema_version: 1,
                    schema_json: serde_json::to_vec(&schema::generate::<FolderParameters>())
                        .expect("schema"),
                }],
                selected: 0,
                form: None,
                inputs: BTreeMap::new(),
                values: BTreeMap::new(),
                search: cx.new(|cx| InputState::new(window, cx)),
                picker: None,
                busy: false,
                error: None,
                opened: Some(Box::new(|_, _| panic!("cancel must not open a workspace"))),
            };
            panel.select_provider(0, window, cx);
            panel
        });
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let panel = panel.expect("panel");
    let requests = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = requests.clone();
    cx.update(|cx| {
        cx.subscribe(&panel, move |_, _: &ChooseDirectory, _| {
            count.set(count.get() + 1)
        })
        .detach();
    });
    cx.update_window(window.into(), |_, window, cx| {
        panel.update(cx, |panel, cx| panel.present(window, cx));
        assert!(!panel.read(cx).dialog_open);
        assert!(panel.read(cx).choosing_directory());
        assert_eq!(cx.windows().len(), 1);
    })
    .expect("window");
    cx.run_until_parked();
    assert_eq!(requests.get(), 1);
    cx.update_window(window.into(), |_, window, cx| {
        panel.update(cx, |panel, cx| panel.directory_chosen(Ok(None), window, cx));
        assert!(!panel.read(cx).is_active());
        assert!(!panel.read(cx).busy);
        assert!(!panel.read(cx).choosing_directory());
        assert!(panel.read(cx).opened.is_none());
    })
    .expect("window");
}
