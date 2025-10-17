pub fn parse_name(name: &str, index: usize) -> String {
    name.replace("#", &format!("{}", index + 1))
}
