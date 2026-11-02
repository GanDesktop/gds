//! Frontend-owned service profiles; existing workspaces retain their own target.

use crate::shared::config::{ConnectionProfile, Document, Frontend, Store};
use crate::shared::i18n::tr;
use bas::service::tls::ConnectionOptions as Target;
use gpui_kit::{App, AppContext, Context, Entity, Global};

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

struct Shared(Entity<Profiles>);
impl Global for Shared {}

pub(crate) fn shared(cx: &mut App) -> Entity<Profiles> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let store = crate::shared::config::store(cx);
    let profiles = cx.new(|_| Profiles::new(store));
    cx.set_global(Shared(profiles.clone()));
    profiles
}

pub(crate) struct Profiles {
    store: Store,
    document: Option<Document>,
    pub(crate) error: Option<String>,
}

impl Profiles {
    pub(crate) fn new(store: Store) -> Self {
        let mut profiles = Self {
            store,
            document: None,
            error: None,
        };
        profiles.load();
        profiles
    }

    fn load(&mut self) {
        match self.store.load() {
            Ok(document) => {
                self.document = Some(document);
                self.error = None;
            }
            Err(error) => {
                self.document = None;
                self.error = Some(error.to_string());
            }
        }
    }

    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        self.load();
        cx.notify();
    }

    pub(crate) fn entries(&self) -> &[ConnectionProfile] {
        self.document
            .as_ref()
            .map_or(&[], |document| &document.settings.frontend.connections)
    }

    pub(crate) fn selected(&self) -> Option<&ConnectionProfile> {
        self.document
            .as_ref()?
            .settings
            .frontend
            .selected_connection()
    }

    pub(crate) fn matching(&self, options: &Target) -> Option<&ConnectionProfile> {
        self.selected()
            .filter(|profile| profile.options() == *options)
            .or_else(|| {
                self.entries()
                    .iter()
                    .find(|profile| profile.options() == *options)
            })
    }

    pub(crate) fn startup_target(&self, explicit: Option<Target>) -> Result<Target, String> {
        let options = match explicit {
            Some(options) => options,
            None => self
                .selected()
                .map(ConnectionProfile::options)
                .ok_or_else(|| {
                    self.error
                        .clone()
                        .unwrap_or_else(|| "No selected connection".into())
                })?,
        };
        options.validate().map_err(|error| error.to_string())?;
        Ok(options)
    }

    fn frontend(&self) -> Result<Frontend, String> {
        self.document
            .as_ref()
            .map(|document| document.settings.frontend.clone())
            .ok_or_else(|| {
                self.error
                    .clone()
                    .unwrap_or_else(|| tr!("settings-not-loaded").into())
            })
    }

    fn save(&mut self, frontend: Frontend, cx: &mut Context<Self>) -> Result<(), String> {
        let document = self
            .document
            .as_mut()
            .ok_or_else(|| tr!("settings-not-loaded").to_owned())?;
        let result = self
            .store
            .save_frontend(document, frontend)
            .map_err(|error| error.to_string());
        self.error = result.as_ref().err().cloned();
        cx.notify();
        result
    }

    pub(crate) fn select(&mut self, id: &str, cx: &mut Context<Self>) -> Result<Target, String> {
        let mut frontend = self.frontend()?;
        let target = frontend
            .connections
            .iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| tr!("connections-missing").to_owned())?
            .options();
        frontend.selected = id.into();
        self.save(frontend, cx)?;
        Ok(target)
    }

    pub(crate) fn put(
        &mut self,
        profile: ConnectionProfile,
        cx: &mut Context<Self>,
    ) -> Result<Target, String> {
        let mut frontend = self.frontend()?;
        let target = profile.options();
        if let Some(existing) = frontend
            .connections
            .iter_mut()
            .find(|item| item.id == profile.id)
        {
            *existing = profile;
        } else {
            frontend.connections.push(profile);
        }
        self.save(frontend, cx)?;
        Ok(target)
    }

    pub(crate) fn remove(&mut self, id: &str, cx: &mut Context<Self>) -> Result<Target, String> {
        let mut frontend = self.frontend()?;
        if frontend.connections.len() == 1 {
            return Err(tr!("connections-keep-one").into());
        }
        let index = frontend
            .connections
            .iter()
            .position(|profile| profile.id == id)
            .ok_or_else(|| tr!("connections-missing").to_owned())?;
        frontend.connections.remove(index);
        if frontend.selected == id {
            frontend.selected = frontend.connections[0].id.clone();
        }
        let target = frontend
            .selected_connection()
            .expect("selected profile survives removal")
            .options();
        self.save(frontend, cx)?;
        Ok(target)
    }
}
