//! Native chooser controls integrated into the 36px workspace footer.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
};

use gds::service::desktop::protocol::chooser;
use gds::ui::button::{ButtonProps as GdsButtonProps, ButtonSize, ButtonVariant};
use gds::ui::keytips::{HintPlacement, KeytipRegistry};
use gds::ui::{IconName, Shortcut};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, Signal, on_cleanup_local};
use zgui::view::{EventCx, current_host, events, install_stylesheet, request_frame};
use zgui::vocab::{ImeEvent, Key, KeyEvent, Modifiers, NamedKey};
use zgui::{css, view};
use zgui_ui::{
    CheckboxProps, Checked, InputProps, LabelProps, SelectContentProps, SelectItemProps,
    SelectProps, SelectTriggerProps, SelectTriggerSize, SelectValueProps,
};
use zgui_ui_primitives::Binding;

use super::{
    model::{AddressPresentation, ConfirmOptions, Contents, Pane, Prompt, Row},
    ui::{Browser, WeakBrowser},
};
use crate::{
    features::pick_files::{
        self, ActivatedPath, Options, SaveDecision, inspect_activated_path, prepare_save_targets,
    },
    shared::i18n,
};

const PICKER_SHEET: &str = "gds-files-picker-footer";
const PICKER_CSS: &str = css!(
    r#"
    .gds-files-picker-extras {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-wrap: wrap;
        gap: 12px;
        padding: 6px 18px;
        border-top: 1px solid var(--gds-border);
        background: var(--gds-content-bg, var(--gds-surface));
        flex-shrink: 0;
        box-sizing: border-box;
    }

    .gds-files-picker-name-slot {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0%;
        min-width: 140px;
        max-width: 480px;
        height: 100%;
        box-sizing: border-box;
    }

    .gds-files-picker-name-input.zui-field,
    .gds-files-picker-name-input.zui-input,
    .gds-files-picker-name-input.gds-files-picker-name-input {
        width: 100%;
        height: 28px;
        min-height: 28px;
        max-height: 28px;
        padding: 0 8px;
        font-size: var(--gds-font-size-small, 14px);
    }

    .gds-files-picker-controls-slot {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        height: 100%;
        box-sizing: border-box;
    }

    .gds-files-picker-control-item {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        white-space: nowrap;
    }

    .gds-files-picker-label {
        font-size: var(--gds-font-size-xs, 12px);
        color: var(--gds-muted-foreground);
        white-space: nowrap;
    }

    .gds-files-picker-filter-trigger.gds-files-picker-filter-trigger,
    .gds-files-picker-filter-trigger.gds-files-picker-filter-trigger[data-size="sm"] {
        height: 28px;
        min-height: 28px;
        max-height: 28px;
        padding-top: 0;
        padding-bottom: 0;
        max-width: 200px;
    }

    .gds-files-picker-btn.zui-button.gds-button[data-size="sm"] {
        height: 28px;
        min-height: 28px;
        max-height: 28px;
    }
    "#
);

fn install_picker_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(PICKER_SHEET, PICKER_CSS);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Mnemonic {
    pub label: String,
    pub key: Option<char>,
}

impl Mnemonic {
    pub fn parse(input: &str) -> Self {
        let mut label = String::with_capacity(input.len());
        let mut key = None;
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '_' {
                if chars.peek() == Some(&'_') {
                    label.push('_');
                    chars.next();
                } else if let Some(&next) = chars.peek()
                    && key.is_none()
                    && next.is_ascii_graphic()
                {
                    key = Some(next.to_ascii_lowercase());
                }
            } else {
                label.push(ch);
            }
        }
        Self { label, key }
    }
}

struct PickerUiSlot {
    browser: WeakBrowser,
    save_input_node: NodeRef,
}

thread_local! {
    static PICKER_SLOTS: RefCell<HashMap<usize, Rc<PickerUiSlot>>> = RefCell::new(HashMap::new());
}

fn picker_ui_slot(browser: &Browser) -> Rc<PickerUiSlot> {
    let key = Rc::as_ptr(&browser.0) as usize;
    if let Some(existing) = PICKER_SLOTS.with(|map| {
        map.borrow().get(&key).and_then(|slot| {
            slot.browser
                .upgrade()
                .filter(|b| Rc::ptr_eq(&b.0, &browser.0))
                .map(|_| Rc::clone(slot))
        })
    }) {
        return existing;
    }

    let created = browser.0.owner.with(|| {
        on_cleanup_local(move || {
            PICKER_SLOTS.with(|map| {
                map.borrow_mut().remove(&key);
            });
        });
        Rc::new(PickerUiSlot {
            browser: browser.downgrade(),
            save_input_node: NodeRef::new(),
        })
    });
    PICKER_SLOTS.with(|map| {
        map.borrow_mut().insert(key, Rc::clone(&created));
    });
    created
}

impl Browser {
    pub(super) fn initialize_picker(&self) {
        let initial = self.read_untracked(|state| {
            state
                .picker
                .as_ref()
                .and_then(|picker| picker.save_name.as_ref())
                .map(|name| name.to_string_lossy().into_owned())
        });
        if let Some(name) = initial {
            self.0.save_name.set(name);
        }
    }

    fn picker_actions_available(&self) -> bool {
        self.pane() == Pane::Files
            && matches!(self.0.prompt.get(), Prompt::Closed)
            && matches!(self.0.menus.surface.get(), super::menus::Surface::Closed)
            && !self.0.connector.is_connecting()
            && matches!(
                self.0.address_presentation.get(),
                AddressPresentation::Breadcrumbs
            )
    }

    fn picker_actions_available_untracked(&self) -> bool {
        self.pane_untracked() == Pane::Files
            && matches!(self.0.prompt.get_untracked(), Prompt::Closed)
            && !self.0.menus.is_open()
            && !self.0.connector.is_connecting_untracked()
            && matches!(
                self.0.address_presentation.get_untracked(),
                AddressPresentation::Breadcrumbs
            )
    }

    fn picker_candidates_inner(&self, reactive: bool) -> Option<Vec<PathBuf>> {
        let typed_save = if reactive {
            self.0.save_name.get()
        } else {
            self.0.save_name.get_untracked()
        };
        let inspect = |state: &super::model::State| -> Option<Vec<PathBuf>> {
            let picker = state.picker.as_ref()?;
            if picker.submitting || picker.completing {
                return None;
            }
            let tab = state.tab();
            if tab.navigation.is_loading() {
                return None;
            }
            let current_dir = tab.navigation.path().map(Path::to_path_buf);
            let dir_contents = matches!(tab.contents, Contents::Directory);
            let selected = tab.selected();

            if picker.options.is_save() {
                if !dir_contents {
                    return None;
                }
                let dir = current_dir?;
                let file_name = if let Some(original) = picker.save_name.as_ref()
                    && original.to_string_lossy() == typed_save
                {
                    original.clone()
                } else {
                    OsString::from(&typed_save)
                };
                if !Options::valid_name(&file_name) {
                    return None;
                }
                return Some(vec![dir.join(file_name)]);
            }

            if matches!(
                picker.options.portal.as_ref().map(|p| &p.mode),
                Some(chooser::Mode::SaveFiles { .. })
            ) {
                let folder = selected
                    .iter()
                    .find_map(|row| match row {
                        Row::File(entry) if pick_files::Projection::is_directory(entry) => {
                            Some(entry.path.clone())
                        }
                        _ => None,
                    })
                    .or_else(|| dir_contents.then_some(current_dir).flatten())?;
                return Some(vec![folder]);
            }

            if picker.options.directory {
                if selected.is_empty() {
                    if !dir_contents {
                        return None;
                    }
                    let dir = current_dir?;
                    return Some(vec![dir]);
                }
                if !picker.allows_multiple() && selected.len() > 1 {
                    return None;
                }
                let mut dirs = Vec::with_capacity(selected.len());
                for row in selected {
                    match row {
                        Row::File(entry) if pick_files::Projection::is_directory(&entry) => {
                            dirs.push(entry.path.clone());
                        }
                        _ => return None,
                    }
                }
                return (!dirs.is_empty()).then_some(dirs);
            }

            if selected.is_empty() || (!picker.allows_multiple() && selected.len() > 1) {
                return None;
            }
            let mut files = Vec::with_capacity(selected.len());
            for row in selected {
                match row {
                    Row::File(entry) if picker.accepts_entry(&entry) => {
                        files.push(entry.path.clone());
                    }
                    _ => return None,
                }
            }
            (!files.is_empty()).then_some(files)
        };

        if reactive {
            self.read(inspect)
        } else {
            self.read_untracked(inspect)
        }
    }

    fn picker_accept_mnemonic(&self) -> Mnemonic {
        self.read_untracked(|state| {
            let Some(picker) = state.picker.as_ref() else {
                return Mnemonic {
                    label: i18n::text_untracked("picker-confirm"),
                    key: None,
                };
            };
            if let Some(custom) = picker
                .options
                .portal
                .as_ref()
                .and_then(|portal| portal.accept_label.as_deref())
            {
                let parsed = Mnemonic::parse(custom);
                if !parsed.label.is_empty() {
                    return parsed;
                }
            }
            let fallback = if picker.options.is_save() {
                i18n::text_untracked("picker-save")
            } else if picker.options.directory {
                i18n::text_untracked("picker-confirm-directory")
            } else {
                i18n::text_untracked("picker-confirm")
            };
            Mnemonic {
                label: fallback,
                key: None,
            }
        })
    }

    /// Renders optional portal `choices` above the `36px` workspace footer when non-empty.
    pub(super) fn picker_footer(&self) -> AnyView {
        let choices_data = self.read_untracked(|state| {
            state
                .picker
                .as_ref()
                .map(|picker| picker.choices.clone())
                .unwrap_or_default()
        });
        if choices_data.is_empty() {
            return AnyView::new(view! { box(style:display = "none") });
        }
        install_picker_stylesheet();

        let revision = self.0.revision;
        let browser_choice_bool = self.clone();
        let browser_choice_select = self.clone();

        let choice_views: Vec<AnyView> = choices_data
            .into_iter()
            .map(|choice| {
                let choice_id = choice.id.clone();
                let label = Mnemonic::parse(&choice.label).label;
                if choice.options.is_empty() {
                    let read_b = browser_choice_bool.clone();
                    let write_b = browser_choice_bool.clone();
                    let read_id = choice_id.clone();
                    let write_id = choice_id;
                    let checked = Binding::controlled(
                        Signal::derive_local(move || {
                            let _ = revision.get();
                            let active = read_b.read(|state| {
                                state.picker.as_ref().is_some_and(|picker| {
                                    picker
                                        .choices
                                        .iter()
                                        .find(|c| c.id == read_id)
                                        .is_some_and(|c| c.selected == "true")
                                })
                            });
                            if active { Checked::Yes } else { Checked::No }
                        }),
                        move |next: Checked| {
                            write_b.set_picker_choice(
                                &write_id,
                                if next == Checked::Yes {
                                    "true"
                                } else {
                                    "false"
                                },
                            );
                        },
                    );
                    let box_ref = NodeRef::new();
                    let lbl_ref = NodeRef::new();
                    AnyView::new(view! {
                        row(class = "gds-files-picker-control-item") {
                            Checkbox(node_ref = box_ref, checked = checked, labelled_by = lbl_ref)
                            Label(node_ref = lbl_ref, control = box_ref) {{label}}
                        }
                    })
                } else {
                    let read_b = browser_choice_select.clone();
                    let write_b = browser_choice_select.clone();
                    let read_id = choice_id.clone();
                    let write_id = choice_id;
                    let selected_binding = Binding::controlled(
                        Signal::derive_local(move || {
                            let _ = revision.get();
                            read_b.read(|state| {
                                state
                                    .picker
                                    .as_ref()
                                    .and_then(|picker| {
                                        picker
                                            .choices
                                            .iter()
                                            .find(|c| c.id == read_id)
                                            .map(|c| c.selected.clone())
                                    })
                                    .unwrap_or_default()
                            })
                        }),
                        move |next: String| {
                            write_b.set_picker_choice(&write_id, &next);
                        },
                    );
                    let options = Signal::stored_local(choice.options);
                    AnyView::new(view! {
                        row(class = "gds-files-picker-control-item") {
                            text(class = "gds-files-picker-label") {{label.clone()}}
                            Select(value = selected_binding) {
                                SelectTrigger(
                                    size = SelectTriggerSize::Sm,
                                    label = label,
                                    class = "gds-files-picker-filter-trigger",
                                ) {
                                    SelectValue {}
                                }
                                SelectContent {
                                    for opt in move || options.get(), key = |opt| opt.0.clone() {
                                        SelectItem(value = opt.0) {
                                            {Mnemonic::parse(&opt.1).label}
                                        }
                                    }
                                }
                            }
                        }
                    })
                }
            })
            .collect();

        AnyView::new(view! {
            row(class = "gds-files-picker-extras") {
                {choice_views}
            }
        })
    }

    pub(super) fn render_picker_name(&self) -> Option<AnyView> {
        let is_save = self.read_untracked(|state| {
            state
                .picker
                .as_ref()
                .is_some_and(|picker| picker.options.is_save())
        });
        if !is_save {
            return None;
        }
        install_picker_stylesheet();
        let slot = picker_ui_slot(self);
        let save_input_node = slot.save_input_node;
        let save_name = self.0.save_name;
        let save_composing = Rc::new(Cell::new(false));
        let save_focus_browser = self.clone();
        let avail_browser = self.clone();
        let disabled_input =
            Signal::derive_local(move || !avail_browser.picker_actions_available());

        let save_effect = RenderEffect::new(move |prev: Option<bool>| {
            let mounted = save_input_node.get().is_some();
            if mounted && prev != Some(true) {
                save_input_node.focus();
                let name = save_focus_browser.0.save_name.get_untracked();
                let stem_end = save_stem_end(&name);
                save_input_node.set_value(&name);
                save_input_node.set_selection(0..stem_end);
            }
            mounted
        });
        on_cleanup_local(move || drop(save_effect));

        let c_start = Rc::clone(&save_composing);
        let c_pre = Rc::clone(&save_composing);
        let c_commit = Rc::clone(&save_composing);
        let c_end = Rc::clone(&save_composing);
        let c_blur = Rc::clone(&save_composing);
        let browser_accept = self.clone();
        let browser_cancel = self.clone();
        let file_name_label = i18n::text("picker-file-name");

        Some(AnyView::new(view! {
            box(class = "gds-files-picker-name-slot") {
                Input(
                    class = "gds-files-picker-name-input",
                    value = save_name,
                    disabled = disabled_input,
                    placeholder = file_name_label.clone(),
                    label = file_name_label,
                    node_ref = save_input_node,
                    on:ime_start = move |_| c_start.set(true),
                    on:ime_preedit = move |event| {
                        let active = match &**event {
                            ImeEvent::Preedit { text, .. } => !text.is_empty(),
                            _ => event.is_composing(),
                        };
                        c_pre.set(active);
                    },
                    on:ime_commit = move |_| c_commit.set(false),
                    on:ime_end = move |_| c_end.set(false),
                    on:focus_out = move |_| c_blur.set(false),
                    on:key_down = move |event: &mut EventCx<'_, events::KeyDown>| {
                        if save_composing.get() {
                            return;
                        }
                        if event.key == Key::Named(NamedKey::Enter)
                            && !event.modifiers.control()
                            && !event.modifiers.alt()
                            && !event.modifiers.meta()
                        {
                            event.prevent_default();
                            event.stop_propagation();
                            if browser_accept.picker_candidates_inner(false).is_some() {
                                browser_accept.accept_picker();
                            }
                        } else if event.key == Key::Named(NamedKey::Escape)
                            && event.modifiers.is_empty()
                        {
                            event.prevent_default();
                            event.stop_propagation();
                            browser_cancel.cancel_picker();
                        }
                    },
                )
            }
        }))
    }

    pub(super) fn render_picker_controls(&self) -> Option<AnyView> {
        if self.read_untracked(|state| state.picker.is_none()) {
            return None;
        }
        install_picker_stylesheet();

        let revision = self.0.revision;
        let show_read_only = self.read_untracked(|state| {
            state.picker.as_ref().is_some_and(|picker| {
                !picker.options.directory
                    && !picker.options.is_save()
                    && self.0.picker_result.is_none()
            })
        });
        let filters: Vec<(String, String)> = self.read_untracked(|state| {
            state
                .picker
                .as_ref()
                .map(|picker| {
                    picker
                        .filters()
                        .iter()
                        .enumerate()
                        .map(|(idx, filter)| {
                            let label = filter
                                .label
                                .clone()
                                .unwrap_or_else(|| i18n::text_untracked("picker-all-files"));
                            (idx.to_string(), label)
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
        let filter_count = filters.len();
        let show_filters = self.read_untracked(|state| {
            state
                .picker
                .as_ref()
                .is_some_and(|picker| !picker.options.directory && !picker.filters().is_empty())
        });

        let accept_mnemonic = self.picker_accept_mnemonic();
        let confirm_label = accept_mnemonic.label;
        let cancel_label = i18n::text_untracked("picker-cancel");

        let filter_browser = self.clone();
        let active_filter_value = Binding::controlled(
            Signal::derive_local(move || {
                let _ = revision.get();
                filter_browser.read(|state| {
                    state
                        .picker
                        .as_ref()
                        .map_or_else(|| "0".to_owned(), |picker| picker.active_filter.to_string())
                })
            }),
            {
                let browser = self.clone();
                move |next: String| {
                    if let Ok(index) = next.parse::<usize>() {
                        browser.select_picker_filter(index);
                    }
                }
            },
        );

        let ro_read_browser = self.clone();
        let ro_write_browser = self.clone();
        let read_only_binding = Binding::controlled(
            Signal::derive_local(move || {
                let _ = revision.get();
                if ro_read_browser
                    .read(|state| state.picker.as_ref().is_some_and(|picker| picker.read_only))
                {
                    Checked::Yes
                } else {
                    Checked::No
                }
            }),
            move |checked: Checked| {
                ro_write_browser.update(|state| {
                    if let Some(picker) = state.picker.as_mut() {
                        picker.read_only = checked == Checked::Yes;
                    }
                });
                ro_write_browser.changed();
            },
        );

        let disabled_confirm = {
            let browser = self.clone();
            Signal::derive_local(move || {
                !browser.picker_actions_available()
                    || browser.picker_candidates_inner(true).is_none()
            })
        };
        let enabled_actions = {
            let browser = self.clone();
            Signal::derive_local(move || browser.picker_actions_available())
        };
        let confirm_enabled_sig = Signal::derive_local(move || !disabled_confirm.get());

        let filter_node = NodeRef::new();
        let ro_node = NodeRef::new();
        let ro_lbl_node = NodeRef::new();
        let ro_wrap_node = NodeRef::new();
        let confirm_node = NodeRef::new();

        if let Some(reg) = KeytipRegistry::current() {
            if show_filters && filter_count > 1 {
                reg.stage(
                    filter_node,
                    Shortcut::new(Key::character("t"), Modifiers::ALT),
                    HintPlacement::Corner,
                    enabled_actions,
                );
            }
            if show_read_only {
                reg.stage(
                    ro_wrap_node,
                    Shortcut::new(Key::character("r"), Modifiers::ALT),
                    HintPlacement::Corner,
                    enabled_actions,
                );
            }
            reg.stage(
                confirm_node,
                Shortcut::new(Key::Named(NamedKey::Enter), Modifiers::CONTROL),
                HintPlacement::Corner,
                confirm_enabled_sig,
            );
            if let Some(ch) = accept_mnemonic.key
                && ch != 'a'
            {
                reg.stage(
                    confirm_node,
                    Shortcut::new(Key::character(ch.to_string()), Modifiers::ALT),
                    HintPlacement::BottomLeadingCorner,
                    confirm_enabled_sig,
                );
            }
        }

        let filters_signal = Signal::stored_local(filters);
        let file_type_label = i18n::text_untracked("picker-file-type");
        let read_only_label = i18n::text_untracked("picker-read-only");
        let browser_cancel_btn = self.clone();
        let browser_accept_btn = self.clone();

        let cancel_shortcut = Shortcut::new(Key::character("c"), Modifiers::ALT);
        let confirm_shortcut = Shortcut::new(Key::character("a"), Modifiers::ALT);

        Some(AnyView::new(view! {
            row(class = "gds-files-picker-controls-slot") {
                {show_filters.then(|| view! {
                    box(node_ref = filter_node, class = "gds-files-picker-control-item") {
                        Select(value = active_filter_value) {
                            SelectTrigger(
                                size = SelectTriggerSize::Sm,
                                label = file_type_label.clone(),
                                class = "gds-files-picker-filter-trigger",
                            ) {
                                SelectValue {}
                            }
                            SelectContent {
                                for item in move || filters_signal.get(), key = |item| item.0.clone() {
                                    SelectItem(value = item.0) {
                                        {item.1}
                                    }
                                }
                            }
                        }
                    }
                })}
                {show_read_only.then(|| view! {
                    row(node_ref = ro_wrap_node, class = "gds-files-picker-control-item") {
                        Checkbox(
                            node_ref = ro_node,
                            checked = read_only_binding,
                            labelled_by = ro_lbl_node,
                        )
                        Label(node_ref = ro_lbl_node, control = ro_node) {{read_only_label}}
                    }
                })}
                GdsButton(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    icon = IconName::X,
                    tooltip = cancel_label.clone(),
                    shortcut = cancel_shortcut,
                    shortcut_enabled = enabled_actions,
                    class = "gds-files-picker-btn",
                    on:click = move |_| browser_cancel_btn.cancel_picker(),
                ) {
                    {cancel_label}
                }
                GdsButton(
                    node_ref = confirm_node,
                    variant = ButtonVariant::Default,
                    size = ButtonSize::Sm,
                    icon = IconName::Check,
                    disabled = disabled_confirm,
                    tooltip = confirm_label.clone(),
                    shortcut = confirm_shortcut,
                    shortcut_enabled = enabled_actions,
                    class = "gds-files-picker-btn",
                    on:click = move |_| browser_accept_btn.accept_picker(),
                ) {
                    {confirm_label}
                }
            }
        }))
    }

    pub(super) fn activate_picker_path(&self, path: PathBuf) {
        let already_busy = self.read_untracked(|state| {
            state
                .picker
                .as_ref()
                .is_none_or(|picker| picker.submitting || picker.completing)
        });
        if already_busy {
            return;
        }
        let check_path = path.clone();
        self.blocking(
            "Inspect picker selection",
            move || Ok(inspect_activated_path(&check_path)),
            move |browser, result| match result {
                Ok(Ok(ActivatedPath::Directory)) => {
                    let location = browser.local_location(&path);
                    browser.open(location);
                }
                Ok(Ok(ActivatedPath::File)) => {
                    let is_save = browser.read_untracked(|state| {
                        state
                            .picker
                            .as_ref()
                            .is_some_and(|picker| picker.options.is_save())
                    });
                    if is_save {
                        if let Some(name) = path.file_name() {
                            let lossy = name.to_string_lossy().into_owned();
                            let raw = name.to_os_string();
                            browser.0.save_name.set(lossy.clone());
                            browser.update(|state| {
                                if let Some(picker) = state.picker.as_mut() {
                                    picker.save_name = Some(raw);
                                }
                            });
                            let slot = picker_ui_slot(browser);
                            let node = slot.save_input_node;
                            if node.is_bound() {
                                let stem_end = save_stem_end(&lossy);
                                node.focus();
                                node.set_value(&lossy);
                                node.set_selection(0..stem_end);
                            } else {
                                browser.0.owner.with(|| {
                                    drop(request_frame(move |_| {
                                        if node.is_bound() {
                                            let stem_end = save_stem_end(&lossy);
                                            node.focus();
                                            node.set_value(&lossy);
                                            node.set_selection(0..stem_end);
                                        }
                                    }));
                                });
                            }
                        }
                        // Save mode: double-clicking a file populates the filename input ONLY and NEVER submits.
                    } else {
                        browser.validate_and_complete_picker(vec![path]);
                    }
                }
                Ok(Err(failure)) => {
                    browser.report_notice(format_validation_failure(
                        failure.message_id,
                        failure.detail.as_deref(),
                    ));
                }
                Err(error) => browser.report_notice(error),
            },
        );
    }

    pub(super) fn accept_picker(&self) {
        enum AcceptPlan {
            Save {
                mode: chooser::Mode,
                target: PathBuf,
            },
            SaveFiles {
                mode: chooser::Mode,
                folder: PathBuf,
            },
            Open {
                paths: Vec<PathBuf>,
            },
        }

        let Some(candidates) = self.picker_candidates_inner(false) else {
            let invalid_save = self.read_untracked(|state| {
                state
                    .picker
                    .as_ref()
                    .is_some_and(|picker| picker.options.is_save())
            });
            if invalid_save {
                let typed = self.0.save_name.get_untracked();
                if !typed.is_empty() && !Options::valid_name(Path::new(&typed).as_os_str()) {
                    self.report_notice(i18n::text_untracked("rename-error-invalid"));
                }
            }
            return;
        };

        let plan = {
            let mut state = self.0.state.borrow_mut();
            let Some(picker) = state.picker.as_mut() else {
                return;
            };
            if picker.submitting || picker.completing {
                return;
            }
            if picker.options.is_save() {
                let Some(target) = candidates.into_iter().next() else {
                    return;
                };
                let mode = picker
                    .options
                    .portal
                    .as_ref()
                    .map(|portal| portal.mode.clone())
                    .unwrap_or(chooser::Mode::Save {
                        name: OsString::new(),
                    });
                picker.submitting = true;
                AcceptPlan::Save { mode, target }
            } else if let Some(chooser::Mode::SaveFiles { names }) = picker
                .options
                .portal
                .as_ref()
                .map(|portal| &portal.mode)
                .cloned()
            {
                let Some(folder) = candidates.into_iter().next() else {
                    return;
                };
                picker.submitting = true;
                AcceptPlan::SaveFiles {
                    mode: chooser::Mode::SaveFiles { names },
                    folder,
                }
            } else {
                picker.submitting = true;
                AcceptPlan::Open { paths: candidates }
            }
        };
        self.changed();

        match plan {
            AcceptPlan::Save { mode, target } => {
                self.blocking(
                    "Prepare save target",
                    move || Ok(prepare_save_targets(&mode, vec![target])),
                    |browser, result| {
                        browser.update(|state| {
                            if let Some(picker) = state.picker.as_mut() {
                                picker.submitting = false;
                            }
                        });
                        match result {
                            Ok(Ok(SaveDecision::Ready(paths))) => {
                                browser.finish_picker_selection(paths);
                            }
                            Ok(Ok(SaveDecision::ConfirmOverwrite(paths))) => {
                                let overwrite_browser = browser.clone();
                                let items: Vec<String> = paths
                                    .iter()
                                    .take(5)
                                    .map(|p| {
                                        p.file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_else(|| p.display().to_string())
                                    })
                                    .collect();
                                let overflow_count = paths.len().saturating_sub(items.len());
                                browser.confirm_with_options(
                                    i18n::text("picker-overwrite-title"),
                                    i18n::text("picker-overwrite-message"),
                                    i18n::text("picker-save"),
                                    ConfirmOptions {
                                        icon: Some(IconName::TriangleAlert),
                                        primary_icon: Some(IconName::Check),
                                        destructive: false,
                                        items,
                                        overflow_count,
                                    },
                                    move || {
                                        overwrite_browser.finish_picker_selection(paths.clone());
                                    },
                                );
                            }
                            Ok(Err(failure)) => {
                                browser.report_notice(format_validation_failure(
                                    failure.message_id,
                                    failure.detail.as_deref(),
                                ));
                            }
                            Err(error) => browser.report_notice(error),
                        }
                    },
                );
            }
            AcceptPlan::SaveFiles { mode, folder } => {
                self.blocking(
                    "Prepare save folder targets",
                    move || Ok(prepare_save_targets(&mode, vec![folder])),
                    |browser, result| {
                        browser.update(|state| {
                            if let Some(picker) = state.picker.as_mut() {
                                picker.submitting = false;
                            }
                        });
                        match result {
                            Ok(Ok(SaveDecision::Ready(paths)))
                            | Ok(Ok(SaveDecision::ConfirmOverwrite(paths))) => {
                                browser.finish_picker_selection(paths);
                            }
                            Ok(Err(failure)) => {
                                browser.report_notice(format_validation_failure(
                                    failure.message_id,
                                    failure.detail.as_deref(),
                                ));
                            }
                            Err(error) => browser.report_notice(error),
                        }
                    },
                );
            }
            AcceptPlan::Open { paths } => self.validate_and_complete_picker(paths),
        }
    }

    fn validate_and_complete_picker(&self, paths: Vec<PathBuf>) {
        let check = {
            let mut state = self.0.state.borrow_mut();
            let accepted_in_table: Vec<PathBuf> = {
                let tab = state.tab();
                let mut accepted = Vec::new();
                if let Some(picker) = state.picker.as_ref() {
                    for row in &tab.rows {
                        if let Row::File(entry) = row
                            && picker.accepts_entry(entry)
                        {
                            accepted.push(entry.path.clone());
                        }
                    }
                }
                accepted
            };
            let Some(picker) = state.picker.as_mut() else {
                return;
            };
            picker.submitting = true;
            picker.selection_check(accepted_in_table)
        };
        let candidate_paths = paths.clone();
        self.blocking(
            "Validate picker selection",
            move || Ok(check.validate(&candidate_paths)),
            move |browser, result| {
                browser.update(|state| {
                    if let Some(picker) = state.picker.as_mut() {
                        picker.submitting = false;
                    }
                });
                match result {
                    Ok(Ok(())) => browser.finish_picker_selection(paths),
                    Ok(Err(failure)) => {
                        browser.report_notice(format_validation_failure(
                            failure.message_id,
                            failure.detail.as_deref(),
                        ));
                    }
                    Err(error) => browser.report_notice(error),
                }
            },
        );
    }

    fn finish_picker_selection(&self, paths: Vec<PathBuf>) {
        if let Some(callback) = &self.0.picker_result {
            self.update(|state| {
                if let Some(picker) = state.picker.as_mut() {
                    picker.completing = true;
                }
            });
            callback(paths);
            if let Some(window) = &self.0.window {
                window.close();
            }
            return;
        }
        let outcome = self.update(|state| {
            let picker = state.picker.as_mut()?;
            if picker.completing {
                return None;
            }
            Some(picker.complete(&paths))
        });
        match outcome {
            Some(Ok(())) => {
                if let Some(window) = &self.0.window {
                    window.close();
                }
            }
            Some(Err(error)) => {
                self.report_notice(error.to_string());
                if let Some(window) = &self.0.window {
                    window.close();
                }
            }
            None => {}
        }
    }

    pub(super) fn cancel_picker(&self) {
        let picker_result = self.0.picker_result.is_some();
        let should_close = self.update(|state| {
            state
                .picker
                .as_mut()
                .is_some_and(|picker| picker.cancel() || picker_result)
        });
        if should_close && let Some(window) = &self.0.window {
            window.close();
        }
    }

    pub(super) fn picker_key(&self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        if self.read_untracked(|state| state.picker.is_none())
            || !self.picker_actions_available_untracked()
        {
            return false;
        }

        let ctrl_enter = Shortcut::new(Key::Named(NamedKey::Enter), Modifiers::CONTROL);
        if ctrl_enter.matches(&event.key, modifiers)
            || ctrl_enter.matches(&event.key_without_modifiers, modifiers)
        {
            if self.picker_candidates_inner(false).is_some() {
                self.accept_picker();
            }
            return true;
        }

        let alt_only = modifiers.contains(Modifiers::ALT)
            && !modifiers.contains(Modifiers::CONTROL)
            && !modifiers.contains(Modifiers::SHIFT)
            && !modifiers.contains(Modifiers::META);
        if !alt_only {
            return false;
        }

        let pressed_char = match &event.key {
            Key::Character(s) => s.chars().next().map(|c| c.to_ascii_lowercase()),
            _ => match &event.key_without_modifiers {
                Key::Character(s) => s.chars().next().map(|c| c.to_ascii_lowercase()),
                _ => None,
            },
        };
        let Some(ch) = pressed_char else {
            return false;
        };

        if ch == 'a' || self.picker_accept_mnemonic().key == Some(ch) {
            if self.picker_candidates_inner(false).is_some() {
                self.accept_picker();
            }
            return true;
        }

        if ch == 'c' {
            self.cancel_picker();
            return true;
        }

        if ch == 'r' {
            let can_toggle_ro = self.read_untracked(|state| {
                state.picker.as_ref().is_some_and(|picker| {
                    !picker.options.directory
                        && !picker.options.is_save()
                        && self.0.picker_result.is_none()
                })
            });
            if can_toggle_ro {
                self.update(|state| {
                    if let Some(picker) = state.picker.as_mut() {
                        picker.read_only = !picker.read_only;
                    }
                });
                return true;
            }
        }

        if ch == 't' {
            let next_filter = self.read_untracked(|state| {
                let picker = state.picker.as_ref()?;
                let count = picker.filters().len();
                (!picker.options.directory && count > 1)
                    .then_some((picker.active_filter + 1) % count)
            });
            if let Some(next_idx) = next_filter {
                self.select_picker_filter(next_idx);
                return true;
            }
        }

        false
    }

    fn select_picker_filter(&self, index: usize) {
        let active_id = self.update(|state| {
            let changed = state
                .picker
                .as_mut()
                .is_some_and(|picker| picker.select_filter(index));
            if changed { Some(state.tab().id) } else { None }
        });
        if let Some(id) = active_id {
            self.reproject(id);
            self.enrich_directory(id);
        }
    }

    fn set_picker_choice(&self, id: &str, value: &str) {
        let updated = self.update(|state| {
            let Some(picker) = state.picker.as_mut() else {
                return false;
            };
            let Some(choice) = picker.choices.iter_mut().find(|c| c.id == id) else {
                return false;
            };
            if choice.selected == value {
                return false;
            }
            choice.selected = value.to_owned();
            true
        });
        if updated {
            self.changed();
        }
    }
}

fn format_validation_failure(message_id: &str, detail: Option<&str>) -> String {
    let base = i18n::text(message_id);
    match detail {
        Some(detail) if !detail.is_empty() => format!("{base} ({detail})"),
        _ => base,
    }
}

fn save_stem_end(name: &str) -> usize {
    if let Some(stem) = Path::new(name).file_stem().and_then(|s| s.to_str())
        && !stem.is_empty()
        && name.starts_with(stem)
        && name.as_bytes().get(stem.len()) == Some(&b'.')
    {
        return stem.len();
    }
    name.len()
}
