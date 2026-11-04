//! File and directory selection modes.
//!
//! In this mode the browser stays a browser: folders open, tabs and locations
//! work, and the file operations the user already has are never intercepted.
//! What changes is only the outcome of choosing a file: the window reports the
//! path on stdout and closes instead of handing it to another program. The
//! report is decided here, never while rendering.

use gds::appearance::tokens;
use gds::service::desktop::protocol::chooser::{self, Mode};
use gpui_kit::component::{
    WindowExt,
    dialog::AlertDialog,
    input::{Input, InputState},
};

mod save;
use std::sync::Arc;

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

use super::directory::{Entry, EntryKind, LinkStatus};
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

/// The visible file projection of one filter group.
///
/// Every tab keeps a copy so a directory read projects its fresh entries
/// through the same filter instead of replacing a narrowed list with the whole
/// folder.
#[derive(Clone)]
pub(super) struct Projection {
    options: Arc<crate::features::pick_files::Options>,
    active_filter: usize,
}

impl Projection {
    /// Whether an entry belongs in the list: every folder stays visible and
    /// navigable, files have to match the active group.
    pub(super) fn keeps(&self, entry: &Entry) -> bool {
        Self::is_directory(entry)
            || (!self.options.directory
                && self.options.matches_type(
                    self.active_filter,
                    &entry.path,
                    entry
                        .details
                        .display_content()
                        .and_then(|content| content.mime.as_deref()),
                ))
    }

    /// Whether the active group narrows files at all.
    pub(super) fn narrows(&self) -> bool {
        self.options
            .filters
            .get(self.active_filter)
            .is_some_and(|filter| filter.label.is_some())
    }

    /// Whether an entry is a folder, following a link only for navigation.
    pub(super) fn is_directory(entry: &Entry) -> bool {
        match entry.kind {
            EntryKind::Directory => true,
            EntryKind::SymbolicLink => entry.details.target.as_ref().is_some_and(|target| {
                target.status == LinkStatus::Resolved && target.kind == EntryKind::Directory
            }),
            _ => false,
        }
    }
}

/// One picker session.
pub(super) struct State {
    options: Arc<crate::features::pick_files::Options>,
    active_filter: usize,
    read_only: bool,
    filter_menu_open: bool,
    /// Set once the selection is being written and the window is closing, so a
    /// second confirm can neither print twice nor reopen anything.
    completing: bool,
    name: Option<Entity<InputState>>,
    original_name: Option<std::ffi::OsString>,
    pending_name: Option<std::ffi::OsString>,
    choices: Vec<chooser::Choice>,
    _name_subscription: Option<Subscription>,
}

impl State {
    pub(super) fn allows_multiple(&self) -> bool {
        self.options.multiple
    }

    pub(super) fn accepts_entry(&self, entry: &Entry) -> bool {
        if self.options.directory {
            Projection::is_directory(entry)
        } else {
            !Projection::is_directory(entry)
                && self.options.matches_type(
                    self.active_filter,
                    &entry.path,
                    entry
                        .details
                        .display_content()
                        .and_then(|content| content.mime.as_deref()),
                )
        }
    }

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
                    InputState::new(window, cx).placeholder(i18n::text("picker-name", cx));
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

        let choices = request
            .map(|request| request.choices.clone())
            .unwrap_or_default();
        Self {
            name,
            original_name,
            choices,
            pending_name: None,
            _name_subscription: subscription,
            read_only: options.read_only,
            filter_menu_open: false,
            options: Arc::new(options),
            active_filter: 0,
            completing: false,
        }
    }

    pub(super) fn projection(&self) -> Projection {
        Projection {
            options: Arc::clone(&self.options),
            active_filter: self.active_filter,
        }
    }

    fn filters(&self) -> &[crate::features::pick_files::Filter] {
        &self.options.filters
    }

    fn filter_label(&self, index: usize, cx: &App) -> SharedString {
        self.filters()
            .get(index)
            .and_then(|filter| filter.label.clone())
            .map_or_else(|| i18n::text("picker-all-files", cx), |label| label.into())
    }

    fn matches(&self, path: &Path) -> bool {
        self.options.matches(self.active_filter, path)
    }

    fn select_filter(&mut self, index: usize) -> bool {
        if index == self.active_filter || index >= self.filters().len() {
            return false;
        }
        self.active_filter = index;
        true
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
            .is_some_and(|picker| !picker.completing && !picker.filter_menu_open)
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
            .filter(|picker| !picker.options.directory)
        {
            self.set_picker_read_only(!picker.read_only, cx);
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
        if let Some(picker) = self
            .picker
            .as_ref()
            .filter(|picker| !picker.options.directory && picker.filters().len() > 1)
        {
            self.set_picker_filter((picker.active_filter + 1) % picker.filters().len(), cx);
        }
    }

    /// Paths a confirm would report. An empty directory selection chooses the
    /// current folder; pending navigation must never confirm the old location.
    fn picker_candidates(&self, cx: &App) -> Option<Vec<PathBuf>> {
        let picker = self.picker.as_ref()?;
        if picker.completing {
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
        if picker.options.directory {
            let selected = &tab.view().selected;
            if selected.is_empty() {
                return Some(vec![tab.path()?.to_path_buf()]);
            }
            if !picker.options.multiple && selected.len() > 1 {
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
        if selected.is_empty() || (!picker.options.multiple && selected.len() > 1) {
            return None;
        }
        selected
            .iter()
            .all(|path| {
                slot.table_rows
                    .iter()
                    .any(|entry| entry.path == *path && picker.accepts_entry(entry))
            })
            .then(|| selected.clone())
    }

    pub(super) fn set_picker_filter(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if index >= picker.filters().len() {
            return;
        }
        let projection = {
            let Some(picker) = self.picker.as_mut() else {
                return;
            };
            if !picker.select_filter(index) {
                return;
            }
            picker.projection()
        };
        self.reproject_picker(&projection);
        cx.notify();
    }

    pub(super) fn set_picker_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        let changed = self.picker.as_mut().is_some_and(|picker| {
            let changed = picker.read_only != read_only;
            picker.read_only = read_only;
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
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                self.navigate(path, cx);
                cx.notify();
            }
            Ok(metadata) if metadata.is_file() => {
                if let Some(picker) = self.picker.as_mut().filter(|picker| picker.name.is_some()) {
                    picker.pending_name = path.file_name().map(std::ffi::OsStr::to_owned);
                    cx.notify();
                    return;
                }
                let paths = self
                    .picker_candidates(cx)
                    .filter(|paths| paths.contains(&path))
                    .unwrap_or_else(|| vec![path]);
                self.complete_picker_selection(paths, cx)
            }
            Ok(_) => self.picker_notice("picker-not-file", None, cx),
            Err(error) => self.picker_notice("picker-missing", Some(error.to_string()), cx),
        }
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
        let (read_only, filter_index, directory, multiple) = match self.picker.as_ref() {
            Some(picker) if !picker.completing => (
                picker.read_only,
                picker.active_filter,
                picker.options.directory,
                picker.options.multiple,
            ),
            _ => return,
        };
        if paths.is_empty()
            || (!multiple
                && paths.len() != 1
                && !self.picker.as_ref().is_some_and(|picker| {
                    matches!(
                        picker.options.portal.as_ref().map(|request| &request.mode),
                        Some(Mode::SaveFiles { .. })
                    )
                }))
        {
            self.picker_notice(
                if directory {
                    "picker-select-directory"
                } else {
                    "picker-select-file"
                },
                None,
                cx,
            );
            return;
        }
        let saving = self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.options.is_save());
        for path in &paths {
            if saving {
                if path.is_dir() || !path.parent().is_some_and(|parent| parent.is_dir()) {
                    self.picker_notice("picker-not-file", None, cx);
                    return;
                }
                continue;
            }
            match std::fs::metadata(path) {
                Ok(metadata)
                    if (directory && metadata.is_dir()) || (!directory && metadata.is_file()) => {}
                Ok(_) => {
                    self.picker_notice(
                        if directory {
                            "picker-not-directory"
                        } else {
                            "picker-not-file"
                        },
                        None,
                        cx,
                    );
                    return;
                }
                Err(error) => {
                    self.picker_notice("picker-missing", Some(error.to_string()), cx);
                    return;
                }
            }
            if !directory
                && !self.picker.as_ref().is_some_and(|picker| {
                    self.active()
                        .and_then(|slot| slot.table_rows.iter().find(|entry| entry.path == *path))
                        .map_or_else(|| picker.matches(path), |entry| picker.accepts_entry(entry))
                })
            {
                self.picker_notice("picker-filter-mismatch", None, cx);
                return;
            }
        }
        let written = if let Some(picker) = self
            .picker
            .as_ref()
            .filter(|picker| picker.options.portal.is_some())
        {
            let result = chooser::Result {
                paths: paths
                    .iter()
                    .map(|path| path.clone().into_os_string())
                    .collect(),
                filter_index,
                read_only,
                choices: picker
                    .choices
                    .iter()
                    .map(|choice| (choice.id.clone(), choice.selected.clone()))
                    .collect(),
            };
            crate::features::pick_files::emit_portal_result(&result)
        } else {
            crate::features::pick_files::emit_selections(&paths, read_only, filter_index)
        };
        self.mark_picker_complete();
        match written {
            Ok(()) => crate::features::pick_files::mark_confirmed(),
            Err(error) => {
                tracing::warn!("Cannot report the picker selection: {error}");
                crate::features::pick_files::mark_failed();
            }
        }
        cx.quit();
    }

    /// Leave the picker without reporting anything.
    pub(super) fn cancel_picker(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if picker.completing {
            return;
        }
        self.mark_picker_complete();
        crate::features::pick_files::mark_cancelled();
        cx.quit();
    }

    fn mark_picker_complete(&mut self) {
        if let Some(picker) = self.picker.as_mut() {
            picker.completing = true;
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
            .is_some_and(|picker| picker.options.is_save())
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
        let labels: Vec<SharedString> = (0..picker.filters().len())
            .map(|index| picker.filter_label(index, cx))
            .collect();
        let active_filter = picker.active_filter;
        let completion_blocked = picker.completing;
        let read_only = picker.read_only;
        let directory = picker.options.directory;
        let view = cx.entity().downgrade();

        let next_filter_shortcut = Shortcut::new(Box::new(PickerNextFilter), Some(PICKER_CONTEXT));
        let filter_view = view.clone();
        let filter = Button::new("picker-filter")
            .small()
            .label(picker.filter_label(active_filter, cx))
            .when(picker.filters().len() > 1, |button| {
                button.tooltip_with_action(
                    i18n::text("picker-next-filter", cx),
                    &PickerNextFilter,
                    Some(PICKER_CONTEXT),
                )
            })
            .when(picker.filters().len() <= 1, |button| {
                button.tooltip(i18n::text("picker-filter", cx))
            })
            .when(available && picker.filters().len() > 1, |button| {
                button.shortcut(next_filter_shortcut)
            })
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                filter_menu(menu, labels.clone(), active_filter, filter_view.clone())
            });
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

        let read_only_button = Checkbox::new("picker-read-only")
            .small()
            .label(i18n::text("picker-read-only", cx))
            .disabled(!available)
            .checked(read_only);
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
        let read_only_button = div()
            .id("picker-read-only-control")
            .flex()
            .items_center()
            .tooltip(ui::shortcut_tooltip(
                i18n::text("picker-read-only", cx),
                read_only_shortcut,
            ))
            .child(read_only_button)
            .child(read_only_hint);

        let on_cancel = cx.listener(Self::on_picker_cancel_click);
        let cancel_shortcut = cancel_shortcut();
        let cancel_label = i18n::text("picker-cancel", cx);
        let cancel = Button::new("picker-cancel")
            .small()
            .icon(Icon::new(IconName::X))
            .label(cancel_label.clone())
            .disabled(!available)
            .shortcut(cancel_shortcut.clone())
            .on_click(on_cancel);
        let cancel = el! {
            .div {
                # "picker-cancel-button"; flex_shrink_0;
                tooltip_show_delay: tokens::TOOLTIP_DELAY;
                tooltip: $(ui::shortcut_tooltip(cancel_label, cancel_shortcut));
            } [cancel]
        };

        // Disabled decisions do not advertise a shortcut.
        let enabled = available && !completion_blocked && self.picker_candidates(cx).is_some();
        let saving = picker.options.is_save();
        let custom_label = picker
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
        let confirm = Button::new("picker-confirm")
            .primary()
            .small()
            .icon(Icon::new(IconName::Check))
            .label(confirm_label.clone())
            .disabled(!enabled)
            .shortcut(confirm_shortcut.clone())
            .on_click(on_confirm);
        let confirm = el! {
            .div {
                # "picker-confirm-button"; relative; flex_shrink_0;
                tooltip_show_delay: tokens::TOOLTIP_DELAY;
                tooltip: $(ui::shortcut_tooltip(confirm_label, confirm_shortcut));
            } [confirm, control_hint]
        };

        let optional_controls = if directory {
            Vec::new()
        } else {
            vec![filter.into_any_element()]
        };
        el! {
            .div {
                # "picker-controls"; flex; items_center; gap: CONTROL_GAP.px;
                flex_shrink_0; min_w_0;
            }
        }
        .children(optional_controls)
        .when(!saving && !directory, |control| {
            control.child(read_only_button)
        })
        .child(cancel)
        .child(confirm)
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
            menu.item(
                PopupMenuItem::new(label)
                    .checked(index == active_filter)
                    .on_click(move |_, _, cx| {
                        if let Some(view) = target.upgrade() {
                            view.update(cx, |this, cx| this.set_picker_filter(index, cx));
                        }
                    }),
            )
        })
}
