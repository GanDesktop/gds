//! Saved places and view settings keyed by content location.
//!
//! Location overrides inherit database defaults. Clearing a column layout
//! preserves widths, ordering, details visibility and grid/list mode.

use super::{
    address::{Location, ViewScope},
    view::{ColumnWidths, LocationView, ViewSettings},
};
use crate::entities::file::x::location::{Layout, Sort, ViewMode};

static DEFAULT_COLUMNS: std::sync::LazyLock<Layout> = std::sync::LazyLock::new(Layout::default);

pub(crate) const RECENT_LIMIT: usize = 100;

#[derive(Clone, Debug)]
pub struct Places {
    pub favorites: Vec<Location>,
    pub recent: Vec<Location>,
    pub pinned: Vec<Location>,
    pub defaults: ViewSettings,
    pub views: Vec<LocationView>,
}

impl Default for Places {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            recent: Vec::new(),
            pinned: Vec::new(),
            defaults: ViewSettings::defaults(),
            views: Vec::new(),
        }
    }
}

impl Places {
    pub fn record(&mut self, location: impl Into<Location>) {
        let location = location.into();
        self.recent.retain(|old| old != &location);
        self.recent.insert(0, location);
        self.recent.truncate(RECENT_LIMIT);
    }

    pub fn is_favorite(&self, location: impl Into<Location>) -> bool {
        self.favorites.contains(&location.into())
    }

    pub fn view(&self, location: impl Into<Location>) -> Option<&ViewSettings> {
        let location = location.into();
        self.views
            .iter()
            .find(|view| view.location == location)
            .map(|view| &view.settings)
    }

    pub fn columns_for(&self, location: impl Into<Location>) -> Option<&Layout> {
        self.view(location).and_then(|view| view.columns.as_ref())
    }

    pub fn effective_columns(&self, location: impl Into<Location>) -> &Layout {
        self.columns_for(location)
            .or(self.defaults.columns.as_ref())
            .unwrap_or(&DEFAULT_COLUMNS)
    }

    pub fn effective_view_mode(&self, location: impl Into<Location>) -> ViewMode {
        self.view(location)
            .and_then(|view| view.mode)
            .or(self.defaults.mode)
            .unwrap_or_default()
    }

    pub fn effective_sort(&self, location: impl Into<Location>) -> Sort {
        self.view(location)
            .and_then(|view| view.sort)
            .or(self.defaults.sort)
            .unwrap_or_default()
    }

    pub fn details_visible(&self, location: impl Into<Location>) -> bool {
        self.view(location)
            .and_then(|view| view.details)
            .or(self.defaults.details)
            .unwrap_or(false)
    }

    pub fn column_widths(&self, location: impl Into<Location>) -> ColumnWidths {
        let mut widths = self.defaults.widths.clone();
        if let Some(view) = self.view(location) {
            widths.extend(view.widths.clone());
        }
        widths
    }

    pub fn set_view_mode(&mut self, location: impl Into<Location>, mode: ViewMode) {
        self.settings_mut(ViewScope::Location(location.into())).mode = Some(mode);
    }

    pub(crate) fn settings_mut(&mut self, scope: ViewScope) -> &mut ViewSettings {
        let ViewScope::Location(location) = scope else {
            return &mut self.defaults;
        };
        let index = self
            .views
            .iter()
            .position(|view| view.location == location)
            .unwrap_or_else(|| {
                self.views.push(LocationView {
                    location,
                    settings: ViewSettings::default(),
                });
                self.views.len() - 1
            });
        &mut self.views[index].settings
    }
}

/// Idempotent edits let separate windows update records without replacing a
/// stale snapshot of another window's favorites or directory settings.
#[derive(Clone)]
pub(crate) enum Change {
    Recent(Location),
    ClearRecent,
    Favorite {
        location: Location,
        present: bool,
    },
    Pin {
        location: Location,
        present: bool,
    },
    PinOrder(Vec<Location>),
    Columns {
        scope: ViewScope,
        layout: Option<Layout>,
    },
    ViewMode {
        location: Location,
        mode: ViewMode,
    },
    Widths {
        location: Location,
        widths: ColumnWidths,
    },
    Sort {
        location: Location,
        sort: Sort,
    },
    Details {
        location: Location,
        visible: bool,
    },
}

impl Change {
    pub(crate) fn apply(&self, places: &mut Places) {
        match self {
            Self::Recent(location) => places.record(location),
            Self::ClearRecent => places.recent.clear(),
            Self::Favorite { location, present } => {
                set_membership(&mut places.favorites, location, *present)
            }
            Self::Pin { location, present } => {
                set_membership(&mut places.pinned, location, *present)
            }
            Self::PinOrder(order) => reorder_existing(&mut places.pinned, order),
            Self::Columns { scope, layout } => {
                places.settings_mut(scope.clone()).columns = layout.clone()
            }
            Self::ViewMode { location, mode } => places.set_view_mode(location, *mode),
            Self::Widths { location, widths } => places
                .settings_mut(ViewScope::Location(location.clone()))
                .widths
                .extend(widths.clone()),
            Self::Sort { location, sort } => {
                places
                    .settings_mut(ViewScope::Location(location.clone()))
                    .sort = Some(*sort)
            }
            Self::Details { location, visible } => {
                places
                    .settings_mut(ViewScope::Location(location.clone()))
                    .details = Some(*visible)
            }
        }
    }
}

fn set_membership<T: Clone + PartialEq>(items: &mut Vec<T>, item: &T, present: bool) {
    if !present {
        items.retain(|old| old != item);
    } else if !items.contains(item) {
        items.push(item.clone());
    }
}

/// Reorder only members of this window's snapshot; preserve pins owned by others.
pub(crate) fn reorder_existing(items: &mut [Location], order: &[Location]) {
    let mut seen = std::collections::HashSet::new();
    let ordered: Vec<_> = order
        .iter()
        .filter(|path| items.contains(path) && seen.insert((*path).clone()))
        .cloned()
        .collect();
    let mut ordered = ordered.into_iter();
    for item in items {
        if order.contains(item) {
            *item = ordered.next().expect("one ordered value per existing pin");
        }
    }
}
