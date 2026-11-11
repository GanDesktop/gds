//! File and directory selection modes.
//!
//! In this mode the browser stays a browser: folders open, tabs and locations
//! work, and the file operations the user already has are never intercepted.
//! What changes is only the outcome of choosing a file: the window reports the
//! path on stdout and closes instead of handing it to another program. The
//! report is decided here, never while rendering.

use gds::appearance::tokens;
use gds::service::desktop::protocol::chooser::Mode;
use gpui_kit::component::{
    WindowExt,
    dialog::AlertDialog,
    input::{Input, InputState},
};

mod save;

use gds::ui::{self, Shortcut, button::Button};
use gds_uim::el;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable, Icon, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::*;

use super::*;

gpui_kit::actions!(
    gds_files_picker,
    [
        PickerConfirm,
        PickerConfirmWithControl,
        PickerCancel,
        PickerReadOnly,
        PickerNextFilter
    ]
);

/// Key context of the picker page.
///
/// The picker decisions live here, so `Alt+A` / `Alt+C` keep their dialog and
/// workspace meanings everywhere else.
pub(super) const PICKER_CONTEXT: &str = "Picker";

const CONTROL_GAP: f32 = 8.;

/// Toast slot of the picker's own validation messages.
pub(super) struct PickerNotice;

/// Register the picker decisions once per picker window.
///
/// Registered after the shared bindings, so within the picker scope these
/// strokes win only while [`PICKER_CONTEXT`] is on the focused path.
pub(super) fn register(cx: &mut App) {
    let bindings = [
        binding("alt-a", Box::new(PickerConfirm), cx),
        binding("ctrl-enter", Box::new(PickerConfirmWithControl), cx),
        binding("alt-c", Box::new(PickerCancel), cx),
        binding("alt-r", Box::new(PickerReadOnly), cx),
        binding("alt-t", Box::new(PickerNextFilter), cx),
    ];
    cx.bind_keys(bindings);
}

fn binding(stroke: &'static str, action: Box<dyn Action>, cx: &App) -> KeyBinding {
    let context = KeyBindingContextPredicate::parse(PICKER_CONTEXT)
        .expect("the bundled picker context is valid")
        .into();
    KeyBinding::load(stroke, action, Some(context), false, None, cx)
        .expect("the bundled picker shortcuts are valid")
}

pub(super) fn confirm_shortcut() -> Shortcut {
    Shortcut::new(Box::new(PickerConfirm), Some(PICKER_CONTEXT))
}

pub(super) fn cancel_shortcut() -> Shortcut {
    Shortcut::new(Box::new(PickerCancel), Some(PICKER_CONTEXT))
}

pub(super) use crate::features::pick_files::Projection;
use crate::features::pick_files::Session;

/// One picker session.
pub(super) struct State {
    pub session: Session,
    filter_menu_open: bool,
    name: Option<Entity<InputState>>,
    original_name: Option<std::ffi::OsString>,
    pending_name: Option<std::ffi::OsString>,
    _name_subscription: Option<Subscription>,
}

impl State {
    pub(super) fn new(
        options: crate::features::pick_files::Options,
        window: &mut Window,
        cx: &mut Context<Browser>,
    ) -> Self {
        let request = options.portal.as_ref();
        let original_name = request.and_then(|request| match &request.mode {
            Mode::Save { name } => Some(
                request
                    .current_file
                    .as_ref()
                    .and_then(|file| Path::new(file).file_name())
                    .unwrap_or(name)
                    .to_owned(),
            ),
            _ => None,
        });
        let name = original_name.as_ref().map(|name| {
            cx.new(|cx| {
                let mut input =
                    InputState::new(window, cx).placeholder(i18n::text("picker-file-name", cx));
                input.set_value(name.to_string_lossy().into_owned(), window, cx);
                input
            })
        });
        let subscription = name.as_ref().map(|name| {
            cx.subscribe_in(
                name,
                window,
                |this, _, event: &gpui_kit::component::input::InputEvent, window, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        this.confirm_picker(window, cx);
                    }
                    cx.notify();
                },
            )
        });
        if let Some(key) = request
            .and_then(|request| request.accept_label.as_ref())
            .and_then(|label| save::Mnemonic::parse(label).key)
        {
            let stroke = format!("alt-{}", key.to_lowercase());
            let context = KeyBindingContextPredicate::parse(PICKER_CONTEXT)
                .expect("picker context")
                .into();
            match KeyBinding::load(
                &stroke,
                Box::new(PickerConfirm),
                Some(context),
                false,
                None,
                cx,
            ) {
                Ok(binding) => cx.bind_keys([binding]),
                Err(error) => tracing::warn!(%error, "cannot bind chooser mnemonic"),
            }
        }

        Self {
            name,
            original_name,
            pending_name: None,
            _name_subscription: subscription,
            filter_menu_open: false,
            session: Session::new(options),
        }
    }

    fn filter_label(&self, index: usize, cx: &App) -> SharedString {
        self.session
            .filters()
            .get(index)
            .and_then(|filter| filter.label.clone())
            .map_or_else(|| i18n::text("picker-all-files", cx), |label| label.into())
    }
}

impl Browser {
    pub(super) fn picker_actions_available(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.picker
            .as_ref()
            .is_some_and(|picker| !picker.session.completing && !picker.filter_menu_open)
            && self.pages.is_empty()
            && self.menus.open.is_none()
            && !self.toolbar.path.editing
            && !self.operations.rename.read(cx).is_open()
            && !window.has_active_dialog(cx)
    }

    pub(super) fn on_picker_confirm_with_control(
        &mut self,
        _: &PickerConfirmWithControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_picker(window, cx);
    }

    pub(super) fn on_picker_read_only(
        &mut self,
        _: &PickerReadOnly,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.picker_actions_available(window, cx) {
            return;
        }
        if let Some(picker) = self
            .picker
            .as_ref()
            .filter(|picker| !picker.session.options.directory)
        {
            self.set_picker_read_only(!picker.session.read_only, cx);
        }
    }

    pub(super) fn on_picker_next_filter(
        &mut self,
        _: &PickerNextFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.picker_actions_available(window, cx) {
            return;
        }
        if let Some(picker) = self.picker.as_ref().filter(|picker| {
            !picker.session.options.directory && picker.session.filters().len() > 1
        }) {
            self.set_picker_filter(
                (picker.session.active_filter + 1) % picker.session.filters().len(),
                cx,
            );
        }
    }

    /// Paths a confirm would report. An empty directory selection chooses the
    /// current folder; pending navigation must never confirm the old location.
    fn picker_candidates(&self, cx: &App) -> Option<Vec<PathBuf>> {
        let picker = self.picker.as_ref()?;
        if picker.session.completing {
            return None;
        }
        let slot = self.active()?;
        if slot.is_loading() {
            return None;
        }
        let tab = &slot.navigation;
        if picker.name.is_some() {
            let name = picker.save_name(cx)?;
            let path = tab.path()?;
            return crate::features::pick_files::Options::valid_name(&name)
                .then(|| vec![path.join(name)]);
        }
        if picker.session.options.directory {
            let selected = &tab.view().selected;
            if selected.is_empty() {
                return Some(vec![tab.path()?.to_path_buf()]);
            }
            if !picker.session.options.multiple && selected.len() > 1 {
                return None;
            }
            return selected
                .iter()
                .all(|path| {
                    slot.table_rows
                        .iter()
                        .any(|entry| entry.path == *path && Projection::is_directory(entry))
                })
                .then(|| selected.clone());
        }
        let selected = &tab.view().selected;
        if selected.is_empty() || (!picker.session.options.multiple && selected.len() > 1) {
            return None;
        }
        selected
            .iter()
            .all(|path| {
                slot.table_rows
                    .iter()
                    .any(|entry| entry.path == *path && picker.session.accepts_entry(entry))
            })
            .then(|| selected.clone())
    }

    pub(super) fn set_picker_filter(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if index >= picker.session.filters().len() {
            return;
        }
        let projection = {
            let Some(picker) = self.picker.as_mut() else {
                return;
            };
            if !picker.session.select_filter(index) {
                return;
            }
            picker.session.projection()
        };
        self.reproject_picker(&projection);
        cx.notify();
    }

    pub(super) fn set_picker_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        let changed = self.picker.as_mut().is_some_and(|picker| {
            let changed = picker.session.read_only != read_only;
            picker.session.read_only = read_only;
            changed
        });
        if changed {
            cx.notify();
        }
    }

    /// Apply one projection to every file slot, dropping a selection the new
    /// group no longer shows.
    fn reproject_picker(&mut self, projection: &Projection) {
        for slot in &mut self.tabs.items {
            slot.picker = Some(projection.clone());
            // A selection pending from a rename or a new folder is applied by
            // the refresh that follows it; consuming it now would drop it
            // before the entry exists.
            if slot.pending_selection.is_none() {
                slot.refresh_visible();
            }
        }
    }

    /// A click or Enter on a file row in picker mode.
    pub(super) fn activate_picker_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let target = path.clone();
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Inspect picker activation target", move || {
                crate::features::pick_files::inspect_activated_path(&target)
            });
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            let _ = this.update(cx, |this, cx| match outcome {
                Ok(Ok(crate::features::pick_files::ActivatedPath::Directory)) => {
                    this.navigate(path, cx);
                    cx.notify();
                }
                Ok(Ok(crate::features::pick_files::ActivatedPath::File)) => {
                    if let Some(picker) =
                        this.picker.as_mut().filter(|picker| picker.name.is_some())
                    {
                        picker.pending_name = path.file_name().map(std::ffi::OsStr::to_owned);
                        cx.notify();
                        return;
                    }
                    let paths = this
                        .picker_candidates(cx)
                        .filter(|paths| paths.contains(&path))
                        .unwrap_or_else(|| vec![path]);
                    this.complete_picker_selection(paths, cx);
                }
                Ok(Err(failure)) => {
                    this.picker_notice(failure.message_id, failure.detail, cx);
                }
                Err(error) => {
                    this.picker_notice("picker-missing", Some(error.to_string()), cx);
                }
            });
        })
        .detach();
    }

    /// Report the selection, then close the window.
    ///
    /// The path is revalidated here rather than trusted from the render state:
    /// the entry may have gone away or stopped being a regular file, and a
    /// stale projection must not let an out-of-group file through.
    fn complete_picker_selection(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths
            .iter()
            .any(|path| !self.device_path_available(path, cx))
        {
            notifications::error(i18n::text("device-path-unavailable", cx), cx);
            return;
        }
        let Some(picker) = self
            .picker
            .as_ref()
            .filter(|picker| !picker.session.completing)
        else {
            return;
        };
        let accepted_in_table = self
            .active()
            .map(|slot| {
                slot.table_rows
                    .iter()
                    .filter(|entry| picker.session.accepts_entry(entry))
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let check = picker.session.selection_check(accepted_in_table);
        let target_paths = paths.clone();
        let work = self
            .window
            .tasks
            .spawn_blocking_named("Validate picker selection", move || {
                check.validate(&target_paths)
            });
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            let _ = this.update(cx, |this, cx| match outcome {
                Ok(Ok(())) => {
                    let Some(picker) = this.picker.as_mut() else {
                        return;
                    };
                    if picker.session.completing {
                        return;
                    }
                    if let Err(error) = picker.session.complete(&paths) {
                        tracing::warn!("Cannot report the picker selection: {error}");
                    }
                    cx.quit();
                }
                Ok(Err(failure)) => {
                    this.picker_notice(failure.message_id, failure.detail, cx);
                }
                Err(error) => {
                    this.picker_notice("picker-missing", Some(error.to_string()), cx);
                }
            });
        })
        .detach();
    }

    /// Leave the picker without reporting anything.
    pub(super) fn cancel_picker(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if picker.session.cancel() {
            cx.quit();
        }
    }

    fn picker_notice(&mut self, message_id: &str, detail: Option<String>, cx: &mut Context<Self>) {
        let mut report =
            notifications::Report::new(notifications::Level::Warning, i18n::text(message_id, cx));
        if let Some(detail) = detail {
            report = report.detail(detail);
        }
        report.replacing::<PickerNotice>().show(cx);
        cx.notify();
    }

    /// Confirm from the button or `Alt+A`.
    ///
    /// Local modes keep their own keys: a dialog, the menu, Settings or a rename
    /// editor must never reach out and close the window underneath it.
    pub(super) fn confirm_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.picker_actions_available(window, cx) {
            return;
        }
        let paths = self.picker_candidates(cx).unwrap_or_default();
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.session.options.is_save())
        {
            self.confirm_save(paths, window, cx);
        } else {
            self.complete_picker_selection(paths, cx);
        }
    }

    pub(super) fn on_picker_confirm(
        &mut self,
        _: &PickerConfirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_picker(window, cx);
    }

    pub(super) fn on_picker_cancel(
        &mut self,
        _: &PickerCancel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picker_actions_available(window, cx) {
            self.cancel_picker(cx);
        }
    }

    pub(super) fn on_picker_confirm_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_picker(window, cx);
    }

    pub(super) fn on_picker_cancel_click(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_picker(cx);
    }

    pub(super) fn sync_picker_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker.as_mut() {
            if let Some(name) = picker.pending_name.take() {
                if let Some(input) = &picker.name {
                    input.update(cx, |input, cx| {
                        input.set_value(name.to_string_lossy().into_owned(), window, cx)
                    });
                    picker.original_name = Some(name);
                }
            }
        }
    }

    /// The footer controls a picker shows in place of the terminal toggle.
    pub(super) fn render_picker_controls(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let available = self.picker_actions_available(window, cx);
        let Some(picker) = self.picker.as_ref() else {
            return el! { .div }.into_any_element();
        };
        let labels: Vec<SharedString> = (0..picker.session.filters().len())
            .map(|index| picker.filter_label(index, cx))
            .collect();
        let active_filter = picker.session.active_filter;
        let completion_blocked = picker.session.completing;
        let read_only = picker.session.read_only;
        let directory = picker.session.options.directory;
        let view = cx.entity().downgrade();

        let next_filter_shortcut = Shortcut::new(Box::new(PickerNextFilter), Some(PICKER_CONTEXT));
        let filter_view = view.clone();
        let filter = el! { "picker-filter".Button::new {
            small;
            label: $(picker.filter_label(active_filter, cx));
            when: $(picker.session.filters().len() > 1), $(|button| {
                button.tooltip_with_action(
                    i18n::text("picker-next-filter", cx),
                    &PickerNextFilter,
                    Some(PICKER_CONTEXT),
                )
            });
            when: $(picker.session.filters().len() <= 1), $(|button| {
                button.tooltip(i18n::text("picker-file-type", cx))
            });
            when: $(available && picker.session.filters().len() > 1), $(|button| {
                button.shortcut(next_filter_shortcut)
            });
            dropdown_caret: true;
            dropdown_menu: $(move |menu, _, _| {
                filter_menu(menu, labels.clone(), active_filter, filter_view.clone())
            });
        } };
        let menu_view = view.clone();
        let filter = filter.on_open_change(move |open, _, cx| {
            if let Some(view) = menu_view.upgrade() {
                view.update(cx, |this, cx| {
                    if let Some(picker) = this.picker.as_mut() {
                        picker.filter_menu_open = *open;
                    }
                    cx.notify();
                });
            }
        });

        let read_only_button = el! { "picker-read-only".Checkbox::new {
            small;
            label: $(i18n::text("picker-read-only", cx));
            disabled: $(!available);
            checked: read_only;
        } };
        let read_view = view.clone();
        let read_only_button = read_only_button.on_change(move |checked, _, cx| {
            if let Some(view) = read_view.upgrade() {
                view.update(cx, |this, cx| this.set_picker_read_only(*checked, cx));
            }
        });
        let read_only_shortcut = Shortcut::new(Box::new(PickerReadOnly), Some(PICKER_CONTEXT));
        let read_only_hint = if available {
            ui::keytips::suffix_anchor(
                window,
                cx,
                "picker-read-only-hint".into(),
                read_only_shortcut.clone(),
            )
        } else {
            el! { .div { size_0 } }.into_any_element()
        };
        let read_only_button = el! {
            .div { # "picker-read-only-control"; flex; items_center;
                tooltip: $(ui::shortcut_tooltip(i18n::text("picker-read-only", cx), read_only_shortcut));
            } [read_only_button, read_only_hint]
        };

        let on_cancel = cx.listener(Self::on_picker_cancel_click);
        let cancel_shortcut = cancel_shortcut();
        let cancel_label = i18n::text("picker-cancel", cx);
        let cancel = el! { "picker-cancel".Button::new {
            small;
            icon: $(Icon::new(IconName::X));
            label: $(cancel_label.clone());
            disabled: $(!available);
            shortcut: $(cancel_shortcut.clone());
            on_click: on_cancel;
        } };
        let cancel = el! {
            .div {
                # "picker-cancel-button"; flex_shrink_0;
                tooltip_show_delay: tokens::TOOLTIP_DELAY;
                tooltip: $(ui::shortcut_tooltip(cancel_label, cancel_shortcut));
            } [cancel]
        };

        // Disabled decisions do not advertise a shortcut.
        let enabled = available && !completion_blocked && self.picker_candidates(cx).is_some();
        let saving = picker.session.options.is_save();
        let custom_label = picker
            .session
            .options
            .portal
            .as_ref()
            .and_then(|request| request.accept_label.as_ref())
            .map(|label| save::Mnemonic::parse(label));
        let confirm_label = custom_label
            .as_ref()
            .map(|label| SharedString::from(label.text.clone()))
            .unwrap_or_else(|| {
                i18n::text(
                    if saving {
                        "picker-save"
                    } else if directory {
                        "picker-confirm-directory"
                    } else {
                        "picker-confirm"
                    },
                    cx,
                )
            });
        let confirm_shortcut = confirm_shortcut();
        let on_confirm = cx.listener(Self::on_picker_confirm_click);
        let control_hint = if enabled {
            ui::keytips::anchor(
                window,
                cx,
                "picker-confirm-control-hint".into(),
                Shortcut::new(Box::new(PickerConfirmWithControl), Some(PICKER_CONTEXT)),
            )
        } else {
            el! { .div { size_0 } }.into_any_element()
        };
        let confirm = el! { "picker-confirm".Button::new {
            primary;
            small;
            icon: $(Icon::new(IconName::Check));
            label: $(confirm_label.clone());
            disabled: $(!enabled);
            shortcut: $(confirm_shortcut.clone());
            on_click: on_confirm;
        } };
        let confirm = el! {
            .div {
                # "picker-confirm-button"; relative; flex_shrink_0;
                tooltip_show_delay: tokens::TOOLTIP_DELAY;
                tooltip: $(ui::shortcut_tooltip(confirm_label, confirm_shortcut));
            } [confirm, control_hint]
        };

        el! {
            .div {
                # "picker-controls"; flex; items_center; gap: CONTROL_GAP.px;
                flex_shrink_0; min_w_0;
            } [
                @if (!directory) filter, @end,
                @if (!saving && !directory) read_only_button, @end,
                cancel,
                confirm,
            ]
        }
        .into_any_element()
    }
}

/// The filter group rows, with the active group checked.
fn filter_menu(
    menu: PopupMenu,
    labels: Vec<SharedString>,
    active_filter: usize,
    view: WeakEntity<Browser>,
) -> PopupMenu {
    labels
        .into_iter()
        .enumerate()
        .fold(menu, |menu, (index, label)| {
            let target = view.clone();
            menu.item(el! { label.PopupMenuItem::new {
                checked: $(index == active_filter);
                on_click: $(move |_, _, cx| {
                    if let Some(view) = target.upgrade() {
                        view.update(cx, |this, cx| this.set_picker_filter(index, cx));
                    }
                });
            } })
        })
}
