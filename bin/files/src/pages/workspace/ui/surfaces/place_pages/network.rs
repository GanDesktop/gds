//! UI state for mounted network folder discovery.

use std::{ops::Deref, path::PathBuf};

use crate::entities::location::volumes::NetworkMountError;

#[derive(Default)]
pub(in crate::pages::workspace) enum State {
    #[default]
    Idle,
    Loading {
        paths: Vec<PathBuf>,
    },
    Ready(Vec<PathBuf>),
    Failed {
        paths: Vec<PathBuf>,
        error: String,
    },
}

#[doc(hidden)]
pub(in crate::pages::workspace) struct LoadFlag {
    pub loaded: bool,
}

impl Deref for State {
    type Target = LoadFlag;

    fn deref(&self) -> &Self::Target {
        const UNLOADED: LoadFlag = LoadFlag { loaded: false };
        const LOADED: LoadFlag = LoadFlag { loaded: true };
        if self.is_loaded() { &LOADED } else { &UNLOADED }
    }
}

impl State {
    pub(in crate::pages::workspace) fn is_loading(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }

    pub(in crate::pages::workspace) fn is_loaded(&self) -> bool {
        matches!(self, Self::Ready(_) | Self::Failed { .. })
    }

    pub(in crate::pages::workspace) fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Idle => &[],
            Self::Loading { paths } | Self::Ready(paths) | Self::Failed { paths, .. } => paths,
        }
    }

    pub(in crate::pages::workspace) fn error(&self) -> Option<&str> {
        match self {
            Self::Failed { error, .. } => Some(error.as_str()),
            _ => None,
        }
    }

    pub(in crate::pages::workspace) fn begin_loading(&mut self) -> bool {
        if self.is_loading() {
            return false;
        }
        let paths = self.paths().to_vec();
        *self = Self::Loading { paths };
        true
    }

    pub(in crate::pages::workspace) fn finish(
        &mut self,
        result: Result<Vec<PathBuf>, NetworkMountError>,
    ) {
        let previous = self.paths().to_vec();
        *self = match result {
            Ok(paths) => Self::Ready(paths),
            Err(error) => Self::Failed {
                paths: previous,
                error: error.to_string(),
            },
        };
    }
}
