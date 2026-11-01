use std::collections::HashMap;

use nix::unistd::{Gid, Group, Uid, User, getegid, geteuid};

use super::super::details::Permissions;

/// Per-read cache of owner and group lookups.
#[derive(Debug)]
pub(crate) struct IdentityCache {
    euid: u32,
    egid: u32,
    /// Supplementary groups plus the effective group.
    groups: Vec<u32>,
    user_names: HashMap<u32, Option<String>>,
    group_names: HashMap<u32, Option<String>>,
}

impl Default for IdentityCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentityCache {
    pub(crate) fn new() -> Self {
        let egid = getegid().as_raw();
        let mut groups = supplementary_groups();
        if !groups.contains(&egid) {
            groups.push(egid);
        }
        Self {
            euid: geteuid().as_raw(),
            egid,
            groups,
            user_names: HashMap::new(),
            group_names: HashMap::new(),
        }
    }

    pub(crate) fn is_current_user(&self, uid: u32) -> bool {
        uid == self.euid
    }

    pub(crate) fn user_name(&mut self, uid: u32) -> Option<String> {
        if let Some(name) = self.user_names.get(&uid) {
            return name.clone();
        }
        let name = User::from_uid(Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|user| user.name);
        self.user_names.insert(uid, name.clone());
        name
    }

    pub(crate) fn group_name(&mut self, gid: u32) -> Option<String> {
        if let Some(name) = self.group_names.get(&gid) {
            return name.clone();
        }
        let name = Group::from_gid(Gid::from_raw(gid))
            .ok()
            .flatten()
            .map(|group| group.name);
        self.group_names.insert(gid, name.clone());
        name
    }

    /// Whether the effective user may write an entry with these ownership and
    /// permission bits. Group membership is checked against the full group list,
    /// not only the primary group.
    pub(crate) fn can_write(&self, owner: u32, group: u32, mode: u32) -> bool {
        // The superuser bypasses permission bits; a read-only mount is a property
        // of the mount, not of the entry, and is not reported here.
        if self.euid == 0 {
            return true;
        }
        let permissions = Permissions::from_mode(mode);
        let access = if owner == self.euid {
            permissions.owner
        } else if group == self.egid || self.groups.contains(&group) {
            permissions.group
        } else {
            permissions.other
        };
        access.write
    }
}

#[cfg(target_vendor = "apple")]
fn supplementary_groups() -> Vec<u32> {
    // `getgroups` is not exposed on Apple platforms; the effective group still
    // participates in permission checks.
    Vec::new()
}

#[cfg(not(target_vendor = "apple"))]
fn supplementary_groups() -> Vec<u32> {
    nix::unistd::getgroups()
        .map(|groups| groups.into_iter().map(Gid::as_raw).collect())
        .unwrap_or_default()
}
