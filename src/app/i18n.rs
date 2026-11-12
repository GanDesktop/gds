//! Application-context access to localization and Fluent catalogs.
use std::ops::Deref;

use crate::i18n::{self, Catalog, Error, FluentArgs, Language};
use gpui_kit::{App, SharedString};

#[derive(Clone, Copy)]
pub struct I18n<C> {
    pub(super) cx: C,
}

impl<C: Deref<Target = App>> I18n<C> {
    pub fn language(&self) -> Language {
        i18n::language(&self.cx)
    }

    pub fn text(&self, id: &str) -> SharedString {
        i18n::text(id, &self.cx)
    }

    pub fn format(&self, id: &str, arguments: Option<&FluentArgs<'_>>) -> SharedString {
        i18n::format(id, arguments, &self.cx)
    }

    pub fn count(&self, id: &str, count: usize) -> SharedString {
        i18n::count(id, count, &self.cx)
    }
}

impl I18n<&mut App> {
    /// Install the suite's base messages and component translations.
    ///
    /// # Errors
    /// Returns an error when bundled Fluent cannot be parsed or registered.
    pub fn install(&mut self, language: Language) -> Result<(), Error> {
        i18n::install(language, self.cx)
    }

    /// Install messages owned by the current application alongside the base catalog.
    ///
    /// # Errors
    /// Returns an error for invalid Fluent or duplicate message IDs.
    pub fn install_resources(
        &mut self,
        language: Language,
        application: &[&str],
    ) -> Result<(), Error> {
        i18n::install_resources(language, application, self.cx)
    }

    /// Install a fully parsed catalog without introducing another fallible step.
    pub fn install_catalog(&mut self, catalog: Catalog) {
        catalog.install(self.cx);
    }
}
