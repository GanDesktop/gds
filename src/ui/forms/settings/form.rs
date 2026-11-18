//! Typed configuration metadata and native reactive editors.
mod editor;
mod model;
pub use crate::app::Gds as App;
pub use editor::Editor;
pub use model::{Choice, Choices, Field, Integer, Kind, Model, Number, decode, encode};
pub use serde_json::Value;
