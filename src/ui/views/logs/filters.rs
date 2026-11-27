//! Filtering state and target extraction for the diagnostic log viewer.

use std::collections::BTreeSet;

use crate::service::logs::{Entry, Level};

pub(crate) const ALL_KEY: &str = "__all__";

/// Active log filtering parameters applied to the retained store snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FilterState {
    pub level: Option<Level>,
    pub target: Option<String>,
    pub query: String,
}

impl FilterState {
    #[must_use]
    pub fn matches(&self, entry: &Entry) -> bool {
        let level_ok = self.level.is_none_or(|level| level == entry.level);
        if !level_ok {
            return false;
        }
        let target_ok = self
            .target
            .as_deref()
            .is_none_or(|target| target == entry.target);
        if !target_ok {
            return false;
        }
        if self.query.is_empty() {
            return true;
        }
        let needle = self.query.to_lowercase();
        entry.message.to_lowercase().contains(&needle)
            || entry.target.to_lowercase().contains(&needle)
    }

    #[must_use]
    pub fn visible_indices(&self, entries: &[Entry]) -> Vec<usize> {
        entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| self.matches(entry).then_some(index))
            .collect()
    }
}

#[must_use]
pub(crate) fn targets(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| entry.target.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[must_use]
pub(crate) const fn level_to_key(level: Option<Level>) -> &'static str {
    match level {
        None => ALL_KEY,
        Some(Level::Trace) => "TRACE",
        Some(Level::Debug) => "DEBUG",
        Some(Level::Info) => "INFO",
        Some(Level::Warn) => "WARN",
        Some(Level::Error) => "ERROR",
    }
}

#[must_use]
pub(crate) fn key_to_level(key: &str) -> Option<Level> {
    match key {
        "TRACE" => Some(Level::Trace),
        "DEBUG" => Some(Level::Debug),
        "INFO" => Some(Level::Info),
        "WARN" => Some(Level::Warn),
        "ERROR" => Some(Level::Error),
        _ => None,
    }
}
