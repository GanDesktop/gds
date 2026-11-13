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

pub trait Context: Deref<Target = App> {
    type Reborrowed<'a>: Context
    where
        Self: 'a;

    fn reborrow(&mut self) -> Self::Reborrowed<'_>;
}

impl<'cx> Context for &'cx App {
    type Reborrowed<'a>
        = &'cx App
    where
        Self: 'a;

    fn reborrow(&mut self) -> &'cx App {
        *self
    }
}

impl Context for &mut App {
    type Reborrowed<'a>
        = &'a mut App
    where
        Self: 'a;

    fn reborrow(&mut self) -> &mut App {
        self
    }
}

pub trait AppExt {
    fn gds(&self) -> Gds<&App>;
    fn gds_mut(&mut self) -> Gds<&mut App>;
}

impl AppExt for App {
    fn gds(&self) -> Gds<&App> {
        Gds { cx: self }
    }

    fn gds_mut(&mut self) -> Gds<&mut App> {
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
}

impl<C: Context> Gds<C> {
    pub fn reborrow(&mut self) -> Gds<C::Reborrowed<'_>> {
        Gds {
            cx: self.cx.reborrow(),
        }
    }

    pub fn tasks(&mut self) -> Tasks<C::Reborrowed<'_>> {
        Tasks {
            cx: self.cx.reborrow(),
        }
    }

    #[cfg(feature = "appearance")]
    pub fn appearance(&mut self) -> Appearance<C::Reborrowed<'_>> {
        Appearance {
            cx: self.cx.reborrow(),
        }
    }

    #[cfg(feature = "i18n")]
    pub fn i18n(&mut self) -> I18n<C::Reborrowed<'_>> {
        I18n {
            cx: self.cx.reborrow(),
        }
    }

    #[cfg(feature = "ui")]
    pub fn ui(&mut self) -> Ui<C::Reborrowed<'_>> {
        Ui {
            cx: self.cx.reborrow(),
        }
    }
}

impl<C: Deref<Target = App>> Gds<C> {
    /// Underlying read-only context for application-specific accessor extensions.
    pub fn app(&self) -> &App {
        &self.cx
    }
}

impl Gds<&mut App> {
    /// Underlying context for application-specific accessor extensions.
    pub fn app_mut(&mut self) -> &mut App {
        self.cx
    }
}
