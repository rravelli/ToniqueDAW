use crate::utils::parse_name;

#[test]
fn test_parse_name() {
    let parsed = parse_name("# This is a test n°#", 3);
    assert_eq!(parsed, "4 This is a test n°4")
}
