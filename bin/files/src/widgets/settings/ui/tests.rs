use super::controls::Field;
use crate::entities::preferences::Preferences;
use gds::appearance::tokens::SurfaceOpacity;

#[test]
fn boolean_fields_detect_modifications_and_reset_to_defaults() {
    let mut preferences = Preferences::default();

    assert!(!Field::RoundedCorners.is_modified(&preferences));
    assert_eq!(Field::RoundedCorners.bool(&preferences), Some(false));
    assert!(Field::RoundedCorners.set_bool(&mut preferences, true));
    assert!(Field::RoundedCorners.is_modified(&preferences));
    Field::RoundedCorners.reset(&mut preferences);
    assert!(!Field::RoundedCorners.is_modified(&preferences));

    assert!(!Field::Animations.is_modified(&preferences));
    assert_eq!(Field::Animations.bool(&preferences), Some(true));
    assert!(Field::Animations.set_bool(&mut preferences, false));
    assert!(Field::Animations.is_modified(&preferences));
    Field::Animations.reset(&mut preferences);
    assert!(!Field::Animations.is_modified(&preferences));

    assert!(!Field::TerminalEnabled.is_modified(&preferences));
    assert_eq!(Field::TerminalEnabled.bool(&preferences), Some(true));
    assert!(Field::TerminalEnabled.set_bool(&mut preferences, false));
    assert!(Field::TerminalEnabled.is_modified(&preferences));
    Field::TerminalEnabled.reset(&mut preferences);
    assert!(!Field::TerminalEnabled.is_modified(&preferences));

    assert!(!Field::TagsEnabled.is_modified(&preferences));
    assert_eq!(Field::TagsEnabled.bool(&preferences), Some(false));
    assert!(Field::TagsEnabled.set_bool(&mut preferences, true));
    assert!(Field::TagsEnabled.is_modified(&preferences));
    Field::TagsEnabled.reset(&mut preferences);
    assert!(!Field::TagsEnabled.is_modified(&preferences));
}

#[test]
fn numeric_fields_detect_modifications_and_reset_to_defaults() {
    let mut preferences = Preferences::default();
    let defaults = SurfaceOpacity::default();

    assert!(!Field::ContentOpacity.is_modified(&preferences));
    assert_eq!(
        Field::ContentOpacity.number(&preferences),
        Some(defaults.content * 100.0)
    );
    Field::ContentOpacity.set_number(&mut preferences, 50.0);
    assert!(Field::ContentOpacity.is_modified(&preferences));
    Field::ContentOpacity.reset(&mut preferences);
    assert!(!Field::ContentOpacity.is_modified(&preferences));

    assert!(!Field::SidebarOpacity.is_modified(&preferences));
    assert_eq!(
        Field::SidebarOpacity.number(&preferences),
        Some(defaults.sidebar * 100.0)
    );
    Field::SidebarOpacity.set_number(&mut preferences, 40.0);
    assert!(Field::SidebarOpacity.is_modified(&preferences));
    Field::SidebarOpacity.reset(&mut preferences);
    assert!(!Field::SidebarOpacity.is_modified(&preferences));

    assert!(!Field::ProbeBudget.is_modified(&preferences));
    assert_eq!(Field::ProbeBudget.number(&preferences), Some(256.0));
    Field::ProbeBudget.set_number(&mut preferences, 512.0);
    assert_eq!(preferences.files.probe_budget, 512);
    assert!(Field::ProbeBudget.is_modified(&preferences));
    Field::ProbeBudget.reset(&mut preferences);
    assert_eq!(preferences.files.probe_budget, 256);
    assert!(!Field::ProbeBudget.is_modified(&preferences));
}
