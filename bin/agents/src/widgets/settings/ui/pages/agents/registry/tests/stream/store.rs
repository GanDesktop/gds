//! Persistence for the mock RPC service only; no daemon configuration code is linked into the GUI.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(super) struct Store(PathBuf);
pub(super) struct Document {
    pub settings: Settings,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Settings {
    pub daemon: bas::config::Daemon,
}

impl Store {
    pub fn at(path: PathBuf) -> Self {
        Self(path)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn load(&self) -> anyhow::Result<Document> {
        let settings = match std::fs::read(&self.0) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(error) => return Err(error.into()),
        };
        Ok(Document { settings })
    }
    pub fn save_agents(
        &self,
        document: &mut Document,
        agents: Vec<bas::config::AgentRegistration>,
    ) -> anyhow::Result<()> {
        let mut current = self.load()?;
        current.settings.daemon.agents = agents;
        self.save(document, current)
    }
    pub fn save_acp(
        &self,
        document: &mut Document,
        policy: bas::config::AcpLifecycle,
    ) -> anyhow::Result<()> {
        let mut current = self.load()?;
        current.settings.daemon.acp = policy;
        self.save(document, current)
    }
    fn save(&self, document: &mut Document, current: Document) -> anyhow::Result<()> {
        current.settings.daemon.validate()?;
        std::fs::write(&self.0, serde_json::to_vec(&current.settings)?)?;
        *document = current;
        Ok(())
    }
}
