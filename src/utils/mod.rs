#[cfg(test)]
mod tests;

pub fn display_name(name: &str, index: usize) -> String {
    name.replace("#", &format!("{}", index + 1))
}
