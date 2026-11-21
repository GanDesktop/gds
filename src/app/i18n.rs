//! Application-owned reactive Fluent catalogue.
use crate::i18n::{Catalog, Error, FluentArgs, Language};
use std::sync::Arc;
use zgui::reactive::{LocalStorage, ReadSignal, RwSignal, prelude::*};

#[derive(Clone, Copy)]
pub struct I18n {
    catalog: RwSignal<Arc<Catalog>, LocalStorage>,
}

impl I18n {
    pub(super) fn new() -> Self {
        Self {
            catalog: RwSignal::new_local(Arc::new(
                Catalog::new(Language::detect(), &[]).expect("bundled Fluent is valid"),
            )),
        }
    }

    pub fn language(&self) -> Language {
        self.catalog.with(|catalog| catalog.language())
    }

    pub fn language_untracked(&self) -> Language {
        self.catalog.with_untracked(|catalog| catalog.language())
    }

    pub fn catalog(&self) -> ReadSignal<Arc<Catalog>, LocalStorage> {
        self.catalog.read_only()
    }

    pub fn text(&self, id: &str) -> String {
        self.catalog.with(|catalog| catalog.message(id, None))
    }

    pub fn text_untracked(&self, id: &str) -> String {
        self.catalog
            .with_untracked(|catalog| catalog.message(id, None))
    }

    pub fn format(&self, id: &str, arguments: Option<&FluentArgs<'_>>) -> String {
        self.catalog.with(|catalog| catalog.message(id, arguments))
    }

    pub fn format_untracked(&self, id: &str, arguments: Option<&FluentArgs<'_>>) -> String {
        self.catalog
            .with_untracked(|catalog| catalog.message(id, arguments))
    }

    pub fn count(&self, id: &str, count: usize) -> String {
        let mut arguments = FluentArgs::new();
        arguments.set("count", count as i64);
        self.format(id, Some(&arguments))
    }

    pub fn count_untracked(&self, id: &str, count: usize) -> String {
        let mut arguments = FluentArgs::new();
        arguments.set("count", count as i64);
        self.format_untracked(id, Some(&arguments))
    }

    /// Install the suite's base messages and component translations.
    ///
    /// # Errors
    /// Returns an error when bundled Fluent cannot be parsed or registered.
    pub fn install(&self, language: Language) -> Result<(), Error> {
        self.install_resources(language, &[])
    }

    /// Install messages owned by the current application alongside the base catalog.
    ///
    /// # Errors
    /// Reports invalid Fluent or duplicate message IDs without replacing the current catalogue.
    pub fn install_resources(&self, language: Language, application: &[&str]) -> Result<(), Error> {
        self.install_catalog(Catalog::new(language, application)?);
        Ok(())
    }

    /// Install a fully parsed catalog without introducing another fallible step.
    pub fn install_catalog(&self, catalog: Catalog) {
        self.catalog.set(Arc::new(catalog));
    }
}
