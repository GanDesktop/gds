use super::{Field, NumberRange, PERCENTAGE_RANGE};
use crate::entities::preferences::{FilesPreferences, Preferences};
use gds::ui::settings::{FONT_SIZE_RANGE, form::Model};
use gds::{
    appearance::{
        theme::{Appearance, BuiltinTheme, ModePreference, Source},
        typography,
    },
    i18n::Language,
    ui::time::Format,
};

fn modified_preferences() -> Preferences {
    let mut preferences = Preferences {
        appearance: Appearance {
            source: Source::Builtin,
            builtin: BuiltinTheme::Neon,
            mode: ModePreference::Light,
            content_opacity: 0.7,
            sidebar_opacity: 0.8,
            rounded_corners: true,
        },
        animations_enabled: Some(false),
        terminal_enabled: Some(false),
        language: Some(Language::English),
        time_format: Format::Absolute,
        font: typography::Preferences {
            family: Some("Noto Sans".to_owned()),
            size: 18.0,
        },
        terminal: gds::ui::terminal::Preferences {
            font_size: 18.,
            shell_args: vec!["--private".into()],
            ..Default::default()
        },
        files: FilesPreferences {
            terminal_content_height: Some(360.),
            sidebar: Default::default(),
            volume_mount_points: Default::default(),
            show_hidden: true,
        },
    };
    preferences.files.sidebar.show_volumes = true;
    preferences
}

/// Every generated field resets to its own default without rewriting its siblings.
fn assert_generated_reset_is_isolated<T: Model>(before: &T) {
    for field in T::fields() {
        let mut after = before.clone();
        (field.set)(&mut after, (field.get)(&T::default())).expect("default field value");
        for probe in T::fields() {
            if probe.id == field.id {
                continue;
            }
            assert_eq!(
                (probe.get)(&after),
                (probe.get)(before),
                "resetting {} changed {}",
                field.id,
                probe.id
            );
        }
    }
}

#[test]
fn generated_field_reset_only_changes_its_own_value() {
    let before = modified_preferences();
    assert_ne!(before, Preferences::default());
    for field in Preferences::fields() {
        let mut after = before.clone();
        (field.set)(&mut after, (field.get)(&Preferences::default())).expect("default field value");
        assert_eq!(
            (field.get)(&after),
            (field.get)(&Preferences::default()),
            "resetting {} must use its default",
            field.id
        );
        assert_eq!(
            after.appearance, before.appearance,
            "resetting {} changed appearance",
            field.id
        );
        assert_eq!(
            after.files, before.files,
            "resetting {} changed files",
            field.id
        );
        assert_eq!(
            after.font, before.font,
            "resetting {} changed font",
            field.id
        );
        assert_eq!(
            after.terminal, before.terminal,
            "resetting {} changed terminal",
            field.id
        );
        assert_eq!(
            after.animations_enabled, before.animations_enabled,
            "resetting {} changed animations",
            field.id
        );
        assert_eq!(
            after.terminal_enabled, before.terminal_enabled,
            "resetting {} changed terminal enabled",
            field.id
        );
    }
    assert_generated_reset_is_isolated(&before.font);
    assert_generated_reset_is_isolated(&before.terminal);
}

fn control_value(field: Field, preferences: &Preferences) -> serde_json::Value {
    let value = match field {
        Field::Source => serde_json::to_value(preferences.appearance.source),
        Field::Builtin => serde_json::to_value(preferences.appearance.builtin),
        Field::Mode => serde_json::to_value(preferences.appearance.mode),
        Field::ContentOpacity => serde_json::to_value(preferences.appearance.content_opacity),
        Field::SidebarOpacity => serde_json::to_value(preferences.appearance.sidebar_opacity),
        Field::ShowHidden => serde_json::to_value(preferences.files.show_hidden),
        Field::SidebarVolumes => serde_json::to_value(preferences.files.sidebar.show_volumes),
        Field::Animations => serde_json::to_value(preferences.animations_enabled),
        Field::RoundedCorners => serde_json::to_value(preferences.appearance.rounded_corners),
    };
    value.expect("preferences value")
}

#[test]
fn control_reset_only_clears_its_own_field() {
    const FIELDS: &[Field] = &[
        Field::Source,
        Field::Builtin,
        Field::Mode,
        Field::ContentOpacity,
        Field::SidebarOpacity,
        Field::ShowHidden,
        Field::SidebarVolumes,
        Field::Animations,
        Field::RoundedCorners,
    ];
    let before = modified_preferences();
    let values: Vec<_> = FIELDS
        .iter()
        .map(|field| control_value(*field, &before))
        .collect();
    let defaults: Vec<_> = FIELDS
        .iter()
        .map(|field| control_value(*field, &Preferences::default()))
        .collect();
    for (index, field) in FIELDS.iter().enumerate() {
        assert!(
            field.is_modified(&before),
            "{field:?} must be modified by the fixture"
        );
        let mut after = before.clone();
        assert!(
            field.reset(&mut after),
            "{field:?} reset must report its modification"
        );
        for (other_index, other) in FIELDS.iter().enumerate() {
            let value = control_value(*other, &after);
            if other_index == index {
                assert_eq!(
                    value, defaults[index],
                    "{field:?} must reset to its default value"
                );
            } else {
                assert_eq!(
                    value, values[other_index],
                    "{field:?} reset changed {other:?}"
                );
            }
        }
    }
}

#[test]
fn numeric_ranges_quantize_relative_to_the_minimum_and_clamp() {
    let range = NumberRange {
        min: 5.0,
        max: 15.0,
        step: 2.0,
    };
    assert_eq!(range.quantize(8.0), 9.0);
    assert_eq!(range.quantize(3.0), 5.0);
    assert_eq!(range.quantize(20.0), 15.0);
    assert_eq!(PERCENTAGE_RANGE.quantize(69.4), 69.0);
    assert_eq!(PERCENTAGE_RANGE.quantize(69.8), 70.0);
    assert_eq!(FONT_SIZE_RANGE.quantize(9.0), typography::MIN_SIZE);
    assert_eq!(FONT_SIZE_RANGE.quantize(30.0), typography::MAX_SIZE);
    assert_eq!(FONT_SIZE_RANGE.quantize(16.4), typography::DEFAULT_SIZE);
}

#[test]
fn legacy_float_rounding_is_equivalent_without_rewriting_the_preference() {
    assert!(PERCENTAGE_RANGE.equivalent(0.6999999 * 100.0, 70.0));
    assert!(!PERCENTAGE_RANGE.equivalent(69.0, 70.0));
    let mut preferences = Preferences::default();
    let rounded = preferences.appearance.content_opacity - f32::EPSILON;
    preferences.appearance.content_opacity = rounded;
    assert!(!Field::ContentOpacity.is_modified(&preferences));
    assert!(!Field::ContentOpacity.reset(&mut preferences));
    assert_eq!(preferences.appearance.content_opacity, rounded);
}

#[cfg(feature = "gui-tests")]
#[gpui_kit::test]
fn appearance_navigation_exposes_animation_toggle_and_independent_reset(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::widgets::settings::ui::{Event, Page, View};
    use gpui_kit::{AppContext, component::Root, px, size, test::TestWindowExt};
    use std::{cell::RefCell, rc::Rc};
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(Language::English, cx).expect("translations");
    });
    let mut view = None;
    let window = cx.open_window(size(px(1000.), px(850.)), |window, cx| {
        let settings =
            cx.new(|cx| View::new(Preferences::default(), Default::default(), window, cx));
        view = Some(settings.clone());
        Root::new(settings, window, cx)
    });
    let view = view.expect("settings view");
    let changes = Rc::new(RefCell::new(Vec::new()));
    let observed = changes.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&view, move |_, event, cx| {
            let Event::Apply(preferences) = event;
            let previous = observed.borrow().last().cloned().unwrap_or_default();
            preferences
                .apply_shared(&previous, cx)
                .expect("live motion update");
            observed.borrow_mut().push(preferences.clone());
        })
    });
    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |view, cx| {
            view.select_page(Page::Appearance, window, cx)
        });
        window.render_frame(cx);
        window.click("settings-animations", cx);
        assert_eq!(view.read(cx).preferences.animations_enabled, Some(false));
    })
    .expect("navigate and toggle");
    cx.update(|cx| assert!(cx.reduce_motion(), "native Kit motion is stopped"));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("settings-reset", Field::Animations as usize), cx);
    })
    .expect("reset animations");
    cx.update(|cx| assert!(!cx.reduce_motion(), "system preference is restored"));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("settings-rounded-corners", cx);
        assert!(view.read(cx).preferences.appearance.rounded_corners);
    })
    .expect("toggle window corners");
    cx.update(|cx| {
        assert!(
            cx.global::<gpui_kit::component::WindowBorderStyle>()
                .corner_radius()
                > px(0.)
        )
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("settings-reset", Field::RoundedCorners as usize), cx);
    })
    .expect("reset window corners");
    cx.update(|cx| {
        assert_eq!(
            cx.global::<gpui_kit::component::WindowBorderStyle>()
                .corner_radius(),
            px(0.)
        )
    });
    let changes = changes.borrow();
    assert_eq!(changes.len(), 4);
    assert_eq!(changes[0].animations_enabled, Some(false));
    assert_eq!(changes[1], Preferences::default());
    assert!(changes[2].appearance.rounded_corners);
    assert_eq!(changes[3], Preferences::default());
}

#[cfg(feature = "gui-tests")]
#[gpui_kit::test]
fn changing_and_resetting_builtin_palette_applies_without_changing_mode(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gds::appearance::theme::{self, BuiltinTheme};
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut previous = modified_preferences();
        previous.appearance.source = Source::Builtin;
        previous.appearance.builtin = BuiltinTheme::default();
        theme::apply(&previous.appearance, cx).expect("initial embedded palette");
        let original = theme::Colors::current(cx);
        let mut changed = previous.clone();
        changed.appearance.builtin = BuiltinTheme::Neon;
        changed
            .apply_shared(&previous, cx)
            .expect("apply selected palette");
        assert_ne!(theme::Colors::current(cx), original);
        let neon = changed.clone();
        assert!(Field::Builtin.reset(&mut changed));
        changed
            .apply_shared(&neon, cx)
            .expect("reset embedded palette");
        assert_eq!(theme::Colors::current(cx), original);
        assert_eq!(changed.appearance.mode, previous.appearance.mode);
        assert_eq!(
            changed.appearance.content_opacity,
            previous.appearance.content_opacity
        );
        assert_eq!(
            changed.appearance.sidebar_opacity,
            previous.appearance.sidebar_opacity
        );
    });
}
