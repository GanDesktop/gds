//! Configurable visible file columns and their display order.

use serde::{Deserialize, Serialize};

/// A file table column whose visibility and order the user can configure.
///
/// The identifiers returned by [`Column::id`] are persisted, so they must stay
/// stable when variants are added or renamed in the UI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Column {
    Name,
    Type,
    Size,
    Owner,
    Group,
    Permissions,
    Modified,
    Accessed,
    DetailedType,
}

impl Column {
    /// Every column in canonical order.
    pub const ALL: [Self; 9] = [
        Self::Name,
        Self::Type,
        Self::Size,
        Self::Owner,
        Self::Group,
        Self::Permissions,
        Self::Modified,
        Self::Accessed,
        Self::DetailedType,
    ];

    /// Stable identifier persisted in settings and used as the table column id.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Type => "type",
            Self::Size => "size",
            Self::Owner => "owner",
            Self::Group => "group",
            Self::Permissions => "permissions",
            Self::Modified => "modified",
            Self::Accessed => "accessed",
            Self::DetailedType => "detailed-type",
        }
    }

    /// Fluent message key for the column header.
    ///
    /// It intentionally matches [`Column::id`] so the existing `name`, `type`,
    /// `size` and `modified` messages are reused; the interface supplies the
    /// remaining messages.
    pub const fn label_id(self) -> &'static str {
        self.id()
    }

    /// Resolve a persisted identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|column| column.id() == id)
    }

    /// The name column is always visible.
    pub const fn is_required(self) -> bool {
        matches!(self, Self::Name)
    }
}

/// Ordered visible columns. [`Column::Name`] is always present.
///
/// Deserialization removes duplicates, ignores unknown identifiers and prepends
/// a missing name column. Unknown identifiers are ignored so settings written by
/// a newer version with additional columns remain loadable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Layout {
    columns: Vec<Column>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            columns: vec![Column::Name, Column::Modified, Column::Type, Column::Size],
        }
    }
}

impl Layout {
    /// Build a normalized layout from the given column order.
    pub fn new(columns: impl IntoIterator<Item = Column>) -> Self {
        let mut layout = Self {
            columns: columns.into_iter().collect(),
        };
        layout.normalize();
        layout
    }

    /// Visible columns in display order.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn contains(&self, column: Column) -> bool {
        self.columns.contains(&column)
    }

    /// Append a hidden column. Returns whether the layout changed.
    pub fn show(&mut self, column: Column) -> bool {
        if self.contains(column) {
            return false;
        }
        self.columns.push(column);
        true
    }

    /// Hide a column. The name column cannot be hidden.
    /// Returns whether the layout changed.
    pub fn hide(&mut self, column: Column) -> bool {
        if column.is_required() || !self.contains(column) {
            return false;
        }
        self.columns.retain(|visible| *visible != column);
        true
    }

    /// Show or hide a column. Returns whether the layout changed.
    pub fn set_visible(&mut self, column: Column, visible: bool) -> bool {
        if visible {
            self.show(column)
        } else {
            self.hide(column)
        }
    }

    /// Move a visible column to `index`, clamped to the last position.
    /// Returns whether the order changed.
    pub fn move_to(&mut self, column: Column, index: usize) -> bool {
        let Some(current) = self.position(column) else {
            return false;
        };
        let target = index.min(self.columns.len() - 1);
        if current == target {
            return false;
        }
        self.columns.remove(current);
        self.columns.insert(target, column);
        true
    }

    /// Remove duplicates and prepend a missing name column.
    /// Returns whether the layout changed.
    pub fn normalize(&mut self) -> bool {
        let mut columns = Vec::with_capacity(self.columns.len().max(1));
        for column in self.columns.iter().copied() {
            if !columns.contains(&column) {
                columns.push(column);
            }
        }
        if !columns.contains(&Column::Name) {
            columns.insert(0, Column::Name);
        }
        if columns == self.columns {
            return false;
        }
        self.columns = columns;
        true
    }

    fn position(&self, column: Column) -> Option<usize> {
        self.columns.iter().position(|visible| *visible == column)
    }
}

impl<'de> Deserialize<'de> for Layout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let ids = Vec::<String>::deserialize(deserializer)?;
        let mut layout = Self {
            columns: ids.iter().filter_map(|id| Column::from_id(id)).collect(),
        };
        layout.normalize();
        Ok(layout)
    }
}

#[cfg(test)]
mod tests;
