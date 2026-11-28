//! Shared MIME-Info based executable detection for the `+x` open gate.
//!
//! Filesystems such as FAT32, exFAT, NTFS, and CIFS/SMB frequently mount every
//! regular file with `0755` permissions. Checking POSIX execute bits alone
//! blocks ordinary documents (`.pdf`, `.txt`, `.md`, `.svg`, `.docx`, `.html`).
//! Instead, this classifier inspects the file header for disguised scripts or
//! binaries and consults installed FreeDesktop `shared-mime-info` rules
//! (`globs2` / `globs`, `aliases`, `subclasses`) to gate only genuinely
//! executable types and desktop launchers.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const EXECUTABLE_MIMES: &[&str] = &[
    "application/x-executable",
    "application/x-sharedlib",
    "application/x-pie-executable",
    "application/x-desktop",
    "application/x-shellscript",
    "application/x-csh",
    "application/x-ms-dos-executable",
    "application/vnd.microsoft.portable-executable",
    "application/x-mach-binary",
    "application/x-bat",
    "application/x-msi",
    "application/java-archive",
    "application/x-java-archive",
    "application/java",
    "application/wasm",
    "application/vnd.android.dex",
    "application/vnd.android.dey",
];

const FALLBACK_EXECUTABLE_EXTENSIONS: &[&str] = &[
    "appimage", "awk", "bat", "bash", "bin", "cjs", "cmd", "com", "csh", "desktop", "dll", "elf",
    "exe", "fish", "jar", "js", "ksh", "lua", "mjs", "msi", "pl", "py", "rb", "run", "sh", "so",
    "zsh",
];

static CLASSIFIER: LazyLock<ExecutableClassifier> = LazyLock::new(ExecutableClassifier::from_xdg);

/// Return `true` when an executable-bit file should require an explicit application
/// chooser instead of opening with the default document handler.
pub(super) fn requires_application_for_executable(
    original_path: &Path,
    resolved_path: &Path,
    header: &[u8],
) -> bool {
    CLASSIFIER.requires_application(original_path, resolved_path, header)
}

#[derive(Debug, Clone)]
struct GlobRule {
    weight: u32,
    mime: String,
    pattern: String,
    case_sensitive: bool,
}

#[derive(Default)]
pub(super) struct ExecutableClassifier {
    aliases: HashMap<String, String>,
    parents: HashMap<String, Vec<String>>,
    globs: Vec<GlobRule>,
    has_xdg_globs: bool,
}

impl ExecutableClassifier {
    fn from_xdg() -> Self {
        Self::from_mime_dirs(&xdg_mime_dirs())
    }

    pub(super) fn from_mime_dirs(mime_dirs: &[PathBuf]) -> Self {
        let mut classifier = Self::default();
        let mut noglobs: HashSet<String> = HashSet::new();

        for dir in mime_dirs {
            if !dir.is_dir() {
                continue;
            }
            classifier.load_aliases(&dir.join("aliases"));
            classifier.load_subclasses(&dir.join("subclasses"));
            classifier.load_globs(dir, &mut noglobs);
        }

        classifier
    }

    fn load_aliases(&mut self, path: &Path) {
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            if let (Some(alias), Some(canonical)) = (parts.next(), parts.next()) {
                self.aliases
                    .entry(alias.to_ascii_lowercase())
                    .or_insert_with(|| canonical.to_ascii_lowercase());
            }
        }
    }

    fn load_subclasses(&mut self, path: &Path) {
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            if let (Some(child), Some(parent)) = (parts.next(), parts.next()) {
                let list = self.parents.entry(child.to_ascii_lowercase()).or_default();
                let parent_lower = parent.to_ascii_lowercase();
                if !list.contains(&parent_lower) {
                    list.push(parent_lower);
                }
            }
        }
    }

    fn load_globs(&mut self, dir: &Path, noglobs: &mut HashSet<String>) {
        let globs2_path = dir.join("globs2");
        if let Ok(content) = fs::read_to_string(&globs2_path) {
            self.has_xdg_globs = true;
            let mut dir_noglobs = Vec::new();
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.splitn(4, ':');
                let (Some(weight_str), Some(mime_raw), Some(pattern)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let mime = self.canonical(mime_raw);
                if pattern == "__NOGLOBS__" {
                    dir_noglobs.push(mime);
                    continue;
                }
                if noglobs.contains(&mime) {
                    continue;
                }
                let Ok(weight) = weight_str.parse::<u32>() else {
                    continue;
                };
                let case_sensitive = parts
                    .next()
                    .is_some_and(|flags| flags.split(',').any(|flag| flag == "cs"));
                self.globs.push(GlobRule {
                    weight,
                    mime,
                    pattern: pattern.to_owned(),
                    case_sensitive,
                });
            }
            noglobs.extend(dir_noglobs);
            return;
        }

        let globs_path = dir.join("globs");
        if let Ok(content) = fs::read_to_string(&globs_path) {
            self.has_xdg_globs = true;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((mime_raw, pattern)) = line.split_once(':') else {
                    continue;
                };
                let mime = self.canonical(mime_raw);
                if pattern == "__NOGLOBS__" || noglobs.contains(&mime) {
                    continue;
                }
                self.globs.push(GlobRule {
                    weight: 50,
                    mime,
                    pattern: pattern.to_owned(),
                    case_sensitive: false,
                });
            }
        }
    }

    fn canonical(&self, raw_mime: &str) -> String {
        let mut current = raw_mime.trim().to_ascii_lowercase();
        for _ in 0..8 {
            let Some(next) = self.aliases.get(&current) else {
                break;
            };
            if next == &current {
                break;
            }
            current.clone_from(next);
        }
        current
    }

    pub(super) fn is_executable_mime(&self, raw_mime: &str) -> bool {
        let start = self.canonical(raw_mime);
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start.clone());
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            if EXECUTABLE_MIMES.contains(&current.as_str()) {
                return true;
            }
            if let Some(parents) = self.parents.get(&current) {
                for parent in parents {
                    let canon = self.canonical(parent);
                    if visited.insert(canon.clone()) {
                        queue.push_back(canon);
                    }
                }
            }
        }
        false
    }

    fn filename_mimes(&self, path: &Path) -> Vec<String> {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return Vec::new();
        };
        let lower_name = name.to_ascii_lowercase();
        let mut best_weight = 0u32;
        let mut best_len = 0usize;
        let mut matched: Vec<String> = Vec::new();

        for rule in &self.globs {
            let is_match = if rule.case_sensitive {
                glob_matches(&rule.pattern, name)
            } else {
                glob_matches(&rule.pattern.to_ascii_lowercase(), &lower_name)
            };
            if !is_match {
                continue;
            }
            let pat_len = rule.pattern.len();
            if rule.weight > best_weight || (rule.weight == best_weight && pat_len > best_len) {
                best_weight = rule.weight;
                best_len = pat_len;
                matched.clear();
                matched.push(rule.mime.clone());
            } else if rule.weight == best_weight
                && pat_len == best_len
                && !matched.contains(&rule.mime)
            {
                matched.push(rule.mime.clone());
            }
        }
        matched
    }

    pub(super) fn requires_application(
        &self,
        original_path: &Path,
        resolved_path: &Path,
        header: &[u8],
    ) -> bool {
        // 1. Content checks run first so an executable script or binary disguised under a
        // harmless document/media extension is always gated.
        if header.starts_with(b"#!") || header.starts_with(b"\x7fELF") {
            return true;
        }
        if looks_like_desktop_entry(header) {
            return true;
        }
        if let Some(kind) = infer::get(header)
            && self.is_executable_mime(kind.mime_type())
        {
            return true;
        }

        // 2. Respect explicit `user.mime_type` xattr when set on Linux.
        if let Some(xattr_mime) = read_xattr_mime_type(resolved_path)
            && self.is_executable_mime(&xattr_mime)
        {
            return true;
        }

        // 3. Consult Shared MIME-Info globs and subclass hierarchy for both symlink and target
        // names, with a minimal extension fallback if no XDG MIME database is installed.
        if self.has_xdg_globs {
            for candidate in [original_path, resolved_path] {
                if self
                    .filename_mimes(candidate)
                    .iter()
                    .any(|mime| self.is_executable_mime(mime))
                {
                    return true;
                }
            }
        } else {
            for candidate in [original_path, resolved_path] {
                if has_fallback_executable_extension(candidate) {
                    return true;
                }
            }
        }

        false
    }
}

fn xdg_mime_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| directories::BaseDirs::new().map(|base| base.data_dir().to_path_buf()));
    if let Some(home) = data_home {
        dirs.push(home.join("mime"));
    }

    let data_dirs: Vec<PathBuf> = env::var_os("XDG_DATA_DIRS")
        .map(|raw| {
            env::split_paths(&raw)
                .filter(|path| path.is_absolute())
                .collect::<Vec<_>>()
        })
        .filter(|list| !list.is_empty())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    for dir in data_dirs {
        let mime_dir = dir.join("mime");
        if !dirs.contains(&mime_dir) {
            dirs.push(mime_dir);
        }
    }
    dirs
}

fn looks_like_desktop_entry(header: &[u8]) -> bool {
    let text = header.strip_prefix(b"\xef\xbb\xbf").unwrap_or(header);
    let Ok(text) = std::str::from_utf8(text) else {
        return false;
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed == "[Desktop Entry]";
    }
    false
}

fn has_fallback_executable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            FALLBACK_EXECUTABLE_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

#[cfg(target_os = "linux")]
fn read_xattr_mime_type(path: &Path) -> Option<String> {
    let mut buf = [0u8; 256];
    let count = rustix::fs::getxattr(path, "user.mime_type", &mut buf).ok()?;
    let text = std::str::from_utf8(&buf[..count]).ok()?.trim();
    let (top, sub) = text.split_once('/')?;
    if top.is_empty() || sub.is_empty() || sub.contains('/') {
        return None;
    }
    Some(text.to_ascii_lowercase())
}

#[cfg(not(target_os = "linux"))]
fn read_xattr_mime_type(_path: &Path) -> Option<String> {
    None
}

/// Simple wildcard matcher supporting `*` and `?` for `shared-mime-info` `globs2` rules.
fn glob_matches(pattern: &str, text: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('*')
        && !suffix.contains(['*', '?', '['])
    {
        return text.ends_with(suffix);
    }
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0, 0);
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star_pi = Some(pi);
            pi += 1;
            star_ti = ti;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_classifier(root: &Path) -> ExecutableClassifier {
        let mime_dir = root.join("mime");
        fs::create_dir_all(&mime_dir).expect("create mime dir");
        fs::write(
            mime_dir.join("aliases"),
            "application/x-pdf application/pdf\ntext/x-shellscript application/x-shellscript\n",
        )
        .expect("write aliases");
        fs::write(
            mime_dir.join("subclasses"),
            concat!(
                "application/x-shellscript application/x-executable\n",
                "application/x-shellscript text/plain\n",
                "text/x-python application/x-executable\n",
                "text/markdown text/plain\n",
                "image/svg+xml application/xml\n",
            ),
        )
        .expect("write subclasses");
        fs::write(
            mime_dir.join("globs2"),
            concat!(
                "50:application/x-shellscript:*.sh\n",
                "50:text/x-python:*.py\n",
                "50:application/pdf:*.pdf\n",
                "50:text/plain:*.txt\n",
                "50:text/markdown:*.md\n",
                "50:image/svg+xml:*.svg\n",
                "50:text/html:*.html\n",
                "50:application/vnd.openxmlformats-officedocument.wordprocessingml.document:*.docx\n",
            ),
        )
        .expect("write globs2");
        ExecutableClassifier::from_mime_dirs(&[mime_dir])
    }

    #[test]
    fn allows_non_executable_documents_and_blocks_binaries_scripts_and_disguised_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let classifier = fixture_classifier(dir.path());

        let allowed = [
            ("report.pdf", b"%PDF-1.7\n1 0 obj\n".as_slice()),
            ("notes.txt", b"plain text notes\n"),
            ("README.md", b"# Heading\nbody\n"),
            ("icon.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            ("doc.docx", b"PK\x03\x04dummy-zip"),
            ("index.html", b"<!doctype html><html></html>"),
            (
                "cert.pem",
                b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n",
            ),
        ];
        for (name, content) in allowed {
            let path = dir.path().join(name);
            assert!(
                !classifier.requires_application(&path, &path, content),
                "expected document {name} with +x to open directly"
            );
        }

        let blocked = [
            ("run.sh", b"echo hello without shebang\n".as_slice()),
            ("tool.py", b"print('hi')\n"),
            ("disguised.pdf", b"\x7fELF\x02\x01\x01\x00\0\0\0\0\0\0\0\0"),
            ("disguised.txt", b"#!/bin/sh\nexit 0\n"),
            (
                "disguised.md",
                b"[Desktop Entry]\nType=Application\nExec=sh\n",
            ),
            ("win.bin", b"MZ\x90\x00\x03\x00\x00\x00"),
        ];
        for (name, content) in blocked {
            let path = dir.path().join(name);
            assert!(
                classifier.requires_application(&path, &path, content),
                "expected executable/disguised {name} to require application chooser"
            );
        }
    }

    #[test]
    fn resolves_mime_aliases_and_subclasses() {
        let dir = tempfile::tempdir().expect("tempdir");
        let classifier = fixture_classifier(dir.path());

        assert!(classifier.is_executable_mime("text/x-shellscript"));
        assert!(classifier.is_executable_mime("text/x-python"));
        assert!(!classifier.is_executable_mime("application/x-pdf"));
        assert!(!classifier.is_executable_mime("text/markdown"));
    }
}
