//! Application-wide navigation and lifecycle action identities.
gpui_kit::actions!(application, [OpenSettings, Quit]);

gpui_kit::actions!(
    settings_window,
    [CloseWindow, NextPage, PreviousPage, ToggleSidebar]
);
