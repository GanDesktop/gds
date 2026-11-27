//! Sibling directory image catalog ordered naturally.

mod api;
mod model;

pub use api::{first_in_folder, scan_siblings};
pub use model::{CatalogEntryMeta, CatalogState, format_byte_size};
