use bas::workspace::{
    Input,
    schema::{SERVER_DIRECTORY_PATH, Validator},
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct Field {
    pub path: Vec<String>,
    pub title: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: Kind,
    pub default: Option<Value>,
    pub input: Input,
}

#[derive(Clone)]
pub(super) enum Kind {
    String,
    Number,
    Integer,
    Boolean,
    Json,
    Enum(Vec<Value>),
    Const(Value),
}

impl Field {
    pub fn key(&self) -> String {
        self.path
            .iter()
            .map(|part| format!("/{}", part.replace('~', "~0").replace('/', "~1")))
            .collect()
    }

    pub fn value(&self, text: &str) -> Result<Option<Value>, String> {
        if text.is_empty() && !self.required {
            return Ok(None);
        }
        let value = match self.kind {
            Kind::String => Value::String(text.into()),
            _ => serde_json::from_str(text).map_err(|error| error.to_string())?,
        };
        Ok(Some(value))
    }
}

#[derive(Clone)]
pub(super) struct Branch {
    pub key: String,
    pub titles: Vec<String>,
    pub selected: usize,
}

pub(super) struct Form {
    schema: Value,
    validator: Validator,
    selections: BTreeMap<String, usize>,
    pub fields: Vec<Field>,
    pub branches: Vec<Branch>,
}

impl Form {
    pub fn new(schema: Value) -> Result<Self, String> {
        let validator = Validator::new(&schema).map_err(|error| error.to_string())?;
        let mut form = Self {
            schema,
            validator,
            selections: BTreeMap::new(),
            fields: Vec::new(),
            branches: Vec::new(),
        };
        // Inspect every branch so unsupported required controls are never hidden by initial selection.
        form.inspect(&form.schema.clone(), "#", &[], true, 0, true)?;
        form.rebuild()?;
        Ok(form)
    }

    pub fn directory_field(&self) -> Option<&Field> {
        if !self.branches.is_empty() {
            return None;
        }
        let mut fields = self
            .fields
            .iter()
            .filter(|field| !matches!(field.kind, Kind::Const(_)));
        let field = fields.next()?;
        (fields.next().is_none()
            && field.required
            && matches!(field.kind, Kind::String)
            && field.input.semantic_type.as_deref() == Some(SERVER_DIRECTORY_PATH)
            && field.input.choices.is_none())
        .then_some(field)
    }

    pub fn select(&mut self, key: String, index: usize) -> Result<(), String> {
        self.selections.insert(key, index);
        self.rebuild()
    }

    fn rebuild(&mut self) -> Result<(), String> {
        self.fields.clear();
        self.branches.clear();
        self.inspect(&self.schema.clone(), "#", &[], true, 0, false)
    }

    fn inspect(
        &mut self,
        schema: &Value,
        key: &str,
        path: &[String],
        required: bool,
        depth: usize,
        all: bool,
    ) -> Result<(), String> {
        if depth > 32 {
            return Err(format!("{key}: recursive forms are unsupported"));
        }
        let Some(object) = schema.as_object() else {
            return Err(format!("{key}: unsupported form schema"));
        };
        for keyword in [
            "allOf",
            "anyOf",
            "not",
            "if",
            "then",
            "else",
            "patternProperties",
            "dependentSchemas",
            "$dynamicRef",
        ] {
            if object.contains_key(keyword) {
                return Err(format!("{key}: unsupported form keyword {keyword}"));
            }
        }
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let pointer = reference
                .strip_prefix('#')
                .ok_or_else(|| format!("{key}: only local references are supported"))?;
            let mut target = self
                .schema
                .pointer(pointer)
                .cloned()
                .ok_or_else(|| format!("{key}: unresolved reference"))?;
            let target_object = target
                .as_object_mut()
                .ok_or_else(|| format!("{key}: unsupported reference"))?;
            for (name, value) in object {
                if name == "$ref" {
                    continue;
                }
                if target_object.contains_key(name)
                    && !matches!(name.as_str(), "title" | "description" | "default" | "x-bas")
                {
                    return Err(format!("{key}: unsupported reference sibling {name}"));
                }
                target_object.insert(name.clone(), value.clone());
            }
            return self.inspect(&target, key, path, required, depth + 1, all);
        }
        let input: Input = schema
            .get("x-bas")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| format!("{key}: {error}"))?
            .unwrap_or_default();
        if input
            .semantic_type
            .as_deref()
            .is_some_and(|kind| kind != SERVER_DIRECTORY_PATH)
        {
            return Err(format!("{key}: unsupported input type"));
        }
        if (input.semantic_type.is_some() || input.choices.is_some())
            && (object.contains_key("properties") || object.contains_key("oneOf"))
        {
            return Err(format!("{key}: input annotations require a scalar field"));
        }
        if input.semantic_type.is_some()
            && (schema.get("type").and_then(Value::as_str) != Some("string")
                || object.contains_key("enum")
                || object.contains_key("const"))
        {
            return Err(format!(
                "{key}: directory input requires an editable string"
            ));
        }
        if let Some(types) = schema.get("type").and_then(Value::as_array)
            && (types.len() != 2 || !types.iter().any(|kind| kind == "null"))
        {
            return Err(format!("{key}: use oneOf for multiple input types"));
        }
        if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
            if object.contains_key("properties") {
                return Err(format!("{key}: branch properties must be inside oneOf"));
            }
            let selected = self.selections.get(key).copied().unwrap_or(0);
            if selected >= variants.len() {
                return Err(format!("{key}: missing branch"));
            }
            self.branches.push(Branch {
                key: key.into(),
                selected,
                titles: variants
                    .iter()
                    .enumerate()
                    .map(|(i, value)| {
                        value
                            .get("title")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .unwrap_or_else(|| (i + 1).to_string())
                    })
                    .collect(),
            });
            for (index, variant) in variants.iter().enumerate() {
                if all || index == selected {
                    self.inspect(
                        variant,
                        &format!("{key}/oneOf/{index}"),
                        path,
                        required,
                        depth + 1,
                        all,
                    )?;
                }
            }
            return Ok(());
        }
        let kind = schema.get("type").and_then(|value| {
            value.as_str().or_else(|| {
                value
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|kind| *kind != "null")
            })
        });
        if kind == Some("object") {
            let properties = schema
                .get("properties")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{key}: object fields are missing"))?;
            let required_fields = schema.get("required").and_then(Value::as_array);
            for (name, child) in properties {
                let mut child_path = path.to_vec();
                child_path.push(name.clone());
                let required = required
                    && required_fields.is_some_and(|fields| {
                        fields.iter().any(|field| field.as_str() == Some(name))
                    });
                self.inspect(
                    child,
                    &format!(
                        "{key}/properties/{}",
                        name.replace('~', "~0").replace('/', "~1")
                    ),
                    &child_path,
                    required,
                    depth + 1,
                    all,
                )?;
            }
            return Ok(());
        }
        let kind = if let Some(value) = schema.get("const") {
            Kind::Const(value.clone())
        } else if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            Kind::Enum(values.clone())
        } else {
            match kind {
                Some("string") => Kind::String,
                Some("number") => Kind::Number,
                Some("integer") => Kind::Integer,
                Some("boolean") => Kind::Boolean,
                Some("array" | "null") => Kind::Json,
                _ => return Err(format!("{key}: unsupported field type")),
            }
        };
        self.fields.push(Field {
            path: path.to_vec(),
            title: schema
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| path.join(" / ")),
            description: schema
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            required,
            kind,
            input,
            default: schema.get("default").cloned(),
        });
        Ok(())
    }

    pub fn parameters(
        &self,
        values: &BTreeMap<String, Value>,
        validate: bool,
    ) -> Result<Value, String> {
        let mut result = Value::Object(Map::new());
        for field in &self.fields {
            let value = match &field.kind {
                Kind::Const(value) => Some(value),
                _ => values.get(&field.key()),
            };
            if let Some(value) = value {
                Self::insert(&mut result, &field.path, value.clone());
            }
        }
        if validate {
            self.validator
                .validate(&result)
                .map_err(|error| error.to_string())?;
        }
        Ok(result)
    }

    fn insert(target: &mut Value, path: &[String], value: Value) {
        let Some((key, rest)) = path.split_first() else {
            *target = value;
            return;
        };
        let object = target.as_object_mut().expect("field ancestors are objects");
        let child = object
            .entry(key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        Self::insert(child, rest, value);
    }
}

#[cfg(test)]
mod tests;
