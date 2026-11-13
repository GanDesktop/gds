use super::*;
use std::collections::HashSet;

#[test]
fn embedded_rules_have_unique_mime_types() {
    let config: Config =
        toml::from_str(CONFIG).expect("embedded MIME presentation config must parse");
    let mimes: Vec<String> = config
        .formats
        .into_iter()
        .flat_map(|rule| rule.mimes)
        .collect();
    let unique: HashSet<&str> = mimes.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), mimes.len());
    assert_eq!(FORMATS.len(), mimes.len());
}
