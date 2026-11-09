//! File tag associations and replaceable tag catalogue providers.
mod baloo;
pub(crate) use baloo::Baloo;
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf, pin::Pin};

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

#[derive(Clone, Debug, Default)]
pub struct Listing {
    /// Full tag identities; never filesystem paths.
    pub tags: Vec<String>,
    pub files: Vec<PathBuf>,
}

pub type Query<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Catalogue queries and tag-wide operations. Per-file edits use `file_tags`.
/// A future embedded backend shares this interface; no scanner is provided.
pub trait TagIndex: Send + Sync {
    fn browse<'a>(&'a self, tag: &'a str) -> Query<'a, Listing>;
    /// Remove a tag and its descendants from indexed files, preserving the files.
    fn delete_tag<'a>(&'a self, tag: &'a str) -> Query<'a, ()>;
}

pub async fn connect(commands: &Commands) -> Result<Box<dyn TagIndex>, String> {
    match commands.backend {
        Backend::Baloo => Ok(Box::new(Baloo::connect(commands).await?)),
    }
}
