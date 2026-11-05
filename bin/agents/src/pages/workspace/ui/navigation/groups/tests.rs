use super::*;
use bas::model::{Owner, SessionId, Timestamp, Workspace, WorkspaceId};

fn workspace(cwd: &str) -> Workspace {
    Workspace {
        id: WorkspaceId::new(),
        name: cwd.into(),
        cwd: cwd.into(),
        archived: false,
        revision: 1,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    }
}

#[test]
fn groups_by_workspace_identity_and_keeps_empty_workspaces() {
    let workspaces = [workspace("/one"), workspace("/two"), workspace("/empty")];
    let mut first = SessionMeta::new(
        workspaces[0].id,
        SessionId::new(),
        "pi".into(),
        Owner::current(),
    );
    first.cwd = Some("/one/symlink".into());
    let second = SessionMeta::new(
        workspaces[1].id,
        SessionId::new(),
        "pi".into(),
        Owner::current(),
    );
    let third = SessionMeta::new(
        workspaces[0].id,
        SessionId::new(),
        "pi".into(),
        Owner::current(),
    );
    let sessions = [first, second, third];
    let groups = group_threads(&sessions, &workspaces, "");
    assert_eq!(groups.len(), 3);
    assert_eq!(
        groups[0]
            .threads
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
        vec![sessions[0].id, sessions[2].id]
    );
    assert_eq!(groups[0].directory.as_deref(), Some(Path::new("/one")));
    assert!(groups[2].threads.is_empty());
}
