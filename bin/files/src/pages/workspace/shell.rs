//! Layer 6 workspace shell composing `Browser`, chrome, collections, keyboard, and view.

pub(super) mod browser;
pub(super) mod chrome;
pub(super) mod collections;
pub(super) mod keyboard;
pub(super) mod view;

pub(crate) use browser::{Browser, WeakBrowser};
