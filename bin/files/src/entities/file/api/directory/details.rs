//! Presentation metadata for a directory entry.
//!
//! Every value that the platform cannot provide stays `None`. Callers must show
//! such a value as unknown instead of inventing one; in particular an entry whose
//! metadata could not be read keeps its place in the listing with empty details.

use std::{borrow::Cow, fs, time::SystemTime};

use super::{ChildCount, EntryKind, identity::IdentityCache};

/// Everything the table and details panel need beyond the entry name and size.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryDetails {
    /// Last access time, sampled before any content probe reads the entry.
    pub accessed: Option<SystemTime>,
    /// Birth time, only when the platform and filesystem report one. A change
    /// time must never stand in for it.
    pub created: Option<SystemTime>,
    pub owner: Option<Owner>,
    pub group: Option<Group>,
    pub permissions: Option<Permissions>,
    /// `true` when the current effective user cannot write the entry.
    pub read_only: Option<bool>,
    /// Broad category and concrete format of the entry content.
    pub content: Option<ContentType>,
    /// Symlink target classification; `None` for entries that are not symlinks.
    pub target: Option<LinkTarget>,
    /// Direct child count, only for a directory that could be read. `None` for
    /// other entries and for unreadable directories, which must show as unknown.
    pub children: Option<ChildCount>,
}

impl EntryDetails {
    /// Sample the metadata of one entry. The result never fails: unavailable
    /// fields stay `None` so a listing is not lost over missing metadata.
    pub(super) fn from_metadata(metadata: &fs::Metadata, identities: &mut IdentityCache) -> Self {
        let mut details = Self {
            accessed: metadata.accessed().ok(),
            created: metadata.created().ok(),
            ..Self::default()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let uid = metadata.uid();
            let gid = metadata.gid();
            let permissions = Permissions::from_mode(metadata.mode());
            details.owner = Some(Owner {
                uid,
                name: identities.user_name(uid),
                is_current_user: identities.is_current_user(uid),
            });
            details.group = Some(Group {
                gid,
                name: identities.group_name(gid),
            });
            details.read_only = Some(!identities.can_write(uid, gid, permissions.mode));
            details.permissions = Some(permissions);
        }
        #[cfg(not(unix))]
        {
            let _ = identities;
        }
        details
    }

    /// Content shown for the entry itself, or for a symlink target when the link
    /// itself has no content of its own.
    pub fn display_content(&self) -> Option<&ContentType> {
        self.content
            .as_ref()
            .or_else(|| self.target.as_ref()?.content.as_ref())
    }
}

/// Owner of an entry, identified by numeric id and, when resolvable, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub uid: u32,
    /// Login name; `None` when the id has no database entry or lookup failed.
    pub name: Option<String>,
    /// Compared against the effective user id of the process, not `$USER`.
    pub is_current_user: bool,
}

impl Owner {
    /// Name for display; the numeric id is the documented fallback.
    pub fn label(&self) -> Cow<'_, str> {
        match &self.name {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned(self.uid.to_string()),
        }
    }
}

/// Owning group of an entry, identified by numeric id and, when resolvable, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub gid: u32,
    /// Group name; `None` when the id has no database entry or lookup failed.
    pub name: Option<String>,
}

impl Group {
    /// Name for display; the numeric id is the documented fallback.
    pub fn label(&self) -> Cow<'_, str> {
        match &self.name {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned(self.gid.to_string()),
        }
    }
}

/// Permission bits of one owner class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Access {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Access {
    fn from_mode(mode: u32, read: u32, write: u32, execute: u32) -> Self {
        Self {
            read: mode & read != 0,
            write: mode & write != 0,
            execute: mode & execute != 0,
        }
    }
}

/// Unix permission and special bits of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// Permission and special bits only (`st_mode & 0o7777`), never the file type.
    pub mode: u32,
    pub owner: Access,
    pub group: Access,
    pub other: Access,
    pub setuid: bool,
    pub setgid: bool,
    pub sticky: bool,
}

impl Permissions {
    pub fn from_mode(mode: u32) -> Self {
        let mode = mode & 0o7777;
        Self {
            mode,
            owner: Access::from_mode(mode, 0o400, 0o200, 0o100),
            group: Access::from_mode(mode, 0o040, 0o020, 0o010),
            other: Access::from_mode(mode, 0o004, 0o002, 0o001),
            setuid: mode & 0o4000 != 0,
            setgid: mode & 0o2000 != 0,
            sticky: mode & 0o1000 != 0,
        }
    }

    /// Nine `rwx` characters. A setuid/setgid/sticky bit replaces the matching
    /// execute letter with `s`/`t`, or with `S`/`T` when execute is not set, as
    /// `ls -l` does.
    pub fn symbols(&self) -> String {
        let mut symbols = String::with_capacity(9);
        push_class(&mut symbols, self.owner, self.setuid, 's');
        push_class(&mut symbols, self.group, self.setgid, 's');
        push_class(&mut symbols, self.other, self.sticky, 't');
        symbols
    }

    /// Four-digit octal form such as `0644`, `4755` or `1777`.
    pub fn octal(&self) -> String {
        format!("{:04o}", self.mode)
    }
}

fn push_class(out: &mut String, access: Access, special: bool, special_char: char) {
    out.push(if access.read { 'r' } else { '-' });
    out.push(if access.write { 'w' } else { '-' });
    out.push(match (access.execute, special) {
        (true, true) => special_char,
        (false, true) => special_char.to_ascii_uppercase(),
        (true, false) => 'x',
        (false, false) => '-',
    });
}

/// Broad kind of content shown in the type column.
///
/// The declaration order is the order used when sorting by type, so variants are
/// grouped from text to binary rather than alphabetically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentCategory {
    Text,
    Code,
    Document,
    Image,
    Audio,
    Video,
    Archive,
    Executable,
    Font,
    /// Recognised as non-text content, but no narrower category applies.
    Binary,
}

/// Broad category plus the concrete format name for one piece of content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentType {
    pub category: ContentCategory,
    /// Format name shown verbatim by the interface, e.g. `PNG`, `Rust`, `MPEG-4`.
    /// `None` when only the broad category is known.
    pub format: Option<String>,
    /// MIME type, used as the fallback label when no friendly name is known.
    pub mime: Option<String>,
}

impl ContentType {
    pub(super) fn new(category: ContentCategory, format: Option<&str>, mime: Option<&str>) -> Self {
        Self {
            category,
            format: format.map(str::to_owned),
            mime: mime.map(str::to_owned),
        }
    }

    /// Format name for the details column; the MIME type is the fallback when no
    /// friendly name is known.
    pub fn label(&self) -> Option<&str> {
        self.format.as_deref().or(self.mime.as_deref())
    }
}

/// Result of inspecting a symlink without replacing the link with its target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    pub status: LinkStatus,
    /// Target entry kind; `Unknown` unless `status` is [`LinkStatus::Resolved`].
    pub kind: EntryKind,
    /// Content of a resolved regular-file target.
    pub content: Option<ContentType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    /// The target exists; `kind` and `content` describe it.
    Resolved,
    /// The target does not exist.
    Broken,
    /// The target could not be inspected, for example a permission error or a loop.
    Unreachable,
}
