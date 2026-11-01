//! D-Bus values and the lossless native chooser process protocol.
use std::{collections::HashMap, ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};

use gds::service::desktop::protocol::chooser::{self, Choice, Mode, Pattern, Request};
use thiserror::Error;
use zbus::zvariant::{OwnedValue, Value};

pub type Options = HashMap<String, OwnedValue>;
pub type Response = (u32, Options);
pub type Filter = (String, Vec<(u32, String)>);
type WireChoice = (String, String, Vec<(String, String)>, String);

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid Portal value: {0}")]
    Variant(#[from] zbus::zvariant::Error),
    #[error("invalid picker JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("picker returned an invalid selection count")]
    SelectionCount,
    #[error("picker returned an invalid path: {0:?}")]
    InvalidPath(PathBuf),
    #[error("picker returned an invalid filter index")]
    FilterIndex,
    #[error("invalid chooser option: {0}")]
    InvalidOption(&'static str),
}

pub struct Picker {
    pub request: Request,
}

impl Picker {
    /// # Errors
    /// Reports malformed protocol values or unknown filter kinds.
    pub fn from_invocation(
        member: &str,
        app_id: &str,
        parent: &str,
        title: &str,
        options: &Options,
    ) -> Result<Self, Error> {
        let mode = match member {
            "OpenFile" => Mode::Open {
                directory: Self::flag(options, "directory", false)?,
                multiple: Self::flag(options, "multiple", false)?,
            },
            "SaveFile" => Mode::Save {
                name: Self::text(options, "current_name")?
                    .unwrap_or_default()
                    .into(),
            },
            "SaveFiles" => Mode::SaveFiles {
                names: options
                    .get("files")
                    .map(|v| Vec::<Vec<u8>>::try_from(v.try_clone()?))
                    .transpose()?
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|v| !v.is_empty() && v != &[0])
                    .map(Self::bytes)
                    .collect(),
            },
            _ => return Err(Error::InvalidOption("method")),
        };
        let mut filters = options
            .get("filters")
            .map(|v| Vec::<Filter>::try_from(v.try_clone()?))
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(Self::filter)
            .collect::<Result<Vec<_>, _>>()?;
        let current_filter = options
            .get("current_filter")
            .map(|v| Filter::try_from(v.try_clone()?))
            .transpose()?
            .map(Self::filter)
            .transpose()?;
        if let Some(current) = &current_filter {
            filters.retain(|filter| filter != current);
            filters.insert(0, current.clone());
        }
        let choices = options
            .get("choices")
            .map(|v| Vec::<WireChoice>::try_from(v.try_clone()?))
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|(id, label, options, selected)| {
                let selected = if options.is_empty() {
                    if selected == "true" {
                        "true".to_owned()
                    } else {
                        "false".to_owned()
                    }
                } else if options.iter().any(|(id, _)| id == &selected) {
                    selected
                } else {
                    options[0].0.clone()
                };
                Choice {
                    id,
                    label,
                    options,
                    selected,
                }
            })
            .collect();
        Ok(Self {
            request: Request {
                app_id: app_id.into(),
                title: title.into(),
                parent_window: parent.into(),
                modal: Self::flag(options, "modal", true)?,
                accept_label: Self::text(options, "accept_label")?,
                mode,
                current_folder: Self::path(options, "current_folder")?,
                current_file: Self::path(options, "current_file")?,
                filters,
                current_filter,
                choices,
            },
        })
    }

    fn filter((label, patterns): Filter) -> Result<chooser::Filter, Error> {
        let patterns = patterns
            .into_iter()
            .map(|(kind, pattern)| match kind {
                0 => Ok(Pattern::Glob(pattern)),
                1 => Ok(Pattern::Mime(pattern)),
                _ => Err(Error::InvalidOption("filter kind")),
            })
            .collect::<Result<_, _>>()?;
        Ok(chooser::Filter { label, patterns })
    }

    fn flag(options: &Options, name: &str, default: bool) -> Result<bool, Error> {
        Ok(options
            .get(name)
            .map(bool::try_from)
            .transpose()?
            .unwrap_or(default))
    }

    fn text(options: &Options, name: &str) -> Result<Option<String>, Error> {
        Ok(options
            .get(name)
            .map(|v| String::try_from(v.try_clone()?))
            .transpose()?)
    }

    fn bytes(mut bytes: Vec<u8>) -> OsString {
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        OsString::from_vec(bytes)
    }

    fn path(options: &Options, name: &str) -> Result<Option<OsString>, Error> {
        Ok(options
            .get(name)
            .map(|v| Vec::<u8>::try_from(v.try_clone()?))
            .transpose()?
            .map(Self::bytes))
    }

    /// # Errors
    /// Reports malformed output or a result inconsistent with the request.
    pub fn results(&self, output: &[u8]) -> Result<Options, Error> {
        let result: chooser::Result = serde_json::from_slice(output)?;
        let valid_count = match &self.request.mode {
            Mode::Open { multiple, .. } => {
                !result.paths.is_empty() && (*multiple || result.paths.len() == 1)
            }
            Mode::Save { .. } => result.paths.len() == 1,
            Mode::SaveFiles { names } => result.paths.len() == names.len() && !names.is_empty(),
        };
        if !valid_count {
            return Err(Error::SelectionCount);
        }
        let mut uris = Vec::new();
        for path in &result.paths {
            let path = PathBuf::from(path);
            let valid = match self.request.mode {
                Mode::Open {
                    directory: true, ..
                } => path.is_dir(),
                Mode::Open { .. } => path.is_file(),
                _ => path.parent().is_some_and(|parent| parent.is_dir()) && !path.is_dir(),
            };
            if !path.is_absolute() || !valid {
                return Err(Error::InvalidPath(path));
            }
            uris.push(
                url::Url::from_file_path(&path)
                    .map_err(|()| Error::InvalidPath(path))?
                    .to_string(),
            );
        }
        let mut results = Options::from([
            ("uris".into(), Value::from(uris).try_to_owned()?),
            ("writable".into(), (!result.read_only).into()),
        ]);
        if !self.request.filters.is_empty() {
            let filter = self
                .request
                .filters
                .get(result.filter_index)
                .ok_or(Error::FilterIndex)?;
            let patterns = filter
                .patterns
                .iter()
                .map(|p| match p {
                    Pattern::Glob(p) => (0u32, p.clone()),
                    Pattern::Mime(p) => (1, p.clone()),
                })
                .collect::<Vec<_>>();
            results.insert(
                "current_filter".into(),
                Value::from((filter.label.clone(), patterns)).try_to_owned()?,
            );
        }
        if result.choices.len() != self.request.choices.len()
            || self.request.choices.iter().any(|choice| {
                !result.choices.iter().any(|(id, selected)| {
                    id == &choice.id
                        && if choice.options.is_empty() {
                            selected == "true" || selected == "false"
                        } else {
                            choice.options.iter().any(|(id, _)| id == selected)
                        }
                })
            })
        {
            return Err(Error::InvalidOption("choice result"));
        }
        if !self.request.choices.is_empty() {
            results.insert(
                "choices".into(),
                Value::from(result.choices).try_to_owned()?,
            );
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests;
