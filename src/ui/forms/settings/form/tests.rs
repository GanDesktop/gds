use super::*;
use crate::app::AppExt as _;
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
    #[setting(label = "test-arguments", kind = "string_list")]
    arguments: Vec<String>,
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
            arguments: Vec::new(),
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
        cx.gds_mut()
            .i18n()
            .install_resources(
                crate::i18n::Language::English,
                &["test-enabled = Enabled\ntest-name = Name\ntest-count = Count\ntest-opacity = Opacity\ntest-arguments = Arguments\n"],
            )
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

#[gpui_kit::test]
fn shell_arguments_edit_as_literal_lines_and_reset(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("config-field-arguments", cx);
        window.input(
            "--private\nliteral argument with spaces\n$(echo literal)",
            cx,
        );
    })
    .expect("edit literal arguments");
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert_eq!(
            fixture.editor.read(cx).value().arguments,
            [
                "--private",
                "literal argument with spaces",
                "$(echo literal)"
            ]
        );
        window.render_frame(cx);
        window.click("config-reset-arguments", cx);
        assert!(fixture.editor.read(cx).value().arguments.is_empty());
        assert_eq!(fixture.editor.read(cx).value().preserved, ["keep"]);
    })
    .expect("reset only arguments");
}

thread_local! {
    static SLIDER_VALIDATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, PartialEq, Serialize, Deserialize, crate::ConfigEditor)]
#[config(validate = "Self::check")]
struct SliderModel {
    #[setting(label = "test-count", min = 0, max = 300, step = 1)]
    byte: u8,
    #[setting(label = "test-enabled")]
    fail_validation: bool,
}

impl Default for SliderModel {
    fn default() -> Self {
        Self {
            byte: 10,
            fail_validation: false,
        }
    }
}

impl SliderModel {
    fn check(&self) -> Result<(), &'static str> {
        SLIDER_VALIDATIONS.with(|calls| calls.set(calls.get() + 1));
        if self.fail_validation {
            Err("Validation failed")
        } else {
            Ok(())
        }
    }
}

fn slider_validations() -> usize {
    SLIDER_VALIDATIONS.with(std::cell::Cell::get)
}

#[gpui_kit::test]
fn slider_interactions_emit_one_change_and_programmatic_sync_emits_none(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds_mut()
            .i18n()
            .install_resources(
                crate::i18n::Language::English,
                &["test-enabled = Enabled\ntest-count = Count\n"],
            )
            .expect("test catalog");
    });
    SLIDER_VALIDATIONS.with(|calls| calls.set(0));
    let mut editor = None;
    let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
        let view = cx.new(|cx| Editor::new(SliderModel::default(), window, cx));
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.expect("editor");
    let changes = Rc::new(RefCell::new(Vec::new()));
    let observed = changes.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&editor, move |_, event: &Changed<SliderModel>, _| {
            observed.borrow_mut().push(event.clone())
        })
    });

    let bar = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window
                .within("config-field-byte")
                .find("slider-bar-container")
                .bounds()
        })
        .expect("slider bar bounds");

    // 1. Interactive click on the slider track fires Changed once and validates once.
    let before_changes = changes.borrow().len();
    let before_validations = slider_validations();
    cx.update_window(window.into(), |_, window, cx| {
        window.within("config-field-byte").click_at(
            "slider-bar-container",
            gpui_kit::point(bar.size.width * 0.25, bar.size.height / 2.),
            cx,
        );
    })
    .expect("track click");
    assert_eq!(changes.borrow().len() - before_changes, 1);
    assert_eq!(slider_validations() - before_validations, 1);
    let after_click = cx.update(|cx| editor.read(cx).value().byte);
    assert_ne!(after_click, 10);

    // 2. Interactive drag emits one Changed and one validation per notified drag batch.
    let before_changes = changes.borrow().len();
    let before_validations = slider_validations();
    cx.update_window(window.into(), |_, window, cx| {
        window.drag(
            gpui_kit::point(bar.origin.x + bar.size.width * 0.25, bar.center().y),
            gpui_kit::point(bar.origin.x + bar.size.width * 0.5, bar.center().y),
            cx,
        );
    })
    .expect("track drag");
    let drag_changes = changes.borrow().len() - before_changes;
    let drag_validations = slider_validations() - before_validations;
    assert!(drag_changes >= 1);
    assert_eq!(drag_changes, drag_validations);
    let after_drag = cx.update(|cx| editor.read(cx).value().byte);
    assert!(after_drag > after_click);

    // 3. Keyboard step fires Changed once and validates once.
    cx.update_window(window.into(), |_, window, cx| {
        let field_bounds = window.find("config-field-byte").bounds();
        window.click_at(
            "config-field-byte",
            gpui_kit::point(
                field_bounds.size.width - px(8.),
                field_bounds.size.height / 2.,
            ),
            cx,
        );
    })
    .expect("focus slider field");
    let before_changes = changes.borrow().len();
    let before_validations = slider_validations();
    cx.update_window(window.into(), |_, window, cx| {
        window.press("right", cx);
    })
    .expect("keyboard step");
    assert_eq!(changes.borrow().len() - before_changes, 1);
    assert_eq!(slider_validations() - before_validations, 1);
    assert_eq!(cx.update(|cx| editor.read(cx).value().byte), after_drag + 1);

    // 4. Direct SliderState::set_value fires Changed once and validates once.
    let before_changes = changes.borrow().len();
    let before_validations = slider_validations();
    cx.update_window(window.into(), |_, window, cx| {
        let state = editor.read(cx).number_control("byte").state.clone();
        state.update(cx, |state, cx| state.set_value(42., window, cx));
    })
    .expect("direct SliderState::set_value");
    assert_eq!(changes.borrow().len() - before_changes, 1);
    assert_eq!(slider_validations() - before_validations, 1);
    assert_eq!(cx.update(|cx| editor.read(cx).value().byte), 42);

    // 5. Programmatic sync (both via Editor::set_value and NumberControl::sync) fires zero events.
    let before_changes = changes.borrow().len();
    let before_validations = slider_validations();
    cx.update_window(window.into(), |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_value(
                SliderModel {
                    byte: 20,
                    fail_validation: false,
                },
                window,
                cx,
            )
        });
    })
    .expect("Editor::set_value sync");
    assert_eq!(changes.borrow().len(), before_changes);
    assert_eq!(slider_validations(), before_validations);

    cx.update_window(window.into(), |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.number_control("byte").sync(25., window, cx)
        });
    })
    .expect("NumberControl::sync");
    assert_eq!(changes.borrow().len(), before_changes);
    assert_eq!(slider_validations(), before_validations);

    // 6. Failed validation runs once (not twice) for a single slider interaction.
    cx.update_window(window.into(), |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_value(
                SliderModel {
                    byte: 20,
                    fail_validation: true,
                },
                window,
                cx,
            )
        });
    })
    .expect("arm failing validation");
    let before_changes = changes.borrow().len();
    let before_validations = slider_validations();
    cx.update_window(window.into(), |_, window, cx| {
        window.within("config-field-byte").click_at(
            "slider-bar-container",
            gpui_kit::point(bar.size.width * 0.25, bar.size.height / 2.),
            cx,
        );
    })
    .expect("failing validation click");
    assert_eq!(changes.borrow().len(), before_changes);
    assert_eq!(slider_validations() - before_validations, 1);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("config-error-byte").visible());
    })
    .expect("validation error visible");

    // 7. Narrow integer bounds catch > u8::MAX cleanly before serde decode.
    cx.update_window(window.into(), |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_value(
                SliderModel {
                    byte: 20,
                    fail_validation: false,
                },
                window,
                cx,
            )
        });
    })
    .expect("reset before range test");
    let before_changes = changes.borrow().len();
    let before_validations = slider_validations();
    cx.update_window(window.into(), |_, window, cx| {
        window.within("config-field-byte").click_at(
            "slider-bar-container",
            gpui_kit::point(bar.size.width * 0.95, bar.size.height / 2.),
            cx,
        );
    })
    .expect("out-of-range click");
    assert_eq!(changes.borrow().len(), before_changes);
    assert_eq!(slider_validations(), before_validations);
    cx.update_window(window.into(), |_, window, cx| {
        assert_eq!(editor.read(cx).value().byte, 20);
        window.render_frame(cx);
        assert!(window.find("config-error-byte").visible());
    })
    .expect("range error visible");
}

#[gpui_kit::test]
fn number_validation_checks_exact_integer_bounds_and_localizes_errors(cx: &mut TestAppContext) {
    let number = |integer| Number {
        min: 0.,
        max: 100.,
        step: 1.,
        scale: 1.,
        unit: "",
        integer,
    };

    cx.update(|cx| {
        gpui_kit::init(cx);
        for (language, expected_finite, expected_range) in [
            (
                crate::i18n::Language::English,
                "Number must be finite",
                "Number is outside the field's representable range",
            ),
            (
                crate::i18n::Language::Chinese,
                "数值必须为有限数",
                "数值超出此字段可表示的范围",
            ),
        ] {
            cx.gds_mut()
                .i18n()
                .install_resources(language, &[])
                .expect("catalog");

            assert_eq!(
                number(Integer::I8).value(-128., cx),
                Ok(Value::from(-128i8))
            );
            assert_eq!(number(Integer::I8).value(127., cx), Ok(Value::from(127i8)));
            assert_eq!(
                number(Integer::I8).value(-129., cx),
                Err(expected_range.to_owned())
            );
            assert_eq!(
                number(Integer::I8).value(128., cx),
                Err(expected_range.to_owned())
            );

            assert_eq!(
                number(Integer::I16).value(-32_768., cx),
                Ok(Value::from(-32_768i16))
            );
            assert_eq!(
                number(Integer::I16).value(32_767., cx),
                Ok(Value::from(32_767i16))
            );
            assert_eq!(
                number(Integer::I16).value(-32_769., cx),
                Err(expected_range.to_owned())
            );
            assert_eq!(
                number(Integer::I16).value(32_768., cx),
                Err(expected_range.to_owned())
            );

            assert_eq!(
                number(Integer::I32).value(i32::MIN as f32, cx),
                Ok(Value::from(i32::MIN))
            );
            assert_eq!(
                number(Integer::I32).value(2_147_483_520., cx),
                Ok(Value::from(2_147_483_520i32))
            );
            assert_eq!(
                number(Integer::I32).value(2_147_483_648., cx),
                Err(expected_range.to_owned())
            );

            assert_eq!(number(Integer::U8).value(0., cx), Ok(Value::from(0u8)));
            assert_eq!(number(Integer::U8).value(255., cx), Ok(Value::from(255u8)));
            assert_eq!(
                number(Integer::U8).value(-1., cx),
                Err(expected_range.to_owned())
            );
            assert_eq!(
                number(Integer::U8).value(256., cx),
                Err(expected_range.to_owned())
            );

            assert_eq!(
                number(Integer::U16).value(65_535., cx),
                Ok(Value::from(65_535u16))
            );
            assert_eq!(
                number(Integer::U16).value(65_536., cx),
                Err(expected_range.to_owned())
            );

            assert_eq!(
                number(Integer::U32).value(4_294_967_040., cx),
                Ok(Value::from(4_294_967_040u32))
            );
            assert_eq!(
                number(Integer::U32).value(4_294_967_296., cx),
                Err(expected_range.to_owned())
            );

            assert_eq!(
                number(Integer::I64).value(i64::MAX as f32, cx),
                Err(expected_range.to_owned())
            );
            assert_eq!(
                number(Integer::U64).value(u64::MAX as f32, cx),
                Err(expected_range.to_owned())
            );
            assert_eq!(
                number(Integer::Usize).value(-1., cx),
                Err(expected_range.to_owned())
            );
            assert_eq!(
                number(Integer::Isize).value(0., cx),
                Ok(Value::from(0isize))
            );

            for kind in [Integer::Float, Integer::U8] {
                assert_eq!(
                    number(kind).value(f32::NAN, cx),
                    Err(expected_finite.to_owned())
                );
                assert_eq!(
                    number(kind).value(f32::INFINITY, cx),
                    Err(expected_finite.to_owned())
                );
                assert_eq!(
                    number(kind).value(f32::NEG_INFINITY, cx),
                    Err(expected_finite.to_owned())
                );
            }
        }
    });
}
