use super::*;

fn roles() -> theme::TerminalColors {
    let color = |value| rgb(value).into();
    theme::TerminalColors {
        foreground: color(0xeeeeee),
        background: color(0x111111),
        cursor: color(0xffcc00),
        cursor_text: color(0x123456),
        selection: color(0x334455),
        selection_text: color(0xfedcba),
        normal: std::array::from_fn(|i| color(0x102030 + i as u32)),
        bright: std::array::from_fn(|i| color(0x405060 + i as u32)),
    }
}

#[test]
fn paint_and_query_share_roles_and_standard_indexed_colors() {
    let roles = roles();
    let palette = Palette::from_roles(roles, 0.4);
    let queries = palette.query_colors();
    let overrides = Colors::default();
    for (i, expected) in roles.normal.into_iter().chain(roles.bright).enumerate() {
        assert_eq!(queries[i], to_rgb(expected));
        assert_eq!(
            to_rgb(palette.resolve(Color::Indexed(i as u8), &overrides)),
            queries[i]
        );
    }
    for (index, rgb) in [
        (16, (0, 0, 0)),
        (21, (0, 0, 255)),
        (196, (255, 0, 0)),
        (231, (255, 255, 255)),
        (232, (8, 8, 8)),
        (255, (238, 238, 238)),
    ] {
        assert_eq!(
            queries[index],
            Rgb {
                r: rgb.0,
                g: rgb.1,
                b: rgb.2
            }
        );
    }
    assert_eq!(palette.cursor_text, roles.cursor_text);
    assert_eq!(palette.selection, roles.selection);
    assert_eq!(palette.selected_text, roles.selection_text);
    assert_eq!(palette.background(&overrides).a, 0.4);
}

#[test]
fn explicit_rgb_and_osc_overrides_survive_theme_changes_and_reset() {
    let first = Palette::from_roles(roles(), 0.4);
    let mut changed = roles();
    changed.normal[1] = rgb(0x778899).into();
    changed.background = rgb(0xeeeeee).into();
    let second = Palette::from_roles(changed, 0.7);
    let explicit = Rgb { r: 1, g: 2, b: 3 };
    let mut overrides = Colors::default();
    overrides[1] = Some(explicit);
    overrides[NamedColor::Background] = Some(explicit);
    for palette in [&first, &second] {
        assert_eq!(
            to_rgb(palette.resolve(Color::Spec(explicit), &overrides)),
            explicit
        );
        assert_eq!(
            to_rgb(palette.resolve(Color::Indexed(1), &overrides)),
            explicit
        );
        assert_eq!(to_rgb(palette.background(&overrides)), explicit);
    }
    assert_eq!(second.background(&overrides).a, 0.7);
    overrides[1] = None;
    overrides[NamedColor::Background] = None;
    assert_eq!(
        to_rgb(second.resolve(Color::Indexed(1), &overrides)),
        to_rgb(changed.normal[1])
    );
    assert_eq!(
        to_rgb(second.background(&overrides)),
        to_rgb(changed.background)
    );
}
