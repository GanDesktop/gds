use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{app::Gds, i18n};

/// Metadata and validation contract implemented by `#[derive(ConfigEditor)]` structs.
pub trait Model: Clone + Default + PartialEq + 'static {
    fn fields() -> Vec<Field<Self>>;

    /// Validate the current configuration values.
    ///
    /// # Errors
    /// Returns a human-readable validation message when the current values are invalid.
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Option catalog implemented by `#[derive(ConfigEditor)]` enums.
pub trait Choices {
    fn choices() -> Vec<Choice>;
}

/// One selectable option in a dropdown or searchable choice field.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: Value,
    pub title: String,
    search_terms: &'static str,
}

impl Choice {
    #[must_use]
    pub fn new(value: Value, title: impl Into<String>) -> Self {
        Self {
            value,
            title: title.into(),
            search_terms: "",
        }
    }

    #[must_use]
    pub fn search_terms(mut self, terms: &'static str) -> Self {
        self.search_terms = terms;
        self
    }

    #[must_use]
    pub const fn terms(&self) -> &'static str {
        self.search_terms
    }

    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self.value.to_string().to_lowercase().contains(&query)
            || self.search_terms.to_lowercase().contains(&query)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integer {
    Float,
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Number {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub scale: f64,
    pub unit: &'static str,
    pub integer: Integer,
}

impl Number {
    #[must_use]
    pub fn range(self) -> super::super::NumberRange {
        assert!(
            self.min.is_finite()
                && self.max.is_finite()
                && self.min <= self.max
                && self.step.is_finite()
                && self.step > 0.
                && self.scale.is_finite()
                && self.scale > 0.,
            "settings numeric annotations need a finite ordered range and positive step/scale"
        );
        super::super::NumberRange {
            min: (self.min * self.scale) as f32,
            max: (self.max * self.scale) as f32,
            step: (self.step * self.scale) as f32,
        }
    }

    /// Convert a scaled slider display value back into the field's JSON value.
    ///
    /// # Errors
    /// Returns an error if the scaled value is not finite or exceeds the target numeric bounds.
    pub fn value(self, display: f64) -> Result<Value, String> {
        let value = display / self.scale;
        if !value.is_finite() {
            return Err(Gds::current()
                .i18n()
                .text_untracked("form-number-not-finite"));
        }
        if matches!(self.integer, Integer::Float) {
            return serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| {
                    Gds::current()
                        .i18n()
                        .text_untracked("form-number-not-finite")
                });
        }
        let rounded = value.round();
        let converted = match self.integer {
            Integer::Float => unreachable!(),
            Integer::I8 => fits_signed(rounded, i8::MIN.into(), i8::MAX.into())
                .then(|| Value::from(rounded as i8)),
            Integer::I16 => fits_signed(rounded, i16::MIN.into(), i16::MAX.into())
                .then(|| Value::from(rounded as i16)),
            Integer::I32 => fits_signed(rounded, i32::MIN.into(), i32::MAX.into())
                .then(|| Value::from(rounded as i32)),
            Integer::I64 => fits_signed(rounded, i64::MIN.into(), i64::MAX.into())
                .then(|| Value::from(rounded as i64)),
            Integer::Isize => fits_signed(rounded, isize::MIN as i128, isize::MAX as i128)
                .then(|| Value::from(rounded as isize)),
            Integer::U8 => {
                fits_unsigned(rounded, u8::MAX.into()).then(|| Value::from(rounded as u8))
            }
            Integer::U16 => {
                fits_unsigned(rounded, u16::MAX.into()).then(|| Value::from(rounded as u16))
            }
            Integer::U32 => {
                fits_unsigned(rounded, u32::MAX.into()).then(|| Value::from(rounded as u32))
            }
            Integer::U64 => {
                fits_unsigned(rounded, u64::MAX.into()).then(|| Value::from(rounded as u64))
            }
            Integer::Usize => {
                fits_unsigned(rounded, usize::MAX as u128).then(|| Value::from(rounded as usize))
            }
        };
        converted.ok_or_else(|| {
            Gds::current()
                .i18n()
                .text_untracked("form-number-out-of-range")
        })
    }

    #[must_use]
    pub fn label(self, display: f64) -> String {
        let step = self.step * self.scale;
        let precision = if step >= 1. {
            0
        } else {
            (-step.log10()).ceil().clamp(0., 6.) as usize
        };
        format!("{display:.precision$}{}", self.unit)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Toggle,
    Number(Number),
    Text,
    StringList,
    Font,
    Language,
    Choice(fn() -> Vec<Choice>),
}

/// Only annotated fields are editable; callbacks preserve every other field.
pub struct Field<T> {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    pub nullable: bool,
    pub default: Option<fn() -> Value>,
    pub get: fn(&T) -> Value,
    pub set: fn(&mut T, Value) -> Result<(), String>,
}

impl<T> Field<T> {
    pub(super) fn is_modified(&self, model: &T, defaults: &T) -> bool {
        let current = (self.get)(model);
        let default = (self.get)(defaults);
        if current == default {
            return false;
        }
        if let Kind::Number(number) = self.kind
            && let (Some(current), Some(default)) = (current.as_f64(), default.as_f64())
        {
            return !number.range().equivalent(
                (current * number.scale) as f32,
                (default * number.scale) as f32,
            );
        }
        true
    }

    pub(super) fn effective(&self, model: &T) -> Value {
        let value = (self.get)(model);
        if !value.is_null() {
            return value;
        }
        self.default
            .map(|default| default())
            .unwrap_or_else(|| match self.kind {
                Kind::Toggle => Value::Bool(false),
                Kind::Number(number) => Value::from(number.min),
                _ => Value::Null,
            })
    }

    pub(super) fn choices(&self, model: &T) -> Vec<Choice> {
        let gds = Gds::current();
        let mut choices = match self.kind {
            Kind::Font => gds
                .appearance()
                .typography()
                .installed_families()
                .into_iter()
                .map(|name| Choice::new(Value::String(name.clone()), name))
                .collect(),
            Kind::Language => vec![
                Choice::new(encode(&i18n::Language::Chinese), "简体中文")
                    .search_terms("zh zh-CN Chinese 中文 简体"),
                Choice::new(encode(&i18n::Language::English), "English")
                    .search_terms("en en-US 英语 英文"),
            ],
            Kind::Choice(catalog) => catalog(),
            _ => Vec::new(),
        };
        if self.nullable {
            choices.insert(
                0,
                Choice::new(Value::Null, gds.i18n().text("form-default"))
                    .search_terms("system default automatic 系统 默认 自动"),
            );
        }
        let current = (self.get)(model);
        if !choices.iter().any(|choice| choice.value == current) {
            let mut title = current
                .as_str()
                .map_or_else(|| current.to_string(), str::to_owned);
            if matches!(self.kind, Kind::Font) {
                let mut arguments = fluent_bundle::FluentArgs::new();
                arguments.set("family", title);
                title = gds.i18n().format("form-font-missing", Some(&arguments));
            }
            choices.push(Choice::new(current, title));
        }
        choices
    }
}

fn fits_signed(rounded: f64, min: i128, max: i128) -> bool {
    if max <= (1i128 << 53) {
        rounded >= min as f64 && rounded <= max as f64
    } else {
        rounded >= min as f64 && rounded < max as f64
    }
}

fn fits_unsigned(rounded: f64, max: u128) -> bool {
    if max <= (1u128 << 53) {
        rounded >= 0. && rounded <= max as f64
    } else {
        rounded >= 0. && rounded < max as f64
    }
}

#[doc(hidden)]
#[must_use]
pub fn encode<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("editable configuration fields must serialize as values")
}

/// Decode a JSON configuration field value back into `T`.
///
/// # Errors
/// Returns a serialization error string when `value` cannot deserialize as `T`.
#[doc(hidden)]
pub fn decode<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}
