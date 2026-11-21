//! Precompiled freedesktop `mime.cache` (v1.2) reader backed by `mmap`.
//!
//! Aligns MIME index lookups with GNOME (GIO) and KDE (Qt): aliases, parents,
//! literals, and generic icons use `O(log N)` binary search, and extensions use
//! the precompiled reverse-suffix tree rather than scanning all globs linearly.
//! Spec: <https://specifications.freedesktop.org/shared-mime-info/latest-single/>

use std::{cmp::Ordering, fs::File, path::Path};

use globset::{Glob, GlobMatcher};
use memmap2::Mmap;

use super::GlobMatch;

const HEADER_LEN: usize = 40;
const SUPPORTED_MAJOR: u16 = 1;
const SUPPORTED_MINOR: u16 = 2;
const CASE_SENSITIVE_FLAG: u32 = 0x100;

enum CacheBytes {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl CacheBytes {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Mapped(map) => map,
            Self::Owned(bytes) => bytes,
        }
    }
}

struct CompiledGlob {
    matcher: GlobMatcher,
    mime_offset: u32,
    weight: u32,
    pattern_len: usize,
    case_sensitive: bool,
}

pub(super) struct MimeCache {
    bytes: CacheBytes,
    alias_list_offset: usize,
    parent_list_offset: usize,
    literal_list_offset: usize,
    reverse_suffix_tree_offset: usize,
    magic_list_offset: usize,
    generic_icons_list_offset: usize,
    globs: Vec<CompiledGlob>,
}

impl MimeCache {
    /// Load `<mime_dir>/mime.cache` if present and valid v1.2 format.
    pub(super) fn load(mime_dir: &Path) -> Option<Self> {
        let path = mime_dir.join("mime.cache");
        let file = File::open(&path).ok()?;
        let len = file.metadata().ok()?.len();
        if len < HEADER_LEN as u64 {
            return None;
        }
        // SAFETY: `update-mime-database` updates `mime.cache` atomically via
        // write-and-rename; we only perform read-only bounds-checked indexing.
        let bytes = match unsafe { Mmap::map(&file) } {
            Ok(map) => CacheBytes::Mapped(map),
            Err(_) => CacheBytes::Owned(std::fs::read(&path).ok()?),
        };
        Self::from_bytes(bytes)
    }

    fn from_bytes(bytes: CacheBytes) -> Option<Self> {
        let data = bytes.as_slice();
        if data.len() < HEADER_LEN {
            return None;
        }
        let major = read_u16(data, 0)?;
        let minor = read_u16(data, 2)?;
        if major != SUPPORTED_MAJOR || minor != SUPPORTED_MINOR {
            return None;
        }
        let alias_list_offset = read_u32(data, 4)? as usize;
        let parent_list_offset = read_u32(data, 8)? as usize;
        let literal_list_offset = read_u32(data, 12)? as usize;
        let reverse_suffix_tree_offset = read_u32(data, 16)? as usize;
        let glob_list_offset = read_u32(data, 20)? as usize;
        let magic_list_offset = read_u32(data, 24)? as usize;
        let generic_icons_list_offset = read_u32(data, 36)? as usize;

        let globs = compile_globs(data, glob_list_offset);
        Some(Self {
            bytes,
            alias_list_offset,
            parent_list_offset,
            literal_list_offset,
            reverse_suffix_tree_offset,
            magic_list_offset,
            generic_icons_list_offset,
            globs,
        })
    }

    fn data(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub(super) fn unalias(&self, mime: &str) -> Option<&str> {
        binary_search_pair(self.data(), self.alias_list_offset, mime)
    }

    pub(super) fn generic_icon(&self, mime: &str) -> Option<&str> {
        binary_search_pair(self.data(), self.generic_icons_list_offset, mime)
    }

    pub(super) fn parents(&self, mime: &str) -> Vec<&str> {
        let data = self.data();
        let Some(parents_offset) = binary_search_u32(data, self.parent_list_offset, mime) else {
            return Vec::new();
        };
        let parents_offset = parents_offset as usize;
        let Some(count) = read_u32(data, parents_offset) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            let Some(offset) = read_u32(data, parents_offset + 4 + index * 4) else {
                break;
            };
            if let Some(parent) = read_cstr(data, offset as usize) {
                out.push(parent);
            }
        }
        out
    }

    pub(super) fn match_filename(&self, name: &str, out: &mut Vec<GlobMatch>) {
        self.match_literals(name, out);
        self.match_reverse_suffix_tree(name, out);
        self.match_globs(name, out);
    }

    pub(super) fn match_data(&self, sample: &[u8]) -> Option<(&str, u32)> {
        if sample.is_empty() {
            return None;
        }
        let data = self.data();
        let count = read_u32(data, self.magic_list_offset)? as usize;
        let first_match_offset = read_u32(data, self.magic_list_offset + 8)? as usize;

        for index in 0..count {
            let entry_offset = first_match_offset.checked_add(index * 16)?;
            let priority = read_u32(data, entry_offset)?;
            let mime_offset = read_u32(data, entry_offset + 4)? as usize;
            let n_matchlets = read_u32(data, entry_offset + 8)? as usize;
            let matchlets_offset = read_u32(data, entry_offset + 12)? as usize;
            if matchlets_match(data, matchlets_offset, n_matchlets, sample) {
                let mime = read_cstr(data, mime_offset)?;
                return Some((mime, priority));
            }
        }
        None
    }

    fn match_literals(&self, name: &str, out: &mut Vec<GlobMatch>) {
        self.lookup_literal(name, false, out);
        let lower = name.to_lowercase();
        if lower != name {
            self.lookup_literal(&lower, true, out);
        }
    }

    fn lookup_literal(
        &self,
        query: &str,
        require_case_insensitive: bool,
        out: &mut Vec<GlobMatch>,
    ) {
        let data = self.data();
        let Some(count) = read_u32(data, self.literal_list_offset).map(|v| v as usize) else {
            return;
        };
        if count == 0 {
            return;
        }
        let base = self.literal_list_offset + 4;
        let mut low = 0usize;
        let mut high = count;
        let mut found = None;
        while low < high {
            let mid = low + (high - low) / 2;
            let Some(lit_off) = read_u32(data, base + mid * 12) else {
                return;
            };
            let Some(lit) = read_cstr(data, lit_off as usize) else {
                return;
            };
            match lit.cmp(query) {
                Ordering::Less => low = mid + 1,
                Ordering::Greater => high = mid,
                Ordering::Equal => {
                    found = Some(mid);
                    break;
                }
            }
        }
        let Some(pivot) = found else {
            return;
        };
        let mut start = pivot;
        while start > 0 {
            let prev = start - 1;
            let Some(lit) =
                read_u32(data, base + prev * 12).and_then(|off| read_cstr(data, off as usize))
            else {
                break;
            };
            if lit == query {
                start = prev;
            } else {
                break;
            }
        }
        for idx in start..count {
            let entry = base + idx * 12;
            let Some(lit) = read_u32(data, entry).and_then(|off| read_cstr(data, off as usize))
            else {
                break;
            };
            if lit != query {
                break;
            }
            let Some(mime_off) = read_u32(data, entry + 4) else {
                continue;
            };
            let Some(wf) = read_u32(data, entry + 8) else {
                continue;
            };
            let case_sensitive = (wf & CASE_SENSITIVE_FLAG) != 0;
            if require_case_insensitive && case_sensitive {
                continue;
            }
            if let Some(mime) = read_cstr(data, mime_off as usize) {
                out.push(GlobMatch {
                    mime: mime.to_owned(),
                    weight: wf & 0xff,
                    pattern_len: lit.len(),
                });
            }
        }
    }

    fn match_reverse_suffix_tree(&self, name: &str, out: &mut Vec<GlobMatch>) {
        let data = self.data();
        let Some(n_roots) = read_u32(data, self.reverse_suffix_tree_offset) else {
            return;
        };
        let Some(first_root) = read_u32(data, self.reverse_suffix_tree_offset + 4) else {
            return;
        };
        let chars: Vec<char> = name.chars().collect();
        self.walk_suffix_nodes(
            first_root as usize,
            n_roots as usize,
            &chars,
            chars.len(),
            0,
            false,
            out,
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "recursive suffix-tree cursor avoids allocating per-step state"
    )]
    fn walk_suffix_nodes(
        &self,
        nodes_offset: usize,
        node_count: usize,
        chars: &[char],
        pos: usize,
        matched_bytes: usize,
        used_case_folding: bool,
        out: &mut Vec<GlobMatch>,
    ) {
        let data = self.data();
        if node_count == 0 {
            return;
        }
        // Leaf nodes have character == 0 and are sorted first in each sibling block.
        let mut first_branch = 0usize;
        while first_branch < node_count {
            let entry = nodes_offset + first_branch * 12;
            let Some(ch) = read_u32(data, entry) else {
                return;
            };
            if ch != 0 {
                break;
            }
            if matched_bytes > 0
                && let (Some(mime_off), Some(wf)) =
                    (read_u32(data, entry + 4), read_u32(data, entry + 8))
            {
                let case_sensitive = (wf & CASE_SENSITIVE_FLAG) != 0;
                if (!case_sensitive || !used_case_folding)
                    && let Some(mime) = read_cstr(data, mime_off as usize)
                {
                    out.push(GlobMatch {
                        mime: mime.to_owned(),
                        weight: wf & 0xff,
                        pattern_len: matched_bytes,
                    });
                }
            }
            first_branch += 1;
        }

        if pos == 0 || first_branch >= node_count {
            return;
        }

        let target = chars[pos - 1];
        if let Some((n_children, children_off)) =
            binary_search_suffix_branch(data, nodes_offset, first_branch, node_count, target as u32)
        {
            self.walk_suffix_nodes(
                children_off,
                n_children,
                chars,
                pos - 1,
                matched_bytes + target.len_utf8(),
                used_case_folding,
                out,
            );
        }

        let mut lower_iter = target.to_lowercase();
        if let (Some(lower), None) = (lower_iter.next(), lower_iter.next())
            && lower != target
            && let Some((n_children, children_off)) = binary_search_suffix_branch(
                data,
                nodes_offset,
                first_branch,
                node_count,
                lower as u32,
            )
        {
            self.walk_suffix_nodes(
                children_off,
                n_children,
                chars,
                pos - 1,
                matched_bytes + lower.len_utf8(),
                true,
                out,
            );
        }
    }

    fn match_globs(&self, name: &str, out: &mut Vec<GlobMatch>) {
        if self.globs.is_empty() {
            return;
        }
        let data = self.data();
        let mut lowered: Option<String> = None;
        for glob in &self.globs {
            let matched = if glob.case_sensitive {
                glob.matcher.is_match(name)
            } else {
                let candidate = lowered.get_or_insert_with(|| name.to_lowercase());
                glob.matcher.is_match(candidate.as_str())
            };
            if matched && let Some(mime) = read_cstr(data, glob.mime_offset as usize) {
                out.push(GlobMatch {
                    mime: mime.to_owned(),
                    weight: glob.weight,
                    pattern_len: glob.pattern_len,
                });
            }
        }
    }
}

fn compile_globs(data: &[u8], glob_list_offset: usize) -> Vec<CompiledGlob> {
    let Some(count) = read_u32(data, glob_list_offset).map(|v| v as usize) else {
        return Vec::new();
    };
    let mut compiled = Vec::with_capacity(count);
    let base = glob_list_offset + 4;
    for index in 0..count {
        let entry = base + index * 12;
        let (Some(glob_off), Some(mime_off), Some(wf)) = (
            read_u32(data, entry),
            read_u32(data, entry + 4),
            read_u32(data, entry + 8),
        ) else {
            break;
        };
        let Some(pattern) = read_cstr(data, glob_off as usize) else {
            continue;
        };
        let case_sensitive = (wf & CASE_SENSITIVE_FLAG) != 0;
        let effective_pattern = if case_sensitive {
            pattern.to_owned()
        } else {
            pattern.to_lowercase()
        };
        let Ok(glob) = Glob::new(&effective_pattern) else {
            continue;
        };
        compiled.push(CompiledGlob {
            matcher: glob.compile_matcher(),
            mime_offset: mime_off,
            weight: wf & 0xff,
            pattern_len: pattern.len(),
            case_sensitive,
        });
    }
    compiled
}

fn binary_search_suffix_branch(
    data: &[u8],
    nodes_offset: usize,
    start: usize,
    end: usize,
    target_char: u32,
) -> Option<(usize, usize)> {
    let mut low = start;
    let mut high = end;
    while low < high {
        let mid = low + (high - low) / 2;
        let entry = nodes_offset + mid * 12;
        let ch = read_u32(data, entry)?;
        match ch.cmp(&target_char) {
            Ordering::Less => low = mid + 1,
            Ordering::Greater => high = mid,
            Ordering::Equal => {
                let n_children = read_u32(data, entry + 4)? as usize;
                let children_off = read_u32(data, entry + 8)? as usize;
                return Some((n_children, children_off));
            }
        }
    }
    None
}

fn binary_search_pair<'a>(data: &'a [u8], list_offset: usize, key: &str) -> Option<&'a str> {
    let value_offset = binary_search_u32(data, list_offset, key)? as usize;
    read_cstr(data, value_offset)
}

fn binary_search_u32(data: &[u8], list_offset: usize, key: &str) -> Option<u32> {
    let count = read_u32(data, list_offset)? as usize;
    let base = list_offset + 4;
    let mut low = 0usize;
    let mut high = count;
    while low < high {
        let mid = low + (high - low) / 2;
        let entry = base + mid * 8;
        let key_off = read_u32(data, entry)? as usize;
        let candidate = read_cstr(data, key_off)?;
        match candidate.cmp(key) {
            Ordering::Less => low = mid + 1,
            Ordering::Greater => high = mid,
            Ordering::Equal => return read_u32(data, entry + 4),
        }
    }
    None
}

fn matchlets_match(data: &[u8], offset: usize, count: usize, sample: &[u8]) -> bool {
    for index in 0..count {
        let Some(entry) = offset.checked_add(index * 32) else {
            return false;
        };
        if matchlet_matches(data, entry, sample) {
            return true;
        }
    }
    false
}

fn matchlet_matches(data: &[u8], entry: usize, sample: &[u8]) -> bool {
    let (
        Some(range_start),
        Some(range_length),
        Some(word_size),
        Some(value_length),
        Some(value_offset),
        Some(mask_offset),
        Some(n_children),
        Some(children_offset),
    ) = (
        read_u32(data, entry),
        read_u32(data, entry + 4),
        read_u32(data, entry + 8),
        read_u32(data, entry + 12),
        read_u32(data, entry + 16),
        read_u32(data, entry + 20),
        read_u32(data, entry + 24),
        read_u32(data, entry + 28),
    )
    else {
        return false;
    };

    let range_start = range_start as usize;
    let range_length = range_length as usize;
    let value_length = value_length as usize;
    let value_offset = value_offset as usize;
    let mask_offset = mask_offset as usize;
    let word_size = word_size as usize;

    let Some(value) = data.get(value_offset..value_offset.saturating_add(value_length)) else {
        return false;
    };
    let mask = if mask_offset == 0 {
        None
    } else {
        let Some(mask_slice) = data.get(mask_offset..mask_offset.saturating_add(value_length))
        else {
            return false;
        };
        Some(mask_slice)
    };

    if range_length == 0 || range_start.saturating_add(value_length) > sample.len() {
        return false;
    }
    let max_offset = sample
        .len()
        .saturating_sub(value_length)
        .min(range_start.saturating_add(range_length.saturating_sub(1)));

    for pos in range_start..=max_offset {
        let slice = &sample[pos..pos + value_length];
        if bytes_match(slice, value, mask, word_size) {
            if n_children == 0 {
                return true;
            }
            if matchlets_match(data, children_offset as usize, n_children as usize, sample) {
                return true;
            }
        }
    }
    false
}

fn bytes_match(sample: &[u8], value: &[u8], mask: Option<&[u8]>, word_size: usize) -> bool {
    let swap_words = cfg!(target_endian = "little")
        && matches!(word_size, 2 | 4)
        && value.len().is_multiple_of(word_size);

    for (idx, &sample_byte) in sample.iter().enumerate() {
        let pat_idx = if swap_words {
            let chunk_start = (idx / word_size) * word_size;
            chunk_start + (word_size - 1 - (idx % word_size))
        } else {
            idx
        };
        let expected = value[pat_idx];
        let matched = match mask {
            Some(mask_bytes) => {
                (sample_byte & mask_bytes[pat_idx]) == (expected & mask_bytes[pat_idx])
            }
            None => sample_byte == expected,
        };
        if !matched {
            return false;
        }
    }
    true
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

fn read_cstr(data: &[u8], offset: usize) -> Option<&str> {
    let tail = data.get(offset..)?;
    let nul = tail.iter().position(|&b| b == 0)?;
    std::str::from_utf8(&tail[..nul]).ok()
}
