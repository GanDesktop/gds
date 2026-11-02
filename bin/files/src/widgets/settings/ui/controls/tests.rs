use super::{Field, NumberRange, PERCENTAGE_RANGE};
use crate::entities::preferences::{FilesPreferences, Preferences};
use gds::ui::settings::{FONT_SIZE_RANGE, form::Model};
use gds::{
    appearance::{
        theme::{Appearance, ModePreference, Source},
        typography,
    },
    i18n::Language,
    ui::time::Format,
};

fn modified_preferences() -> Preferences {
    Preferences {
        appearance: Appearance {
            source: Source::System,
            builtin: Default::default(),
            mode: ModePreference::Light,
            content_opacity: 0.7,
            sidebar_opacity: 0.8,
        },
        animations_enabled: Some(false),
        terminal_enabled: None,
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
    }
}

fn reset_generated<T: Model>(model: &mut T, id: &str) {
    let field = T::fields()
        .into_iter()
        .find(|field| field.id == id)
        .expect("generated field");
    (field.set)(model, (field.get)(&T::default())).expect("default field value");
}

#[test]
fn animation_reset_preserves_all_other_preferences() {
    let before = modified_preferences();
    let mut reset = before.clone();
    assert!(Field::Animations.reset(&mut reset));
    assert!(reset.animations_enabled.unwrap_or(true));
    assert!(!Field::Animations.reset(&mut reset));
    reset.animations_enabled = before.animations_enabled;
    assert_eq!(reset, before);
}

#[test]
fn resetting_a_theme_choice_does_not_reset_the_other_theme_fields() {
    let before = modified_preferences();
    let mut source = before.clone();
    assert!(Field::Source.reset(&mut source));
    assert_eq!(
        source.appearance.source,
        Preferences::default().appearance.source
    );
    assert_eq!(source.appearance.mode, before.appearance.mode);
    assert_eq!(
        source.appearance.content_opacity,
        before.appearance.content_opacity
    );
    assert_eq!(
        source.appearance.sidebar_opacity,
        before.appearance.sidebar_opacity
    );
    assert_eq!(source.language, before.language);
    assert_eq!(source.files, before.files);

    let mut mode = before.clone();
    assert!(Field::Mode.reset(&mut mode));
    assert_eq!(mode.appearance.mode, Preferences::default().appearance.mode);
    assert_eq!(mode.appearance.source, before.appearance.source);
    assert_eq!(
        mode.appearance.content_opacity,
        before.appearance.content_opacity
    );
    assert_eq!(
        mode.appearance.sidebar_opacity,
        before.appearance.sidebar_opacity
    );
}

#[test]
fn resetting_one_opacity_keeps_the_other_material_unchanged() {
    let before = modified_preferences();
    let mut content = before.clone();
    assert!(Field::ContentOpacity.reset(&mut content));
    assert_eq!(
        content.appearance.content_opacity,
        Preferences::default().appearance.content_opacity
    );
    assert_eq!(content.appearance.sidebar_opacity, 0.8);
    assert_eq!(content.appearance.source, Source::System);
    assert_eq!(content.appearance.mode, ModePreference::Light);
    assert_eq!(content.language, before.language);
    assert_eq!(content.files, before.files);

    let mut sidebar = before;
    assert!(Field::SidebarOpacity.reset(&mut sidebar));
    assert_eq!(
        sidebar.appearance.sidebar_opacity,
        Preferences::default().appearance.sidebar_opacity
    );
    assert_eq!(sidebar.appearance.content_opacity, 0.7);
    assert_eq!(sidebar.appearance.source, Source::System);
    assert_eq!(sidebar.appearance.mode, ModePreference::Light);
}

#[test]
fn language_and_file_preference_reset_are_independent() {
    let before = modified_preferences();
    let mut language = before.clone();
    reset_generated(&mut language, "language");
    assert_eq!(language.language, Preferences::default().language);
    assert_eq!(language.appearance, before.appearance);
    assert_eq!(language.files, before.files);

    let mut files = before.clone();
    assert!(Field::ShowHidden.reset(&mut files));
    assert!(!files.files.show_hidden);
    assert_eq!(files.language, before.language);
    assert_eq!(files.appearance, before.appearance);
}

#[test]
fn font_family_and_size_reset_independently_of_every_other_preference() {
    let before = modified_preferences();

    let mut family = before.clone();
    reset_generated(&mut family.font, "family");
    assert_eq!(family.font.family, Preferences::default().font.family);
    assert_eq!(family.font.size, before.font.size);
    assert_eq!(family.appearance, before.appearance);
    assert_eq!(family.language, before.language);
    assert_eq!(family.time_format, before.time_format);
    assert_eq!(family.files, before.files);

    let mut size = before.clone();
    reset_generated(&mut size.font, "size");
    assert_eq!(size.font.size, Preferences::default().font.size);
    assert_eq!(size.font.family, before.font.family);
    assert_eq!(size.files, before.files);
}

#[test]
fn terminal_edits_and_general_resets_preserve_other_preferences() {
    let before = modified_preferences();
    let mut changed = before.clone();
    reset_generated(&mut changed.terminal, "font_size");
    assert_eq!(
        changed.terminal.font_size,
        gds::ui::terminal::Preferences::default().font_size
    );
    assert_eq!(changed.terminal.shell_args, before.terminal.shell_args);
    assert_eq!(changed.font, before.font);
    assert_eq!(changed.files, before.files);

    let mut general = before.clone();
    reset_generated(&mut general, "language");
    assert_eq!(general.terminal, before.terminal);
    assert_eq!(
        general.files.terminal_content_height,
        before.files.terminal_content_height
    );
}

#[test]
fn time_format_reset_is_independent_of_files_language_and_columns() {
    let before = modified_preferences();
    assert_ne!(before.time_format, Preferences::default().time_format);

    let mut time = before.clone();
    reset_generated(&mut time, "time_format");
    assert_eq!(time.time_format, Preferences::default().time_format);
    assert_eq!(time.files, before.files);
    assert_eq!(time.language, before.language);
    assert_eq!(time.appearance, before.appearance);

    let mut files = before.clone();
    assert!(Field::ShowHidden.reset(&mut files));
    assert_eq!(files.time_format, before.time_format);

    let mut language = before.clone();
    reset_generated(&mut language, "language");
    assert_eq!(language.time_format, before.time_format);
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
    let changes = changes.borrow();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].animations_enabled, Some(false));
    assert_eq!(changes[1], Preferences::default());
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
