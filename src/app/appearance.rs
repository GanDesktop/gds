//! Reactive theme, typography, motion, and Stylo CSS custom property generation.
use crate::appearance::{motion, theme, typography};
use std::{cell::RefCell, rc::Rc};
use zgui::canvas::zgui_color::Color;
use zgui::reactive::{
    LocalStorage, Mounted, Owner, ReadSignal, RenderEffect, RwSignal, on_cleanup_local, prelude::*,
};
use zgui::view::{HostHandle, Stylesheet, current_host};
use zgui_ui_tokens::{ColorScheme, Theme as TokensTheme};

struct WindowSheet {
    host: HostHandle,
    refs: usize,
    scope: Mounted,
    sheet: Stylesheet,
}

#[derive(Clone)]
pub struct Appearance {
    owner: Owner,
    sheets: Rc<RefCell<Vec<WindowSheet>>>,
    theme: Theme,
    typography: Typography,
    motion: Motion,
}

impl Appearance {
    pub(super) fn new() -> Self {
        let owner = Owner::current().unwrap_or_default();
        Self {
            owner,
            sheets: Rc::new(RefCell::new(Vec::new())),
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

    /// Active `zgui-ui-tokens` scheme matching the currently resolved theme mode.
    pub fn color_scheme(&self) -> ColorScheme {
        self.theme.mode().color_scheme()
    }

    /// Reactively construct a `zgui-ui-tokens::Theme` synchronized with palette, typography,
    /// corner radius, and motion policy.
    pub fn tokens_theme(&self) -> TokensTheme {
        let policy = self.motion.policy.get();
        self.typography.current.with(|typo| {
            self.theme
                .prepared
                .with(|prepared| prepared.tokens_theme(typo, policy))
        })
    }

    /// Reactively generate the `:root` Stylo CSS sheet containing all `--gds-*` custom properties.
    pub fn gds_stylesheet_css(&self) -> String {
        let policy = self.motion.policy.get();
        self.typography.current.with(|typo| {
            self.theme
                .prepared
                .with(|prepared| prepared.gds_stylesheet_css(typo, policy))
        })
    }

    /// Reactively generate the combined `:root` Stylo CSS sheet for `--zui-*` and `--gds-*`.
    pub fn full_stylesheet_css(&self) -> String {
        let policy = self.motion.policy.get();
        self.typography.current.with(|typo| {
            self.theme
                .prepared
                .with(|prepared| prepared.full_stylesheet_css(typo, policy))
        })
    }

    /// Construct the combined `:root` Stylo CSS sheet without subscribing a reactive observer.
    pub fn full_stylesheet_css_untracked(&self) -> String {
        let policy = self.motion.policy.get_untracked();
        self.typography.current.with_untracked(|typo| {
            self.theme
                .prepared
                .with_untracked(|prepared| prepared.full_stylesheet_css(typo, policy))
        })
    }

    /// Install and reactively update `"gds-appearance"` inside the current `zgui` window scope.
    ///
    /// Reference-counted per window [`HostHandle`] with the reactive update effect anchored under
    /// the application's root [`Owner`], so mounting a replacement [`crate::ui::Frame`] before an
    /// outgoing `Frame` unmounts never removes `--gds-*` / `--zui-*` tokens from the active window.
    /// Returns `true` when a window document host is active, or `false` in headless contexts.
    pub fn install_window_sheet(&self) -> bool {
        let Some(host) = current_host() else {
            return false;
        };
        let already_installed = {
            let mut entries = self.sheets.borrow_mut();
            if let Some(entry) = entries.iter_mut().find(|entry| entry.host.ptr_eq(&host)) {
                entry.refs += 1;
                true
            } else {
                false
            }
        };
        if !already_installed {
            let initial_css = self.full_stylesheet_css_untracked();
            let Some(sheet) = Stylesheet::install(theme::APPEARANCE_SHEET, &initial_css) else {
                return false;
            };
            let appearance = self.clone();
            let host_for_effect = host.clone();
            let scope = self.owner.with(Mounted::new);
            scope.with(|| {
                let effect = RenderEffect::new(move |previous: Option<()>| {
                    let css = appearance.full_stylesheet_css();
                    if previous.is_some() {
                        host_for_effect.install_stylesheet(theme::APPEARANCE_SHEET, &css);
                    }
                });
                on_cleanup_local(move || drop(effect));
            });
            self.sheets.borrow_mut().push(WindowSheet {
                host: host.clone(),
                refs: 1,
                scope,
                sheet,
            });
        }
        let sheets = Rc::clone(&self.sheets);
        let host_for_cleanup = host;
        on_cleanup_local(move || {
            let removed = {
                let mut entries = sheets.borrow_mut();
                if let Some(pos) = entries
                    .iter()
                    .position(|entry| entry.host.ptr_eq(&host_for_cleanup))
                {
                    entries[pos].refs -= 1;
                    if entries[pos].refs == 0 {
                        Some(entries.swap_remove(pos))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(entry) = removed {
                entry.scope.unmount();
                drop(entry.sheet);
            }
        });
        true
    }
}

#[derive(Clone)]
pub struct Theme {
    prepared: RwSignal<theme::Prepared, LocalStorage>,
    reload: Rc<RefCell<Option<theme::HotReload>>>,
}

impl Theme {
    fn new() -> Self {
        let initial = theme::Appearance {
            source: theme::Source::Builtin,
            ..Default::default()
        }
        .prepare(Self::platform_mode())
        .expect("embedded theme is valid");
        Self {
            prepared: RwSignal::new_local(initial),
            reload: Rc::new(RefCell::new(None)),
        }
    }

    pub fn platform_mode() -> theme::Mode {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        if let Some(mode) = crate::appearance::portal::snapshot().color_scheme {
            return mode;
        }
        theme::Mode::Dark
    }

    pub fn prepared(&self) -> ReadSignal<theme::Prepared, LocalStorage> {
        self.prepared.read_only()
    }

    pub fn mode(&self) -> theme::Mode {
        self.prepared.with(theme::Prepared::mode)
    }

    pub fn mode_untracked(&self) -> theme::Mode {
        self.prepared.with_untracked(theme::Prepared::mode)
    }

    pub fn colors(&self) -> theme::Colors {
        self.prepared.with(theme::Prepared::colors)
    }

    pub fn material(&self) -> theme::Material {
        self.prepared.with(theme::Prepared::material)
    }

    pub fn content_background(&self) -> Color {
        self.material().content_background()
    }

    pub fn sidebar_background(&self) -> Color {
        self.material().sidebar_background()
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
        if self.prepared.with_untracked(|current| *current != prepared) {
            self.prepared.set(prepared);
        }
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
                *reload = Some(super::Gds::current().with(|| theme::HotReload::new(appearance)));
            }
        }
    }

    pub fn refresh(&self) {
        if let Some(reload) = self.reload.borrow().as_ref() {
            reload.refresh();
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub(crate) fn refresh_for_portal(&self) {
        if let Some(reload) = self.reload.borrow().as_ref() {
            reload.refresh_for_portal();
        }
    }
}

#[derive(Clone, Copy)]
pub struct Typography {
    current: RwSignal<typography::Resolved, LocalStorage>,
}

impl Typography {
    fn new() -> Self {
        Self {
            current: RwSignal::new_local(
                typography::Preferences::default()
                    .resolve()
                    .expect("default typography is valid"),
            ),
        }
    }

    pub fn resolved(&self) -> ReadSignal<typography::Resolved, LocalStorage> {
        self.current.read_only()
    }

    pub fn family(&self) -> String {
        self.current.with(|value| value.family.clone())
    }

    pub fn mono_family(&self) -> String {
        self.current.with(|value| value.mono_family.clone())
    }

    pub fn size(&self) -> f32 {
        self.current.with(|value| value.size)
    }

    pub fn size_untracked(&self) -> f32 {
        self.current.with_untracked(|value| value.size)
    }

    pub fn scaled(&self, base: f32) -> f32 {
        base * self.size() / typography::DEFAULT_SIZE
    }

    pub fn scaled_untracked(&self, base: f32) -> f32 {
        base * self.size_untracked() / typography::DEFAULT_SIZE
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
        if self.current.with_untracked(|current| *current != resolved) {
            self.current.set(resolved);
        }
        Ok(applied)
    }
}

#[derive(Clone, Copy)]
pub struct Motion {
    policy: RwSignal<motion::Policy, LocalStorage>,
    following: RwSignal<bool, LocalStorage>,
}

impl Motion {
    fn new() -> Self {
        Self {
            policy: RwSignal::new_local(motion::Policy::default()),
            following: RwSignal::new_local(false),
        }
    }

    pub fn enabled(&self) -> bool {
        self.policy.with(|policy| !policy.reduced())
    }

    pub fn reduced(&self) -> bool {
        self.policy.with(|policy| policy.reduced())
    }

    pub fn reduced_untracked(&self) -> bool {
        self.policy.with_untracked(|policy| policy.reduced())
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.policy.update(|policy| policy.enabled = enabled);
    }

    pub fn follow_system(&self) {
        if self.following.get_untracked() {
            return;
        }
        self.following.set(true);
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        super::Gds::current().with(crate::appearance::portal::follow);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub(crate) fn system_changed(&self, reduced: bool) {
        self.policy.update(|policy| policy.system_reduced = reduced);
    }
}
