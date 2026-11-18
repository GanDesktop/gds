//! Follow the active desktop's palette and appearance preferences.
mod dms;
mod gnome;
mod kde;
mod material;
mod noctalia;

use crate::appearance::theme::Mode;
use directories::BaseDirs;

use std::path::{Path, PathBuf};

use super::{Error, Loaded, ModePreference, read};

const DEFAULT_WAYLAND_DISPLAY: &str = "wayland-0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desktop {
    Dms,
    Noctalia,
    Kde,
    Gnome,
}

struct Session<'a> {
    desktop: &'a str,
    runtime_dir: &'a Path,
    wayland_display: Option<&'a str>,
    dms_socket: Option<&'a Path>,
}

impl Desktop {
    fn current(dirs: &BaseDirs) -> Option<Self> {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let runtime = runtime_dir(dirs);
        let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
        let dms_socket = std::env::var_os("DMS_SOCKET").map(PathBuf::from);
        Self::detect(&Session {
            desktop: &desktop,
            runtime_dir: &runtime,
            wayland_display: wayland_display.as_deref(),
            dms_socket: dms_socket.as_deref(),
        })
    }

    fn detect(session: &Session<'_>) -> Option<Self> {
        Self::from_session(session.desktop).or_else(|| Self::active_shell(session))
    }

    fn from_session(desktop: &str) -> Option<Self> {
        for name in desktop.split(':') {
            if name.eq_ignore_ascii_case("KDE") {
                return Some(Self::Kde);
            }
            if name.eq_ignore_ascii_case("GNOME") {
                return Some(Self::Gnome);
            }
        }
        None
    }

    #[cfg(unix)]
    fn active_shell(session: &Session<'_>) -> Option<Self> {
        if noctalia_alive(session.runtime_dir, session.wayland_display) {
            return Some(Self::Noctalia);
        }
        if dms_alive(session.runtime_dir, session.dms_socket) {
            return Some(Self::Dms);
        }
        None
    }

    #[cfg(not(unix))]
    fn active_shell(_session: &Session<'_>) -> Option<Self> {
        None
    }

    fn path(self, dirs: &BaseDirs) -> PathBuf {
        match self {
            Self::Kde => dirs.config_dir().join("kdeglobals"),
            Self::Dms => dirs.cache_dir().join("DankMaterialShell/dms-colors.json"),
            Self::Noctalia => noctalia::output_path(dirs.cache_dir()),
            Self::Gnome => dirs.config_dir().join("dconf/user"),
        }
    }
}

fn runtime_dir(dirs: &BaseDirs) -> PathBuf {
    dirs.runtime_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir)
}

fn noctalia_socket(runtime_dir: &Path, display: &str) -> PathBuf {
    runtime_dir.join(format!("noctalia-{display}.sock"))
}

#[cfg(unix)]
fn is_noctalia_shell_socket(name: &str) -> bool {
    name.strip_prefix("noctalia-")
        .and_then(|rest| rest.strip_suffix(".sock"))
        .is_some_and(|display| {
            !display.is_empty() && display != "dmenu" && !display.starts_with("dmenu-")
        })
}

#[cfg(unix)]
fn socket_alive(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(unix)]
fn find_noctalia_socket(runtime_dir: &Path, wayland_display: Option<&str>) -> Option<PathBuf> {
    if let Some(display) = wayland_display.filter(|value| !value.is_empty()) {
        let socket = noctalia_socket(runtime_dir, display);
        return socket_alive(&socket).then_some(socket);
    }
    let default = noctalia_socket(runtime_dir, DEFAULT_WAYLAND_DISPLAY);
    if socket_alive(&default) {
        return Some(default);
    }
    find_live_socket(runtime_dir, is_noctalia_shell_socket)
}

#[cfg(unix)]
fn noctalia_alive(runtime_dir: &Path, wayland_display: Option<&str>) -> bool {
    find_noctalia_socket(runtime_dir, wayland_display).is_some()
}

#[cfg(unix)]
fn dms_alive(runtime_dir: &Path, dms_socket: Option<&Path>) -> bool {
    if let Some(socket) = dms_socket.filter(|path| !path.as_os_str().is_empty()) {
        return socket_alive(socket);
    }
    find_live_socket(runtime_dir, |name| {
        let Some(stem) = name.strip_suffix(".sock") else {
            return false;
        };
        matches!(stem, "dms" | "danklinux")
            || stem.starts_with("dms-")
            || stem.starts_with("danklinux-")
    })
    .is_some()
}

#[cfg(unix)]
fn find_live_socket(runtime_dir: &Path, matches_name: impl Fn(&str) -> bool) -> Option<PathBuf> {
    let entries = std::fs::read_dir(runtime_dir).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name();
            file_name
                .to_str()
                .is_some_and(&matches_name)
                .then(|| entry.path())
        })
        .collect();
    candidates.sort();
    candidates.into_iter().find(|path| socket_alive(path))
}

pub(super) fn source_paths() -> Vec<PathBuf> {
    let Some(dirs) = BaseDirs::new() else {
        return Vec::new();
    };
    match Desktop::from_session(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()) {
        Some(Desktop::Kde) => vec![Desktop::Kde.path(&dirs)],
        Some(Desktop::Gnome) => vec![
            Desktop::Gnome.path(&dirs),
            dirs.config_dir().join("gtk-4.0/gtk.css"),
            dirs.config_dir().join("gtk-4.0/settings.ini"),
            dirs.config_dir().join("gtk-3.0/gtk.css"),
            dirs.config_dir().join("gtk-3.0/settings.ini"),
        ],
        Some(Desktop::Dms | Desktop::Noctalia) => {
            unreachable!("standalone shells are detected via runtime IPC")
        }
        None => {
            let runtime = runtime_dir(&dirs);
            let display = std::env::var("WAYLAND_DISPLAY")
                .ok()
                .filter(|value| !value.is_empty());
            let noctalia_sock = noctalia_socket(
                &runtime,
                display.as_deref().unwrap_or(DEFAULT_WAYLAND_DISPLAY),
            );
            vec![
                Desktop::Noctalia.path(&dirs),
                Desktop::Dms.path(&dirs),
                noctalia_sock,
            ]
        }
    }
}

/// # Errors
/// Reports missing user directories, inactive desktop shells, unreadable files,
/// or invalid desktop palettes.
pub(super) fn load(preference: ModePreference, platform: Mode) -> Result<Loaded, Error> {
    let dirs = BaseDirs::new().ok_or(Error::NoUserDirectories)?;
    let desktop = Desktop::current(&dirs).ok_or(Error::UnsupportedDesktop)?;
    match desktop {
        Desktop::Gnome => gnome::load(preference, platform, &dirs),
        Desktop::Noctalia => {
            #[cfg(unix)]
            {
                let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
                let socket = find_noctalia_socket(&runtime_dir(&dirs), wayland_display.as_deref())
                    .ok_or(Error::UnsupportedDesktop)?;
                noctalia::load(&dirs, &socket, preference)
            }
            #[cfg(not(unix))]
            {
                Err(Error::UnsupportedDesktop)
            }
        }
        Desktop::Kde => {
            let path = desktop.path(&dirs);
            let contents = read(&path)?;
            kde::parse(&contents, &path, preference)
        }
        Desktop::Dms => {
            let path = desktop.path(&dirs);
            let contents = read(&path)?;
            let parsed = dms::parse(&contents, &path, preference)?;
            Ok(Loaded {
                palette: parsed.palette,
                mode: parsed.mode,
                source_path: Some(path),
            })
        }
    }
}

#[cfg(test)]
mod tests;
