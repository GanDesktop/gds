use super::*;
use bas::authentication::{Execution, Source};
use serde_json::json;

#[test]
fn start_encodes_the_exact_selected_method_and_registration() {
    let spec = AgentSpec::embedded();
    let method = Method {
        id: "account".into(),
        name: "Account".into(),
        description: Some("current description".into()),
        raw: json!({"id":"account","custom":"fresh metadata"}),
        source: Source::Initialize,
        execution: Execution::Unsupported {
            reason: "fixture".into(),
        },
    };
    let input = preparation(&spec, &method).expect("start input");
    let Some(pb::prepare_acp_request::Target::Authentication(start)) = input.target else {
        panic!("start control");
    };
    assert_eq!(start.agent, spec.id.to_string());
    assert_eq!(start.method_id, method.id);
    assert_eq!(
        serde_json::from_slice::<AgentSpec>(&start.expected_spec_json).expect("spec"),
        spec
    );
    assert_eq!(
        serde_json::from_slice::<Method>(&start.expected_method_json).expect("method"),
        method
    );
}
