//! Hierarchical tag address value type shared across location models and URI schemes.

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TagAddressError {
    #[error("Invalid tag path")]
    InvalidPath,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TagAddress(String);

impl TagAddress {
    /// Validate a hierarchical tag path.
    ///
    /// # Errors
    /// Rejects control characters, empty segments, and `.` / `..` traversal segments.
    pub fn new(tag: &str) -> Result<Self, TagAddressError> {
        if tag.contains(['\0', '\n', '\r'])
            || (!tag.is_empty() && tag.split('/').any(|s| matches!(s, "" | "." | "..")))
        {
            return Err(TagAddressError::InvalidPath);
        }
        Ok(Self(tag.to_owned()))
    }

    pub fn tag(&self) -> &str {
        &self.0
    }

    pub fn uri(&self) -> String {
        format!(
            "tags:/{}",
            self.0
                .split('/')
                .map(|s| utf8_percent_encode(s, NON_ALPHANUMERIC).to_string())
                .collect::<Vec<_>>()
                .join("/")
        )
    }

    /// Human-readable address; preserve escapes that affect parsing or display.
    pub fn display_uri(&self) -> String {
        let mut output = String::from("tags:/");
        for c in self.0.chars() {
            if c.is_ascii() && (!c.is_ascii_graphic() || "%?#\"<>`{}|\\^".contains(c)) {
                output.push_str(&format!("%{:02X}", c as u32));
            } else {
                output.push(c);
            }
        }
        output
    }

    pub fn parent(&self) -> Option<Self> {
        (!self.0.is_empty()).then(|| {
            Self(
                self.0
                    .rsplit_once('/')
                    .map(|(p, _)| p)
                    .unwrap_or_default()
                    .into(),
            )
        })
    }

    pub fn label(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or_default()
    }
}
