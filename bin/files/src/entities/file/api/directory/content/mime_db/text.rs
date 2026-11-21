//! Plain-text `shared-mime-info` directory fallback when `mime.cache` is absent.
//!
//! Reads `globs2`/`globs`, `aliases`, `subclasses`, `generic-icons`, and `magic`
//! directly from a single `<base>/mime` directory so fixtures and uncompiled XDG
//! directories share the exact same query semantics as `mime.cache`.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use globset::{Glob, GlobMatcher};
use xdg_mime::SharedMimeInfo;

use super::GlobMatch;

enum GlobPattern {
    Literal {
        text: String,
        case_sensitive: bool,
    },
    Suffix {
        suffix: String,
        case_sensitive: bool,
    },
    Full {
        matcher: GlobMatcher,
        len: usize,
        case_sensitive: bool,
    },
}

struct TextGlob {
    pattern: GlobPattern,
    mime: String,
    weight: u32,
}

pub(super) struct TextIndex {
    base_dir: PathBuf,
    has_magic: bool,
    aliases: HashMap<String, String>,
    parents: HashMap<String, Vec<String>>,
    generic_icons: HashMap<String, String>,
    globs: Vec<TextGlob>,
    /// Delegate binary `magic` file parsing/matching to `xdg-mime` lazily so
    /// directories without `magic` (or filename-only lookups) never re-read the text indexes.
    magic_db: OnceLock<SharedMimeInfo>,
}

impl TextIndex {
    pub(super) fn load(base_dir: &Path) -> Self {
        let mime_dir = base_dir.join("mime");
        let has_magic = mime_dir.join("magic").is_file();
        let aliases = read_aliases(&mime_dir.join("aliases"));
        let parents = read_subclasses(&mime_dir.join("subclasses"));
        let generic_icons = read_generic_icons(&mime_dir.join("generic-icons"));
        let globs = read_globs(&mime_dir);
        Self {
            base_dir: base_dir.to_path_buf(),
            has_magic,
            aliases,
            parents,
            generic_icons,
            globs,
            magic_db: OnceLock::new(),
        }
    }

    pub(super) fn unalias(&self, mime: &str) -> Option<&str> {
        self.aliases.get(mime).map(String::as_str)
    }

    pub(super) fn generic_icon(&self, mime: &str) -> Option<&str> {
        self.generic_icons.get(mime).map(String::as_str)
    }

    pub(super) fn parents(&self, mime: &str) -> Vec<&str> {
        self.parents
            .get(mime)
            .map(|list| list.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    pub(super) fn match_filename(&self, name: &str, out: &mut Vec<GlobMatch>) {
        let mut lowered: Option<String> = None;
        for entry in &self.globs {
            let pattern_len = match &entry.pattern {
                GlobPattern::Literal {
                    text,
                    case_sensitive,
                } => {
                    let matched = if *case_sensitive {
                        name == text
                    } else {
                        let candidate = lowered.get_or_insert_with(|| name.to_lowercase());
                        candidate.as_str() == text.as_str()
                    };
                    matched.then_some(text.len())
                }
                GlobPattern::Suffix {
                    suffix,
                    case_sensitive,
                } => {
                    let matched = if *case_sensitive {
                        name.ends_with(suffix.as_str())
                    } else {
                        let candidate = lowered.get_or_insert_with(|| name.to_lowercase());
                        candidate.ends_with(suffix.as_str())
                    };
                    matched.then_some(suffix.len())
                }
                GlobPattern::Full {
                    matcher,
                    len,
                    case_sensitive,
                } => {
                    let matched = if *case_sensitive {
                        matcher.is_match(name)
                    } else {
                        let candidate = lowered.get_or_insert_with(|| name.to_lowercase());
                        matcher.is_match(candidate.as_str())
                    };
                    matched.then_some(*len)
                }
            };
            if let Some(pattern_len) = pattern_len {
                out.push(GlobMatch {
                    mime: entry.mime.clone(),
                    weight: entry.weight,
                    pattern_len,
                });
            }
        }
    }

    pub(super) fn match_data(&self, sample: &[u8]) -> Option<(String, u32)> {
        if sample.is_empty() || !self.has_magic {
            return None;
        }
        self.magic_db
            .get_or_init(|| SharedMimeInfo::new_for_directory(&self.base_dir))
            .get_mime_type_for_data(sample)
            .map(|(mime, priority)| (mime.essence_str().to_owned(), priority))
    }
}

fn read_lines(path: &Path) -> Option<impl Iterator<Item = String> + use<>> {
    let file = File::open(path).ok()?;
    Some(
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter(|line| !line.is_empty() && !line.starts_with('#')),
    )
}

fn read_aliases(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(lines) = read_lines(path) else {
        return map;
    };
    for line in lines {
        let mut parts = line.split_whitespace();
        let (Some(alias), Some(canonical), None) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        map.entry(alias.to_owned())
            .or_insert_with(|| canonical.to_owned());
    }
    map
}

fn read_subclasses(path: &Path) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let Some(lines) = read_lines(path) else {
        return map;
    };
    for line in lines {
        let mut parts = line.split_whitespace();
        let (Some(child), Some(parent), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let entry = map.entry(child.to_owned()).or_default();
        if !entry.iter().any(|existing| existing == parent) {
            entry.push(parent.to_owned());
        }
    }
    map
}

fn read_generic_icons(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(lines) = read_lines(path) else {
        return map;
    };
    for line in lines {
        let Some((mime, icon)) = line.split_once(':') else {
            continue;
        };
        let mime = mime.trim();
        let icon = icon.trim();
        if !mime.is_empty() && !icon.is_empty() {
            map.entry(mime.to_owned())
                .or_insert_with(|| icon.to_owned());
        }
    }
    map
}

fn read_globs(mime_dir: &Path) -> Vec<TextGlob> {
    if let Some(lines) = read_lines(&mime_dir.join("globs2")) {
        let mut globs = Vec::new();
        for line in lines {
            let mut parts = line.split(':');
            let (Some(weight), Some(mime), Some(pattern)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Ok(weight) = weight.trim().parse::<u32>() else {
                continue;
            };
            let case_sensitive = parts
                .next()
                .is_some_and(|flags| flags.split(',').any(|flag| flag.trim() == "cs"));
            if let Some(parsed) = parse_glob_pattern(pattern, case_sensitive) {
                globs.push(TextGlob {
                    pattern: parsed,
                    mime: mime.trim().to_owned(),
                    weight,
                });
            }
        }
        return globs;
    }

    let Some(lines) = read_lines(&mime_dir.join("globs")) else {
        return Vec::new();
    };
    let mut globs = Vec::new();
    for line in lines {
        let Some((mime, pattern)) = line.split_once(':') else {
            continue;
        };
        if let Some(parsed) = parse_glob_pattern(pattern, false) {
            globs.push(TextGlob {
                pattern: parsed,
                mime: mime.trim().to_owned(),
                weight: 50,
            });
        }
    }
    globs
}

fn parse_glob_pattern(pattern: &str, case_sensitive: bool) -> Option<GlobPattern> {
    if pattern.is_empty() {
        return None;
    }
    if let Some(rest) = pattern.strip_prefix('*')
        && !rest.is_empty()
        && !rest.contains(['*', '?', '[', '\\'])
    {
        return Some(GlobPattern::Suffix {
            suffix: if case_sensitive {
                rest.to_owned()
            } else {
                rest.to_lowercase()
            },
            case_sensitive,
        });
    }
    if !pattern.contains(['*', '?', '[', '\\']) {
        return Some(GlobPattern::Literal {
            text: if case_sensitive {
                pattern.to_owned()
            } else {
                pattern.to_lowercase()
            },
            case_sensitive,
        });
    }
    let effective = if case_sensitive {
        pattern.to_owned()
    } else {
        pattern.to_lowercase()
    };
    let matcher = Glob::new(&effective).ok()?.compile_matcher();
    Some(GlobPattern::Full {
        matcher,
        len: pattern.len(),
        case_sensitive,
    })
}
