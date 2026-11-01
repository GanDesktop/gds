use super::*;
use gpui_kit::{
    AppContext, Entity, TestAppContext, WindowHandle, component::Root, px, size,
    test::TestWindowExt,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, PartialEq, Serialize, Deserialize, crate::ConfigEditor)]
#[config(validate = "Self::check")]
struct Preferences {
    #[setting(label = "test-enabled")]
    enabled: bool,
    #[setting(label = "test-name")]
    name: String,
    #[setting(label = "test-count", min = 0, max = 8)]
    count: u32,
    #[setting(
        label = "test-opacity",
        min = 0.,
        max = 1.,
        step = 0.01,
        scale = 100.,
        unit = "%",
        default = 0.85
    )]
    opacity: Option<f32>,
    #[setting(label = "form-language")]
    language: Option<crate::i18n::Language>,
    #[setting(label = "form-font-family", kind = "font")]
    family: Option<String>,
    preserved: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: false,
            name: "ready".into(),
            count: 2,
            opacity: None,
            language: None,
            family: None,
            preserved: vec!["keep".into()],
        }
    }
}

impl Preferences {
    fn check(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            Err("Name is required")
        } else {
            Ok(())
        }
    }
}

struct Fixture {
    editor: Entity<PreferencesEditor>,
    window: WindowHandle<Root>,
    changes: Rc<RefCell<Vec<Changed<Preferences>>>>,
    _subscription: gpui_kit::Subscription,
}

fn fixture(cx: &mut TestAppContext) -> Fixture {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::i18n::install_resources(crate::i18n::Language::English,
            &["test-enabled = Enabled\ntest-name = Name\ntest-count = Count\ntest-opacity = Opacity\n"], cx)
            .expect("test catalog");
    });
    let mut editor = None;
    let window = cx.open_window(size(px(900.), px(850.)), |window, cx| {
        let view = cx.new(|cx| Editor::new(Preferences::default(), window, cx));
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.expect("editor");
    let changes = Rc::new(RefCell::new(Vec::new()));
    let observed = changes.clone();
    let subscription = cx.update(|cx| {
        cx.subscribe(&editor, move |_, event: &Changed<Preferences>, _| {
            observed.borrow_mut().push(event.clone())
        })
    });
    Fixture {
        editor,
        window,
        changes,
        _subscription: subscription,
    }
}

#[gpui_kit::test]
fn generated_controls_validate_and_reset_only_the_edited_field(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("config-field-enabled", cx);
        assert!(fixture.editor.read(cx).value().enabled);
        assert_eq!(fixture.editor.read(cx).value().preserved, ["keep"]);
        window.click("config-field-name", cx);
    })
    .expect("focus text input");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.dispatch_action(Box::new(gpui_kit::component::input::SelectAll), cx);
    })
    .expect("select input contents");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.press("backspace", cx);
    })
    .expect("edit input");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(fixture.editor.read(cx).value().name, "ready");
        window.render_frame(cx);
        assert!(window.find("config-error-name").visible());
        window.input("renamed", cx);
    })
    .expect("correct invalid input");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(fixture.editor.read(cx).value().name, "renamed");
        window.click("config-reset-enabled", cx);
        assert!(!fixture.editor.read(cx).value().enabled);
        assert_eq!(fixture.editor.read(cx).value().name, "renamed");
    })
    .expect("generated field workflow");
    assert_eq!(
        fixture
            .changes
            .borrow()
            .iter()
            .map(|change| change.field)
            .collect::<Vec<_>>(),
        ["enabled", "name", "enabled"]
    );
}

#[gpui_kit::test]
fn generated_numbers_preserve_integer_types_and_optional_defaults(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(fixture.editor.read(cx).value().opacity, None);
        window.click("config-field-count", cx);
        window.press("end", cx);
    })
    .expect("edit integer");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(fixture.editor.read(cx).value().count, 8);
        window.click("config-field-opacity", cx);
        window.press("end", cx);
    })
    .expect("edit inherited number");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(fixture.editor.read(cx).value().opacity, Some(1.));
        window.click("config-reset-opacity", cx);
        assert_eq!(fixture.editor.read(cx).value().opacity, None);
        assert_eq!(fixture.editor.read(cx).value().count, 8);
        let accepted = fixture.editor.read(cx).value().clone();
        let before = fixture.changes.borrow().len();
        fixture
            .editor
            .update(cx, |editor, cx| editor.set_value(accepted, window, cx));
        window.render_frame(cx);
        assert_eq!(
            fixture.changes.borrow().len(),
            before,
            "programmatic synchronization cannot submit changes"
        );
    })
    .expect("generated number workflow");
}

#[gpui_kit::test]
fn common_catalogs_preserve_system_choices_and_uninstalled_fonts(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    cx.update(|cx| {
        let fields = Preferences::fields();
        let mut value = Preferences::default();
        value.family = Some("An unavailable custom font".into());
        let fonts = fields
            .iter()
            .find(|field| field.id == "family")
            .expect("font metadata")
            .choices(&value, cx);
        assert!(fonts.iter().any(|choice| choice.value == Value::Null));
        assert!(
            fonts
                .iter()
                .any(|choice| choice.value == encode(&value.family))
        );
        let languages = fields
            .iter()
            .find(|field| field.id == "language")
            .expect("language metadata")
            .choices(&value, cx);
        for language in [
            None,
            Some(crate::i18n::Language::English),
            Some(crate::i18n::Language::Chinese),
        ] {
            assert!(
                languages
                    .iter()
                    .any(|choice| choice.value == encode(&language))
            );
        }
        assert!(fixture.editor.read(cx).value().language.is_none());
    });
}
