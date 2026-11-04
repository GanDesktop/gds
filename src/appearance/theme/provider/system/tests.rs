use super::Desktop;

#[test]
fn plasma_sessions_select_kde_without_changing_dms_sessions() {
    for desktop in ["KDE", "kde", "custom:KDE"] {
        assert_eq!(Desktop::from_session(desktop), Desktop::Kde);
    }
    for desktop in ["niri", "Hyprland", "", "not-KDE"] {
        assert_eq!(Desktop::from_session(desktop), Desktop::Dms);
    }
}

#[test]
fn gnome_sessions_do_not_read_dms_exports() {
    for desktop in ["GNOME", "ubuntu:GNOME", "GNOME-Classic:GNOME", "gnome"] {
        assert_eq!(Desktop::from_session(desktop), Desktop::Gnome);
    }
}
