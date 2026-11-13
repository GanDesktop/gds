use super::*;

#[test]
fn schemes_keep_distinct_resource_identity() {
    let tag = TagAddress::new("工作/a #?%\" b").expect("valid tag fixture");
    assert_eq!(
        parse(&tag.uri()).expect("parse encoded tag URI"),
        Location::Tags(tag.clone())
    );
    assert_eq!(
        parse(&tag.display_uri()).expect("parse display tag URI"),
        Location::Tags(tag.clone())
    );
    assert!(tag.display_uri().starts_with("tags:/工作/"));
    assert_display_round_trip("工作/设计");
    assert_display_round_trip("%E5%B7%A5");
    assert_display_round_trip("a#?% b");
    assert_display_round_trip("emoji/🐬");
    assert_eq!(
        parent(&Location::Tags(tag)),
        Some(Location::Tags(
            TagAddress::new("工作").expect("valid parent tag")
        ))
    );
    assert_eq!(
        parse("location:/recent").expect("parse recent URI"),
        Location::Recent
    );
    assert!(parse("tags://host/path").is_err());
    assert!(parse("tags:/a/../b").is_err());
    assert!(parse("tags:/a?query").is_err());
    assert!(
        Location::Tags(TagAddress::new("").expect("root tag"))
            .local_path()
            .is_none()
    );
}

fn assert_display_round_trip(text: &str) {
    let tag = TagAddress::new(text).expect("valid tag fixture");
    assert_eq!(
        parse(&tag.display_uri()).expect("parse displayed tag URI"),
        parse(&tag.uri()).expect("parse encoded tag URI")
    );
}
