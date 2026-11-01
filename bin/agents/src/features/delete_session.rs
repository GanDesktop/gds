//! Confirm session deletion and commit the user's future-confirmation preference.
use crate::shared::i18n::tr;
use crate::shared::lib::appearance;
use crate::shared::ui::dialogs::{self, Confirmation, ConfirmationOption};
use gpui_kit::App;

/// # Errors
/// Reports a native-window failure; preference errors remain visible in the dialog.
pub(crate) fn confirm(
    request: Confirmation,
    on_confirm: impl FnOnce(&mut App) + 'static,
    cx: &mut App,
) -> anyhow::Result<()> {
    if !cx.global::<appearance::Appearance>().confirm_session_delete {
        on_confirm(cx);
        return Ok(());
    }
    let option = ConfirmationOption {
        id: "delete-dont-ask".into(),
        label: tr!("delete-dont-ask").into(),
        checked: false,
        on_confirm: Box::new(|checked, window, cx| {
            if !checked {
                return Ok(());
            }
            let editor = appearance::Editor::current(cx);
            editor
                .load()
                .and_then(|mut document| {
                    let mut candidate = document.preferences.clone();
                    candidate.confirm_session_delete = Some(false);
                    editor.save(&mut document, candidate, window, cx)
                })
                .map_err(|error| tr!("settings-apply-error", error = error.to_string()))
        }),
    };
    dialogs::open_confirmation_with_option(request, Some(option), on_confirm, cx).map(|_| ())
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
