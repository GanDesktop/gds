//! Application-context access to shared services and startup assembly.
use gpui_kit::App;

mod task_manager;
#[cfg(feature = "ui")]
mod ui;

pub use task_manager::Tasks;

pub trait AppExt {
    fn gds(&mut self) -> Gds<'_>;
}

impl AppExt for App {
    fn gds(&mut self) -> Gds<'_> {
        Gds { cx: self }
    }
}

/// A borrowed accessor, with no state or service initialization of its own.
pub struct Gds<'a> {
    cx: &'a mut App,
}

impl Gds<'_> {
    pub fn tasks(&mut self) -> Tasks<'_> {
        Tasks { cx: self.cx }
    }

    /// Underlying context for application-specific accessor extensions.
    pub fn app(&mut self) -> &mut App {
        self.cx
    }
}
