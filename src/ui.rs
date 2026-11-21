//! Reusable `zgui` components and their reactive state, events, and rendering helpers.

pub mod feedback;
pub mod forms;
pub mod interaction;
pub mod time;
pub use feedback::{dialog, notifications, tooltip};
pub use forms::{ToggleSwitch, ToggleSwitchProps, ToggleSwitchSize, field, settings, switch};
pub(crate) use interaction::shortcut;
pub use interaction::{actions, keytips, region};

pub mod about;
pub mod button;
pub mod capacity;
mod container;
pub mod disclosure;
pub mod drag;
pub mod file_inspection;
pub mod frame;
pub mod help;
pub mod icon;
pub mod list;
pub mod logs;
pub mod menu;
pub mod navigation;
pub mod page;
pub mod picker;
pub mod sidebar;
pub mod split;
pub mod styles;
pub mod surface;
pub mod task_manager;
pub mod terminal;
pub mod thumbnail;
pub mod window;

pub use button::{IconButton, IconButtonProps};
pub use container::{ContentContainer, ContentContainerProps};
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
pub use shortcut::{
    HeldModifier, Shortcut, ShortcutBadge, ShortcutBadgeProps, ShortcutHint, ShortcutHintProps,
    command_tooltip, shortcut_tooltip,
};
pub use sidebar::{
    SidebarPresentation, SidebarState, use_sidebar_presentation, use_sidebar_presentation_widths,
};
pub use surface::{CardSurface, CardSurfaceProps, GlassPanel, GlassPanelProps};
