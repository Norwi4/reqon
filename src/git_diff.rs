//! Aligned, virtualized two-column diffs. Git supplies the hunks; no quadratic
//! text diff runs on the UI thread, even for large files.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Context,
    Added,
    Deleted,
    Modified,
    Separator,
}

#[derive(Clone, Default, Debug)]
pub struct Row {
    pub left_number: Option<usize>,
    pub right_number: Option<usize>,
    pub left: String,
    pub right: String,
    pub kind: Kind,
}

#[derive(Clone, Default, Debug)]
pub struct ChangedFile {
    pub path: String,
    pub old: String,
    pub status: char,
}

#[derive(Clone, Default)]
pub struct Review {
    pub path: String,
    pub hash: String,
    pub summary: String,
    pub left_title: String,
    pub right_title: String,
    pub files: Vec<ChangedFile>,
    pub rows: Vec<Row>,
    pub unified: String,
    pub notice: String,
    pub additions: usize,
    pub deletions: usize,
}

pub fn parse_name_status(text: &str) -> Vec<ChangedFile> {
    let mut parts = text.split('\0');
    let mut result = Vec::new();
    while let Some(status) = parts.next().filter(|s| !s.is_empty()) {
        let Some(first) = parts.next() else { break };
        let code = status.chars().next().unwrap_or('M');
        let (old, path) = if matches!(code, 'R' | 'C') {
            let Some(new) = parts.next() else { break };
            (first.to_owned(), new.to_owned())
        } else {
            (String::new(), first.to_owned())
        };
        result.push(ChangedFile {
            path,
            old,
            status: code,
        });
    }
    result
}

pub fn parse_patch(patch: &str) -> Vec<Row> {
    fn flush(
        rows: &mut Vec<Row>,
        removed: &mut Vec<(usize, String)>,
        added: &mut Vec<(usize, String)>,
    ) {
        let mut left = std::mem::take(removed).into_iter();
        let mut right = std::mem::take(added).into_iter();
        loop {
            let l = left.next();
            let r = right.next();
            if l.is_none() && r.is_none() {
                break;
            }
            let kind = match (&l, &r) {
                (Some(_), Some(_)) => Kind::Modified,
                (Some(_), None) => Kind::Deleted,
                _ => Kind::Added,
            };
            let (left_number, left) = l.map(|(n, s)| (Some(n), s)).unwrap_or_default();
            let (right_number, right) = r.map(|(n, s)| (Some(n), s)).unwrap_or_default();
            rows.push(Row {
                left_number,
                right_number,
                left,
                right,
                kind,
            });
        }
    }
    let mut rows = Vec::new();
    let (mut old, mut new) = (0, 0);
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let mut in_hunk = false;
    for line in patch.lines() {
        if line.starts_with("@@ ") {
            flush(&mut rows, &mut removed, &mut added);
            let ranges: Vec<_> = line.split_whitespace().collect();
            let number = |s: &str| {
                s.get(1..)
                    .and_then(|s| s.split(',').next())
                    .and_then(|n| n.parse::<usize>().ok())
            };
            let Some((a, b)) = ranges
                .get(1)
                .and_then(|a| number(a))
                .zip(ranges.get(2).and_then(|b| number(b)))
            else {
                continue;
            };
            old = a;
            new = b;
            in_hunk = true;
            rows.push(Row {
                left: line.into(),
                right: line.into(),
                kind: Kind::Separator,
                ..Default::default()
            });
        } else if line.starts_with("diff --git ") {
            flush(&mut rows, &mut removed, &mut added);
            in_hunk = false;
        } else if in_hunk {
            if let Some(text) = line.strip_prefix('-') {
                removed.push((old, text.into()));
                old += 1;
            } else if let Some(text) = line.strip_prefix('+') {
                added.push((new, text.into()));
                new += 1;
            } else if let Some(text) = line.strip_prefix(' ') {
                flush(&mut rows, &mut removed, &mut added);
                rows.push(Row {
                    left_number: Some(old),
                    right_number: Some(new),
                    left: text.into(),
                    right: text.into(),
                    kind: Kind::Context,
                });
                old += 1;
                new += 1;
            }
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    rows
}

/// Byte ranges are always UTF-8 boundaries, including Cyrillic and emoji.
pub fn changed_ranges(left: &str, right: &str) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let prefix = left
        .chars()
        .zip(right.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = left[prefix..]
        .chars()
        .rev()
        .zip(right[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    (prefix..left.len() - suffix, prefix..right.len() - suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aligns_replacements_and_keeps_line_numbers_across_hunks() {
        let rows = parse_patch(
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -2,3 +2,4 @@\n same\n-old\n+new\n+extra\n tail\n@@ -20 +21 @@\n-last\n+next\n\\ No newline at end of file\n",
        );
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[2].kind, Kind::Modified);
        assert_eq!(rows[2].left_number, Some(3));
        assert_eq!(rows[3].kind, Kind::Added);
        assert_eq!(rows[3].right_number, Some(4));
        assert_eq!(rows[6].left_number, Some(20));
        assert_eq!(rows[6].right_number, Some(21));
    }
    #[test]
    fn paths_and_unicode_changes_are_not_split_on_spaces_or_multibyte_characters() {
        let files = parse_name_status("R100\0old name.req\0новое имя.req\0A\0line\nbreak.req\0");
        assert_eq!(files[0].old, "old name.req");
        assert_eq!(files[0].path, "новое имя.req");
        assert_eq!(files[1].path, "line\nbreak.req");
        let a = "Привет 🦀 конец";
        let b = "Привет Rust конец";
        let (l, r) = changed_ranges(a, b);
        assert_eq!(&a[l], "🦀");
        assert_eq!(&b[r], "Rust");
    }
}
