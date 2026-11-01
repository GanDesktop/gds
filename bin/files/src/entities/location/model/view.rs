use std::collections::BTreeMap;

use super::address::Location;
use crate::entities::file::x::location::{Layout, Sort, ViewMode};

/// Logical pixel widths indexed by column identity, including hidden columns.
pub(crate) type ColumnWidths = BTreeMap<String, f32>;

/// Missing fields inherit the application defaults stored in the database.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ViewSettings {
    pub columns: Option<Layout>,
    pub widths: ColumnWidths,
    pub sort: Option<Sort>,
    pub details: Option<bool>,
    pub mode: Option<ViewMode>,
}

impl ViewSettings {
    pub(crate) fn defaults() -> Self {
        Self {
            columns: Some(Layout::default()),
            sort: Some(Sort::default()),
            details: Some(false),
            mode: Some(ViewMode::List),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LocationView {
    pub location: Location,
    pub settings: ViewSettings,
}
