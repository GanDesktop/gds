//! Location segments for the address bar.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Segment {
    /// Visible text; parent segments keep their trailing separator.
    pub label: String,
    pub path: PathBuf,
    pub current: bool,
}

/// Split a path into independently clickable blocks, matching the address bar.
///
/// `/mnt/st2000/assets` becomes `/`, `mnt/`, `st2000/`, `assets`; the final
/// block is the current location. `.` components are navigationally irrelevant
/// and do not get their own block.
pub(super) fn segments(path: &Path) -> Vec<Segment> {
    let components: Vec<Component<'_>> = path.components().collect();
    let last_visible = components
        .iter()
        .rposition(|component| !matches!(component, Component::CurDir));
    let mut target = PathBuf::new();
    let mut segments = Vec::new();
    for (index, component) in components.iter().enumerate() {
        target.push(component.as_os_str());
        if matches!(component, Component::CurDir) {
            continue;
        }
        let raw = component.as_os_str().to_string_lossy();
        let current = last_visible == Some(index);
        let label = match component {
            Component::RootDir | Component::Prefix(_) => raw.into_owned(),
            _ if current => raw.into_owned(),
            _ => format!("{raw}{}", std::path::MAIN_SEPARATOR),
        };
        segments.push(Segment {
            label,
            path: target.clone(),
            current,
        });
    }
    if segments.is_empty() {
        segments.push(Segment {
            label: path.display().to_string(),
            path: path.to_path_buf(),
            current: true,
        });
    }
    segments
}

#[cfg(test)]
mod tests;
