use crate::shared::i18n::tr;
use bas::model::SessionMeta;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub(in crate::pages::workspace) struct Group<'a> {
    pub directory: Option<PathBuf>,
    pub threads: Vec<&'a SessionMeta>,
}

pub(in crate::pages::workspace) fn group_threads<'a>(
    sessions: &'a [SessionMeta],
    workspaces: &[bas::model::Workspace],
    query: &str,
) -> Vec<Group<'a>> {
    workspaces
        .iter()
        .filter_map(|workspace| {
            let threads: Vec<_> = sessions
                .iter()
                .filter(|session| session.workspace == workspace.id)
                .filter(|session| crate::pages::workspace::ui::matches_query(session, query))
                .collect();
            let matches = query.is_empty()
                || workspace.name.to_lowercase().contains(query)
                || workspace
                    .cwd
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(query);
            (matches || !threads.is_empty()).then(|| Group {
                directory: Some(workspace.cwd.clone()),
                threads,
            })
        })
        .collect()
}

pub(in crate::pages::workspace) fn name(directory: Option<&Path>) -> String {
    directory
        .map(|path| {
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| tr!("workspace-default").into())
}

pub(in crate::pages::workspace) fn path_label(directory: Option<&Path>) -> String {
    directory
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| tr!("workspace-default-directory").into())
}

pub(in crate::pages::workspace) fn normalize(path: &Path) -> PathBuf {
    // These are daemon-side paths. Local filesystem canonicalization would be incorrect for remote bas.
    path.components().collect()
}
