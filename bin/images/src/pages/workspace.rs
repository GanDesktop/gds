//! Window-owned image viewing and editing workspace.
mod ui;
use std::{path::PathBuf, sync::Arc};
pub use ui::mount;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchMode {
    EmptySmartState,
    OpenFile(PathBuf),
    Clipboard,
    Stdin(Arc<[u8]>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureWorkflow {
    pub output: Option<PathBuf>,
    pub copy_on_save: bool,
    pub early_exit: bool,
}
