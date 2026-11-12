//! Application-context access to theme, typography and motion preferences.
use std::ops::Deref;

use crate::appearance::{motion, theme, typography};
use gpui_kit::{App, Entity, Hsla, Pixels, Window};

#[derive(Clone, Copy)]
pub struct Appearance<C> {
    pub(super) cx: C,
}

impl<C> Appearance<C> {
    pub fn theme(self) -> Theme<C> {
        Theme { cx: self.cx }
    }

    pub fn typography(self) -> Typography<C> {
        Typography { cx: self.cx }
    }

    pub fn motion(self) -> Motion<C> {
        Motion { cx: self.cx }
    }
}

impl Appearance<&mut App> {
    /// Initialize Kit with the system fonts before it probes default typography.
    pub fn init(&mut self) {
        crate::appearance::init(self.cx);
    }
}

#[derive(Clone, Copy)]
pub struct Theme<C> {
    cx: C,
}

impl<C: Deref<Target = App>> Theme<C> {
    /// Read the same snapshot used to populate GPUI Kit's theme.
    pub fn colors(&self) -> theme::Colors {
        theme::Colors::current(&self.cx)
    }

    /// Background for the application-owned content surface, not text or controls.
    pub fn content_background(&self) -> Hsla {
        theme::content_background(&self.cx)
    }

    /// Background for the application-owned sidebar and toolbar surfaces.
    pub fn sidebar_background(&self) -> Hsla {
        theme::sidebar_background(&self.cx)
    }
}

impl Theme<&mut App> {
    /// Read the selected source and apply one fully parsed palette to GPUI Kit.
    ///
    /// # Errors
    /// Returns an error for invalid opacity, unavailable source files, disabled Qt
    /// custom palettes, or unsupported/malformed palette data.
    pub fn apply(
        &mut self,
        appearance: &theme::Appearance,
    ) -> Result<theme::Applied, theme::Error> {
        theme::apply(appearance, self.cx)
    }

    /// Install the prepared palette without reading source files again.
    ///
    /// # Errors
    /// Reports invalid surface opacity; the previous palette remains installed.
    pub fn install(&mut self, prepared: theme::Prepared) -> Result<theme::Applied, theme::Error> {
        prepared.install(self.cx)
    }

    /// Update only the existing surface opacity, without reloading a palette.
    ///
    /// # Errors
    /// Returns an error for non-finite/out-of-range alpha or when no appearance has
    /// been applied yet.
    pub fn update_opacity(&mut self, content: f32, sidebar: f32) -> Result<(), theme::Error> {
        theme::update_opacity(content, sidebar, self.cx)
    }

    /// Update client-side window corners without reloading the palette.
    pub fn update_window_corners(&mut self, rounded: bool) {
        theme::update_window_corners(rounded, self.cx);
    }

    /// Install the application's palette watcher before creating its windows.
    pub fn init_hot_reload(&mut self, appearance: theme::Appearance) -> Entity<theme::HotReload> {
        theme::HotReload::initialize(appearance, self.cx)
    }

    pub fn configure_hot_reload(&mut self, appearance: theme::Appearance) {
        theme::HotReload::get(self.cx)
            .update(self.cx, |reload, cx| reload.configure(appearance, cx));
    }

    pub fn observer(&mut self) -> Entity<theme::Observer> {
        theme::Observer::shared(self.cx)
    }

    /// Register one window without transferring ownership of the process watcher to it.
    pub fn attach_observer(&mut self, window: &mut Window) {
        theme::Observer::attach(window, self.cx);
    }
}

#[derive(Clone, Copy)]
pub struct Typography<C> {
    cx: C,
}

impl<C: Deref<Target = App>> Typography<C> {
    /// Installed font family names reported by the platform text system.
    pub fn installed_families(&self) -> Vec<String> {
        typography::installed_families(&self.cx)
    }

    /// Scale an explicit pixel text size by the shared font size preference.
    pub fn scaled(&self, base_px: f32) -> Pixels {
        typography::scaled(base_px, &self.cx)
    }
}

impl Typography<&mut App> {
    /// Apply the shared font preference to the theme.
    ///
    /// # Errors
    /// Rejects non-finite sizes outside [`typography::MIN_SIZE`]..=[`typography::MAX_SIZE`].
    pub fn apply(
        &mut self,
        preferences: &typography::Preferences,
    ) -> Result<typography::Applied, typography::Error> {
        typography::install(preferences, self.cx)
    }
}

#[derive(Clone, Copy)]
pub struct Motion<C> {
    cx: C,
}

impl Motion<&mut App> {
    /// Enable decoration following the system preference, or force it to be static.
    pub fn set_enabled(&mut self, enabled: bool) {
        motion::set_animations_enabled(enabled, self.cx);
    }

    /// Follow the Settings Portal using the application's existing Tokio runtime.
    pub fn follow_system(&mut self) {
        let tasks = super::Tasks { cx: &*self.cx }.scope();
        motion::follow_system(&tasks, self.cx);
    }
}
