use crate::{
    git::{State, View, argument, output, relative},
    git_diff::{Kind, Review, Row, parse_name_status, parse_patch},
};
use anyhow::{Result, ensure};
use std::{path::Path, sync::atomic::AtomicBool};

pub fn load(state: &State, view: &View, cancel: &AtomicBool) -> Result<Review> {
    let Some(root) = &state.root else {
        return Ok(Review::default());
    };
    let run = |args: &[&str]| output(root, args, cancel);
    let mut result = Review::default();
    let mut path = String::new();
    match view {
        View::Home => return Ok(result),
        View::Working(file) | View::File(file, _) => {
            path.clone_from(file);
            result.left_title = "HEAD / Repository version".into();
            result.right_title = if matches!(view, View::File(_, true)) {
                "Index"
            } else {
                "Working tree"
            }
            .into();
            if state.files.iter().any(|f| f.path == path && f.index == '?') {
                return untracked(root, &path);
            }
            let mut args = vec![
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--unified=4",
                "--find-renames",
            ];
            if matches!(view, View::File(_, true)) {
                args.push("--cached");
            }
            if run(&["rev-parse", "--verify", "HEAD"]).is_ok() {
                args.push("HEAD");
            } else if !matches!(view, View::File(_, true)) {
                args.push("--cached");
            }
            args.extend(["--", relative(&path)?]);
            if let Some(f) = state
                .files
                .iter()
                .find(|f| f.path == path && !f.old.is_empty())
            {
                args.push(&f.old);
            }
            result.unified = run(&args)?;
        }
        View::Commit(hash) | View::CommitFile(hash, _) => {
            argument(hash)?;
            result.hash.clone_from(hash);
            result.summary = run(&[
                "show",
                "-s",
                "--format=%B%n%an <%ae>%n%ad%n%H",
                "--date=iso-local",
                hash,
            ])?;
            let parent = run(&["rev-parse", "--verify", &format!("{hash}^1")]).ok();
            let files = if let Some(parent) = &parent {
                run(&[
                    "diff",
                    "--name-status",
                    "-z",
                    "--find-renames",
                    parent.trim(),
                    hash,
                    "--",
                ])?
            } else {
                run(&[
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "-r",
                    "--name-status",
                    "-z",
                    "--find-renames",
                    hash,
                    "--",
                ])?
            };
            result.files = parse_name_status(&files);
            if let View::CommitFile(_, file) = view {
                path.clone_from(file);
            } else if let Some(file) = result.files.first() {
                path.clone_from(&file.path);
            }
            result.left_title = format!("Parent of {}", &hash[..hash.len().min(8)]);
            result.right_title = hash[..hash.len().min(8)].into();
            if !path.is_empty() {
                let mut args = vec![
                    "show",
                    "--format=",
                    "--root",
                    "--first-parent",
                    "--diff-merges=first-parent",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=4",
                    "--find-renames",
                    hash,
                    "--",
                    relative(&path)?,
                ];
                if let Some(f) = result
                    .files
                    .iter()
                    .find(|f| f.path == path && !f.old.is_empty())
                {
                    args.push(&f.old);
                }
                result.unified = run(&args)?;
            }
        }
        View::Compare(branch, file) => {
            argument(branch)?;
            result.files = parse_name_status(&run(&[
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                branch,
                "--",
            ])?);
            path.clone_from(file);
            if path.is_empty() {
                if let Some(f) = result.files.first() {
                    path.clone_from(&f.path);
                }
            }
            result.left_title.clone_from(branch);
            result.right_title = "Working tree".into();
            result.summary = format!("Changes between {branch} and the working tree");
            if !path.is_empty() {
                let mut args = vec![
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=4",
                    "--find-renames",
                    branch,
                    "--",
                    relative(&path)?,
                ];
                if let Some(f) = result
                    .files
                    .iter()
                    .find(|f| f.path == path && !f.old.is_empty())
                {
                    args.push(&f.old);
                }
                result.unified = run(&args)?;
            }
        }
        View::CompareRefs(left, right, file) => {
            argument(left)?;
            argument(right)?;
            result.files = parse_name_status(&run(&[
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                left,
                right,
                "--",
            ])?);
            path.clone_from(file);
            if path.is_empty() {
                if let Some(f) = result.files.first() {
                    path.clone_from(&f.path);
                }
            }
            result.left_title.clone_from(left);
            result.right_title.clone_from(right);
            result.summary = format!("Changes between {left} and {right}");
            if !path.is_empty() {
                let mut args = vec![
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--unified=4",
                    "--find-renames",
                    left,
                    right,
                    "--",
                    relative(&path)?,
                ];
                if let Some(f) = result
                    .files
                    .iter()
                    .find(|f| f.path == path && !f.old.is_empty())
                {
                    args.push(&f.old);
                }
                result.unified = run(&args)?;
            }
        }
    }
    result.path = path;
    result.rows = parse_patch(&result.unified);
    result.additions = result
        .rows
        .iter()
        .filter(|r| matches!(r.kind, Kind::Added | Kind::Modified))
        .count();
    result.deletions = result
        .rows
        .iter()
        .filter(|r| matches!(r.kind, Kind::Deleted | Kind::Modified))
        .count();
    if result.rows.is_empty() {
        result.notice = if result.unified.contains("Binary files") {
            "Binary files differ"
        } else if result.files.is_empty() && matches!(view, View::Compare(_, _)) {
            "No differences"
        } else if result.unified.contains("rename from") {
            "File renamed; contents are unchanged"
        } else if result.unified.contains("mode ") {
            "File mode changed; contents are unchanged"
        } else {
            "No text differences"
        }
        .into();
    }
    if result.unified.len() >= 4 * 1024 * 1024 {
        result.notice = "Diff preview limited to 4 MiB".into();
    }
    Ok(result)
}

fn untracked(root: &Path, file: &str) -> Result<Review> {
    relative(file)?;
    let canonical = root.join(file).canonicalize()?;
    ensure!(
        canonical.starts_with(root.canonicalize()?),
        "File is outside the repository"
    );
    let mut result = Review {
        path: file.into(),
        left_title: "File does not exist in HEAD".into(),
        right_title: "Working tree — New file".into(),
        ..Default::default()
    };
    let metadata = std::fs::metadata(&canonical)?;
    if metadata.len() > 4 * 1024 * 1024 {
        result.notice = "File preview limited to 4 MiB".into();
        return Ok(result);
    }
    let bytes = std::fs::read(canonical)?;
    if bytes.contains(&0) {
        result.notice = "New binary file".into();
        return Ok(result);
    }
    let text = String::from_utf8_lossy(&bytes);
    result.rows = text
        .lines()
        .enumerate()
        .map(|(i, line)| Row {
            right_number: Some(i + 1),
            right: line.into(),
            kind: Kind::Added,
            ..Default::default()
        })
        .collect();
    result.additions = result.rows.len();
    result.unified = format!(
        "+++ {file}\n{}",
        text.lines().map(|l| format!("+{l}\n")).collect::<String>()
    );
    Ok(result)
}
