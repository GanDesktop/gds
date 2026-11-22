use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use zgui::prelude::*;
use zgui::reactive::{Mounted, RwSignal, install};

use super::editor::field_matches_query;
use super::*;
use crate::app::Gds;
use crate::i18n::Language;
use crate::service::task_manager::Scope;
use crate::ui::forms::field::placeholder::PlaceholderProps;
use crate::ui::forms::field::{
    FieldLabelProps, InputFieldProps, SearchChoiceFieldProps, SearchOption, TextareaFieldProps,
};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, crate::ConfigEditor,
)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    #[setting(label = "form-default")]
    Auto,
    #[setting(label = "form-time-absolute")]
    Manual,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, crate::ConfigEditor)]
#[serde(default)]
#[config(validate = "Self::validate")]
struct SampleSettings {
    #[setting(label = "form-language")]
    enabled: bool,
    #[setting(label = "form-time-format")]
    mode: Mode,
    #[setting(label = "form-font-size", min = 10, max = 30, step = 2, unit = " px")]
    count: u32,
}

impl SampleSettings {
    fn validate(&self) -> Result<(), String> {
        if self.count > 30 {
            return Err("too large".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, crate::ConfigEditor)]
#[serde(default)]
#[config(validate = "Self::validate")]
struct RichSettings {
    #[setting(group = "form-font-family", label = "form-font-family", kind = "font")]
    font_family: Option<String>,
    #[setting(
        group = "form-font-family",
        label = "form-font-size",
        min = 8.0,
        max = 32.0,
        step = 0.5,
        unit = " px"
    )]
    font_size: f32,
    #[setting(group = "form-language", label = "form-language")]
    language: Language,
    #[setting(group = "form-language", label = "form-time-format")]
    mode_opt: Option<Mode>,
    #[setting(label = "form-search")]
    query_note: Option<String>,
    #[setting(label = "form-reset", kind = "string_list")]
    args: Vec<String>,
    #[setting(label = "form-default")]
    enabled: bool,
}

impl Default for RichSettings {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 14.0,
            language: Language::English,
            mode_opt: None,
            query_note: None,
            args: vec!["--flag".to_owned()],
            enabled: true,
        }
    }
}

impl RichSettings {
    fn validate(&self) -> Result<(), String> {
        if self.font_size < 8.0 {
            return Err("font_size too small".into());
        }
        Ok(())
    }
}

#[test]
fn derived_settings_validate_and_localize_choices() {
    install().ok();
    let root = Mounted::new();
    root.with(|| {
        let gds = Gds::install(Scope::default());
        gds.i18n().install(Language::English).expect("en-US");
        let fields = SampleSettings::fields();
        assert_eq!(fields.len(), 3);
        let expected_choices = || {
            [
                (json!("auto"), gds.i18n().text_untracked("form-default")),
                (
                    json!("manual"),
                    gds.i18n().text_untracked("form-time-absolute"),
                ),
            ]
        };
        let choices = || {
            Mode::choices()
                .into_iter()
                .map(|choice| (choice.value, choice.title))
                .collect::<Vec<_>>()
        };
        assert_eq!(choices(), expected_choices());
        gds.i18n().install(Language::Chinese).expect("zh-CN");
        assert_eq!(choices(), expected_choices());
        assert!(
            SampleSettings {
                enabled: true,
                mode: Mode::Manual,
                count: 16
            }
            .validate()
            .is_ok()
        );
        assert!(
            SampleSettings {
                enabled: true,
                mode: Mode::Manual,
                count: 40
            }
            .validate()
            .is_err()
        );

        let sig = RwSignal::new_local(SampleSettings::default());
        let observed: Rc<RefCell<Vec<SampleSettings>>> = Rc::new(RefCell::new(Vec::new()));
        let observed_cb = Rc::clone(&observed);
        let filter = RwSignal::new_local(String::new());
        let focus_target = NodeRef::new();
        let _editor = SampleSettingsEditor::new(sig)
            .disabled(false)
            .filter(filter)
            .reset_focus(focus_target)
            .on_change(move |changed| observed_cb.borrow_mut().push(changed.value))
            .view();
        assert_eq!(sig.get_untracked(), SampleSettings::default());
        assert!(observed.borrow().is_empty());
    });
    root.unmount();
}

#[test]
fn rich_settings_editor_builds_all_control_kinds_and_supports_filtering() {
    install().ok();
    let root = Mounted::new();
    root.with(|| {
        let gds = Gds::install(Scope::default());
        gds.i18n().install(Language::English).expect("en-US");

        let fields = RichSettings::fields();
        assert_eq!(fields.len(), 7);

        let sig = RwSignal::new_local(RichSettings::default());
        let disabled = RwSignal::new_local(false);
        let filter = RwSignal::new_local(String::new());

        let _editor = RichSettingsEditor::new(sig)
            .disabled(disabled)
            .filter(filter)
            .view();

        let font_field = fields
            .iter()
            .find(|f| f.id == "font_family")
            .expect("font_family field");
        assert!(field_matches_query(font_field, ""));
        assert!(field_matches_query(font_field, "font"));
        assert!(field_matches_query(font_field, "font_family"));
        assert!(!field_matches_query(font_field, "language"));

        // Missing custom font appears in choices formatted via i18n `form-font-missing`.
        let custom_font = RichSettings {
            font_family: Some("Missing Custom Sans".to_owned()),
            ..RichSettings::default()
        };
        let font_choices = font_field.choices(&custom_font);
        assert_eq!(font_choices[0].value, Value::Null);
        assert_eq!(font_choices[0].title, "System default");
        assert!(
            font_choices
                .iter()
                .any(|c| c.title == "Missing Custom Sans (not installed)")
        );

        // Language choices carry search terms matching locale codes & native names.
        let lang_field = fields
            .iter()
            .find(|f| f.id == "language")
            .expect("language field");
        let lang_choices = lang_field.choices(&custom_font);
        assert!(lang_choices.iter().any(|c| !c.terms().is_empty()));

        // Standalone SearchOption & field wrappers match hidden keywords cleanly.
        let zh_opt = SearchOption::new("chinese", "简体中文").with_keywords(["zh-CN", "Chinese"]);
        assert!(zh_opt.matches_query(""));
        assert!(zh_opt.matches_query("zh-cn"));
        assert!(zh_opt.matches_query("chinese 简体"));
        assert!(!zh_opt.matches_query("french"));

        let choice_sig = RwSignal::new_local("chinese".to_owned());
        let opts_sig = RwSignal::new_local(vec![zh_opt]);
        let _fields_view = view! {
            column {
                FieldLabel { "Search" }
                SearchChoiceField(value = choice_sig, options = opts_sig)
                InputField(placeholder = "Name")
                TextareaField(placeholder = "Args")
                Placeholder(visible = true, text = "Empty")
            }
        };

        filter.set("language".to_owned());
        disabled.set(true);
    });
    root.unmount();
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize, crate::ConfigEditor)]
struct NumericSettings {
    #[setting(label = "signed8")]
    signed8: i8,
    #[setting(label = "signed16")]
    signed16: i16,
    #[setting(label = "signed32")]
    signed32: i32,
    #[setting(label = "signed64")]
    signed64: i64,
    #[setting(label = "signed-word")]
    signed_word: isize,
    #[setting(label = "unsigned8")]
    unsigned8: Option<u8>,
    #[setting(label = "unsigned16")]
    unsigned16: u16,
    #[setting(label = "unsigned32")]
    unsigned32: u32,
    #[setting(label = "unsigned64")]
    unsigned64: u64,
    #[setting(label = "unsigned-word")]
    unsigned_word: usize,
    #[setting(
        label = "fraction",
        min = 0,
        max = 1,
        step = 0.1,
        scale = 100,
        unit = "%"
    )]
    fraction: f32,
}

#[test]
fn derived_numeric_fields_enforce_their_types_range_and_localize_errors() {
    install().ok();
    let root = Mounted::new();
    root.with(|| {
        let gds = Gds::install(Scope::default());
        gds.i18n().install(Language::English).expect("en-US");

        let fields = NumericSettings::fields();
        let number = |id: &str| {
            let field = fields
                .iter()
                .find(|field| field.id == id)
                .expect("derived field");
            let Kind::Number(number) = field.kind else {
                panic!("{id} requires a numeric editor")
            };
            number
        };
        let check_range = |id: &str, min, max, overflow| {
            let number = number(id);
            assert!(number.value(min).is_ok(), "{id} minimum");
            assert!(number.value(max).is_ok(), "{id} maximum");
            assert_eq!(
                number.value(overflow),
                Err(gds.i18n().text_untracked("form-number-out-of-range")),
                "{id} overflow"
            );
        };
        check_range("signed8", -128.0, 127.0, 128.0);
        check_range("signed16", -32768.0, 32767.0, 32768.0);
        check_range(
            "signed32",
            i32::MIN.into(),
            i32::MAX.into(),
            f64::from(i32::MAX) + 1.0,
        );
        check_range("signed64", i64::MIN as f64, (1_i64 << 62) as f64, 1e25);
        check_range(
            "signed_word",
            isize::MIN as f64,
            (isize::MAX / 2) as f64,
            1e25,
        );
        check_range("unsigned8", 0.0, 255.0, 256.0);
        check_range("unsigned16", 0.0, 65535.0, 65536.0);
        check_range(
            "unsigned32",
            0.0,
            u32::MAX.into(),
            f64::from(u32::MAX) + 1.0,
        );
        check_range("unsigned64", 0.0, (1_u64 << 63) as f64, -1.0);
        check_range("unsigned_word", 0.0, (usize::MAX / 2) as f64, -1.0);

        let float_num = number("fraction");
        assert_eq!(float_num.value(50.0), Ok(json!(0.5)));
        assert_eq!(float_num.label(50.0), "50%");
        assert!(float_num.value(f64::INFINITY).is_err());

        gds.i18n()
            .install(Language::Chinese)
            .expect("switch to Chinese");
        let u8_num = number("unsigned8");
        assert_eq!(
            u8_num.value(300.0),
            Err(gds.i18n().text_untracked("form-number-out-of-range"))
        );
        assert_eq!(
            u8_num.value(f64::NAN),
            Err(gds.i18n().text_untracked("form-number-not-finite"))
        );
    });
    root.unmount();
}

#[test]
fn literal_string_list_lines_preserve_whitespace_and_modified_tracking() {
    let mut model = RichSettings::default();
    let defaults = RichSettings::default();
    let fields = RichSettings::fields();
    let args_field = fields.iter().find(|f| f.id == "args").expect("args field");
    assert!(!args_field.is_modified(&model, &defaults));

    let literal_text = "  leading and trailing  \n\n--opt\n";
    let list: Vec<Value> = literal_text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| Value::String(line.to_owned()))
        .collect();
    (args_field.set)(&mut model, Value::Array(list)).expect("set args");
    assert_eq!(
        model.args,
        vec!["  leading and trailing  ".to_owned(), "--opt".to_owned()]
    );
    assert!(args_field.is_modified(&model, &defaults));
}

#[test]
fn headless_settings_editor_wires_labelled_by_reset_and_filter() {
    use std::collections::HashMap;
    use std::sync::Arc;

    use zgui::platform::{AppHandler, PlatformError, Surface};
    use zgui::runtime::AppError;
    use zgui_platform_headless::Harness;

    #[derive(Debug, Default)]
    struct HeadlessSink {
        atlas: zgui::atlas::MemorySink,
        target: Option<zgui::render::RenderTarget>,
    }

    impl zgui::render::Renderer for HeadlessSink {
        fn capabilities(&self) -> zgui::render::RenderCapabilities {
            zgui::render::RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: zgui::render::RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<zgui::render::RenderTarget> {
            self.target
        }

        fn register_external(
            &mut self,
            _texture: zgui::render::ExternalTexture,
        ) -> zgui::render::TextureHandle {
            zgui::render::TextureHandle(0)
        }

        fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}

        fn memory(&self) -> zgui::render::MemoryReport {
            zgui::render::MemoryReport::default()
        }

        fn draw(
            &mut self,
            _scene: &zgui::scene::Scene,
            _damage: &zgui::bits::DamageSet,
        ) -> zgui::render::FrameOutcome {
            zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
        }

        fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
            &mut self.atlas
        }
    }

    let sig_slot: Rc<RefCell<Option<RwSignal<SampleSettings, LocalStorage>>>> =
        Rc::new(RefCell::new(None));
    let filter_slot: Rc<RefCell<Option<RwSignal<String, LocalStorage>>>> =
        Rc::new(RefCell::new(None));
    let changes_log: Rc<RefCell<Vec<Changed<SampleSettings>>>> = Rc::new(RefCell::new(Vec::new()));
    let sig_slot_view = Rc::clone(&sig_slot);
    let filter_slot_view = Rc::clone(&filter_slot);
    let changes_view = Rc::clone(&changes_log);
    let sig_slot_driver = Rc::clone(&sig_slot);
    let filter_slot_driver = Rc::clone(&filter_slot);
    let changes_driver = Rc::clone(&changes_log);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(zgui::platform::SurfaceEvent::Resized(
            zgui::geom::Size::new(zgui::geom::DevicePx(640.0), zgui::geom::DevicePx(480.0)),
        ));
        if let Some(surface) = harness.platform().offscreens().first() {
            surface.set_a11y_listener(true);
        }
        harness.settle(8);

        let offscreen = harness.platform().offscreens()[0].clone();
        let mut nodes_by_id = HashMap::new();
        let mut root_id = None;
        for update in offscreen.a11y_log() {
            if let Some(tree) = update.tree {
                root_id = Some(tree.root);
            }
            for (id, node) in update.nodes {
                nodes_by_id.insert(id, node);
            }
        }

        let compute_window_rects = || {
            let mut rects = HashMap::new();
            let Some(r_id) = root_id else {
                return rects;
            };
            let Some(r_node) = nodes_by_id.get(&r_id) else {
                return rects;
            };
            let mut stack: Vec<(_, (f64, f64))> = r_node
                .children()
                .iter()
                .map(|&child| (child, (0.0, 0.0)))
                .collect();
            while let Some((id, (ox, oy))) = stack.pop() {
                let Some(node) = nodes_by_id.get(&id) else {
                    continue;
                };
                let (nx, ny) = match node.transform() {
                    Some(affine) => {
                        let c = affine.as_coeffs();
                        (ox + c[4], oy + c[5])
                    }
                    None => (ox, oy),
                };
                if let Some(b) = node.bounds() {
                    rects.insert(
                        id,
                        (
                            (nx + b.x0) as f32,
                            (ny + b.y0) as f32,
                            (nx + b.x1) as f32,
                            (ny + b.y1) as f32,
                        ),
                    );
                }
                for &child in node.children() {
                    stack.push((child, (nx, ny)));
                }
            }
            rects
        };

        let initial_rects = compute_window_rects();
        let (&switch_id, switch_node) = nodes_by_id
            .iter()
            .find(|(_, n)| format!("{:?}", n.role()) == "Switch")
            .expect("Switch rendered in Editor");
        let initial_switch_bounds = initial_rects
            .get(&switch_id)
            .copied()
            .expect("Switch window bounds");
        assert!(
            !switch_node.labelled_by().is_empty(),
            "Switch must have labelled_by pointing to FieldLabel"
        );
        let switch_label_text = switch_node
            .labelled_by()
            .iter()
            .filter_map(|id| nodes_by_id.get(id))
            .find_map(|n| n.value().or_else(|| n.label()));
        assert_eq!(switch_label_text, Some("Language"));

        let slider_node = nodes_by_id
            .values()
            .find(|n| format!("{:?}", n.role()) == "Slider")
            .expect("Slider rendered in Editor");
        assert!(
            !slider_node.labelled_by().is_empty(),
            "Slider must have labelled_by pointing to FieldLabel"
        );

        // Click Switch to toggle `enabled` -> verifies `Changed.field == "enabled"`.
        let switch_cx = (initial_switch_bounds.0 + initial_switch_bounds.2) * 0.5;
        let switch_cy = (initial_switch_bounds.1 + initial_switch_bounds.3) * 0.5;
        let click_pt =
            zgui::geom::Point::new(zgui::geom::CssPx(switch_cx), zgui::geom::CssPx(switch_cy));
        for action in [
            zgui::vocab::PointerAction::Pressed,
            zgui::vocab::PointerAction::Released,
        ] {
            harness.deliver_to_first(zgui::platform::SurfaceEvent::Pointer {
                action,
                event: zgui::vocab::PointerEvent::mouse(click_pt)
                    .with_button(zgui::vocab::PointerButton::Primary),
                modifiers: zgui::vocab::Modifiers::NONE,
                timestamp: zgui::vocab::Timestamp::ORIGIN,
            });
        }
        harness.settle(8);

        assert_eq!(changes_driver.borrow().len(), 1);
        assert_eq!(changes_driver.borrow()[0].field, "enabled");
        assert!(changes_driver.borrow()[0].value.enabled);

        // Collect updated a11y nodes and verify `[SettingReset, SettingControl]` order & stability.
        for update in offscreen.a11y_log() {
            if let Some(tree) = update.tree {
                root_id = Some(tree.root);
            }
            for (id, node) in update.nodes {
                nodes_by_id.insert(id, node);
            }
        }
        let mut updated_rects = HashMap::new();
        if let Some(r_id) = root_id
            && let Some(r_node) = nodes_by_id.get(&r_id)
        {
            let mut stack: Vec<(_, (f64, f64))> = r_node
                .children()
                .iter()
                .map(|&child| (child, (0.0, 0.0)))
                .collect();
            while let Some((id, (ox, oy))) = stack.pop() {
                let Some(node) = nodes_by_id.get(&id) else {
                    continue;
                };
                let (nx, ny) = match node.transform() {
                    Some(affine) => {
                        let c = affine.as_coeffs();
                        (ox + c[4], oy + c[5])
                    }
                    None => (ox, oy),
                };
                if let Some(b) = node.bounds() {
                    updated_rects.insert(
                        id,
                        (
                            (nx + b.x0) as f32,
                            (ny + b.y0) as f32,
                            (nx + b.x1) as f32,
                            (ny + b.y1) as f32,
                        ),
                    );
                }
                for &child in node.children() {
                    stack.push((child, (nx, ny)));
                }
            }
        }
        let (&reset_id, _) = nodes_by_id
            .iter()
            .find(|(_, n)| n.label() == Some("Reset to default"))
            .expect("expected Reset to default IconButton after modifying setting");
        let reset_bounds = updated_rects
            .get(&reset_id)
            .copied()
            .expect("Reset window bounds");
        let updated_switch_bounds = updated_rects
            .get(&switch_id)
            .copied()
            .expect("updated Switch window bounds");
        assert!(
            reset_bounds.2 <= updated_switch_bounds.0 + 1.0,
            "SettingReset ({reset_bounds:?}) must sit horizontally BEFORE SettingControl ({updated_switch_bounds:?})"
        );
        assert!(
            (updated_switch_bounds.0 - initial_switch_bounds.0).abs() < 0.5,
            "SettingReset 32x32 slot must reserve fixed width so SettingControl never shifts"
        );

        // Click Reset to default -> restores default and emits `Changed { field: "enabled", .. }`.
        let reset_pt = zgui::geom::Point::new(
            zgui::geom::CssPx((reset_bounds.0 + reset_bounds.2) * 0.5),
            zgui::geom::CssPx((reset_bounds.1 + reset_bounds.3) * 0.5),
        );
        for action in [
            zgui::vocab::PointerAction::Pressed,
            zgui::vocab::PointerAction::Released,
        ] {
            harness.deliver_to_first(zgui::platform::SurfaceEvent::Pointer {
                action,
                event: zgui::vocab::PointerEvent::mouse(reset_pt)
                    .with_button(zgui::vocab::PointerButton::Primary),
                modifiers: zgui::vocab::Modifiers::NONE,
                timestamp: zgui::vocab::Timestamp::ORIGIN,
            });
        }
        harness.settle(8);
        assert_eq!(changes_driver.borrow().len(), 2);
        assert_eq!(changes_driver.borrow()[1].field, "enabled");
        assert!(!changes_driver.borrow()[1].value.enabled);
        let _ = sig_slot_driver.borrow().expect("model signal");

        // Search filtering keeps matching row while removing non-matching ones.
        let filter = filter_slot_driver.borrow().expect("filter signal");
        filter.set("language".to_owned());
        harness.settle(4);
        let last = offscreen.last_a11y_update().expect("filter update");
        assert!(
            !last
                .nodes
                .iter()
                .any(|(_, n)| format!("{:?}", n.role()) == "Slider"),
            "non-matching Font size Slider should not remain in filtered update"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_title("settings-editor-test")
        .with_size(640.0, 480.0)
        .with_renderer(Box::new(|_surface: &Arc<dyn Surface>, target| {
            let mut renderer = HeadlessSink::default();
            zgui::render::Renderer::configure(&mut renderer, target);
            Ok::<_, AppError>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
        }))
        .run_on(driver, move || {
            let gds = Gds::try_current().unwrap_or_else(|| Gds::install(Scope::default()));
            gds.i18n().install(Language::English).expect("en-US");
            let sig = RwSignal::new_local(SampleSettings::default());
            let filter = RwSignal::new_local(String::new());
            let log = Rc::clone(&changes_view);
            *sig_slot_view.borrow_mut() = Some(sig);
            *filter_slot_view.borrow_mut() = Some(filter);
            SampleSettingsEditor::new(sig)
                .filter(filter)
                .on_change(move |changed| log.borrow_mut().push(changed))
                .view()
        })
        .expect("headless settings editor test ran");
}

#[test]
fn headless_input_field_supports_leading_icon_compact_height_and_clear_button() {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::Arc;

    use zgui::geom::{CssPx, DevicePx, Point, Size};
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::reactive::UnsyncCallback;
    use zgui::runtime::AppError;
    use zgui::vocab::{Modifiers, PointerAction, PointerButton, PointerEvent, Timestamp};
    use zgui_platform_headless::Harness;

    use crate::ui::icon::IconName;

    #[derive(Default)]
    struct Sink {
        atlas: zgui::atlas::MemorySink,
        target: Option<zgui::render::RenderTarget>,
    }

    impl zgui::render::Renderer for Sink {
        fn capabilities(&self) -> zgui::render::RenderCapabilities {
            zgui::render::RenderCapabilities::MINIMAL
        }
        fn configure(&mut self, target: zgui::render::RenderTarget) {
            self.target = Some(target);
        }
        fn target(&self) -> Option<zgui::render::RenderTarget> {
            self.target
        }
        fn register_external(
            &mut self,
            _: zgui::render::ExternalTexture,
        ) -> zgui::render::TextureHandle {
            zgui::render::TextureHandle(0)
        }
        fn release_external(&mut self, _: zgui::render::TextureHandle) {}
        fn memory(&self) -> zgui::render::MemoryReport {
            zgui::render::MemoryReport::default()
        }
        fn draw(
            &mut self,
            _: &zgui::scene::Scene,
            _: &zgui::bits::DamageSet,
        ) -> zgui::render::FrameOutcome {
            zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
        }
        fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
            &mut self.atlas
        }
    }

    let query_slot: Rc<RefCell<Option<RwSignal<String, LocalStorage>>>> =
        Rc::new(RefCell::new(None));
    let compact_ref: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
    let regular_ref: Rc<RefCell<Option<NodeRef>>> = Rc::new(RefCell::new(None));
    let cleared = Rc::new(Cell::new(0_usize));

    let query_view = Rc::clone(&query_slot);
    let compact_view = Rc::clone(&compact_ref);
    let regular_view = Rc::clone(&regular_ref);
    let cleared_view = Rc::clone(&cleared);

    let query_driver = Rc::clone(&query_slot);
    let compact_driver = Rc::clone(&compact_ref);
    let regular_driver = Rc::clone(&regular_ref);
    let cleared_driver = Rc::clone(&cleared);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(320.0),
            DevicePx(160.0),
        )));
        if let Some(surface) = harness.platform().offscreens().first() {
            surface.set_a11y_listener(true);
        }
        harness.settle(8);

        let compact_node = compact_driver.borrow().expect("compact node");
        let regular_node = regular_driver.borrow().expect("regular node");
        let compact_h = compact_node
            .window_bounds()
            .expect("compact bounds")
            .size
            .height
            .0
            / compact_node.scale().max(1.0);
        let regular_h = regular_node
            .window_bounds()
            .expect("regular bounds")
            .size
            .height
            .0
            / regular_node.scale().max(1.0);
        assert!(
            (compact_h - 30.0).abs() <= 2.5 && regular_h > compact_h + 2.0,
            "compact InputField (32px pill) should be shorter than regular (36px pill): compact={compact_h}, regular={regular_h}"
        );

        let offscreen = harness.platform().offscreens()[0].clone();
        let query = query_driver.borrow().expect("query signal");

        // Initially empty: Clear button is hidden, while capture initial a11y tree.
        let mut nodes = HashMap::new();
        let mut root_id = None;
        for update in offscreen.take_a11y_log() {
            if let Some(tree) = update.tree {
                root_id = Some(tree.root);
            }
            for (id, node) in update.nodes {
                nodes.insert(id, node);
            }
        }
        assert!(
            !nodes.values().any(|n| n.label() == Some("Clear")),
            "Clear button must stay hidden when InputField is empty"
        );

        // Populate query -> Clear button appears inside the pill.
        query.set("documents".to_owned());
        harness.settle(8);
        for update in offscreen.take_a11y_log() {
            if let Some(tree) = update.tree {
                root_id = Some(tree.root);
            }
            for (id, node) in update.nodes {
                nodes.insert(id, node);
            }
        }
        let r_id = root_id.expect("a11y root");
        let r_node = nodes.get(&r_id).expect("root node");
        let mut stack: Vec<(_, (f64, f64))> =
            r_node.children().iter().map(|&c| (c, (0.0, 0.0))).collect();
        let mut clear_rect = None;
        while let Some((id, (ox, oy))) = stack.pop() {
            let Some(node) = nodes.get(&id) else {
                continue;
            };
            let (nx, ny) = match node.transform() {
                Some(affine) => {
                    let c = affine.as_coeffs();
                    (ox + c[4], oy + c[5])
                }
                None => (ox, oy),
            };
            if node.label() == Some("Clear")
                && let Some(b) = node.bounds()
            {
                clear_rect = Some((
                    (nx + b.x0) as f32,
                    (ny + b.y0) as f32,
                    (nx + b.x1) as f32,
                    (ny + b.y1) as f32,
                ));
            }
            for &child in node.children() {
                stack.push((child, (nx, ny)));
            }
        }
        let clear_bounds = clear_rect.expect("Clear button mounted when query is non-empty");

        let pt = Point::new(
            CssPx((clear_bounds.0 + clear_bounds.2) * 0.5),
            CssPx((clear_bounds.1 + clear_bounds.3) * 0.5),
        );
        for action in [PointerAction::Pressed, PointerAction::Released] {
            harness.deliver_to_first(SurfaceEvent::Pointer {
                action,
                event: PointerEvent::mouse(pt).with_button(PointerButton::Primary),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
        }
        harness.settle(8);

        assert_eq!(query.get_untracked(), "");
        assert_eq!(cleared_driver.get(), 1);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_title("input-field-pill-test")
        .with_size(320.0, 160.0)
        .with_renderer(Box::new(|_: &Arc<dyn Surface>, target| {
            let mut sink = Sink::default();
            zgui::render::Renderer::configure(&mut sink, target);
            Ok::<_, AppError>(Box::new(sink) as Box<dyn zgui::render::Renderer>)
        }))
        .run_on(driver, move || {
            let gds = Gds::try_current().unwrap_or_else(|| Gds::install(Scope::default()));
            gds.i18n().install(Language::English).expect("en-US");
            let query = RwSignal::new_local(String::new());
            let c_ref = NodeRef::new();
            let r_ref = NodeRef::new();
            *query_view.borrow_mut() = Some(query);
            *compact_view.borrow_mut() = Some(c_ref);
            *regular_view.borrow_mut() = Some(r_ref);
            let cleared_cb = Rc::clone(&cleared_view);
            view! {
                column(style:width = "260px", style:gap = "8px") {
                    InputField(
                        value = query,
                        placeholder = "Filter this folder…",
                        leading_icon = IconName::Search,
                        clearable = true,
                        compact = true,
                        node_ref = c_ref,
                        on_clear = UnsyncCallback::new(move |()| {
                            cleared_cb.set(cleared_cb.get() + 1);
                        }),
                    )
                    InputField(
                        placeholder = "Regular field",
                        node_ref = r_ref,
                    )
                }
            }
        })
        .expect("headless InputField test ran");
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, crate::ConfigEditor)]
#[serde(default)]
#[config(validate = "Self::validate")]
struct ValidatedTextSettings {
    #[setting(label = "form-search")]
    text: String,
}

impl ValidatedTextSettings {
    fn validate(&self) -> Result<(), String> {
        if self.text == "bad" {
            return Err("bad_setting_value".to_owned());
        }
        Ok(())
    }
}

#[test]
fn headless_settings_editor_clears_validation_error_on_external_model_update() {
    use std::collections::HashMap;
    use std::sync::Arc;

    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::runtime::AppError;
    use zgui::vocab::{KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, Timestamp};
    use zgui_platform_headless::Harness;

    #[derive(Debug, Default)]
    struct HeadlessSink {
        atlas: zgui::atlas::MemorySink,
        target: Option<zgui::render::RenderTarget>,
    }

    impl zgui::render::Renderer for HeadlessSink {
        fn capabilities(&self) -> zgui::render::RenderCapabilities {
            zgui::render::RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: zgui::render::RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<zgui::render::RenderTarget> {
            self.target
        }

        fn register_external(
            &mut self,
            _texture: zgui::render::ExternalTexture,
        ) -> zgui::render::TextureHandle {
            zgui::render::TextureHandle(0)
        }

        fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}

        fn memory(&self) -> zgui::render::MemoryReport {
            zgui::render::MemoryReport::default()
        }

        fn draw(
            &mut self,
            _scene: &zgui::scene::Scene,
            _damage: &zgui::bits::DamageSet,
        ) -> zgui::render::FrameOutcome {
            zgui::render::FrameOutcome::Presented(zgui::render::FrameStats::default())
        }

        fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
            &mut self.atlas
        }
    }

    let sig_slot: Rc<RefCell<Option<RwSignal<ValidatedTextSettings, LocalStorage>>>> =
        Rc::new(RefCell::new(None));
    let sig_slot_view = Rc::clone(&sig_slot);
    let sig_slot_driver = Rc::clone(&sig_slot);

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.pump();
        if let Some(surface) = harness.platform().offscreens().first() {
            surface.set_a11y_listener(true);
        }
        harness.settle(4);

        let offscreen = harness.platform().offscreens()[0].clone();
        let mut nodes = HashMap::new();
        let mut root = None;

        // Focus the single TextInput via Tab and type "bad" to trigger validate().
        for state in [KeyState::Pressed, KeyState::Released] {
            harness.deliver_to_first(SurfaceEvent::Key {
                state,
                event: KeyEvent::named(NamedKey::Tab, PhysicalKey::Code(KeyCode::Tab)),
                modifiers: Modifiers::NONE,
                timestamp: Timestamp::ORIGIN,
            });
        }
        harness.settle(2);
        for ch in ['b', 'a', 'd'] {
            let event = KeyEvent::character(ch.to_string());
            for state in [KeyState::Pressed, KeyState::Released] {
                harness.deliver_to_first(SurfaceEvent::Key {
                    state,
                    event: event.clone(),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
        }
        harness.settle(4);

        let sig = sig_slot_driver.borrow().expect("model signal");
        for phase in 0..2 {
            if phase == 1 {
                // An external signal change must clear the stale validation error via RenderEffect.
                sig.set(ValidatedTextSettings {
                    text: "ok".to_owned(),
                });
                harness.settle(4);
            }

            for update in offscreen.take_a11y_log() {
                if let Some(tree) = update.tree {
                    root = Some(tree.root);
                }
                for (id, node) in update.nodes {
                    nodes.insert(id, node);
                }
            }

            let mut found_error = false;
            let mut stack = vec![root.expect("a11y root")];
            while let Some(id) = stack.pop() {
                if let Some(node) = nodes.get(&id) {
                    if node.value() == Some("bad_setting_value")
                        || node.label() == Some("bad_setting_value")
                    {
                        found_error = true;
                        break;
                    }
                    stack.extend_from_slice(node.children());
                }
            }

            if phase == 0 {
                assert!(found_error, "invalid edit should surface validation error");
                assert_eq!(sig.get_untracked().text, "ba");
            } else {
                assert!(
                    !found_error,
                    "external model update should clear stale validation error"
                );
            }
        }

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_title("settings-error-sync-test")
        .with_size(640.0, 240.0)
        .with_renderer(Box::new(|_surface: &Arc<dyn Surface>, target| {
            let mut renderer = HeadlessSink::default();
            zgui::render::Renderer::configure(&mut renderer, target);
            Ok::<_, AppError>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
        }))
        .run_on(driver, move || {
            let gds = Gds::try_current().unwrap_or_else(|| Gds::install(Scope::default()));
            gds.i18n().install(Language::English).expect("en-US");
            let sig = RwSignal::new_local(ValidatedTextSettings::default());
            *sig_slot_view.borrow_mut() = Some(sig);
            ValidatedTextSettingsEditor::new(sig).view()
        })
        .expect("headless settings error sync test ran");
}
