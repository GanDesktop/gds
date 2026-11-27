//! Tag catalogue deletion operations.

use gds::i18n as gds_i18n;
use gds::ui::IconName;
use tokio::sync::mpsc;
use zgui::reactive::spawn_local;

use super::super::{
    navigation::Navigator,
    state::{
        context::Context,
        model::{ConfirmOptions, Contents, Row},
    },
};
use super::lifecycle::{Lifecycle, preview_slice};
use crate::{entities::tags, shared::i18n};

pub(in crate::pages::workspace) struct TagOps<'a>(pub &'a Context);

impl TagOps<'_> {
    pub(in crate::pages::workspace) fn delete_tags(&self, rows: Vec<Row>) {
        if self.0.read_untracked(|state| state.operation.busy()) {
            return;
        }
        let tags_to_delete: Vec<String> = rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Tag(tag) if !tag.is_empty() => Some(tag),
                _ => None,
            })
            .collect();
        if tags_to_delete.is_empty() {
            return;
        }
        let backend = self.0.read_untracked(|state| match &state.tab().contents {
            Contents::Tags { backend, .. } => Some(backend.clone()),
            _ => None,
        });
        let commands = self.0.0.preferences.snapshot().tags.commands;
        let count = tags_to_delete.len();
        let (items, overflow_count) = preview_slice(tags_to_delete.clone());
        let title = i18n::text_untracked("tags-delete-title");
        let message = i18n::text_untracked("tags-delete-message");
        let primary = i18n::text_untracked("tags-delete");
        let options = ConfirmOptions {
            icon: Some(IconName::Trash),
            primary_icon: Some(IconName::Trash),
            destructive: true,
            items,
            overflow_count,
        };
        let ctx = self.0.clone();
        self.0
            .confirm_with_options(title, message, primary, options, move || {
                let Some(_cancel) = Lifecycle(&ctx)
                    .begin_operation(&gds_i18n::count_untracked("delete-working", count))
                else {
                    return;
                };
                let tags_list = tags_to_delete.clone();
                let existing_backend = backend.clone();
                let cmds = commands.clone();
                let (tx, mut rx) = mpsc::unbounded_channel();
                drop(ctx.scope().spawn_named("Delete file tags", async move {
                    let backend = match existing_backend {
                        Some(backend) => Ok(backend),
                        None => tags::connect(&cmds).await,
                    };
                    let outcome = match backend {
                        Ok(backend) => {
                            let mut deleted = 0usize;
                            let mut failures = Vec::new();
                            for tag in tags_list {
                                match backend.delete_tag(&tag).await {
                                    Ok(()) => deleted += 1,
                                    Err(err) => failures.push(format!("{tag}: {err}")),
                                }
                            }
                            Ok((deleted, failures))
                        }
                        Err(err) => Err(err),
                    };
                    let _ = tx.send(outcome);
                }));
                let weak = ctx.downgrade();
                ctx.0.owner.with(|| {
                    drop(spawn_local(async move {
                        let Some(outcome) = rx.recv().await else {
                            return;
                        };
                        let Some(ctx) = weak.upgrade() else {
                            return;
                        };
                        Lifecycle(&ctx).end_operation();
                        Navigator(&ctx).refresh_tag_tabs();
                        match outcome {
                            Ok((deleted, failures)) => {
                                if !failures.is_empty() {
                                    let headline = gds::t_untracked!(
                                        "delete-partial",
                                        succeeded = deleted as i64,
                                        failed = failures.len() as i64
                                    );
                                    ctx.report_notice(format!(
                                        "{headline}\n{}",
                                        failures.join("\n")
                                    ));
                                } else if deleted == 1 {
                                    ctx.report_success(i18n::text_untracked("tags-deleted"));
                                } else {
                                    ctx.report_success(gds_i18n::count_untracked(
                                        "delete-done",
                                        deleted,
                                    ));
                                }
                            }
                            Err(err) => ctx.report_notice(err),
                        }
                        ctx.focus_content();
                    }));
                });
            });
    }
}
