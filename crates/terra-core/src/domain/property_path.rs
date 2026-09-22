//! Property addressing only; entity/branch slugs are not paths.
//! Preserve empty legacy segments. This is representation, not validation.
const SEPARATOR: char = '.';

pub fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split(SEPARATOR)
}
pub fn depth(path: &str) -> usize {
    segments(path).count()
}
pub fn prefix(path: &str, count: usize) -> String {
    segments(path)
        .take(count)
        .collect::<Vec<_>>()
        .join(&SEPARATOR.to_string())
}
pub fn relative<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if path == root {
        Some("")
    } else {
        path.strip_prefix(root)?.strip_prefix(SEPARATOR)
    }
}
pub fn contains(path: &str, root: &str) -> bool {
    relative(path, root).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_and_legacy_empty_segments() {
        assert_eq!(
            segments(".a..b.").collect::<Vec<_>>(),
            ["", "a", "", "b", ""]
        );
        assert_eq!(depth(""), 1);
        assert_eq!(prefix("a..b", 2), "a.");
        assert_eq!(relative("a.b", "a"), Some("b"));
        assert_eq!(relative("a", "a"), Some(""));
        assert!(!contains("ab.c", "a"));
        assert!(contains("a..b", "a."));
    }
}
