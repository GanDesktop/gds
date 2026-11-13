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
        (
            "struct Settings { #[setting(label = \"x\", min = 0)] enabled: bool }",
            "numeric settings annotations",
        ),
        (
            "struct Settings { #[setting(label = \"x\", max = 10)] name: String }",
            "numeric settings annotations",
        ),
        (
            "struct Settings { #[setting(label = \"x\", step = 1)] mode: Mode }",
            "numeric settings annotations",
        ),
        (
            "struct Settings { #[setting(label = \"x\", kind = \"font\", scale = 100)] font: String }",
            "numeric settings annotations",
        ),
        (
            "struct Settings { #[setting(label = \"x\", kind = \"text\", unit = \"px\")] name: String }",
            "numeric settings annotations",
        ),
        (
            "enum Choice { #[setting(label = \"x\", min = 0)] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", max = 1)] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", step = 1)] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", scale = 1)] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", unit = \"%\")] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", help = \"h\")] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", group = \"g\")] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", kind = \"text\")] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", default = 0)] A }",
            "enum choice variants only support",
        ),
        (
            "enum Choice { #[setting(label = \"x\", id = \"a\")] A }",
            "enum choice variants only support",
        ),
        (
            "struct Settings { #[setting(skip, label = \"x\")] skipped: bool, #[setting(label = \"y\")] ok: bool }",
            "#[setting(skip)] cannot be combined",
        ),
        (
            "struct Settings { #[setting(skip)] #[setting(min = 0)] skipped: u32, #[setting(label = \"y\")] ok: bool }",
            "#[setting(skip)] cannot be combined",
        ),
        (
            "enum Choice { #[setting(skip, label = \"x\")] Hidden }",
            "#[setting(skip)] cannot be combined",
        ),
        (
            "struct Settings { #[setting(label = \"x\", label = \"y\")] ok: bool }",
            "duplicate settings UI annotation",
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
        "struct Settings { #[setting(label = \"enabled\")] enabled: bool, #[setting(skip)] skipped: Vec<String>, data: Vec<String> }",
        "enum Choice { #[setting(label = \"x\")] A, #[setting(skip)] Hidden(String) }",
    ] {
        assert!(expand(syn::parse_str(source).expect("valid declaration")).is_ok());
    }
}

#[test]
fn numeric_fields_emit_exact_integer_variants() {
    let input = syn::parse_str(
        "struct Numbers {
            #[setting(label = \"a\")] a: i8,
            #[setting(label = \"b\")] b: i16,
            #[setting(label = \"c\")] c: i32,
            #[setting(label = \"d\")] d: i64,
            #[setting(label = \"e\")] e: isize,
            #[setting(label = \"f\")] f: u8,
            #[setting(label = \"g\")] g: u16,
            #[setting(label = \"h\")] h: u32,
            #[setting(label = \"i\")] i: u64,
            #[setting(label = \"j\")] j: usize,
            #[setting(label = \"k\")] k: f32,
            #[setting(label = \"l\")] l: Option<u8>,
        }",
    )
    .expect("valid Rust declaration");
    let tokens = expand(input).expect("valid numeric settings").to_string();
    for expected in [
        "Integer :: I8",
        "Integer :: I16",
        "Integer :: I32",
        "Integer :: I64",
        "Integer :: Isize",
        "Integer :: U8",
        "Integer :: U16",
        "Integer :: U32",
        "Integer :: U64",
        "Integer :: Usize",
        "Integer :: Float",
    ] {
        assert!(tokens.contains(expected), "missing {expected} in {tokens}");
    }
}
