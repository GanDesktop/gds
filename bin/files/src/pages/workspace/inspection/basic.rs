//! Basic file properties editor tab (file name, owner, group, location, favorite).

#[cfg(unix)]
use std::collections::BTreeMap;
use std::{
    cell::RefCell,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    rc::Rc,
};

use gds::i18n;
use gds::ui::feedback::tooltip::HintTooltipProps;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::view::{FrameHandle, current_host, events, install_stylesheet, request_frame};
use zgui::vocab::{ImeEvent, Key, Modifiers, NamedKey};
use zgui::{css, view};
use zgui_ui::{CheckboxProps, Checked, InputProps, LabelProps};
#[cfg(unix)]
use zgui_ui::{SelectContentProps, SelectItemProps, SelectProps, SelectTriggerProps};
use zgui_ui_primitives::Binding;

use crate::entities::file::directory::Entry;
use crate::entities::location::Location;
use crate::features::file_operations::{history, rename};

use super::super::ui::{Browser, WeakBrowser};

const BASIC_SHEET: &str = "gds-files-basic-editor";

const BASIC_CSS: &str = css!(
    r#"
    .gds-files-basic-editor {
        display: flex;
        flex-direction: column;
        gap: 10px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-basic-field {
        display: flex;
        flex-direction: column;
        gap: 4px;
        flex: 1 1 0%;
        min-width: 0;
    }

    .gds-files-basic-label,
    .gds-files-basic-label.zui-label {
        font-size: var(--gds-text-xs, 12px);
        font-weight: 500;
        line-height: 16px;
        color: var(--gds-muted-foreground);
    }

    .gds-files-basic-input.zui-field {
        width: 100%;
        height: 34px;
        box-sizing: border-box;
    }

    .gds-files-basic-ownership {
        display: flex;
        flex-direction: row;
        gap: 12px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-basic-field .gds-files-basic-select,
    .gds-files-basic-field .zui-select,
    .gds-files-basic-field .zui-select__trigger {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        width: 100%;
        min-width: 0;
        height: 34px;
        min-height: 34px;
        padding: 0 10px;
        background-color: var(--gds-content);
        color: var(--gds-foreground);
        border: 1px solid var(--gds-outline);
        border-radius: var(--gds-radius-control, 6px);
        box-shadow: none;
        outline: none;
        box-sizing: border-box;
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-files-basic-field .gds-files-basic-select:focus,
    .gds-files-basic-field .gds-files-basic-select:focus-visible,
    .gds-files-basic-field .zui-select:focus,
    .gds-files-basic-field .zui-select:focus-visible,
    .gds-files-basic-field .zui-select__trigger:focus,
    .gds-files-basic-field .zui-select__trigger:focus-visible {
        border-color: var(--gds-primary);
        box-shadow: none;
        outline: none;
    }

    .gds-files-basic-select-value {
        flex: 1 1 0%;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        text-align: left;
    }

    .gds-files-basic-location {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        width: 100%;
        min-width: 0;
        min-height: 20px;
        padding-top: 2px;
    }

    .gds-files-basic-location__label {
        flex: none;
        font-size: var(--gds-text-xs, 12px);
        font-weight: 500;
        line-height: 18px;
        color: var(--gds-muted-foreground);
    }

    .gds-files-basic-location__path-wrap {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 0%;
        min-width: 0;
    }

    .gds-files-basic-location__path-wrap .zui-tooltip__trigger {
        display: flex;
        flex-direction: row;
        align-items: center;
        width: 100%;
        max-width: 100%;
        min-width: 0;
        overflow: hidden;
    }

    .gds-files-basic-location__path {
        display: block;
        width: 100%;
        min-width: 0;
        font-size: var(--gds-text-xs, 12px);
        line-height: 18px;
        color: var(--gds-foreground);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gds-files-basic-favorite {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        height: 22px;
    }

    .gds-files-basic-error {
        font-size: var(--gds-text-xs, 12px);
        color: var(--gds-error);
    }
    "#
);

fn install_basic_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(BASIC_SHEET, BASIC_CSS);
    }
}

#[cfg(unix)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct IdentityOption {
    id: u32,
    label: String,
}

#[cfg(unix)]
#[derive(Clone, Debug)]
struct LoadedOwnership {
    uid: u32,
    gid: u32,
    euid: u32,
    owners: Vec<IdentityOption>,
    groups: Vec<IdentityOption>,
    can_change_owner: bool,
    can_change_group: bool,
}

#[cfg(unix)]
fn format_owner_label(uid: u32, base: String, euid: u32) -> String {
    if uid == euid {
        gds::t_untracked!("owner-you", name = base)
    } else {
        base
    }
}

enum BasicSaveError {
    RenameExists(String),
    RenameName(rename::NameError),
    RenameFailed(String),
    #[cfg(unix)]
    OwnershipFailed(String),
}

impl BasicSaveError {
    fn localize(self) -> String {
        match self {
            Self::RenameExists(name) => gds::t_untracked!("rename-target-exists", name = name),
            Self::RenameName(err) => i18n::text_untracked(err.label_id()),
            Self::RenameFailed(error) => gds::t_untracked!("rename-failed", error = error),
            #[cfg(unix)]
            Self::OwnershipFailed(error) => {
                gds::t_untracked!("details-basic-ownership-failed", error = error)
            }
        }
    }
}

#[cfg(unix)]
fn parse_etc_id_file(contents: &str) -> BTreeMap<u32, String> {
    let mut map = BTreeMap::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = trimmed.split(':');
        let Some(name) = fields.next().filter(|s| !s.is_empty()) else {
            continue;
        };
        let _passwd = fields.next();
        let Some(id) = fields.next().and_then(|raw| raw.parse::<u32>().ok()) else {
            continue;
        };
        map.entry(id).or_insert_with(|| name.to_owned());
    }
    map
}

#[cfg(unix)]
fn sorted_identity_options(mut options: Vec<IdentityOption>) -> Vec<IdentityOption> {
    options.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    options
}

#[cfg(unix)]
fn load_ownership(path: &Path) -> Result<LoadedOwnership, String> {
    use nix::unistd::{Gid, Group, Uid, User, getegid, geteuid};
    use std::os::unix::fs::MetadataExt as _;

    let meta = std::fs::symlink_metadata(path).map_err(|err| err.to_string())?;
    let uid = meta.uid();
    let gid = meta.gid();
    let euid = geteuid().as_raw();
    let egid = getegid().as_raw();
    let is_root = euid == 0;

    let owners = if is_root {
        let mut by_id = std::fs::read_to_string("/etc/passwd")
            .map(|raw| parse_etc_id_file(&raw))
            .unwrap_or_default();
        by_id.entry(uid).or_insert_with(|| {
            User::from_uid(Uid::from_raw(uid))
                .ok()
                .flatten()
                .map(|user| user.name)
                .unwrap_or_else(|| uid.to_string())
        });
        sorted_identity_options(
            by_id
                .into_iter()
                .map(|(id, label)| IdentityOption { id, label })
                .collect(),
        )
    } else {
        let label = User::from_uid(Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|user| user.name)
            .unwrap_or_else(|| uid.to_string());
        vec![IdentityOption { id: uid, label }]
    };

    let groups = if is_root {
        let mut by_id = std::fs::read_to_string("/etc/group")
            .map(|raw| parse_etc_id_file(&raw))
            .unwrap_or_default();
        by_id.entry(gid).or_insert_with(|| {
            Group::from_gid(Gid::from_raw(gid))
                .ok()
                .flatten()
                .map(|group| group.name)
                .unwrap_or_else(|| gid.to_string())
        });
        sorted_identity_options(
            by_id
                .into_iter()
                .map(|(id, label)| IdentityOption { id, label })
                .collect(),
        )
    } else {
        let mut ids = vec![gid];
        if !ids.contains(&egid) {
            ids.push(egid);
        }
        #[cfg(not(target_vendor = "apple"))]
        if let Ok(extra) = nix::unistd::getgroups() {
            for group_id in extra {
                let raw = group_id.as_raw();
                if !ids.contains(&raw) {
                    ids.push(raw);
                }
            }
        }
        sorted_identity_options(
            ids.into_iter()
                .map(|id| {
                    let label = Group::from_gid(Gid::from_raw(id))
                        .ok()
                        .flatten()
                        .map(|group| group.name)
                        .unwrap_or_else(|| id.to_string());
                    IdentityOption { id, label }
                })
                .collect(),
        )
    };

    let can_change_owner = is_root && owners.len() > 1;
    let can_change_group = (is_root || uid == euid) && groups.len() > 1;

    Ok(LoadedOwnership {
        uid,
        gid,
        euid,
        owners,
        groups,
        can_change_owner,
        can_change_group,
    })
}

struct BasicEditorInner {
    browser: WeakBrowser,
    path: RefCell<PathBuf>,
    parent_display: String,
    can_rename: bool,
    original_name: RefCell<String>,
    name_draft: RwSignal<String, LocalStorage>,
    name_input_ref: NodeRef,
    composing: RwSignal<bool, LocalStorage>,
    original_favorite: RefCell<bool>,
    favorite: RwSignal<bool, LocalStorage>,
    #[cfg(unix)]
    original_uid: RefCell<Option<u32>>,
    #[cfg(unix)]
    original_gid: RefCell<Option<u32>>,
    #[cfg(unix)]
    selected_uid: RwSignal<Option<u32>, LocalStorage>,
    #[cfg(unix)]
    selected_gid: RwSignal<Option<u32>, LocalStorage>,
    #[cfg(unix)]
    owner_options: RwSignal<Vec<IdentityOption>, LocalStorage>,
    #[cfg(unix)]
    group_options: RwSignal<Vec<IdentityOption>, LocalStorage>,
    #[cfg(unix)]
    can_change_owner: RwSignal<bool, LocalStorage>,
    #[cfg(unix)]
    can_change_group: RwSignal<bool, LocalStorage>,
    loading: RwSignal<bool, LocalStorage>,
    saving: RwSignal<bool, LocalStorage>,
    error: RwSignal<Option<String>, LocalStorage>,
    pending_focus: RefCell<Option<FrameHandle>>,
}

#[derive(Clone)]
pub(super) struct BasicEditor(Rc<BasicEditorInner>);

impl BasicEditor {
    pub(super) fn new(
        browser: &Browser,
        path: PathBuf,
        entry: Option<&Entry>,
        auto_focus: bool,
    ) -> Self {
        let parent_display = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| path.display().to_string());

        let (initial_name, can_rename) = match (path.parent(), path.file_name()) {
            (Some(parent), Some(raw)) if !parent.as_os_str().is_empty() => match raw.to_str() {
                Some(utf8) => (utf8.to_owned(), true),
                None => (raw.to_string_lossy().into_owned(), false),
            },
            (_, Some(raw)) => (raw.to_string_lossy().into_owned(), false),
            (_, None) => (path.display().to_string(), false),
        };

        let initial_favorite = browser
            .read_untracked(|state| state.places.is_favorite(Location::from(path.as_path())));

        #[cfg(unix)]
        let (seed_uid, seed_owners) = match entry.and_then(|e| e.details.owner.as_ref()) {
            Some(owner) => {
                let label = if owner.is_current_user {
                    gds::t_untracked!("owner-you", name = owner.label().into_owned())
                } else {
                    owner.label().into_owned()
                };
                (
                    Some(owner.uid),
                    vec![IdentityOption {
                        id: owner.uid,
                        label,
                    }],
                )
            }
            None => (None, Vec::new()),
        };

        #[cfg(unix)]
        let (seed_gid, seed_groups) = match entry.and_then(|e| e.details.group.as_ref()) {
            Some(group) => (
                Some(group.gid),
                vec![IdentityOption {
                    id: group.gid,
                    label: group.label().into_owned(),
                }],
            ),
            None => (None, Vec::new()),
        };
        #[cfg(not(unix))]
        let _ = entry;

        let editor = browser.0.owner.with(|| {
            Self(Rc::new(BasicEditorInner {
                browser: browser.downgrade(),
                path: RefCell::new(path.clone()),
                parent_display,
                can_rename,
                original_name: RefCell::new(initial_name.clone()),
                name_draft: RwSignal::new_local(initial_name),
                name_input_ref: NodeRef::new(),
                composing: RwSignal::new_local(false),
                original_favorite: RefCell::new(initial_favorite),
                favorite: RwSignal::new_local(initial_favorite),
                #[cfg(unix)]
                original_uid: RefCell::new(seed_uid),
                #[cfg(unix)]
                original_gid: RefCell::new(seed_gid),
                #[cfg(unix)]
                selected_uid: RwSignal::new_local(seed_uid),
                #[cfg(unix)]
                selected_gid: RwSignal::new_local(seed_gid),
                #[cfg(unix)]
                owner_options: RwSignal::new_local(seed_owners),
                #[cfg(unix)]
                group_options: RwSignal::new_local(seed_groups),
                #[cfg(unix)]
                can_change_owner: RwSignal::new_local(false),
                #[cfg(unix)]
                can_change_group: RwSignal::new_local(false),
                loading: RwSignal::new_local(cfg!(unix)),
                saving: RwSignal::new_local(false),
                error: RwSignal::new_local(None),
                pending_focus: RefCell::new(None),
            }))
        });

        #[cfg(unix)]
        editor.load_unix_ownership(browser, path, auto_focus);
        #[cfg(not(unix))]
        if auto_focus {
            editor.focus();
        }

        editor
    }

    #[cfg(unix)]
    fn load_unix_ownership(&self, browser: &Browser, path: PathBuf, auto_focus: bool) {
        let weak = Rc::downgrade(&self.0);
        browser.blocking(
            "Read file ownership",
            move || load_ownership(&path),
            move |_, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                editor.0.loading.set(false);
                if let Ok(loaded) = result {
                    let owners = loaded
                        .owners
                        .into_iter()
                        .map(|opt| IdentityOption {
                            id: opt.id,
                            label: format_owner_label(opt.id, opt.label, loaded.euid),
                        })
                        .collect();
                    *editor.0.original_uid.borrow_mut() = Some(loaded.uid);
                    *editor.0.original_gid.borrow_mut() = Some(loaded.gid);
                    editor.0.owner_options.set(owners);
                    editor.0.group_options.set(loaded.groups);
                    editor.0.selected_uid.set(Some(loaded.uid));
                    editor.0.selected_gid.set(Some(loaded.gid));
                    editor.0.can_change_owner.set(loaded.can_change_owner);
                    editor.0.can_change_group.set(loaded.can_change_group);
                }
                if auto_focus {
                    editor.focus();
                }
            },
        );
    }

    pub(super) fn is_loading(&self) -> bool {
        self.0.loading.get()
    }

    pub(super) fn is_saving(&self) -> bool {
        self.0.saving.get()
    }

    pub(super) fn is_saving_untracked(&self) -> bool {
        self.0.saving.get_untracked()
    }

    pub(super) fn available(&self) -> bool {
        !self.0.loading.get() && !self.0.saving.get()
    }

    fn name_changed_untracked(&self) -> bool {
        self.0.can_rename && self.0.name_draft.get_untracked() != *self.0.original_name.borrow()
    }

    #[cfg(unix)]
    fn ownership_changes_untracked(&self) -> (Option<u32>, Option<u32>) {
        let next_uid = self
            .0
            .can_change_owner
            .get_untracked()
            .then(|| self.0.selected_uid.get_untracked())
            .flatten()
            .filter(|uid| Some(*uid) != *self.0.original_uid.borrow());
        let next_gid = self
            .0
            .can_change_group
            .get_untracked()
            .then(|| self.0.selected_gid.get_untracked())
            .flatten()
            .filter(|gid| Some(*gid) != *self.0.original_gid.borrow());
        (next_uid, next_gid)
    }

    fn favorite_changed_untracked(&self) -> bool {
        self.0.favorite.get_untracked() != *self.0.original_favorite.borrow()
    }

    pub(super) fn has_changes_untracked(&self) -> bool {
        let ownership_changed = {
            #[cfg(unix)]
            {
                let (uid, gid) = self.ownership_changes_untracked();
                uid.is_some() || gid.is_some()
            }
            #[cfg(not(unix))]
            {
                false
            }
        };
        self.name_changed_untracked() || ownership_changed || self.favorite_changed_untracked()
    }

    pub(super) fn validate_draft(&self) -> Result<(), ()> {
        if !self.name_changed_untracked() {
            self.0.error.set(None);
            return Ok(());
        }
        let draft = self.0.name_draft.get_untracked();
        let original = self.0.original_name.borrow().clone();
        if let Err(err) = rename::validate_name(&draft, OsStr::new(&original)) {
            self.0.error.set(Some(i18n::text_untracked(err.label_id())));
            return Err(());
        }
        let current_path = self.0.path.borrow().clone();
        if let Some(parent) = current_path.parent() {
            let destination = parent.join(&draft);
            if destination != current_path && std::fs::symlink_metadata(&destination).is_ok() {
                self.0.error.set(Some(gds::t_untracked!(
                    "rename-target-exists",
                    name = draft
                )));
                return Err(());
            }
        }
        self.0.error.set(None);
        Ok(())
    }

    pub(super) fn focus(&self) {
        if !self.0.can_rename {
            return;
        }
        if self.0.name_input_ref.is_bound() {
            self.0.name_input_ref.focus();
            return;
        }
        let weak = Rc::downgrade(&self.0);
        if let Some(browser) = self.0.browser.upgrade() {
            let handle = browser.0.owner.with(|| {
                request_frame(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        inner.name_input_ref.focus();
                    }
                })
            });
            *self.0.pending_focus.borrow_mut() = Some(handle);
        }
    }

    pub(super) fn save_result(&self, on_done: impl FnOnce(Result<Option<PathBuf>, ()>) + 'static) {
        if self.is_saving_untracked() {
            on_done(Ok(None));
            return;
        }
        if !self.has_changes_untracked() {
            on_done(Ok(None));
            return;
        }
        if self.validate_draft().is_err() {
            on_done(Err(()));
            return;
        }

        let Some(browser) = self.0.browser.upgrade() else {
            on_done(Err(()));
            return;
        };

        let source_path = self.0.path.borrow().clone();
        let new_name = self
            .name_changed_untracked()
            .then(|| self.0.name_draft.get_untracked());
        let want_favorite = self.0.favorite.get_untracked();
        let was_favorite = *self.0.original_favorite.borrow();
        #[cfg(unix)]
        let (target_uid, target_gid) = self.ownership_changes_untracked();

        let needs_fs_work = new_name.is_some() || {
            #[cfg(unix)]
            {
                target_uid.is_some() || target_gid.is_some()
            }
            #[cfg(not(unix))]
            {
                false
            }
        };

        if !needs_fs_work {
            if want_favorite != was_favorite {
                browser.update(|state| {
                    state
                        .places
                        .set_favorite(Location::from(source_path.as_path()), want_favorite);
                });
                browser.save_places();
                browser.refresh_location(&Location::Favorites);
                *self.0.original_favorite.borrow_mut() = want_favorite;
            }
            on_done(Ok(None));
            return;
        }

        self.0.saving.set(true);
        self.0.error.set(None);
        let weak = Rc::downgrade(&self.0);
        let done = RefCell::new(Some(
            Box::new(on_done) as Box<dyn FnOnce(Result<Option<PathBuf>, ()>)>
        ));

        let src_for_work = source_path.clone();
        browser.blocking(
            "Update basic file properties",
            move || -> Result<(Option<rename::Renamed>, Result<(), BasicSaveError>), String> {
                let mut renamed = None;
                let mut active_path = src_for_work.clone();

                if let Some(name) = new_name {
                    let request = rename::Request {
                        source: src_for_work,
                        name: OsString::from(name),
                    };
                    match request.execute() {
                        Ok(outcome) => {
                            active_path = outcome.destination.clone();
                            renamed = Some(outcome);
                        }
                        Err(error) => {
                            let failure = match error {
                                rename::Error::Exists { name, .. } => {
                                    BasicSaveError::RenameExists(name)
                                }
                                rename::Error::Name(err) => BasicSaveError::RenameName(err),
                                other => BasicSaveError::RenameFailed(other.to_string()),
                            };
                            return Ok((None, Err(failure)));
                        }
                    }
                }

                #[cfg(unix)]
                if target_uid.is_some() || target_gid.is_some() {
                    use nix::unistd::{Gid, Uid, chown};
                    if let Err(err) = chown(
                        &active_path,
                        target_uid.map(Uid::from_raw),
                        target_gid.map(Gid::from_raw),
                    ) {
                        let io_err = std::io::Error::from(err);
                        return Ok((
                            renamed,
                            Err(BasicSaveError::OwnershipFailed(io_err.to_string())),
                        ));
                    }
                }

                Ok((renamed, Ok(())))
            },
            move |browser, result| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let editor = Self(inner);
                editor.0.saving.set(false);
                let (renamed, outcome) = match result {
                    Ok((renamed, Ok(()))) => (renamed, Ok(())),
                    Ok((renamed, Err(err))) => (renamed, Err(err.localize())),
                    Err(message) => (None, Err(message)),
                };

                let renamed_dest = if let Some(renamed) = renamed {
                    let dest = renamed.destination.clone();
                    if let Some(name) = dest.file_name().and_then(OsStr::to_str) {
                        *editor.0.original_name.borrow_mut() = name.to_owned();
                    }
                    *editor.0.path.borrow_mut() = dest.clone();
                    if let Some(expected) = renamed.snapshot {
                        browser.record_history(history::Entry::new(
                            history::Kind::Rename,
                            vec![history::Step::Move {
                                source: dest.clone(),
                                destination: source_path.clone(),
                                expected,
                            }],
                        ));
                    }
                    Some(dest)
                } else {
                    None
                };

                let final_path = renamed_dest.clone().unwrap_or_else(|| source_path.clone());
                if want_favorite != was_favorite || (renamed_dest.is_some() && was_favorite) {
                    browser.update(|state| {
                        if renamed_dest.is_some() && was_favorite {
                            state
                                .places
                                .set_favorite(Location::from(source_path.as_path()), false);
                        }
                        state
                            .places
                            .set_favorite(Location::from(final_path.as_path()), want_favorite);
                    });
                    browser.save_places();
                    browser.refresh_location(&Location::Favorites);
                    *editor.0.original_favorite.borrow_mut() = want_favorite;
                }

                match outcome {
                    Ok(()) => {
                        #[cfg(unix)]
                        {
                            if let Some(uid) = target_uid {
                                *editor.0.original_uid.borrow_mut() = Some(uid);
                            }
                            if let Some(gid) = target_gid {
                                *editor.0.original_gid.borrow_mut() = Some(gid);
                            }
                        }
                        editor.0.error.set(None);
                        if let Some(cb) = done.borrow_mut().take() {
                            cb(Ok(renamed_dest));
                        }
                    }
                    Err(message) => {
                        editor.0.error.set(Some(message));
                        if let Some(cb) = done.borrow_mut().take() {
                            cb(Err(()));
                        }
                    }
                }
            },
        );
    }

    pub(super) fn fields(&self, on_enter_submit: impl Fn() + 'static) -> AnyView {
        install_basic_stylesheet();
        let name_draft = self.0.name_draft;
        let name_input_ref = self.0.name_input_ref;
        let composing_sig = self.0.composing;
        let saving_sig = self.0.saving;
        let error_sig = self.0.error;
        let can_rename = self.0.can_rename;
        let name_label_ref = NodeRef::new();

        let name_disabled = Signal::derive_local(move || !can_rename || saving_sig.get());
        let name_invalid = Signal::derive_local(move || error_sig.with(Option::is_some));
        let submit = Rc::new(on_enter_submit);

        #[cfg(unix)]
        let ownership_view = {
            let selected_uid = self.0.selected_uid;
            let selected_gid = self.0.selected_gid;
            let owner_options = self.0.owner_options;
            let group_options = self.0.group_options;
            let can_change_owner = self.0.can_change_owner;
            let can_change_group = self.0.can_change_group;
            let loading_sig = self.0.loading;

            let owner_label_ref = NodeRef::new();
            let group_label_ref = NodeRef::new();

            let owner_disabled = Signal::derive_local(move || {
                loading_sig.get() || saving_sig.get() || !can_change_owner.get()
            });
            let group_disabled = Signal::derive_local(move || {
                loading_sig.get() || saving_sig.get() || !can_change_group.get()
            });

            let owner_value = Signal::derive_local(move || {
                selected_uid
                    .get()
                    .map(|id| id.to_string())
                    .unwrap_or_default()
            });
            let group_value = Signal::derive_local(move || {
                selected_gid
                    .get()
                    .map(|id| id.to_string())
                    .unwrap_or_default()
            });

            let owner_display = Signal::derive_local(move || {
                let current = selected_uid.get();
                owner_options.with(|opts| {
                    current
                        .and_then(|id| opts.iter().find(|opt| opt.id == id))
                        .map(|opt| opt.label.clone())
                        .or_else(|| current.map(|id| id.to_string()))
                        .unwrap_or_else(|| "—".to_owned())
                })
            });
            let group_display = Signal::derive_local(move || {
                let current = selected_gid.get();
                group_options.with(|opts| {
                    current
                        .and_then(|id| opts.iter().find(|opt| opt.id == id))
                        .map(|opt| opt.label.clone())
                        .or_else(|| current.map(|id| id.to_string()))
                        .unwrap_or_else(|| "—".to_owned())
                })
            });

            Some(AnyView::new(view! {
                row(class = "gds-files-basic-ownership") {
                    column(class = "gds-files-basic-field") {
                        Label(
                            node_ref = owner_label_ref,
                            class = "gds-files-basic-label",
                        ) {{move || i18n::text("permissions-owner")}}
                        Select(
                            value = Binding::controlled(owner_value, move |next: String| {
                                if let Ok(id) = next.parse::<u32>() {
                                    selected_uid.set(Some(id));
                                    error_sig.set(None);
                                }
                            })
                        ) {
                            SelectTrigger(
                                class = "gds-files-basic-select",
                                disabled = owner_disabled,
                                label = i18n::text_untracked("permissions-owner"),
                                labelled_by = Some(owner_label_ref),
                            ) {
                                text(class = "gds-files-basic-select-value") {{move || owner_display.get()}}
                            }
                            SelectContent {
                                for opt in move || owner_options.get(), key = |opt| (opt.id, opt.label.clone()) {
                                    SelectItem(value = opt.id.to_string()) {{opt.label}}
                                }
                            }
                        }
                    }
                    column(class = "gds-files-basic-field") {
                        Label(
                            node_ref = group_label_ref,
                            class = "gds-files-basic-label",
                        ) {{move || i18n::text("permissions-group")}}
                        Select(
                            value = Binding::controlled(group_value, move |next: String| {
                                if let Ok(id) = next.parse::<u32>() {
                                    selected_gid.set(Some(id));
                                    error_sig.set(None);
                                }
                            })
                        ) {
                            SelectTrigger(
                                class = "gds-files-basic-select",
                                disabled = group_disabled,
                                label = i18n::text_untracked("permissions-group"),
                                labelled_by = Some(group_label_ref),
                            ) {
                                text(class = "gds-files-basic-select-value") {{move || group_display.get()}}
                            }
                            SelectContent {
                                for opt in move || group_options.get(), key = |opt| (opt.id, opt.label.clone()) {
                                    SelectItem(value = opt.id.to_string()) {{opt.label}}
                                }
                            }
                        }
                    }
                }
            }))
        };
        #[cfg(not(unix))]
        let ownership_view: Option<AnyView> = None;

        let fav_sig = self.0.favorite;
        let fav_ref = NodeRef::new();
        let fav_label_ref = NodeRef::new();
        let fav_checked = Binding::controlled(
            Signal::derive_local(move || {
                if fav_sig.get() {
                    Checked::Yes
                } else {
                    Checked::No
                }
            }),
            move |next| fav_sig.set(matches!(next, Checked::Yes)),
        );
        let parent_text = self.0.parent_display.clone();
        let parent_tooltip = self.0.parent_display.clone();

        AnyView::new(view! {
            column(class = "gds-files-basic-editor") {
                column(class = "gds-files-basic-field") {
                    Label(
                        node_ref = name_label_ref,
                        control = name_input_ref,
                        class = "gds-files-basic-label",
                    ) {{move || i18n::text("details-basic-name")}}
                    Input(
                        node_ref = name_input_ref,
                        class = "gds-files-basic-input",
                        label = i18n::text_untracked("details-basic-name"),
                        disabled = name_disabled,
                        invalid = name_invalid,
                        value = Binding::controlled(name_draft.read_only(), move |next: String| {
                            name_draft.set(next);
                            error_sig.set(None);
                        }),
                        on:ime_start = move |_| composing_sig.set(true),
                        on:ime_preedit = move |ev: &mut EventCx<'_, events::ImePreedit>| {
                            let active = match &**ev {
                                ImeEvent::Preedit { text, .. } => !text.is_empty(),
                                _ => ev.is_composing(),
                            };
                            composing_sig.set(active);
                        },
                        on:ime_commit = move |_| composing_sig.set(false),
                        on:ime_end = move |_| composing_sig.set(false),
                        on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                            if composing_sig.get_untracked() {
                                return;
                            }
                            if ev.key == Key::Named(NamedKey::Enter) && ev.modifiers == Modifiers::NONE {
                                ev.prevent_default();
                                ev.stop_propagation();
                                submit();
                            }
                        }
                    )
                }
                {ownership_view}
                row(class = "gds-files-basic-location") {
                    text(class = "gds-files-basic-location__label") {{move || i18n::text("details-basic-location")}}
                    box(class = "gds-files-basic-location__path-wrap") {
                        HintTooltip(text = parent_tooltip) {
                            text(class = "gds-files-basic-location__path") {{parent_text}}
                        }
                    }
                }
                row(class = "gds-files-basic-favorite") {
                    Checkbox(
                        node_ref = fav_ref,
                        checked = fav_checked,
                        disabled = saving_sig.read_only(),
                        labelled_by = fav_label_ref,
                    )
                    Label(node_ref = fav_label_ref, control = fav_ref) {{move || i18n::text("details-basic-favorite")}}
                }
                {(!can_rename).then(|| AnyView::new(view! {
                    text(class = "gds-files-basic-label") {{move || i18n::text("details-basic-non-utf8-name")}}
                }))}
                {move || {
                    error_sig.get().map(|err| {
                        view! {
                            text(class = "gds-files-basic-error") {{err}}
                        }
                        .into_view()
                    })
                }}
            }
        })
    }
}
