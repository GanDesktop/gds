use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

use super::super::directory::{Entry, EntryKind, LinkTarget};

pub use super::columns::Column as Key;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    #[serde(alias = "ascending")]
    Ascending,
    #[serde(alias = "descending")]
    Descending,
}

impl Direction {
    fn apply(self, order: Ordering) -> Ordering {
        match self {
            Self::Ascending => order,
            Self::Descending => order.reverse(),
        }
    }

    fn reversed(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub key: Key,
    pub direction: Direction,
}

impl Sort {
    /// Whether the sort matches the canonical directories-first, ascending-name order
    /// already maintained by [`super::super::directory::DirectorySnapshot`].
    pub const fn is_canonical(self) -> bool {
        matches!(self.key, Key::Name) && matches!(self.direction, Direction::Ascending)
    }

    pub fn next(self, key: Key) -> Self {
        Self {
            key,
            direction: if self.key == key {
                self.direction.reversed()
            } else {
                Direction::Ascending
            },
        }
    }

    /// Reorder valid entry indices without changing entries or their path identities.
    /// Directories and available metadata retain their priority in both directions.
    pub fn sort_indices(&self, entries: &[Entry], visible: &mut [usize]) {
        visible.sort_by(|&left, &right| self.compare(&entries[left], &entries[right]));
    }

    fn compare(&self, left: &Entry, right: &Entry) -> Ordering {
        let directories_first =
            (right.kind == EntryKind::Directory).cmp(&(left.kind == EntryKind::Directory));
        if directories_first != Ordering::Equal {
            return directories_first;
        }
        let primary = match self.key {
            Key::Name => self.direction.apply(left.name.cmp(&right.name)),
            Key::Modified => self.optional(left.modified.as_ref(), right.modified.as_ref()),
            Key::Accessed => self.optional(
                left.details.accessed.as_ref(),
                right.details.accessed.as_ref(),
            ),
            Key::Size => self.compare_size(left, right),
            Key::Permissions => self.optional(
                left.details
                    .permissions
                    .as_ref()
                    .map(|permissions| permissions.mode)
                    .as_ref(),
                right
                    .details
                    .permissions
                    .as_ref()
                    .map(|permissions| permissions.mode)
                    .as_ref(),
            ),
            Key::Owner => self.compare_owner(left, right),
            Key::Group => self.compare_group(left, right),
            Key::Type => self.compare_type(left, right),
            Key::DetailedType => self.optional(
                Self::format_name(left).as_ref(),
                Self::format_name(right).as_ref(),
            ),
        };
        primary
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.as_os_str().cmp(right.path.as_os_str()))
    }

    fn optional<T: Ord>(&self, left: Option<&T>, right: Option<&T>) -> Ordering {
        match (left, right) {
            (Some(left), Some(right)) => self.direction.apply(left.cmp(right)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    /// The size column shows bytes for files and the direct child count for
    /// directories, so ordering uses the same value the cell displays.
    fn compare_size(&self, left: &Entry, right: &Entry) -> Ordering {
        self.optional(
            Self::size_value(left).as_ref(),
            Self::size_value(right).as_ref(),
        )
    }

    fn size_value(entry: &Entry) -> Option<u64> {
        if entry.kind == EntryKind::Directory {
            entry
                .details
                .children
                .map(|children| u64::from(children.count))
        } else {
            entry.size
        }
    }

    fn compare_owner(&self, left: &Entry, right: &Entry) -> Ordering {
        let left = left.details.owner.as_ref();
        let right = right.details.owner.as_ref();
        match (left, right) {
            (Some(left), Some(right)) => self.owner_order(
                left.name.as_deref(),
                right.name.as_deref(),
                left.uid,
                right.uid,
            ),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    fn compare_group(&self, left: &Entry, right: &Entry) -> Ordering {
        let left = left.details.group.as_ref();
        let right = right.details.group.as_ref();
        match (left, right) {
            (Some(left), Some(right)) => self.owner_order(
                left.name.as_deref(),
                right.name.as_deref(),
                left.gid,
                right.gid,
            ),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    /// Names sort by text and keep unresolved numeric identities last; the
    /// numeric id only breaks ties between two unresolved identities.
    fn owner_order(
        &self,
        left_name: Option<&str>,
        right_name: Option<&str>,
        left_id: u32,
        right_id: u32,
    ) -> Ordering {
        match (left_name, right_name) {
            (Some(left), Some(right)) => self.direction.apply(left.cmp(right)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => self.direction.apply(left_id.cmp(&right_id)),
        }
    }

    fn compare_type(&self, left: &Entry, right: &Entry) -> Ordering {
        let kinds = self.optional(
            Self::kind_rank(left.kind).as_ref(),
            Self::kind_rank(right.kind).as_ref(),
        );
        if kinds != Ordering::Equal {
            return kinds;
        }
        match left.kind {
            EntryKind::File => self.optional(
                Self::category_rank(left).as_ref(),
                Self::category_rank(right).as_ref(),
            ),
            EntryKind::SymbolicLink => {
                let targets = self.optional(
                    Self::target_status_rank(left).as_ref(),
                    Self::target_status_rank(right).as_ref(),
                );
                if targets != Ordering::Equal {
                    return targets;
                }
                self.optional(
                    Self::target_category_rank(left).as_ref(),
                    Self::target_category_rank(right).as_ref(),
                )
            }
            _ => Ordering::Equal,
        }
    }

    fn kind_rank(kind: EntryKind) -> Option<u8> {
        match kind {
            EntryKind::Directory => Some(0),
            EntryKind::File => Some(1),
            EntryKind::SymbolicLink => Some(2),
            EntryKind::Other => Some(3),
            EntryKind::Unknown => None,
        }
    }

    /// Content category of the entry itself or, for a symlink, of its target.
    fn category_rank(entry: &Entry) -> Option<u8> {
        let content = entry.details.display_content()?;
        Some(content.category as u8)
    }

    fn target_status_rank(entry: &Entry) -> Option<u8> {
        let target = entry.details.target.as_ref()?;
        Some(match target {
            LinkTarget::Resolved { .. } => 0,
            LinkTarget::Broken => 1,
            LinkTarget::Unreachable => 2,
        })
    }

    /// Directories first, then files grouped by content category; anything else
    /// the target could be stays last.
    fn target_category_rank(entry: &Entry) -> Option<u8> {
        let LinkTarget::Resolved { kind, .. } = entry.details.target.as_ref()? else {
            return None;
        };
        match kind {
            EntryKind::Directory => Some(0),
            EntryKind::File => Some(1 + Self::category_rank(entry)?),
            _ => None,
        }
    }

    fn format_name(entry: &Entry) -> Option<&str> {
        entry.details.display_content()?.format.as_deref()
    }
}

#[cfg(test)]
mod tests;
