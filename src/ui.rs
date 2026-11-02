//! Reusable components and their dedicated state, events and rendering helpers.

pub mod feedback;
pub mod forms;
pub mod interaction;
pub mod time;
pub use feedback::{dialog, notifications, tooltip};
pub use forms::{field, settings};
pub(crate) use interaction::shortcut;
pub use interaction::{actions, keytips, region};

pub mod about;
pub mod button;
pub mod capacity;
mod container;
pub mod disclosure;
pub mod frame;
pub mod help;
pub mod list;
pub mod logs;
pub mod menu;
pub mod navigation;
pub mod page;
pub mod sidebar;
pub mod split;
pub mod surface;
pub mod terminal;
pub mod thumbnail;
pub mod window;
pub use button::IconButton;
pub use dialog::DialogActions;
pub use frame::{Frame, WindowDragArea};
pub use list::ListSurface;
pub use navigation::{NavItem, hover_action, pinned_icon, with_pin};
pub use page::ContentPage;
pub use shortcut::{
    HeldModifier, Shortcut, ShortcutBadge, ShortcutHint, command_tooltip, shortcut_tooltip,
};
