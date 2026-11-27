//! Reusable `zgui` components and their reactive state, events, and rendering helpers.
//!
//! Organized in acyclic dependency layers:
//! `primitives` <- `interaction` <- `controls` <- (`layout`, `forms`) <- `feedback` <- `views`.

pub mod controls;
pub mod feedback;
pub mod forms;
pub mod interaction;
pub mod layout;
pub mod primitives;
pub mod views;

pub use controls::{button, disclosure, list, menu, time};
pub use feedback::{dialog, notifications};
pub use forms::{ToggleSwitch, ToggleSwitchProps, ToggleSwitchSize, field, settings, switch};
pub(crate) use interaction::shortcut;
pub use interaction::{actions, drag, keytips, region};
pub use layout::{frame, navigation, page, sidebar, split, window};
pub(crate) use primitives::metadata;
pub use primitives::{capacity, icon, styles, surface, tooltip};
pub use views::{about, file_inspection, help, logs, picker, task_manager, terminal, thumbnail};

pub use actions::command_tooltip;
pub use button::{IconButton, IconButtonProps};
pub use dialog::DialogActions;
pub use drag::DragCard;
pub use frame::{
    Frame, FrameContext, FrameFooter, FrameFooterProps, FrameProps, FrameToolbar,
    FrameToolbarProps, FrameWorkspace, FrameWorkspaceProps, WindowDragArea, WindowDragAreaProps,
};
pub use icon::{Icon, IconName, IconProps};
pub use list::ListSurface;
pub use navigation::{
    CornerAction, CornerActionProps, CornerButton, CornerButtonProps, HoverAction,
    HoverActionProps, NavItem, NavItemProps, PinnedIcon, PinnedIconProps, WithPin, WithPinProps,
    corner_action, corner_button, hover_action, pinned_icon, with_pin,
};
pub use page::ContentPage;
pub use primitives::container::{ContentContainer, ContentContainerProps};
pub use shortcut::{
    HeldModifier, Shortcut, ShortcutBadge, ShortcutBadgeProps, ShortcutHint, ShortcutHintProps,
    shortcut_tooltip,
};
pub use sidebar::{
    SidebarPresentation, SidebarState, use_sidebar_presentation, use_sidebar_presentation_widths,
};
pub use surface::{CardSurface, CardSurfaceProps, GlassPanel, GlassPanelProps};
