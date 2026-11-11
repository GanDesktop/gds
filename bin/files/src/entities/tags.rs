//! File tag associations and tag catalogue backend.
mod baloo;
pub(crate) mod x;

pub use baloo::Baloo;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, gds::ConfigEditor)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    #[setting(label = "tags-backend-baloo")]
    Baloo,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, gds::ConfigEditor)]
#[serde(default)]
pub struct Commands {
    #[setting(label = "tags-backend")]
    pub backend: Backend,
    #[setting(label = "tags-control-command", help = "tags-command-hint")]
    pub control: Option<PathBuf>,
    #[setting(label = "tags-kio-command", help = "tags-command-hint")]
    pub kio_client: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub enabled: bool,
    #[serde(flatten)]
    pub commands: Commands,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Full tag identities; never filesystem paths.
    pub tags: Vec<String>,
    pub files: Vec<PathBuf>,
}

/// Connects to the configured file tag backend.
///
/// # Errors
///
/// Returns an error description when the configured backend executables are missing,
/// disabled, or fail to respond.
pub async fn connect(commands: &Commands) -> Result<Baloo, String> {
    match commands.backend {
        Backend::Baloo => Baloo::connect(commands).await.map_err(Into::into),
    }
}
