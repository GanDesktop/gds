//! Reactive theme, typography, and motion owned by the application root.
use crate::appearance::{motion, theme, typography};
use anti_webui::prelude::{Color, Signal, batch, effect, set_theme, signal, update_settings};
use std::cell::RefCell;

pub struct Appearance {
    theme: Theme,
    typography: Typography,
    motion: Motion,
}
impl Appearance {
    pub(super) fn new() -> Self {
        Self {
            theme: Theme::new(),
            typography: Typography::new(),
            motion: Motion::new(),
        }
    }
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
    pub fn typography(&self) -> &Typography {
        &self.typography
    }
    pub fn motion(&self) -> &Motion {
        &self.motion
    }
}

pub struct Theme {
    prepared: Signal<theme::Prepared>,
    reload: RefCell<Option<theme::HotReload>>,
}
impl Theme {
    fn new() -> Self {
        let initial = theme::Appearance {
            source: theme::Source::Builtin,
            ..Default::default()
        }
        .prepare(Self::platform_mode())
        .expect("embedded theme is valid");
        set_theme(initial.colors().widgets(initial.mode()));
        Self {
            prepared: signal(initial),
            reload: RefCell::new(None),
        }
    }
    pub fn platform_mode() -> theme::Mode {
        #[cfg(target_os = "linux")]
        if let Some(mode) = crate::appearance::portal::snapshot().color_scheme {
            return mode;
        }
        theme::Mode::Dark
    }
    pub fn mode(&self) -> theme::Mode {
        self.prepared.with(theme::Prepared::mode)
    }
    pub fn colors(&self) -> theme::Colors {
        self.prepared.with(theme::Prepared::colors)
    }
    pub fn content_background(&self) -> Color {
        self.prepared.with(|p| {
            p.colors()
                .content
                .with_alpha(p.appearance().content_opacity)
        })
    }
    pub fn sidebar_background(&self) -> Color {
        self.prepared.with(|p| {
            p.colors()
                .sidebar
                .with_alpha(p.appearance().sidebar_opacity)
        })
    }
    pub fn rounded_corners(&self) -> bool {
        self.prepared.with(|p| p.appearance().rounded_corners)
    }
    /// # Errors
    /// Propagates source read and palette validation errors without changing the installed theme.
    pub fn apply(&self, appearance: &theme::Appearance) -> Result<theme::Applied, theme::Error> {
        self.install(appearance.prepare(Self::platform_mode())?)
    }
    /// # Errors
    /// Reports invalid surface opacity, retaining the current appearance.
    pub fn install(&self, prepared: theme::Prepared) -> Result<theme::Applied, theme::Error> {
        theme::validate_opacity("content_opacity", prepared.appearance().content_opacity)?;
        theme::validate_opacity("sidebar_opacity", prepared.appearance().sidebar_opacity)?;
        let applied = prepared.applied();
        batch(|| {
            set_theme(prepared.colors().widgets(prepared.mode()));
            self.prepared.set_if_changed(prepared);
        });
        Ok(applied)
    }
    /// # Errors
    /// Rejects non-finite opacity and values outside 0..=1.
    pub fn update_opacity(&self, content: f32, sidebar: f32) -> Result<(), theme::Error> {
        theme::validate_opacity("content_opacity", content)?;
        theme::validate_opacity("sidebar_opacity", sidebar)?;
        self.prepared.update(|p| {
            p.appearance.content_opacity = content;
            p.appearance.sidebar_opacity = sidebar;
        });
        Ok(())
    }
    pub fn update_window_corners(&self, rounded: bool) {
        self.prepared
            .update(|p| p.appearance.rounded_corners = rounded);
    }
    pub fn configure_hot_reload(&self, appearance: theme::Appearance) {
        let mut reload = self.reload.borrow_mut();
        match reload.as_ref() {
            Some(reload) => reload.configure(appearance),
            None => {
                *reload = Some(super::Gds::current().with(|| theme::HotReload::new(appearance)))
            }
        }
    }
    pub fn refresh(&self) {
        if let Some(reload) = self.reload.borrow().as_ref() {
            reload.refresh();
        }
    }
}

#[derive(Clone, Copy)]
pub struct Typography {
    current: Signal<typography::Resolved>,
}
impl Typography {
    fn new() -> Self {
        Self {
            current: signal(
                typography::Preferences::default()
                    .resolve()
                    .expect("default typography is valid"),
            ),
        }
    }
    pub fn family(&self) -> String {
        self.current.with(|value| value.family.clone())
    }
    pub fn size(&self) -> f32 {
        self.current.with(|value| value.size)
    }
    pub fn scaled(&self, base: f32) -> f32 {
        base * self.size() / typography::DEFAULT_SIZE
    }
    pub fn installed_families(&self) -> Vec<String> {
        typography::installed_families()
    }
    /// # Errors
    /// Rejects unsupported text sizes, retaining the last applied typography.
    pub fn apply(
        &self,
        preferences: &typography::Preferences,
    ) -> Result<typography::Applied, typography::Error> {
        let resolved = preferences.resolve()?;
        let applied = typography::Applied {
            family: resolved.family.clone(),
            unavailable: resolved.unavailable.clone(),
        };
        self.current.set_if_changed(resolved);
        Ok(applied)
    }
}

#[derive(Clone, Copy)]
pub struct Motion {
    policy: Signal<motion::Policy>,
}
impl Motion {
    fn new() -> Self {
        let policy = signal(motion::Policy::default());
        effect(move || {
            let reduced = policy.with(|policy| policy.reduced());
            update_settings(|settings| settings.reduce_motion = reduced);
        });
        Self { policy }
    }
    pub fn enabled(&self) -> bool {
        self.policy.with(|policy| !policy.reduced())
    }
    pub fn set_enabled(&self, enabled: bool) {
        self.policy.update(|policy| policy.enabled = enabled);
    }
    pub fn follow_system(&self) {
        #[cfg(target_os = "linux")]
        super::Gds::current().with(crate::appearance::portal::follow);
    }
    pub(crate) fn system_changed(&self, reduced: bool) {
        self.policy.update(|policy| policy.system_reduced = reduced);
    }
}
