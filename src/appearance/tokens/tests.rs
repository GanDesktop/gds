use super::SurfaceOpacity;

#[test]
fn desktop_identity_takes_precedence_over_inherited_sockets() {
    for desktop in ["KDE", "GNOME", "ubuntu:GNOME", "XFCE", "not-niri"] {
        assert_eq!(
            SurfaceOpacity::for_session(Some(desktop), true),
            SurfaceOpacity::FLOATING
        );
    }
    for desktop in [
        "niri",
        " Niri : GNOME ",
        "sway",
        "Hyprland",
        "river",
        "qtile",
        "i3",
    ] {
        assert_eq!(
            SurfaceOpacity::for_session(Some(desktop), false),
            SurfaceOpacity::TILING
        );
    }
}

#[test]
fn missing_desktop_uses_the_compositor_socket() {
    for desktop in [None, Some(""), Some("  ")] {
        assert_eq!(
            SurfaceOpacity::for_session(desktop, true),
            SurfaceOpacity::TILING
        );
        assert_eq!(
            SurfaceOpacity::for_session(desktop, false),
            SurfaceOpacity::FLOATING
        );
    }
}
