use crate::git::{argument, output, output_with_index, relative};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

#[derive(Clone, Default)]
pub struct Selection {
    pub files: Vec<String>,
    pub message: String,
    pub amend: bool,
    pub push: Option<String>,
}

#[derive(Clone)]
pub enum BranchAction {
    CheckoutRemote(String),
    CheckoutRemoteRebase {
        branch: String,
        onto: String,
    },
    PullBranch {
        remote: String,
        branch: String,
        rebase: bool,
    },
    Create {
        name: String,
        base: String,
    },
    Rename {
        old: String,
        new: String,
    },
    Merge(String),
    Rebase(String),
    CheckoutRebase {
        branch: String,
        onto: String,
    },
    CheckoutUpdate(String),
    Update(String),
    Push {
        branch: String,
        remote: String,
    },
    DeleteRemote {
        remote: String,
        branch: String,
    },
    Worktree {
        base: String,
        branch: String,
        destination: PathBuf,
    },
    Abort(String),
    Continue(String),
}

struct TemporaryIndex(PathBuf);
impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        // Only our uniquely named files, never the repository's index.
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(PathBuf::from(format!("{}.lock", self.0.display())));
    }
}

pub fn commit_selected(root: &Path, selection: &Selection, cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !selection.message.trim().is_empty(),
        "Enter a commit message"
    );
    ensure!(
        !selection.files.is_empty() || selection.amend,
        "Select files to commit"
    );
    let run = |args: &[&str]| output(root, args, cancel);
    let status = run(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    let files = crate::git::parse_status(&status);
    ensure!(
        !files.iter().any(|f| f.conflict()),
        "Resolve conflicts and mark the files as resolved before committing"
    );
    let dir = PathBuf::from(run(&["rev-parse", "--absolute-git-dir"])?.trim());
    ensure!(
        !dir.join("MERGE_HEAD").exists()
            && !dir.join("rebase-merge").exists()
            && !dir.join("rebase-apply").exists(),
        "Use Continue to finish the current merge or rebase"
    );
    let mut selected = std::collections::BTreeSet::new();
    for path in &selection.files {
        relative(path)?;
        let file = files.iter().find(|f| &f.path == path).with_context(|| {
            format!("The file is no longer changed: {path}. Refresh the file list.")
        })?;
        selected.insert(file.path.clone());
        if !file.old.is_empty() {
            selected.insert(file.old.clone());
        }
    }
    let index = TemporaryIndex(dir.join(format!("reqon-commit-index-{}", crate::model::id())));
    let temp = |args: &[&str]| output_with_index(root, args, cancel, Some(&index.0));
    if run(&["rev-parse", "--verify", "HEAD"]).is_ok() {
        temp(&["read-tree", "HEAD"])?;
    } else {
        ensure!(!selection.amend, "There is no commit to amend");
        temp(&["read-tree", "--empty"])?;
    }
    if !selected.is_empty() {
        let mut args = vec!["add", "--all", "--"];
        args.extend(selected.iter().map(String::as_str));
        temp(&args)?;
    }
    let changes = temp(&["diff", "--cached", "--name-only", "-z"])?;
    ensure!(
        selection.amend || !changes.is_empty(),
        "The selected files have no changes to commit"
    );
    let mut args = vec!["commit", "-m", selection.message.trim()];
    if selection.amend {
        args.push("--amend");
    }
    temp(&args)?;
    // Reconcile only selected paths. Unselected staged files retain their exact
    // index contents, and failed hooks never alter the user's index at all.
    if !selected.is_empty() {
        let mut args = vec!["reset", "--quiet", "HEAD", "--"];
        args.extend(selected.iter().map(String::as_str));
        run(&args)
            .context("Commit created, but the selected index entries could not be refreshed")?;
    }
    if let Some(remote) = &selection.push {
        push(root, remote, None, cancel).context(
            "Commit created successfully, but push failed. The commit is kept locally; retry Push",
        )?;
    }
    Ok(())
}

pub fn push(root: &Path, remote: &str, branch: Option<&str>, cancel: &AtomicBool) -> Result<()> {
    let run = |args: &[&str]| output(root, args, cancel);
    let branch = if let Some(branch) = branch {
        branch.to_owned()
    } else {
        run(&["symbolic-ref", "--short", "HEAD"])?.trim().to_owned()
    };
    argument(&branch)?;
    argument(remote)?;
    let tracked_remote = run(&["config", &format!("branch.{branch}.remote")]).unwrap_or_default();
    let target = if tracked_remote.trim() == remote {
        run(&["config", &format!("branch.{branch}.merge")])
            .unwrap_or_else(|_| format!("refs/heads/{branch}"))
            .trim()
            .to_owned()
    } else {
        format!("refs/heads/{branch}")
    };
    ensure!(target.starts_with("refs/heads/"), "Invalid upstream branch");
    run(&[
        "push",
        "--set-upstream",
        remote,
        &format!("refs/heads/{branch}:{target}"),
    ])?;
    Ok(())
}

fn update(root: &Path, branch: &str, cancel: &AtomicBool) -> Result<()> {
    argument(branch)?;
    let run = |args: &[&str]| output(root, args, cancel);
    let remote = run(&["config", &format!("branch.{branch}.remote")])
        .context("This branch has no tracked remote. Push it first or choose an upstream.")?;
    let merge = run(&["config", &format!("branch.{branch}.merge")])?;
    let current = run(&["symbolic-ref", "--short", "HEAD"])?;
    if current.trim() == branch {
        run(&[
            "pull",
            "--ff-only",
            argument(remote.trim())?,
            argument(merge.trim())?,
        ])?;
    } else {
        run(&[
            "fetch",
            argument(remote.trim())?,
            &format!("{}:refs/heads/{branch}", merge.trim()),
        ])?;
    }
    Ok(())
}

pub fn branch_action(root: &Path, action: BranchAction, cancel: &AtomicBool) -> Result<()> {
    let run = |args: &[&str]| output(root, args, cancel);
    match action {
        BranchAction::CheckoutRemoteRebase { branch, onto } => {
            branch_action(root, BranchAction::CheckoutRemote(branch), cancel)?;
            run(&["rebase", argument(&onto)?])?;
        }
        BranchAction::PullBranch {
            remote,
            branch,
            rebase,
        } => {
            run(&[
                "pull",
                if rebase { "--rebase" } else { "--no-rebase" },
                "--no-edit",
                argument(&remote)?,
                argument(&branch)?,
            ])?;
        }
        BranchAction::CheckoutRemote(name) => {
            argument(&name)?;
            let (_, local) = name.split_once('/').context("Invalid remote branch")?;
            if run(&["show-ref", "--verify", &format!("refs/heads/{local}")]).is_ok() {
                let upstream = run(&[
                    "for-each-ref",
                    "--format=%(upstream:short)",
                    &format!("refs/heads/{local}"),
                ])?;
                ensure!(
                    upstream.trim() == name,
                    "A local branch named {local} already exists with a different upstream. Use New Branch to choose another name."
                );
                run(&["switch", local])?;
            } else {
                run(&["switch", "--track", "-c", local, &name])?;
            }
        }
        BranchAction::Create { name, base } => {
            run(&["check-ref-format", "--branch", argument(&name)?])?;
            run(&["switch", "-c", &name, argument(&base)?])?;
        }
        BranchAction::Rename { old, new } => {
            run(&["check-ref-format", "--branch", argument(&new)?])?;
            run(&["branch", "-m", argument(&old)?, &new])?;
        }
        BranchAction::Merge(branch) => {
            run(&["merge", "--no-edit", argument(&branch)?])?;
        }
        BranchAction::Rebase(branch) => {
            run(&["rebase", argument(&branch)?])?;
        }
        BranchAction::CheckoutRebase { branch, onto } => {
            run(&["switch", argument(&branch)?])?;
            run(&["rebase", argument(&onto)?])?;
        }
        BranchAction::CheckoutUpdate(branch) => {
            run(&["switch", argument(&branch)?])?;
            update(root, &branch, cancel)?;
        }
        BranchAction::Update(branch) => update(root, &branch, cancel)?,
        BranchAction::Push { branch, remote } => push(root, &remote, Some(&branch), cancel)?,
        BranchAction::DeleteRemote { remote, branch } => {
            run(&["push", argument(&remote)?, "--delete", argument(&branch)?])?;
        }
        BranchAction::Worktree {
            base,
            branch,
            destination,
        } => {
            ensure!(
                !destination.exists(),
                "Choose a new folder for the worktree"
            );
            run(&["check-ref-format", "--branch", argument(&branch)?])?;
            let destination = destination.to_str().context("Invalid worktree path")?;
            run(&[
                "worktree",
                "add",
                "-b",
                &branch,
                "--",
                destination,
                argument(&base)?,
            ])?;
        }
        BranchAction::Abort(operation) => finish(root, &operation, true, cancel)?,
        BranchAction::Continue(operation) => finish(root, &operation, false, cancel)?,
    }
    Ok(())
}

fn finish(root: &Path, operation: &str, abort: bool, cancel: &AtomicBool) -> Result<()> {
    ensure!(
        matches!(operation, "merge" | "rebase" | "cherry-pick"),
        "Unknown Git operation"
    );
    output(
        root,
        &[operation, if abort { "--abort" } else { "--continue" }],
        cancel,
    )?;
    Ok(())
}
