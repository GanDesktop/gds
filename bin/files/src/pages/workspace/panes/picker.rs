//! Native chooser controls integrated into the 36px workspace footer.

use std::{cell::Cell, rc::Rc};

use gds::ui::button::{ButtonProps as GdsButtonProps, ButtonSize, ButtonVariant};
use gds::ui::keytips::{HintPlacement, KeytipRegistry};
use gds::ui::{IconName, Shortcut};
use zgui::prelude::*;
use zgui::reactive::{RenderEffect, Signal, on_cleanup_local};
use zgui::view::{EventCx, current_host, events, install_stylesheet};
use zgui::vocab::{ImeEvent, Key, KeyEvent, Modifiers, NamedKey};
use zgui::{css, view};
use zgui_ui::{
    CheckboxProps, Checked, InputProps, LabelProps, SelectContentProps, SelectItemProps,
    SelectProps, SelectTriggerProps, SelectTriggerSize, SelectValueProps,
};
use zgui_ui_primitives::Binding;

use super::super::{
    operations::{PickerOps, picker::save_stem_end},
    state::context::Context,
};
use crate::{features::pick_files::Session, shared::i18n};

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
        height: 24px;
        min-height: 24px;
        max-height: 24px;
        padding: 0 8px;
        font-size: var(--gds-text-sm, 14px);
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
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-muted-foreground);
        white-space: nowrap;
    }

    .gds-files-picker-filter-trigger.gds-files-picker-filter-trigger,
    .gds-files-picker-filter-trigger.gds-files-picker-filter-trigger[data-size="sm"] {
        height: 24px;
        min-height: 24px;
        max-height: 24px;
        padding-top: 0;
        padding-bottom: 0;
        max-width: 200px;
    }

    .gds-files-picker-btn.zui-button.gds-button[data-size="xs"],
    .gds-files-picker-btn.zui-button.gds-button[data-size="sm"] {
        height: 24px;
        min-height: 24px;
        max-height: 24px;
    }
    "#
);

fn install_picker_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(PICKER_SHEET, PICKER_CSS);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) struct Mnemonic {
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

pub(in crate::pages::workspace) struct PickerView<'a>(pub &'a Context);

impl PickerView<'_> {
    fn picker_accept_mnemonic(&self) -> Mnemonic {
        self.0.read_untracked(|state| {
            let Some(picker) = state.picker.as_ref() else {
                return Mnemonic {
                    label: i18n::text_untracked("picker-confirm"),
                    key: None,
                };
            };
            let (custom, fallback_id) = picker.accept_label_parts();
            if let Some(custom) = custom {
                let parsed = Mnemonic::parse(custom);
                if !parsed.label.is_empty() {
                    return parsed;
                }
            }
            Mnemonic {
                label: i18n::text_untracked(fallback_id),
                key: None,
            }
        })
    }

    /// Renders optional portal `choices` above the `36px` workspace footer when non-empty.
    pub(in crate::pages::workspace) fn picker_footer(&self) -> AnyView {
        let choices_data = self.0.read_untracked(|state| {
            state
                .picker
                .as_ref()
                .map(Session::choice_controls)
                .unwrap_or_default()
        });
        if choices_data.is_empty() {
            return AnyView::new(view! { box(style:display = "none") });
        }
        install_picker_stylesheet();

        let revision = self.0.0.revision;
        let ctx_choice_bool = self.0.clone();
        let ctx_choice_select = self.0.clone();

        let choice_views: Vec<AnyView> = choices_data
            .into_iter()
            .map(|choice| {
                let choice_id = choice.id.clone();
                let label = Mnemonic::parse(&choice.label).label;
                if choice.options.is_empty() {
                    let read_ctx = ctx_choice_bool.clone();
                    let write_ctx = ctx_choice_bool.clone();
                    let read_id = choice_id.clone();
                    let write_id = choice_id;
                    let checked = Binding::controlled(
                        Signal::derive_local(move || {
                            let _ = revision.get();
                            let active = read_ctx.read(|state| {
                                state.picker.as_ref().is_some_and(|picker| {
                                    picker.choice_selected(&read_id) == Some("true")
                                })
                            });
                            if active { Checked::Yes } else { Checked::No }
                        }),
                        move |next: Checked| {
                            PickerOps(&write_ctx).set_picker_choice(
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
                    let read_ctx = ctx_choice_select.clone();
                    let write_ctx = ctx_choice_select.clone();
                    let read_id = choice_id.clone();
                    let write_id = choice_id;
                    let selected_binding = Binding::controlled(
                        Signal::derive_local(move || {
                            let _ = revision.get();
                            read_ctx.read(|state| {
                                state
                                    .picker
                                    .as_ref()
                                    .and_then(|picker| picker.choice_selected(&read_id))
                                    .map(str::to_owned)
                                    .unwrap_or_default()
                            })
                        }),
                        move |next: String| {
                            PickerOps(&write_ctx).set_picker_choice(&write_id, &next);
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

    pub(in crate::pages::workspace) fn is_save_picker(&self) -> bool {
        self.0
            .read(|state| state.picker.as_ref().is_some_and(Session::is_save))
    }

    pub(in crate::pages::workspace) fn render_picker_name(&self) -> Option<AnyView> {
        let is_save = self
            .0
            .read_untracked(|state| state.picker.as_ref().is_some_and(Session::is_save));
        if !is_save {
            return None;
        }
        install_picker_stylesheet();
        let save_input_node = self.0.0.save_input_node;
        let save_name = self.0.0.save_name;
        let save_composing = Rc::new(Cell::new(false));
        let save_focus_ctx = self.0.clone();
        let avail_ctx = self.0.clone();
        let disabled_input =
            Signal::derive_local(move || !PickerOps(&avail_ctx).picker_actions_available());

        let save_effect = RenderEffect::new(move |prev: Option<bool>| {
            let mounted = save_input_node.get().is_some();
            if mounted && prev != Some(true) {
                save_input_node.focus();
                let name = save_focus_ctx.0.save_name.get_untracked();
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
        let ctx_accept = self.0.clone();
        let ctx_cancel = self.0.clone();
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
                            let picker = PickerOps(&ctx_accept);
                            if picker.picker_candidates(false).is_some() {
                                picker.accept_picker();
                            }
                        } else if event.key == Key::Named(NamedKey::Escape)
                            && event.modifiers.is_empty()
                        {
                            event.prevent_default();
                            event.stop_propagation();
                            PickerOps(&ctx_cancel).cancel_picker();
                        }
                    },
                )
            }
        }))
    }

    pub(in crate::pages::workspace) fn render_picker_controls(&self) -> Option<AnyView> {
        if self.0.read_untracked(|state| state.picker.is_none()) {
            return None;
        }
        install_picker_stylesheet();

        let revision = self.0.0.revision;
        let show_read_only = self.0.read_untracked(|state| {
            state.picker.as_ref().is_some_and(|picker| {
                picker.supports_read_only() && self.0.0.picker_result.is_none()
            })
        });
        let filters: Vec<(String, String)> = self.0.read_untracked(|state| {
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
        let show_filters = !filters.is_empty();

        let accept_mnemonic = self.picker_accept_mnemonic();
        let confirm_label = accept_mnemonic.label;
        let cancel_label = i18n::text_untracked("picker-cancel");

        let filter_ctx = self.0.clone();
        let active_filter_value = Binding::controlled(
            Signal::derive_local(move || {
                let _ = revision.get();
                filter_ctx.read(|state| {
                    state.picker.as_ref().map_or_else(
                        || "0".to_owned(),
                        |picker| picker.active_filter().to_string(),
                    )
                })
            }),
            {
                let ctx = self.0.clone();
                move |next: String| {
                    if let Ok(index) = next.parse::<usize>() {
                        PickerOps(&ctx).select_picker_filter(index);
                    }
                }
            },
        );

        let ro_read_ctx = self.0.clone();
        let ro_write_ctx = self.0.clone();
        let read_only_binding = Binding::controlled(
            Signal::derive_local(move || {
                let _ = revision.get();
                if ro_read_ctx
                    .read(|state| state.picker.as_ref().is_some_and(Session::is_read_only))
                {
                    Checked::Yes
                } else {
                    Checked::No
                }
            }),
            move |checked: Checked| {
                let updated = ro_write_ctx.update(|state| {
                    state
                        .picker
                        .as_mut()
                        .is_some_and(|picker| picker.set_read_only(checked == Checked::Yes))
                });
                if updated {
                    ro_write_ctx.changed();
                }
            },
        );

        let disabled_confirm = {
            let ctx = self.0.clone();
            Signal::derive_local(move || {
                let picker = PickerOps(&ctx);
                !picker.picker_actions_available() || picker.picker_candidates(true).is_none()
            })
        };
        let enabled_actions = {
            let ctx = self.0.clone();
            Signal::derive_local(move || PickerOps(&ctx).picker_actions_available())
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
        let ctx_cancel_btn = self.0.clone();
        let ctx_accept_btn = self.0.clone();

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
                    size = ButtonSize::Xs,
                    icon = IconName::X,
                    tooltip = cancel_label.clone(),
                    shortcut = cancel_shortcut,
                    shortcut_enabled = enabled_actions,
                    class = "gds-files-picker-btn",
                    on:click = move |_| PickerOps(&ctx_cancel_btn).cancel_picker(),
                ) {
                    {cancel_label}
                }
                GdsButton(
                    node_ref = confirm_node,
                    variant = ButtonVariant::Default,
                    size = ButtonSize::Xs,
                    icon = IconName::Check,
                    disabled = disabled_confirm,
                    tooltip = confirm_label.clone(),
                    shortcut = confirm_shortcut,
                    shortcut_enabled = enabled_actions,
                    class = "gds-files-picker-btn",
                    on:click = move |_| PickerOps(&ctx_accept_btn).accept_picker(),
                ) {
                    {confirm_label}
                }
            }
        }))
    }

    pub(in crate::pages::workspace) fn picker_key(
        &self,
        event: &KeyEvent,
        modifiers: Modifiers,
    ) -> bool {
        let picker = PickerOps(self.0);
        if self.0.read_untracked(|state| state.picker.is_none())
            || !picker.picker_actions_available_untracked()
        {
            return false;
        }

        let ctrl_enter = Shortcut::new(Key::Named(NamedKey::Enter), Modifiers::CONTROL);
        if ctrl_enter.matches(&event.key, modifiers)
            || ctrl_enter.matches(&event.key_without_modifiers, modifiers)
        {
            if picker.picker_candidates(false).is_some() {
                picker.accept_picker();
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
            if picker.picker_candidates(false).is_some() {
                picker.accept_picker();
            }
            return true;
        }

        if ch == 'c' {
            picker.cancel_picker();
            return true;
        }

        if ch == 'r' && self.0.0.picker_result.is_none() {
            let toggled = self.0.update(|state| {
                state
                    .picker
                    .as_mut()
                    .is_some_and(|picker| picker.set_read_only(!picker.is_read_only()))
            });
            if toggled {
                return true;
            }
        }

        if ch == 't' {
            let next_filter = self
                .0
                .read_untracked(|state| state.picker.as_ref().and_then(Session::next_filter_index));
            if let Some(next_idx) = next_filter {
                picker.select_picker_filter(next_idx);
                return true;
            }
        }

        false
    }
}
