use super::model::{Entry, Page};

/// Filtered rows projected for directory picker display.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryRows {
    pub entries: Vec<Entry>,
}

impl DirectoryRows {
    /// Builds filtered display rows from `page`, visibility toggle `show_hidden`, and search
    /// `query` matched strictly against each entry's display `location.label`.
    #[must_use]
    pub fn from_page(page: Option<&Page>, show_hidden: bool, query: &str) -> Self {
        let Some(page) = page else {
            return Self::default();
        };
        let normalized_query = query.to_lowercase();
        let entries = page
            .entries
            .iter()
            .filter(|entry| {
                (show_hidden || !entry.hidden)
                    && (normalized_query.is_empty()
                        || entry
                            .location
                            .label
                            .to_lowercase()
                            .contains(&normalized_query))
            })
            .cloned()
            .collect();
        Self { entries }
    }

    /// Returns the number of visible filtered rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when no visible rows match.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the row at `index`, if within bounds.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    /// Finds the visible row index matching `path`, if present.
    #[must_use]
    pub fn index_of_path(&self, path: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.location.path == path)
    }
}
