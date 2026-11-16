//! Stable file-tab identifiers, also used by workspace migration.
pub fn file_id(collection: &str, relative: &str) -> String {
    format!("__file__{collection}|{relative}")
}
pub fn parse_id(id: &str) -> Option<(&str, &str)> {
    id.strip_prefix("__file__")?.split_once('|')
}
