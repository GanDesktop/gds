//! Application-owned reactive Fluent catalogue.
use crate::i18n::{Catalog, Error, FluentArgs, Language};
use anti_webui::prelude::{Signal, signal};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub struct I18n {
    catalog: Signal<Rc<Catalog>>,
}
impl I18n {
    pub(super) fn new() -> Self {
        Self {
            catalog: signal(Rc::new(
                Catalog::new(Language::detect(), &[]).expect("bundled Fluent is valid"),
            )),
        }
    }
    pub fn language(&self) -> Language {
        self.catalog.with(|catalog| catalog.language())
    }
    pub fn text(&self, id: &str) -> String {
        self.catalog.with(|catalog| catalog.message(id, None))
    }
    pub fn format(&self, id: &str, arguments: Option<&FluentArgs<'_>>) -> String {
        self.catalog.with(|catalog| catalog.message(id, arguments))
    }
    pub fn count(&self, id: &str, count: usize) -> String {
        let mut arguments = FluentArgs::new();
        arguments.set("count", count as i64);
        self.format(id, Some(&arguments))
    }
    /// # Errors
    /// Reports invalid Fluent or duplicate message IDs.
    pub fn install(&self, language: Language) -> Result<(), Error> {
        self.install_resources(language, &[])
    }
    /// # Errors
    /// Reports invalid Fluent or duplicate message IDs without replacing the current catalogue.
    pub fn install_resources(&self, language: Language, application: &[&str]) -> Result<(), Error> {
        self.install_catalog(Catalog::new(language, application)?);
        Ok(())
    }
    pub fn install_catalog(&self, catalog: Catalog) {
        self.catalog.set(Rc::new(catalog));
    }
}
