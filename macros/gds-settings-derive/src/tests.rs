use super::*;

#[test]
fn invalid_ui_annotations_fail_at_the_configuration_type() {
    for (source, message) in [
        (
            "struct Settings { #[setting(lable = \"name\")] name: String }",
            "unknown settings UI annotation",
        ),
        (
            "struct Settings { #[setting(min = 0)] count: u32 }",
            "editable fields need",
        ),
        (
            "struct Settings { #[setting(label = \"x\")] a: Vec<String> }",
            "custom editor",
        ),
        (
            "struct Settings { #[setting(label = \"x\", id = \"a\")] x: bool, #[setting(label = \"y\", id = \"a\")] y: bool }",
            "duplicate settings field id",
        ),
        (
            "enum Choice { #[setting(label = \"x\")] A(String) }",
            "unit enum variants",
        ),
    ] {
        let input = syn::parse_str(source).expect("valid Rust declaration");
        let error = expand(input).expect_err("invalid form annotation");
        assert!(error.to_string().contains(message), "{error}");
    }
}

#[test]
fn data_only_fields_and_explicitly_hidden_variants_need_no_ui_support() {
    for source in [
        "struct Settings { #[setting(label = \"enabled\")] enabled: bool, data: Vec<String> }",
        "enum Choice { #[setting(label = \"x\")] A, #[setting(skip)] Hidden(String) }",
    ] {
        assert!(expand(syn::parse_str(source).expect("valid declaration")).is_ok());
    }
}
