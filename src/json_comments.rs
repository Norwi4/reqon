//! JSONC editing without changing string values, numeric tokens or saved drafts.
use anyhow::{Result, bail};
use serde::Deserialize;
use std::borrow::Cow;

/// Remove comments and trailing commas only outside strings. Keeping byte and
/// newline positions makes serde's diagnostics refer to the original document.
pub fn normalize(source: &str) -> Result<Cow<'_, str>> {
    let b = source.as_bytes();
    let mut clean: Option<Vec<u8>> = None;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i = (i + 2).min(b.len());
                } else if b[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if b[i..].starts_with(b"//") || b[i..].starts_with(b"/*") {
            let start = i;
            if b[i + 1] == b'/' {
                while i < b.len() && !matches!(b[i], b'\r' | b'\n') {
                    i += 1;
                }
            } else {
                i += 2;
                while i + 1 < b.len() && &b[i..i + 2] != b"*/" {
                    i += 1;
                }
                if i + 1 >= b.len() {
                    let line = b[..start].iter().filter(|&&c| c == b'\n').count() + 1;
                    let column = start
                        - b[..start]
                            .iter()
                            .rposition(|&c| c == b'\n')
                            .map_or(0, |p| p + 1)
                        + 1;
                    bail!("Unterminated block comment at line {line} column {column}");
                }
                i += 2;
            }
            for c in &mut clean.get_or_insert_with(|| b.to_vec())[start..i] {
                if !matches!(c, b'\r' | b'\n') {
                    *c = b' ';
                }
            }
        } else {
            i += 1;
        }
    }
    let mut i = 0;
    while i < b.len() {
        let bytes = clean.as_deref().unwrap_or(b);
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if bytes[i] == b',' {
            let mut next = i + 1;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if next < bytes.len() && matches!(bytes[next], b'}' | b']') {
                clean.get_or_insert_with(|| b.to_vec())[i] = b' ';
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    Ok(match clean {
        Some(bytes) => Cow::Owned(String::from_utf8(bytes)?),
        None => Cow::Borrowed(source),
    })
}

pub fn validate(source: &str) -> Result<Cow<'_, str>> {
    let clean = normalize(source)?;
    let mut de = serde_json::Deserializer::from_str(&clean);
    serde::de::IgnoredAny::deserialize(&mut de)?;
    de.end()?;
    Ok(clean)
}
pub fn value(source: &str) -> Result<serde_json::Value> {
    Ok(serde_json::from_str(&normalize(source)?)?)
}

fn tokens(source: &str) -> Vec<(usize, &str)> {
    let b = source.as_bytes();
    let mut result = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i = (i + 2).min(b.len());
                } else if b[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if b[i..].starts_with(b"//") {
            while i < b.len() && !matches!(b[i], b'\r' | b'\n') {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            i = source[i + 2..]
                .find("*/")
                .map_or(b.len(), |end| i + end + 4);
        } else if b"{}[],:".contains(&b[i]) {
            i += 1;
        } else {
            while i < b.len() && !b[i].is_ascii_whitespace() && !b"/{}[],:".contains(&b[i]) {
                i += 1;
            }
            if i == start {
                i += 1;
            }
        }
        result.push((start, &source[start..i]));
    }
    result
}
pub fn format(source: &str) -> Result<String> {
    if source.trim().is_empty() {
        return Ok(source.into());
    }
    validate(source)?;
    let tokens = tokens(source);
    let mut out = String::with_capacity(source.len());
    let mut line = String::new();
    let mut depth = 0usize;
    fn flush(out: &mut String, line: &mut String, depth: usize) {
        if !line.trim().is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
            out.push_str(line.trim_end());
        }
        line.clear();
    }
    for (index, &(start, token)) in tokens.iter().enumerate() {
        let prev = index.checked_sub(1).map(|p| tokens[p]);
        let next = tokens.get(index + 1).copied();
        let is_comment = |s: &str| s.starts_with("//") || s.starts_with("/*");
        if is_comment(token) {
            let gap = prev.map_or("\n", |(p, s)| &source[p + s.len()..start]);
            if gap.contains(['\r', '\n']) {
                flush(&mut out, &mut line, depth);
            }
            if !line.is_empty() && !line.ends_with(' ') {
                line.push(' ');
            }
            line.push_str(token);
            if token.starts_with("//")
                || token.contains(['\r', '\n'])
                || prev.is_some_and(|(_, s)| s == ",")
            {
                flush(&mut out, &mut line, depth);
            } else {
                line.push(' ');
            }
        } else {
            match token {
                "{" | "[" => {
                    line.push_str(token);
                    if !next.is_some_and(|(_, s)| matches!(s, "}" | "]")) {
                        flush(&mut out, &mut line, depth);
                    }
                    depth += 1;
                }
                "}" | "]" => {
                    if !prev.is_some_and(|(_, s)| matches!(s, "{" | "[")) {
                        flush(&mut out, &mut line, depth);
                    }
                    depth = depth.saturating_sub(1);
                    line.push_str(token);
                }
                "," => {
                    line.push(',');
                    let inline = next.is_some_and(|(n, s)| {
                        is_comment(s) && !source[start + 1..n].contains(['\r', '\n'])
                    });
                    if !inline {
                        flush(&mut out, &mut line, depth);
                    }
                }
                ":" => line.push_str(": "),
                _ => line.push_str(token),
            }
        }
    }
    flush(&mut out, &mut line, depth);
    Ok(out)
}

/// Selection indices are character positions, matching egui's text cursor.
pub fn toggle_lines(
    source: &str,
    selection: std::ops::Range<usize>,
) -> (String, std::ops::Range<usize>) {
    let chars: Vec<char> = source.chars().collect();
    let a = selection.start.min(chars.len());
    let b = selection.end.min(chars.len());
    let start = chars[..a]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);
    let last = if b > a && chars.get(b.wrapping_sub(1)) == Some(&'\n') {
        b - 1
    } else {
        b
    };
    let end = chars[last..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |i| last + i);
    let part: String = chars[start..end].iter().collect();
    let uncomment = part
        .lines()
        .filter(|l| !l.trim().is_empty())
        .all(|l| l.trim_start().starts_with("//"));
    let replacement = part
        .split('\n')
        .map(|line| {
            let trimmed = line.trim_start_matches([' ', '\t']);
            if trimmed.is_empty() {
                return line.to_owned();
            }
            let indent = &line[..line.len() - trimmed.len()];
            if uncomment {
                format!(
                    "{indent}{}",
                    trimmed
                        .strip_prefix("//")
                        .unwrap_or(trimmed)
                        .strip_prefix(' ')
                        .unwrap_or(trimmed.strip_prefix("//").unwrap_or(trimmed))
                )
            } else {
                format!("{indent}// {trimmed}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut result: String = chars[..start].iter().collect();
    result.push_str(&replacement);
    result.extend(&chars[end..]);
    let range = start..start + replacement.chars().count();
    (result, range)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_survive_collection_formats_and_are_readable_in_scripts() {
        use crate::{model::*, storage};
        let body =
            "{\n  // closing braces } are comment text\n  \"text\": \"Привет\", /* retained */\n}";
        let request = Request {
            name: "Comments".into(),
            method: "POST".into(),
            body_kind: BodyKind::Json,
            body: body.into(),
            collection: "comments".into(),
            ..Default::default()
        };
        assert_eq!(
            storage::parse_bru(&storage::to_bru(&request)).unwrap().body,
            body
        );
        let dir = tempfile::tempdir().unwrap();
        let yaml = dir.path().join("comments.yml");
        std::fs::write(&yaml, crate::yaml::serialize(&request).unwrap()).unwrap();
        assert_eq!(crate::yaml::read_request(&yaml).unwrap().body, body);
        let collection = Collection {
            id: "comments".into(),
            name: "Comments".into(),
            path: dir.path().into(),
            environments: vec![],
            vars: vec![],
            folders: vec![],
        };
        let exported = storage::export_postman(&collection, &[request.clone()]).unwrap();
        let (_, requests) =
            storage::import_postman(&exported, &dir.path().join("imported")).unwrap();
        assert_eq!(requests[0].body, body);
        let result = crate::scripts::run(
            "test('commented JSON',()=>expect(req.getBody().text).to.equal('Привет'));",
            &request,
            &Reply::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(result.tests.iter().all(|test| test.passed));
        assert_eq!(result.request.body, body);
    }
    #[cfg(feature = "desktop")]
    #[test]
    fn editor_comment_shortcut_preserves_selection_and_undo() {
        use eframe::egui::{
            self,
            text::{CCursor, CCursorRange},
        };
        let ctx = egui::Context::default();
        crate::theme::fonts(&ctx);
        let original = "{\n  \"text\": \"Привет\"\n}";
        let mut text = original.to_owned();
        let t = crate::theme::Theme::new(true);
        t.apply(&ctx);
        let mut id = egui::Id::NULL;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            id = ui.scope_id().with("source-editor");
            ui.memory_mut(|m| m.request_focus(id));
            crate::widgets::editor(ui, &mut text, false, t);
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(0),
                CCursor::new(text.chars().count()),
            )));
            state.store(&ctx, id);
        });
        output.textures_delta.clear();
        for (key, physical) in [
            (egui::Key::Period, Some(egui::Key::Slash)),
            (egui::Key::Z, Some(egui::Key::Z)),
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: physical,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
                    }],
                    ..Default::default()
                },
                |ui| {
                    crate::widgets::editor(ui, &mut text, false, t);
                },
            );
            output.textures_delta.clear();
            if key == egui::Key::Period {
                assert!(text.starts_with("// {"), "{text}");
            } else {
                assert_eq!(text, original);
            }
        }
    }
    #[test]
    fn comments_trailing_commas_strings_precision_and_format_roundtrip() {
        for text in [
            "// intro\r\n{\n\"url\":\"https://x/*keep*/\", // note\n\"v\":123456789012345678901234567890,/* рус */\n}",
            "{\"a\":true// { text , :\n}",
            "{/*first*/\"name\"/*key*/:/*value*/\"https://x/\",\"items\":[1,2,]//last\n}",
            "[//start\n{\"empty\":{/*nested*/},\"values\":[/*nothing*/]},/*tail*/]",
            "// start\n{\"x\":1, /* multiple\n lines */ \"y\":2}",
        ] {
            let formatted = format(text).unwrap();
            assert_eq!(value(text).unwrap(), value(&formatted).unwrap());
            assert_eq!(format(&formatted).unwrap(), formatted);
            let comments = |s: &str| {
                tokens(s)
                    .into_iter()
                    .filter(|(_, s)| s.starts_with("//") || s.starts_with("/*"))
                    .map(|(_, s)| s.to_owned())
                    .collect::<Vec<_>>()
            };
            assert_eq!(comments(text), comments(&formatted));
        }
        assert!(
            format("{/* missing")
                .unwrap_err()
                .to_string()
                .contains("line 1 column 2")
        );
        assert!(
            validate("{\n// note\n\"x\":???\n}")
                .unwrap_err()
                .to_string()
                .contains("line 3")
        );
        assert!(format("{unquoted: 1}").is_err());
        let text = "  \"русский\": true,\n  \"url\": \"//keep\"";
        let (commented, range) = toggle_lines(text, 0..text.chars().count());
        assert!(commented.starts_with("  // "));
        assert_eq!(toggle_lines(&commented, range).0, text);
    }
}
