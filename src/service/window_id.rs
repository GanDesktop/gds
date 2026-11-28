//! XDG Desktop Portal / Secret Service external parent-window identifier.

use std::{fmt, str::FromStr};

/// Parent-window handle encoded in the XDG Desktop Portal format
/// (`"wayland:<handle>"` or `"x11:<hex-xid>"`).
///
/// Reference: <https://flatpak.github.io/xdg-desktop-portal/docs/window-identifiers.html>
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParentWindow {
    /// Handle exported via Wayland `zxdg_exporter_v2`.
    Wayland(String),
    /// X11 window identifier (XID).
    X11(u64),
}

/// Error parsing an external [`ParentWindow`] string.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid XDG parent window identifier: {0:?}")]
pub struct ParseParentWindowError(String);

impl ParentWindow {
    /// Construct a Wayland parent window handle from a non-empty `xdg_foreign` export string.
    #[must_use]
    pub fn wayland(handle: impl Into<String>) -> Option<Self> {
        let handle = handle.into();
        if handle.is_empty()
            || handle
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
        {
            return None;
        }
        Some(Self::Wayland(handle))
    }

    /// Construct an X11 parent window handle from a non-zero XID.
    #[must_use]
    pub const fn x11(xid: u64) -> Option<Self> {
        if xid == 0 { None } else { Some(Self::X11(xid)) }
    }

    #[cfg(all(feature = "desktop", any(target_os = "linux", target_os = "freebsd")))]
    pub(crate) fn to_ashpd(&self) -> ashpd::WindowIdentifier {
        match self {
            Self::Wayland(handle) => {
                ashpd::WindowIdentifier::from_xdg_foreign_exported(handle.clone())
            }
            #[allow(clippy::cast_possible_truncation)]
            Self::X11(xid) => ashpd::WindowIdentifier::from_xid(*xid as std::os::raw::c_ulong),
        }
    }

    #[cfg(all(feature = "remote", any(target_os = "linux", target_os = "freebsd")))]
    pub(crate) fn to_oo7(&self) -> oo7::ashpd::WindowIdentifier {
        match self {
            Self::Wayland(handle) => {
                oo7::ashpd::WindowIdentifier::from_xdg_foreign_exported(handle.clone())
            }
            #[allow(clippy::cast_possible_truncation)]
            Self::X11(xid) => oo7::ashpd::WindowIdentifier::from_xid(*xid as std::os::raw::c_ulong),
        }
    }
}

impl fmt::Display for ParentWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wayland(handle) => write!(f, "wayland:{handle}"),
            Self::X11(xid) => write!(f, "x11:{xid:x}"),
        }
    }
}

impl FromStr for ParentWindow {
    type Err = ParseParentWindowError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let Some((kind, value)) = raw.split_once(':') else {
            return Err(ParseParentWindowError(raw.to_owned()));
        };
        match kind {
            "wayland" => Self::wayland(value).ok_or_else(|| ParseParentWindowError(raw.to_owned())),
            "x11" => {
                let hex = value
                    .strip_prefix("0x")
                    .or_else(|| value.strip_prefix("0X"))
                    .unwrap_or(value);
                if hex.is_empty() {
                    return Err(ParseParentWindowError(raw.to_owned()));
                }
                let xid = u64::from_str_radix(hex, 16)
                    .map_err(|_| ParseParentWindowError(raw.to_owned()))?;
                Self::x11(xid).ok_or_else(|| ParseParentWindowError(raw.to_owned()))
            }
            _ => Err(ParseParentWindowError(raw.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_wayland_and_x11_identifiers() {
        let wayland: ParentWindow = "wayland:abc-123_XYZ".parse().expect("valid wayland handle");
        assert_eq!(wayland, ParentWindow::Wayland("abc-123_XYZ".to_owned()));
        assert_eq!(wayland.to_string(), "wayland:abc-123_XYZ");

        let x11: ParentWindow = "x11:1a2b3c".parse().expect("valid x11 handle");
        assert_eq!(x11, ParentWindow::X11(0x1a2b3c));
        assert_eq!(x11.to_string(), "x11:1a2b3c");

        let x11_prefixed: ParentWindow = "x11:0x42".parse().expect("valid 0x x11 handle");
        assert_eq!(x11_prefixed, ParentWindow::X11(0x42));
        assert_eq!(x11_prefixed.to_string(), "x11:42");
    }

    #[test]
    fn rejects_empty_malformed_and_zero_identifiers() {
        for invalid in [
            "",
            "wayland:",
            "wayland:bad handle",
            "wayland:bad\nhandle",
            "x11:",
            "x11:0",
            "x11:0x0",
            "x11:xyz",
            "macos:123",
            "bare-handle",
        ] {
            assert!(
                invalid.parse::<ParentWindow>().is_err(),
                "expected {invalid:?} to fail"
            );
        }
    }
}
