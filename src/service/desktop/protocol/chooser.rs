//! Lossless requests shared by the portal process and native chooser.
use serde::{Deserialize, Serialize};
use std::ffi::OsString;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub app_id: String,
    pub title: String,
    pub parent_window: String,
    pub modal: bool,
    pub accept_label: Option<String>,
    pub mode: Mode,
    pub current_folder: Option<OsString>,
    pub current_file: Option<OsString>,
    pub filters: Vec<Filter>,
    pub current_filter: Option<Filter>,
    pub choices: Vec<Choice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Mode {
    Open { directory: bool, multiple: bool },
    Save { name: OsString },
    SaveFiles { names: Vec<OsString> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub label: String,
    pub patterns: Vec<Pattern>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pattern {
    Glob(String),
    Mime(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub options: Vec<(String, String)>,
    pub selected: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Result {
    pub paths: Vec<OsString>,
    pub filter_index: usize,
    pub choices: Vec<(String, String)>,
    pub read_only: bool,
}
