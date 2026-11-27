//! Non-UI file chooser operations: availability, candidate resolution, activation, and submit.

use std::path::{Path, PathBuf};

use gds::ui::IconName;
use zgui::prelude::*;
use zgui::view::request_frame;

use super::super::{
    navigation::{EnrichmentRunner, Navigator},
    projection::Rows,
    state::{
        context::Context,
        menus::Surface,
        model::{AddressPresentation, ConfirmOptions, Contents, Pane, Prompt, Row, State},
    },
};
use crate::{
    features::pick_files::{
        AcceptPlan, ActivatedFile, ActivatedPath, Options, SaveDecision, Session,
        inspect_activated_path, prepare_save_targets,
    },
    shared::i18n,
};

pub(in crate::pages::workspace) struct PickerOps<'a>(pub &'a Context);

impl PickerOps<'_> {
    pub(in crate::pages::workspace) fn initialize_picker(&self) {
        let initial = self
            .0
            .read_untracked(|state| state.picker.as_ref().and_then(Session::initial_save_name));
        if let Some(name) = initial {
            self.0.0.save_name.set(name);
        }
    }

    pub(in crate::pages::workspace) fn picker_actions_available(&self) -> bool {
        self.0.pane() == Pane::Files
            && matches!(self.0.0.prompt.get(), Prompt::Closed)
            && matches!(self.0.0.menus.surface.get(), Surface::Closed)
            && !self.0.0.connector.is_connecting()
            && matches!(
                self.0.0.address_presentation.get(),
                AddressPresentation::Breadcrumbs
            )
    }

    pub(in crate::pages::workspace) fn picker_actions_available_untracked(&self) -> bool {
        self.0.pane_untracked() == Pane::Files
            && matches!(self.0.0.prompt.get_untracked(), Prompt::Closed)
            && !self.0.0.menus.is_open()
            && !self.0.0.connector.is_connecting_untracked()
            && matches!(
                self.0.0.address_presentation.get_untracked(),
                AddressPresentation::Breadcrumbs
            )
    }

    pub(in crate::pages::workspace) fn picker_candidates(
        &self,
        reactive: bool,
    ) -> Option<Vec<PathBuf>> {
        let typed_save = if reactive {
            self.0.0.save_name.get()
        } else {
            self.0.0.save_name.get_untracked()
        };
        let inspect = |state: &State| -> Option<Vec<PathBuf>> {
            let picker = state.picker.as_ref()?;
            let tab = state.tab();
            if tab.navigation.is_loading() {
                return None;
            }
            let current_dir = matches!(tab.contents, Contents::Directory)
                .then(|| tab.navigation.path())
                .flatten();
            let selected = tab.selected();
            let selected_entries: Vec<_> = selected
                .iter()
                .map(|row| match row {
                    Row::File(entry) => Some(entry),
                    _ => None,
                })
                .collect();
            picker.resolve_candidates(&typed_save, current_dir, &selected_entries)
        };

        if reactive {
            self.0.read(inspect)
        } else {
            self.0.read_untracked(inspect)
        }
    }

    pub(super) fn activate_picker_path(&self, path: PathBuf) {
        let already_busy = self
            .0
            .read_untracked(|state| state.picker.as_ref().is_none_or(Session::is_busy));
        if already_busy {
            return;
        }
        let check_path = path.clone();
        self.0.blocking(
            "Inspect picker selection",
            move || Ok(inspect_activated_path(&check_path)),
            move |ctx, result| match result {
                Ok(Ok(ActivatedPath::Directory)) => {
                    let location = Rows(ctx).local_location(&path);
                    Navigator(ctx).open(location);
                }
                Ok(Ok(ActivatedPath::File)) => {
                    let action = ctx.update(|state| {
                        state
                            .picker
                            .as_mut()
                            .map(|picker| picker.handle_activated_file(&path))
                    });
                    match action {
                        Some(ActivatedFile::PopulateSaveName(Some(lossy))) => {
                            ctx.0.save_name.set(lossy.clone());
                            let node = ctx.0.save_input_node;
                            if node.is_bound() {
                                let stem_end = save_stem_end(&lossy);
                                node.focus();
                                node.set_value(&lossy);
                                node.set_selection(0..stem_end);
                            } else {
                                ctx.0.owner.with(|| {
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
                        Some(ActivatedFile::PopulateSaveName(None)) | None => {}
                        Some(ActivatedFile::Confirm) => {
                            PickerOps(ctx).validate_and_complete_picker(vec![path]);
                        }
                    }
                }
                Ok(Err(failure)) => {
                    ctx.report_notice(format_validation_failure(
                        failure.message_id,
                        failure.detail.as_deref(),
                    ));
                }
                Err(error) => ctx.report_notice(error),
            },
        );
    }

    pub(in crate::pages::workspace) fn accept_picker(&self) {
        let Some(candidates) = self.picker_candidates(false) else {
            let invalid_save = self
                .0
                .read_untracked(|state| state.picker.as_ref().is_some_and(Session::is_save));
            if invalid_save {
                let typed = self.0.0.save_name.get_untracked();
                if !typed.is_empty() && !Options::valid_name(Path::new(&typed).as_os_str()) {
                    self.0
                        .report_notice(i18n::text_untracked("rename-error-invalid"));
                }
            }
            return;
        };

        let Some(plan) = self
            .0
            .0
            .state
            .borrow_mut()
            .picker
            .as_mut()
            .and_then(|picker| picker.begin_accept(candidates))
        else {
            return;
        };
        self.0.changed();

        match plan {
            AcceptPlan::Save { mode, target } => {
                self.0.blocking(
                    "Prepare save target",
                    move || Ok(prepare_save_targets(&mode, vec![target])),
                    |ctx, result| {
                        ctx.update(|state| {
                            if let Some(picker) = state.picker.as_mut() {
                                picker.finish_submit();
                            }
                        });
                        match result {
                            Ok(Ok(SaveDecision::Ready(paths))) => {
                                PickerOps(ctx).finish_picker_selection(paths);
                            }
                            Ok(Ok(SaveDecision::ConfirmOverwrite(paths))) => {
                                let overwrite_ctx = ctx.clone();
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
                                ctx.confirm_with_options(
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
                                        PickerOps(&overwrite_ctx)
                                            .finish_picker_selection(paths.clone());
                                    },
                                );
                            }
                            Ok(Err(failure)) => {
                                ctx.report_notice(format_validation_failure(
                                    failure.message_id,
                                    failure.detail.as_deref(),
                                ));
                            }
                            Err(error) => ctx.report_notice(error),
                        }
                    },
                );
            }
            AcceptPlan::SaveFiles { mode, folder } => {
                self.0.blocking(
                    "Prepare save folder targets",
                    move || Ok(prepare_save_targets(&mode, vec![folder])),
                    |ctx, result| {
                        ctx.update(|state| {
                            if let Some(picker) = state.picker.as_mut() {
                                picker.finish_submit();
                            }
                        });
                        match result {
                            Ok(Ok(SaveDecision::Ready(paths)))
                            | Ok(Ok(SaveDecision::ConfirmOverwrite(paths))) => {
                                PickerOps(ctx).finish_picker_selection(paths);
                            }
                            Ok(Err(failure)) => {
                                ctx.report_notice(format_validation_failure(
                                    failure.message_id,
                                    failure.detail.as_deref(),
                                ));
                            }
                            Err(error) => ctx.report_notice(error),
                        }
                    },
                );
            }
            AcceptPlan::Open { paths } => self.validate_and_complete_picker(paths),
        }
    }

    fn validate_and_complete_picker(&self, paths: Vec<PathBuf>) {
        let check = {
            let mut state = self.0.0.state.borrow_mut();
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
            picker.begin_validation(accepted_in_table)
        };
        let candidate_paths = paths.clone();
        self.0.blocking(
            "Validate picker selection",
            move || Ok(check.validate(&candidate_paths)),
            move |ctx, result| {
                ctx.update(|state| {
                    if let Some(picker) = state.picker.as_mut() {
                        picker.finish_submit();
                    }
                });
                match result {
                    Ok(Ok(())) => PickerOps(ctx).finish_picker_selection(paths),
                    Ok(Err(failure)) => {
                        ctx.report_notice(format_validation_failure(
                            failure.message_id,
                            failure.detail.as_deref(),
                        ));
                    }
                    Err(error) => ctx.report_notice(error),
                }
            },
        );
    }

    fn finish_picker_selection(&self, paths: Vec<PathBuf>) {
        if let Some(callback) = &self.0.0.picker_result {
            self.0.update(|state| {
                if let Some(picker) = state.picker.as_mut() {
                    picker.mark_completing();
                }
            });
            callback(paths);
            if let Some(window) = &self.0.0.window {
                window.close();
            }
            return;
        }
        let outcome = self.0.update(|state| {
            state
                .picker
                .as_mut()
                .and_then(|picker| picker.complete(&paths))
        });
        match outcome {
            Some(Ok(())) => {
                if let Some(window) = &self.0.0.window {
                    window.close();
                }
            }
            Some(Err(error)) => {
                self.0.report_notice(error.to_string());
                if let Some(window) = &self.0.0.window {
                    window.close();
                }
            }
            None => {}
        }
    }

    pub(in crate::pages::workspace) fn cancel_picker(&self) {
        let picker_result = self.0.0.picker_result.is_some();
        let should_close = self.0.update(|state| {
            state
                .picker
                .as_mut()
                .is_some_and(|picker| picker.cancel() || picker_result)
        });
        if should_close && let Some(window) = &self.0.0.window {
            window.close();
        }
    }

    pub(in crate::pages::workspace) fn select_picker_filter(&self, index: usize) {
        let active_id = self.0.update(|state| {
            let changed = state
                .picker
                .as_mut()
                .is_some_and(|picker| picker.select_filter(index));
            if changed { Some(state.tab().id) } else { None }
        });
        if let Some(id) = active_id {
            Rows(self.0).reproject(id);
            EnrichmentRunner(self.0).enrich_directory(id);
        }
    }

    pub(in crate::pages::workspace) fn set_picker_choice(&self, id: &str, value: &str) {
        let updated = self.0.update(|state| {
            state
                .picker
                .as_mut()
                .is_some_and(|picker| picker.set_choice(id, value))
        });
        if updated {
            self.0.changed();
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

pub(in crate::pages::workspace) fn save_stem_end(name: &str) -> usize {
    if let Some(stem) = Path::new(name).file_stem().and_then(|s| s.to_str())
        && !stem.is_empty()
        && name.starts_with(stem)
        && name.as_bytes().get(stem.len()) == Some(&b'.')
    {
        return stem.len();
    }
    name.len()
}
