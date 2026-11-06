use super::*;
use bas::workspace::schema::{self, ServerDirectoryPath, schemars::JsonSchema};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(crate = "schema::schemars")]
#[allow(dead_code)]
enum Parameters {
    #[schemars(title = "Existing")]
    Existing { directory: ServerDirectoryPath },
    #[schemars(title = "New")]
    New {
        image: String,
        #[schemars(range(min = 1))]
        cpu: u32,
    },
}

#[test]
fn generated_provider_form_switches_branches_without_submitting_old_fields() {
    let mut form = Form::new(schema::generate::<Parameters>()).expect("generated provider form");
    let directory = form
        .fields
        .iter()
        .find(|field| field.path == ["directory"])
        .expect("directory field");
    assert_eq!(
        directory.input.semantic_type.as_deref(),
        Some("ServerDirectoryPath")
    );
    let mut values = BTreeMap::from([("/directory".into(), json!("/projects"))]);
    assert_eq!(
        form.parameters(&values, true).expect("existing"),
        json!({"mode":"existing","directory":"/projects"})
    );
    form.select("#".into(), 1).expect("new branch");
    values.insert("/image".into(), json!("image"));
    values.insert("/cpu".into(), json!(0));
    assert!(form.parameters(&values, true).is_err());
    values.insert("/cpu".into(), json!(2));
    assert_eq!(
        form.parameters(&values, true).expect("new"),
        json!({"mode":"new","image":"image","cpu":2})
    );
}

#[test]
fn unsupported_controls_in_an_unselected_branch_fail_before_opening() {
    let mut schema = schema::generate::<Parameters>();
    schema["oneOf"][1]["properties"]["image"]["x-bas"] = json!({"type":"UnknownControl"});
    assert!(Form::new(schema).is_err());
}

#[test]
fn local_refs_nested_parameters_and_arrays_use_library_validation() {
    let schema = json!({
        "$schema": schema::DIALECT,
        "type":"object", "required":["source"],
        "properties": {"source":{"$ref":"#/$defs/source"}},
        "$defs":{"source":{"type":"object","required":["names"],"properties":{
            "names":{"type":"array","items":{"type":"string"},"minItems":1}
        }}}
    });
    let form = Form::new(schema).expect("local ref");
    let values = BTreeMap::from([("/source/names".into(), json!(["project"]))]);
    assert_eq!(
        form.parameters(&values, true).expect("nested"),
        json!({"source":{"names":["project"]}})
    );
    let invalid = BTreeMap::from([("/source/names".into(), json!([42]))]);
    assert!(form.parameters(&invalid, true).is_err());
}
