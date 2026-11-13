//! Shared desktop startup assembly and UI service accessors.
use std::ops::Deref;

use super::{Context, Gds};
use crate::service::logs::Store;
use gpui_kit::{App, Window, WindowHandle, component::Root};

impl Gds<&mut App> {
    /// Install task ownership before UI services, with optional diagnostics.
    pub fn initialize(&mut self, scope: crate::service::task_manager::Scope, logs: Option<Store>) {
        self.tasks().init(scope);
        self.init_ui(logs);
    }

    /// Install shared UI services when task ownership was already bound earlier in startup.
    pub fn init_ui(&mut self, logs: Option<Store>) {
        self.appearance().init();
        self.ui().keytips().install();
        self.ui().init_terminal();
        if let Some(logs) = logs {
            self.ui().logs().install(logs);
        }
    }
}

#[derive(Clone, Copy)]
pub struct Ui<C> {
    pub(super) cx: C,
}

impl<C: Context> Ui<C> {
    pub fn time(&mut self) -> Time<C::Reborrowed<'_>> {
        Time {
            cx: self.cx.reborrow(),
        }
    }

    pub fn logs(&mut self) -> Logs<C::Reborrowed<'_>> {
        Logs {
            cx: self.cx.reborrow(),
        }
    }

    pub fn keytips(&mut self) -> Keytips<C::Reborrowed<'_>> {
        Keytips {
            cx: self.cx.reborrow(),
        }
    }
}

impl Ui<&mut App> {
    /// Reserve Tab for PTY input. Applications own service cleanup and other shortcuts.
    pub fn init_terminal(&mut self) {
        crate::ui::terminal::init(self.cx);
    }

    /// Bind the dialog decisions once at startup.
    pub fn register_dialogs(&mut self) {
        crate::ui::dialog::register(self.cx);
    }

    pub fn register_actions(&mut self) {
        crate::ui::actions::register(self.cx);
    }
}

#[derive(Clone, Copy)]
pub struct Time<C> {
    cx: C,
}

impl<C: Deref<Target = App>> Time<C> {
    /// The configured shared format, used by [`crate::ui::time::Timestamp`] by default.
    pub fn format(&self) -> crate::ui::time::Format {
        crate::ui::time::format(&self.cx)
    }

    /// Current wall-clock time for a render.
    pub fn now(&self) -> std::time::SystemTime {
        crate::ui::time::now(&self.cx)
    }
}

impl Time<&mut App> {
    /// Install or reconfigure the shared timestamp format.
    pub fn apply(&mut self, format: crate::ui::time::Format) {
        crate::ui::time::apply(format, self.cx);
    }
}

#[derive(Clone, Copy)]
pub struct Logs<C> {
    cx: C,
}

impl<C: Deref<Target = App>> Logs<C> {
    pub fn is_enabled(&self) -> bool {
        crate::ui::logs::is_enabled(&self.cx)
    }

    pub fn store(&self) -> Option<Store> {
        crate::ui::logs::store(&self.cx)
    }
}

impl Logs<&mut App> {
    /// Install the collector created before the application's background tasks start.
    pub fn install(&mut self, store: Store) {
        crate::ui::logs::install(store, self.cx);
    }

    /// Register the viewer's actions once for this application process.
    pub fn init(&mut self, options: crate::ui::logs::Options) {
        crate::ui::logs::init(options, self.cx);
    }

    /// Reveal the existing viewer or open a window for this process's collector.
    ///
    /// # Errors
    /// Reports disabled logging, missing initialization or native window creation errors.
    pub fn open(&mut self) -> anyhow::Result<WindowHandle<Root>> {
        crate::ui::logs::open(self.cx)
    }
}

#[derive(Clone, Copy)]
pub struct Keytips<C> {
    cx: C,
}

impl Keytips<&mut App> {
    /// Register the hint layer for the windows created afterwards.
    pub fn install(&mut self) {
        crate::ui::keytips::install(self.cx);
    }

    /// Drop any hint that is on screen.
    pub fn release(&mut self, window: &mut Window) {
        crate::ui::keytips::release(window, self.cx);
    }
}
