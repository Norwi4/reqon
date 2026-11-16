//! Bruno's dictionary syntax: quoted keys, disabled pairs, annotations and multiline values.
use crate::model::Pair;

fn outdent(value: &str) -> String {
    let value = value
        .trim_matches('\n')
        .trim_end_matches(['\r', ' ', '\t', '\n']);
    let indent = value
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    value
        .lines()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                &l[indent.min(l.len())..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub(crate) fn split_pair(line: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if c == '"' && !escaped {
            quoted = !quoted;
        }
        if c == ':' && !quoted {
            return Some((&line[..i], line[i + 1..].trim()));
        }
        escaped = c == '\\' && !escaped;
    }
    None
}
fn multiline<'a>(start: &str, lines: &mut impl Iterator<Item = &'a str>) -> String {
    if let Some((value, _)) = start.split_once("'''") {
        return value.to_owned();
    }
    let mut value = String::from(start);
    for line in lines {
        value.push('\n');
        if let Some((last, _)) = line.split_once("'''") {
            value.push_str(last);
            break;
        }
        value.push_str(line);
    }
    outdent(&value)
}
pub fn pairs(block: &str) -> Vec<Pair> {
    let mut output = Vec::new();
    let mut lines = block.lines();
    let mut pending = Pair::default();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@description(") {
            pending.description = if let Some(rest) = rest.strip_prefix("'''") {
                multiline(rest, &mut lines)
            } else {
                let v = rest.trim_end_matches(')');
                serde_json::from_str::<String>(v)
                    .unwrap_or_else(|_| v.trim_matches('\'').to_owned())
            };
            continue;
        }
        if line.starts_with('@') && !line.contains(':') {
            let annotation = line.strip_prefix('@').unwrap();
            if ["string", "number", "boolean", "object", "array", "null"].contains(&annotation) {
                pending.kind = annotation.to_owned();
            } else {
                pending.annotations.push(line.to_owned());
            }
            continue;
        }
        let Some((key, value)) = split_pair(line) else {
            continue;
        };
        pending.enabled = !key.starts_with('~');
        let key = key.trim_start_matches('~').trim();
        pending.key = if key.starts_with('"') {
            serde_json::from_str(key).unwrap_or_else(|_| key.trim_matches('"').to_owned())
        } else {
            key.to_owned()
        };
        pending.value = if let Some(rest) = value.strip_prefix("'''") {
            multiline(rest, &mut lines)
        } else {
            value.to_owned()
        };
        output.push(std::mem::take(&mut pending));
    }
    output
}
pub fn rows_text(rows: &[Pair]) -> String {
    rows.iter()
        .filter(|p| !p.key.is_empty())
        .map(|p| {
            let mut out = String::new();
            if !p.description.is_empty() {
                out.push_str(&format!(
                    "  @description({})\n",
                    serde_json::to_string(&p.description).unwrap()
                ));
            }
            if !p.kind.is_empty() {
                out.push_str(&format!("  @{}\n", p.kind));
            }
            for a in &p.annotations {
                out.push_str("  ");
                out.push_str(a);
                out.push('\n');
            }
            let key = if p
                .key
                .chars()
                .any(|c| c.is_whitespace() || [':', '"', '~'].contains(&c))
            {
                serde_json::to_string(&p.key).unwrap()
            } else {
                p.key.clone()
            };
            out.push_str(&format!("  {}{}: ", if p.enabled { "" } else { "~" }, key));
            if p.secret {
                // Secret values live in the encrypted local workspace store.
            } else if p.value.contains('\n') {
                out.push_str("'''\n");
                for line in p.value.lines() {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("  '''");
            } else {
                out.push_str(&p.value);
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn list(block: &str, key: &str) -> Vec<String> {
    let mut lines = block.lines();
    while let Some(line) = lines.next() {
        if line.trim() == format!("{key}: [") {
            return lines
                .take_while(|line| line.trim() != "]")
                .map(|line| line.trim().trim_end_matches(',').to_owned())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    vec![]
}
/// Keep unhandled dictionary entries verbatim, including nested lists and multiline values.
pub fn unknown_properties(
    block: &str,
    known: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut output = String::new();
    let mut keep = false;
    let mut multiline = false;
    for line in block.lines() {
        if !multiline && !line.starts_with(char::is_whitespace) {
            if let Some((key, _)) = split_pair(line) {
                keep = !known.contains_key(key.trim().trim_start_matches('~').trim_matches('"'));
            }
        }
        if keep {
            output.push_str("  ");
            output.push_str(line);
            output.push('\n');
        }
        if line.matches("'''").count() % 2 == 1 {
            multiline = !multiline;
        }
    }
    output.trim_end().into()
}
/// Preserve unrecognized top-level blocks, including list sections such as tags.
pub fn unknown_sections(source: &str, known: &[&str]) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut name = String::new();
    let mut close = '}';
    for line in source.lines() {
        if name.is_empty() {
            let text = line.trim();
            if let Some(n) = text.strip_suffix('{').or_else(|| text.strip_suffix('[')) {
                name = n.trim().to_owned();
                close = if text.ends_with('[') { ']' } else { '}' };
                current.push_str(line);
                current.push('\n');
            }
        } else {
            current.push_str(line);
            current.push('\n');
            if line == close.to_string() {
                if !known.contains(&name.as_str()) {
                    output.push(std::mem::take(&mut current));
                }
                current.clear();
                name.clear();
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_multiline_and_quoted_keys_round_trip() {
        let input = "@object\npayload: '''\n  {\n    \"a\": 1\n  }\n'''\n@description(\"Line one\\nLine two\")\n~\"X: Unicode имя\": yes";
        let rows = pairs(input);
        assert_eq!(rows[0].value, "{\n  \"a\": 1\n}");
        assert_eq!(rows[0].kind, "object");
        assert_eq!(rows[1].key, "X: Unicode имя");
        assert!(!rows[1].enabled);
        assert_eq!(rows[1].description, "Line one\nLine two");
        assert_eq!(pairs(&rows_text(&rows)), rows);
    }
    #[test]
    fn unknown_lists_and_examples_are_retained() {
        let s = "tags [\n  regression\n]\n\nexample {\n  name: test\n}\n";
        let unknown = unknown_sections(s, &["meta"]);
        assert_eq!(unknown.len(), 2);
        assert!(unknown[0].starts_with("tags ["));
    }
}
