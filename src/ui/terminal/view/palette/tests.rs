use super::*;

fn rgb(value: u32) -> ZguiColor {
    ZguiColor::srgb(
        f32::from(((value >> 16) & 0xff) as u8) / 255.0,
        f32::from(((value >> 8) & 0xff) as u8) / 255.0,
        f32::from((value & 0xff) as u8) / 255.0,
        1.0,
    )
}

fn roles() -> theme::TerminalColors {
    theme::TerminalColors {
        foreground: rgb(0xeeeeee),
        background: rgb(0x111111),
        cursor: rgb(0xffcc00),
        cursor_text: rgb(0x123456),
        selection: rgb(0x334455),
        selection_text: rgb(0xfedcba),
        normal: std::array::from_fn(|i| rgb(0x102030 + i as u32)),
        bright: std::array::from_fn(|i| rgb(0x405060 + i as u32)),
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
    assert_eq!(palette.cursor_text, roles.cursor_text);
    assert_eq!(palette.selection, roles.selection);
    assert_eq!(palette.selected_text, roles.selection_text);
    assert_eq!(palette.background(&overrides).alpha(), 0.4);
}
