//! Application-context access to shared services and startup assembly.
use std::ops::Deref;

use gpui_kit::App;

#[cfg(feature = "appearance")]
pub mod appearance;
#[cfg(feature = "i18n")]
pub mod i18n;
mod task_manager;
#[cfg(feature = "ui")]
pub mod ui;

#[cfg(feature = "appearance")]
pub use appearance::Appearance;
#[cfg(feature = "i18n")]
pub use i18n::I18n;
pub use task_manager::Tasks;
#[cfg(feature = "ui")]
pub use ui::Ui;

pub trait AppExt {
    fn gds(&mut self) -> Gds<&mut App>;
    fn gds_ro(&self) -> Gds<&App>;
}

impl AppExt for App {
    fn gds(&mut self) -> Gds<&mut App> {
        Gds { cx: self }
    }

    fn gds_ro(&self) -> Gds<&App> {
        Gds { cx: self }
    }
}

/// A borrowed accessor, with no state or service initialization of its own.
#[derive(Clone, Copy)]
pub struct Gds<C> {
    cx: C,
}

impl<C> Gds<C> {
    pub fn into_inner(self) -> C {
        self.cx
    }

    pub fn tasks(self) -> Tasks<C> {
        Tasks { cx: self.cx }
    }

    #[cfg(feature = "appearance")]
    pub fn appearance(self) -> Appearance<C> {
        Appearance { cx: self.cx }
    }

    #[cfg(feature = "i18n")]
    pub fn i18n(self) -> I18n<C> {
        I18n { cx: self.cx }
    }

    #[cfg(feature = "ui")]
    pub fn ui(self) -> Ui<C> {
        Ui { cx: self.cx }
    }
}

impl<C: Deref<Target = App>> Gds<C> {
    /// Underlying read-only context for application-specific accessor extensions.
    pub fn app_ro(&self) -> &App {
        &self.cx
    }
}

impl Gds<&mut App> {
    /// Underlying context for application-specific accessor extensions.
    pub fn app(&mut self) -> &mut App {
        self.cx
    }
}
