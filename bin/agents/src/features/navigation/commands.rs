//! Workspace action identities and the metadata shown in settings.
use crate::shared::i18n::tr;
use gpui_kit::{Action, actions};

actions!(
    bas,
    [
        NewSession,
        FocusSearch,
        FocusComposer,
        ToggleSidebar,
        ToggleMode,
        ToggleOutput,
        ToggleTerminal,
        NextThread,
        PreviousThread,
        Dismiss,
        Refresh,
        ScrollConversationToEnd
    ]
);

actions!(
    bas,
    [
        RenameThread,
        ArchiveThread,
        ToggleThreadPin,
        ToggleThreadBookmark,
        DeleteThread,
        AddWorkspace,
        ToggleSession,
        OpenSessionManager,
        ScrollConversationToTop,
        PageConversationUp,
        PageConversationDown,
        PreviousConversationTurn,
        NextConversationTurn
    ]
);

pub(crate) fn settings_entries() -> Vec<(&'static str, gds::ui::Shortcut)> {
    let actions: Vec<(&str, Box<dyn Action>)> = vec![
        (tr!("session-new"), Box::new(NewSession)),
        (tr!("session-rename"), Box::new(RenameThread)),
        (tr!("session-archive"), Box::new(ArchiveThread)),
        (tr!("session-pin"), Box::new(ToggleThreadPin)),
        (tr!("session-bookmark"), Box::new(ToggleThreadBookmark)),
        (tr!("session-delete"), Box::new(DeleteThread)),
        (tr!("workspace-add"), Box::new(AddWorkspace)),
        (tr!("agent-session-close"), Box::new(ToggleSession)),
        (tr!("agent-history-title"), Box::new(OpenSessionManager)),
        (tr!("shortcuts-search"), Box::new(FocusSearch)),
        (tr!("shortcuts-composer"), Box::new(FocusComposer)),
        (tr!("sidebar-toggle"), Box::new(ToggleSidebar)),
        (tr!("conversation-toggle"), Box::new(ToggleOutput)),
        (tr!("shortcuts-window-mode"), Box::new(ToggleMode)),
        (tr!("shortcuts-next-thread"), Box::new(NextThread)),
        (tr!("shortcuts-previous-thread"), Box::new(PreviousThread)),
        (tr!("shortcuts-refresh"), Box::new(Refresh)),
        (tr!("conversation-top"), Box::new(ScrollConversationToTop)),
        (
            tr!("conversation-latest"),
            Box::new(ScrollConversationToEnd),
        ),
        (tr!("shortcuts-page-up"), Box::new(PageConversationUp)),
        (tr!("shortcuts-page-down"), Box::new(PageConversationDown)),
        (
            tr!("turn-marker-previous"),
            Box::new(PreviousConversationTurn),
        ),
        (tr!("turn-marker-next"), Box::new(NextConversationTurn)),
    ];
    actions
        .into_iter()
        .map(|(label, action)| (label, gds::ui::Shortcut::new(action, Some("BasWorkspace"))))
        .chain(std::iter::once((
            tr!("local-terminal-toggle"),
            gds::ui::Shortcut::new(Box::new(ToggleTerminal), Some("AgentsHost")),
        )))
        .collect()
}
