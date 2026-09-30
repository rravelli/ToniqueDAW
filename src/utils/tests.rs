use crate::utils::display_name;

#[test]
fn test_parse_name() {
    let parsed = display_name("# This is a test n°#", 3);
    assert_eq!(parsed, "4 This is a test n°4")
}
