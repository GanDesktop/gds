use crate::ui::Shortcut;
use gpui_kit::*;

actions!(
    directory_picker,
    [
        Back,
        Forward,
        Parent,
        Refresh,
        EditLocation,
        Filter,
        Hidden,
        Open
    ]
);
const CONTEXT: &str = "DirectoryPicker";
struct Registered;
impl Global for Registered {}

pub(super) fn register(cx: &mut App) {
    if cx.has_global::<Registered>() {
        return;
    }
    cx.set_global(Registered);
    use gpui_kit::base::actions::{Cancel, SelectNextColumn, SelectPrevColumn};
    let unbind = |stroke, action: Box<dyn Action>| {
        KeyBinding::new(
            stroke,
            Unbind(action.name().into()),
            Some("DirectoryPicker > DataTable"),
        )
    };
    cx.bind_keys([
        unbind("tab", Box::new(SelectNextColumn)),
        unbind("shift-tab", Box::new(SelectPrevColumn)),
        unbind("escape", Box::new(Cancel)),
        KeyBinding::new("alt-left", Back, Some(CONTEXT)),
        KeyBinding::new("alt-right", Forward, Some(CONTEXT)),
        KeyBinding::new("alt-up", Parent, Some(CONTEXT)),
        KeyBinding::new("ctrl-r", Refresh, Some(CONTEXT)),
        KeyBinding::new("ctrl-l", EditLocation, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", Filter, Some(CONTEXT)),
        KeyBinding::new("ctrl-h", Hidden, Some(CONTEXT)),
        KeyBinding::new("enter", Open, Some("DirectoryPicker > DataTable")),
    ]);
}

pub(super) fn shortcut(action: impl Action) -> Shortcut {
    Shortcut::new(Box::new(action), Some(CONTEXT))
}
