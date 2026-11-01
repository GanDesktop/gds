use gpui_kit::TestAppContext;
use gpui_kit::component::dialog::{Cancel, Confirm};

use super::{DIALOG_CONTEXT, DialogCommand};

/// The dialog keys stay on the base actions, which run the callbacks the
/// dialog already installed instead of a second path around them.
#[test]
fn dialog_shortcuts_reach_the_existing_decisions() {
    let confirm = DialogCommand::Confirm.action();
    let confirm = confirm
        .as_any()
        .downcast_ref::<Confirm>()
        .expect("Alt+A must dispatch the dialog Confirm action");
    assert!(!confirm.secondary);
    assert_eq!(DialogCommand::Confirm.default_stroke(), "alt-a");

    let cancel = DialogCommand::Cancel.action();
    assert!(cancel.as_any().is::<Cancel>());
    assert_eq!(DialogCommand::Cancel.default_stroke(), "alt-c");
}

/// Every dialog decision is bound in the modal context only, so the keys
/// neither fire in the workspace behind the dialog nor while typing in it.
#[gpui_kit::test]
fn dialog_shortcuts_are_scoped_to_the_modal_context(cx: &mut TestAppContext) {
    for command in DialogCommand::ALL {
        let shortcut = command.shortcut();
        assert_eq!(shortcut.context(), Some(DIALOG_CONTEXT));
        cx.update(|cx| command.binding(cx));
    }
}
