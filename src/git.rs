use crate::{
    app::{App, Event, LoadedCollection},
    model::Collection,
};
use anyhow::{Context, Result, ensure};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub use crate::ui_state::GitView as View;
#[derive(Clone, Default)]
pub struct File {
    pub path: String,
    pub old: String,
    pub index: char,
    pub work: char,
}
impl File {
    pub fn staged(&self) -> bool {
        self.index != ' ' && self.index != '?'
    }
    pub fn unstaged(&self) -> bool {
        self.work != ' '
    }
    pub fn conflict(&self) -> bool {
        self.index == 'U'
            || self.work == 'U'
            || matches!((self.index, self.work), ('A', 'A') | ('D', 'D'))
    }
}
#[derive(Clone, Default)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date: String,
    pub parents: Vec<String>,
    pub refs: String,
    pub email: String,
    pub timestamp: i64,
}
#[derive(Clone, Default)]
pub struct Branch {
    pub name: String,
    pub full_ref: String,
    pub hash: String,
    pub upstream: String,
    pub remote: bool,
    pub current: bool,
}
#[derive(Clone, Default)]
pub struct State {
    pub root: Option<PathBuf>,
    pub branch: String,
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<File>,
    pub branches: Vec<String>,
    pub remotes: Vec<(String, String)>,
    pub commits: Vec<Commit>,
    pub graph: Vec<crate::git_graph::Row>,
    pub refs: Vec<Branch>,
    pub review: crate::git_diff::Review,
    pub history_limit: usize,
    pub operation: String,
    pub push_remote: String,
    pub stashes: Vec<(String, String)>,
    pub name: String,
    pub email: String,
    pub diff: String,
    pub diff_lines: Vec<usize>,
    pub view: View,
    pub error: String,
    pub busy: bool,
    pub refreshing: bool,
    pub committed_message: Option<String>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub updated: Option<Instant>,
}
#[derive(Clone)]
pub enum Action {
    Refresh,
    Init,
    Stage,
    StageFile(String),
    Unstage(Option<String>),
    Commit(String),
    Preview(View),
    Fetch,
    Pull,
    Push(String),
    AddRemote(String, String),
    RemoveRemote(String),
    CreateBranch(String),
    Checkout(String),
    DeleteBranch(String),
    Stash(String),
    ApplyStash(String, bool),
    DropStash(String),
    Discard(String),
    Identity(String, String),
    CommitSelected(crate::git_ops::Selection),
    Branch(crate::git_ops::BranchAction),
    History(usize),
}
const LIMIT: u64 = 4 * 1024 * 1024;
fn provider_auth() -> &'static std::sync::Mutex<Option<(String, String)>> {
    static AUTH: std::sync::OnceLock<std::sync::Mutex<Option<(String, String)>>> =
        std::sync::OnceLock::new();
    AUTH.get_or_init(Default::default)
}
pub fn configure_provider(p: &crate::model::Preferences) {
    let next = if p.github.connected {
        p.credentials
            .get("github")
            .filter(|s| !s.is_empty())
            .map(|token| {
                let host = if p.github.endpoint.is_empty() {
                    "github.com".into()
                } else {
                    url::Url::parse(&p.github.endpoint)
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_owned))
                        .unwrap_or("github.com".into())
                };
                (host, token.clone())
            })
    } else {
        None
    };
    *provider_auth().lock().unwrap() = next;
}
pub struct CloneJob {
    pub destination: PathBuf,
    pub cancel: Arc<AtomicBool>,
    pub finished: Arc<AtomicBool>,
}
pub(crate) fn output(path: &Path, args: &[&str], cancel: &AtomicBool) -> Result<String> {
    output_with_index(path, args, cancel, None)
}
pub(crate) fn output_with_index(
    path: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    index: Option<&Path>,
) -> Result<String> {
    ensure!(!cancel.load(Ordering::Relaxed), "Git operation cancelled");
    let mut command = Command::new("git");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    if let Some((host, token)) = provider_auth().lock().unwrap().as_ref() {
        use base64::Engine as _;
        command
            .env("GIT_CONFIG_COUNT", "1")
            .env(
                "GIT_CONFIG_KEY_0",
                format!("http.https://{host}/.extraHeader"),
            )
            .env(
                "GIT_CONFIG_VALUE_0",
                format!(
                    "Authorization: Basic {}",
                    base64::prelude::BASE64_STANDARD.encode(format!("x-access-token:{token}"))
                ),
            );
    }
    command
        .arg("-C")
        .arg(path)
        .args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env(
            "GIT_LITERAL_PATHSPECS",
            if args.first() == Some(&"stash") {
                "0"
            } else {
                "1"
            },
        )
        .env("GIT_PAGER", "cat")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .context("Git is not available. Install Git for Windows and restart Reqon.")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |mut stream: Box<dyn Read + Send>| {
        let mut buffer = Vec::new();
        let mut block = [0; 16384];
        loop {
            match stream.read(&mut block) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let remaining = (LIMIT as usize).saturating_sub(buffer.len());
                    buffer.extend_from_slice(&block[..n.min(remaining)]);
                }
            }
        }
        buffer
    };
    let out = std::thread::spawn(move || read(Box::new(stdout)));
    let err = std::thread::spawn(move || read(Box::new(stderr)));
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill.exe")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .creation_flags(0x08000000)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Git operation cancelled or timed out");
        }
        if let Some(s) = child.try_wait()? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(15));
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    ensure!(
        status.success(),
        "{}",
        String::from_utf8_lossy(if stderr.is_empty() { &stdout } else { &stderr }).trim()
    );
    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    if stdout.len() == LIMIT as usize {
        text.push_str("\n… Output limited to 4 MiB");
    }
    Ok(text)
}
#[cfg(test)]
fn run(path: &Path, args: &[&str]) -> Result<String> {
    output(path, args, &AtomicBool::new(false))
}
pub(crate) fn argument(value: &str) -> Result<&str> {
    ensure!(
        !value.is_empty() && !value.starts_with('-') && !value.contains(['\0', '\n', '\r']),
        "Invalid Git argument"
    );
    Ok(value)
}
pub(crate) fn relative(value: &str) -> Result<&str> {
    ensure!(
        !value.is_empty()
            && !Path::new(value).is_absolute()
            && !Path::new(value).components().any(|c| matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )),
        "Invalid repository path"
    );
    Ok(value)
}
pub(crate) fn parse_status(text: &str) -> Vec<File> {
    let mut parts = text.split('\0');
    let mut result = vec![];
    while let Some(part) = parts.next() {
        if part.len() < 4 {
            continue;
        }
        let b = part.as_bytes();
        let index = b[0] as char;
        let work = b[1] as char;
        let old = if matches!(index, 'R' | 'C') || matches!(work, 'R' | 'C') {
            parts.next().unwrap_or_default().into()
        } else {
            String::new()
        };
        result.push(File {
            path: part[3..].into(),
            old,
            index,
            work,
        });
    }
    result
}
fn snapshot(path: &Path, view: View, cancel: &AtomicBool, history_limit: usize) -> Result<State> {
    let run = |a: &[&str]| output(path, a, cancel);
    let root = match run(&["rev-parse", "--show-toplevel"]) {
        Ok(r) => PathBuf::from(r.trim()),
        Err(e) if e.to_string().contains("not a git repository") => {
            return Ok(State {
                updated: Some(Instant::now()),
                ..Default::default()
            });
        }
        Err(e) => return Err(e),
    };
    let path = &root;
    let run = |a: &[&str]| output(path, a, cancel);
    let branch = run(&["symbolic-ref", "--short", "HEAD"])
        .unwrap_or_else(|_| run(&["rev-parse", "--short", "HEAD"]).unwrap_or_default())
        .trim()
        .to_owned();
    let status = run(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    let files = parse_status(&status);
    let upstream = run(&["rev-parse", "--abbrev-ref", "@{upstream}"])
        .unwrap_or_default()
        .trim()
        .to_owned();
    let counts =
        run(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]).unwrap_or_default();
    let mut counts = counts
        .split_whitespace()
        .filter_map(|n| n.parse::<u32>().ok());
    let ahead = counts.next().unwrap_or(0);
    let behind = counts.next().unwrap_or(0);
    let branches = run(&["branch", "--format=%(refname:short)"])
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let mut remotes: Vec<(String, String)> = vec![];
    for name in run(&["remote"])?.lines() {
        remotes.push((
            name.into(),
            run(&["remote", "get-url", name])?.trim().into(),
        ));
    }
    let refs = run(&["for-each-ref", "--format=%(refname)%09%(refname:short)%09%(objectname)%09%(upstream:short)%09%(HEAD)%09%(symref)", "refs/heads", "refs/remotes", "refs/tags"])?
        .lines().filter_map(|line| {
            let c: Vec<_> = line.split('\t').collect();
            (c.len() >= 6 && c[5].is_empty()).then(|| Branch { full_ref:c[0].into(), name:c[1].into(), hash:c[2].into(), upstream:c[3].into(), current:c[4] == "*", remote:c[0].starts_with("refs/remotes/") })
        }).collect();
    let commits: Vec<Commit> = run(&[
        "log",
        "--all",
        "--topo-order",
        &format!("-{}", history_limit),
        "--date=format:%Y-%m-%d %H:%M",
        "--decorate=short",
        "--format=%H%x1f%s%x1f%an%x1f%ad%x1f%P%x1f%D%x1f%ae%x1f%ct",
    ])
    .unwrap_or_default()
    .lines()
    .filter_map(|l| {
        let c = l.split('\x1f').collect::<Vec<_>>();
        (c.len() == 8).then(|| Commit {
            hash: c[0].into(),
            subject: c[1].into(),
            author: c[2].into(),
            date: c[3].into(),
            parents: c[4].split_whitespace().map(str::to_owned).collect(),
            refs: c[5].into(),
            email: c[6].into(),
            timestamp: c[7].parse().unwrap_or_default(),
        })
    })
    .collect();
    let stashes = run(&["stash", "list", "--format=%gd%x1f%s"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\x1f').map(|(a, b)| (a.into(), b.into())))
        .collect();
    let name = run(&["config", "user.name"])
        .unwrap_or_default()
        .trim()
        .into();
    let email = run(&["config", "user.email"])
        .unwrap_or_default()
        .trim()
        .into();
    let diff = match &view {
        View::Home
        | View::Working(_)
        | View::CommitFile(_, _)
        | View::Compare(_, _)
        | View::CompareRefs(_, _, _) => String::new(),
        View::Commit(hash) => run(&[
            "show",
            "--format=fuller",
            "--no-ext-diff",
            "--no-textconv",
            argument(hash)?,
        ])?,
        View::File(file, staged) => {
            relative(file)?;
            if files.iter().any(|f| f.path == *file && f.index == '?') {
                let p = path.join(file);
                let canonical = p.canonicalize()?;
                ensure!(
                    canonical.starts_with(path.canonicalize()?),
                    "File is outside the repository"
                );
                let data = std::fs::read(&canonical)?;
                if data.len() > LIMIT as usize {
                    "File preview limited to 4 MiB".into()
                } else if data.contains(&0) {
                    "Binary file".into()
                } else {
                    format!(
                        "+++ {file}\n{}",
                        String::from_utf8_lossy(&data)
                            .lines()
                            .map(|l| format!("+{l}\n"))
                            .collect::<String>()
                    )
                }
            } else if *staged {
                run(&[
                    "diff",
                    "--cached",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    file,
                ])?
            } else {
                run(&["diff", "--no-ext-diff", "--no-textconv", "--", file])?
            }
        }
    };
    let git_dir = run(&["rev-parse", "--absolute-git-dir"])?;
    let git_dir = PathBuf::from(git_dir.trim());
    let operation =
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            "rebase"
        } else if git_dir.join("MERGE_HEAD").exists() {
            "merge"
        } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
            "cherry-pick"
        } else {
            ""
        }
        .to_owned();
    let push_remote = run(&["config", &format!("branch.{branch}.pushRemote")])
        .or_else(|_| run(&["config", "remote.pushDefault"]))
        .or_else(|_| run(&["config", &format!("branch.{branch}.remote")]))
        .unwrap_or_else(|_| {
            remotes
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| "origin".into())
        })
        .trim()
        .to_owned();
    let mut state = State {
        root: Some(root),
        branch,
        upstream,
        ahead,
        behind,
        files,
        branches,
        remotes,
        graph: crate::git_graph::layout(&commits),
        commits,
        refs,
        history_limit,
        operation,
        push_remote,
        stashes,
        name,
        email,
        diff_lines: crate::widgets::line_offsets(&diff),
        diff,
        view,
        updated: Some(Instant::now()),
        ..Default::default()
    };
    match crate::git_review::load(&state, &state.view, cancel) {
        Ok(review) => state.review = review,
        Err(error) => state.error = format!("{error:#}"),
    }
    Ok(state)
}
fn perform(path: &Path, action: Action, cancel: &AtomicBool) -> Result<()> {
    let run = |a: &[&str]| output(path, a, cancel);
    match action {
        Action::Refresh | Action::Preview(_) | Action::History(_) => {}
        Action::CommitSelected(selection) => {
            crate::git_ops::commit_selected(path, &selection, cancel)?
        }
        Action::Branch(action) => crate::git_ops::branch_action(path, action, cancel)?,
        Action::Init => {
            run(&["init"])?;
        }
        Action::Stage => {
            run(&["add", "--all"])?;
        }
        Action::StageFile(p) => {
            run(&["add", "--", relative(&p)?])?;
        }
        Action::Unstage(p) => {
            let paths = p.as_deref().map(relative).transpose()?;
            let mut args = if run(&["rev-parse", "--verify", "HEAD"]).is_ok() {
                vec!["restore", "--staged", "--"]
            } else {
                vec!["rm", "--cached", "-r", "--"]
            };
            args.push(paths.unwrap_or("."));
            run(&args)?;
        }
        Action::Commit(message) => {
            ensure!(!message.trim().is_empty(), "Enter a commit message");
            run(&["commit", "-m", message.trim()])?;
        }
        Action::Fetch => {
            run(&["fetch", "--all", "--prune"])?;
        }
        Action::Pull => {
            run(&["pull", "--ff-only"])?;
        }
        Action::Push(remote) => {
            crate::git_ops::push(path, &remote, None, cancel)?;
        }
        Action::AddRemote(name, url) => {
            argument(&name)?;
            argument(&url)?;
            if run(&["remote", "get-url", &name]).is_ok() {
                run(&["remote", "set-url", &name, &url])?;
            } else {
                run(&["remote", "add", &name, &url])?;
            }
        }
        Action::RemoveRemote(name) => {
            run(&["remote", "remove", argument(&name)?])?;
        }
        Action::CreateBranch(name) => {
            run(&["check-ref-format", "--branch", argument(&name)?])?;
            run(&["switch", "-c", &name])?;
        }
        Action::Checkout(name) => {
            run(&["switch", argument(&name)?])?;
        }
        Action::DeleteBranch(name) => {
            run(&["branch", "-d", argument(&name)?])?;
        }
        Action::Stash(message) => {
            run(&["stash", "push", "--include-untracked", "-m", &message])?;
        }
        Action::ApplyStash(name, pop) => {
            ensure!(name.starts_with("stash@{"), "Invalid stash");
            run(&["stash", if pop { "pop" } else { "apply" }, &name])?;
        }
        Action::DropStash(name) => {
            ensure!(name.starts_with("stash@{"), "Invalid stash");
            run(&["stash", "drop", &name])?;
        }
        Action::Discard(file) => {
            relative(&file)?;
            ensure!(
                run(&["ls-files", "--error-unmatch", "--", &file]).is_ok(),
                "Untracked files must be removed in File Explorer"
            );
            run(&["restore", "--worktree", "--", &file])?;
        }
        Action::Identity(name, email) => {
            ensure!(
                !name.trim().is_empty() && !email.trim().is_empty(),
                "Enter name and email"
            );
            run(&["config", "--local", "user.name", &name])?;
            run(&["config", "--local", "user.email", &email])?;
        }
    }
    Ok(())
}
pub(crate) fn execute_cached(
    collection: &Collection,
    action: Action,
    cached: State,
    cancel: &AtomicBool,
) -> Result<State> {
    if let Action::Preview(view) = &action {
        if cached.root.is_some() {
            let mut state = cached;
            state.review = crate::git_review::load(&state, view, cancel)?;
            state.view = view.clone();
            state.diff = state.review.unified.clone();
            state.diff_lines = crate::widgets::line_offsets(&state.diff);
            state.busy = false;
            state.refreshing = false;
            state.cancel = None;
            state.committed_message = None;
            state.updated = Some(Instant::now());
            state.error.clear();
            return Ok(state);
        }
    }
    let root = output(&collection.path, &["rev-parse", "--show-toplevel"], cancel)
        .map(|r| PathBuf::from(r.trim()))
        .unwrap_or_else(|_| collection.path.clone());
    let view = if let Action::Preview(v) = &action {
        v.clone()
    } else {
        cached.view
    };
    let committed_message = match &action {
        Action::Commit(message) => Some(message.clone()),
        Action::CommitSelected(s) => Some(s.message.clone()),
        _ => None,
    };
    let old_head = committed_message
        .as_ref()
        .and_then(|_| output(&root, &["rev-parse", "HEAD"], cancel).ok());
    let history_limit = if let Action::History(limit) = &action {
        *limit
    } else {
        cached.history_limit.max(500)
    };
    let result = perform(&root, action, cancel);
    let new_head = committed_message
        .as_ref()
        .and_then(|_| output(&root, &["rev-parse", "HEAD"], &AtomicBool::new(false)).ok());
    let mut state = snapshot(&root, view, &AtomicBool::new(false), history_limit)?;
    // A failed push must not pretend the preceding commit failed too.
    if committed_message.is_some() && new_head.is_some() && (result.is_ok() || old_head != new_head)
    {
        state.committed_message = committed_message;
    }
    if let Err(e) = result {
        state.error = format!("{e:#}");
    }
    Ok(state)
}
#[cfg(test)]
fn execute_view(
    collection: &Collection,
    action: Action,
    view: View,
    cancel: &AtomicBool,
) -> Result<State> {
    execute_cached(
        collection,
        action,
        State {
            view,
            ..Default::default()
        },
        cancel,
    )
}
pub fn clone_repository(url: &str, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    argument(url)?;
    ensure!(
        !destination.exists(),
        "Destination already exists; choose a new folder"
    );
    let parent = destination
        .parent()
        .context("Choose a destination folder")?;
    std::fs::create_dir_all(parent)?;
    output(
        parent,
        &["clone", "--", url, &destination.to_string_lossy()],
        cancel,
    )?;
    Ok(())
}
impl App {
    pub fn open_git(&mut self, cid: &str) {
        if cid.is_empty() {
            self.notice("Open a collection to use Git");
            return;
        }
        self.open(format!("__git__{cid}"));
        self.git_action(cid, Action::Refresh);
    }
    pub fn git_action(&mut self, cid: &str, action: Action) {
        if self.git.get(cid).is_some_and(|s| s.busy) {
            if !matches!(action, Action::Refresh) {
                if matches!(action, Action::Preview(_)) {
                    self.git_pending
                        .retain(|(id, a)| id != cid || !matches!(a, Action::Preview(_)));
                }
                self.git_pending.push_back((cid.into(), action));
            }
            return;
        }
        let Some(collection) = self.ws.collections.iter().find(|c| c.id == cid).cloned() else {
            return;
        };
        let initial_view = self
            .ws
            .git_drafts
            .get(cid)
            .map(|d| d.view.clone())
            .unwrap_or_default();
        let state = self.git.entry(cid.into()).or_insert_with(|| State {
            view: initial_view,
            ..Default::default()
        });
        state.busy = true;
        state.refreshing = matches!(action, Action::Refresh);
        if !state.refreshing {
            state.error.clear();
        }
        let cancel = Arc::new(AtomicBool::new(false));
        state.cancel = Some(cancel.clone());
        let cached = state.clone();
        let barrier = self.writer.barrier();
        let id = cid.to_owned();
        let tx = self.tx.clone();
        let ctx = self.context.clone();
        self.runtime.spawn_blocking(move || {
            let _ = barrier.recv();
            let result =
                execute_cached(&collection, action, cached, &cancel).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Git(id, result));
            ctx.request_repaint();
        });
    }
    pub fn clone_git(&mut self, url: String, destination: PathBuf) {
        if self.git_clone.is_some() {
            self.notice("A repository is already being cloned");
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        self.git_clone = Some(CloneJob {
            destination: destination.clone(),
            cancel: cancel.clone(),
            finished: finished.clone(),
        });
        let tx = self.tx.clone();
        let ctx = self.context.clone();
        self.notice("Cloning repository…");
        self.runtime.spawn_blocking(move||{
            let result=(||{clone_repository(&url,&destination,&cancel)?;crate::storage::open_collection(&destination).map(LoadedCollection::new).with_context(||format!("Repository cloned to {}. Open its collection folder with Open Collection",destination.display()))})().map_err(|e|format!("{e:#}"));
            finished.store(true,Ordering::Relaxed);
            let _=tx.send(Event::Collection(result));ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collection(path: &Path) -> Collection {
        Collection {
            id: "git-test".into(),
            name: "Test".into(),
            path: path.into(),
            environments: vec![],
            vars: vec![],
            folders: vec![],
        }
    }
    fn execute(c: &Collection, a: Action) -> State {
        let s = execute_view(c, a, View::Home, &AtomicBool::new(false)).unwrap();
        assert!(s.error.is_empty(), "{}", s.error);
        s
    }
    fn initialized(path: &Path) -> Collection {
        std::fs::create_dir_all(path).unwrap();
        let c = collection(path);
        execute(&c, Action::Init);
        run(path, &["config", "core.autocrlf", "false"]).unwrap();
        execute(
            &c,
            Action::Identity("Reqon Test".into(), "test@example.invalid".into()),
        );
        c
    }
    #[test]
    fn status_stage_unstage_commit_diff_and_rename_handle_unicode_literal_paths() {
        let d = tempfile::tempdir().unwrap();
        let c = initialized(d.path());
        let file = "Запрос [one].req";
        std::fs::write(d.path().join(file), "first\n").unwrap();
        let s = execute(&c, Action::Refresh);
        assert_eq!(s.files[0].path, file);
        assert!(!s.files[0].staged());
        assert!(
            execute(&c, Action::Preview(View::File(file.into(), false)))
                .diff
                .contains("+first")
        );
        assert!(execute(&c, Action::StageFile(file.into())).files[0].staged());
        assert!(!execute(&c, Action::Unstage(Some(file.into()))).files[0].staged());
        execute(&c, Action::Stage);
        assert!(
            execute(&c, Action::Commit("Initial".into()))
                .files
                .is_empty()
        );
        std::fs::write(d.path().join(file), "second\n").unwrap();
        execute(&c, Action::Stage);
        let s = execute(&c, Action::Preview(View::File(file.into(), true)));
        assert!(s.diff.contains("-first") && s.diff.contains("+second"));
        execute(&c, Action::Unstage(None));
        execute(&c, Action::Discard(file.into()));
        assert_eq!(
            std::fs::read_to_string(d.path().join(file)).unwrap(),
            "first\n"
        );
        run(d.path(), &["mv", "--", file, "Renamed.req"]).unwrap();
        let s = execute(&c, Action::Refresh);
        assert_eq!(s.files[0].old, file);
        assert_eq!(s.files[0].path, "Renamed.req");
        assert!(relative("../outside").is_err());
        assert!(argument("--upload-pack=bad").is_err());
    }
    #[test]
    fn clone_remote_push_fetch_pull_branch_history_and_stash_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let bare = d.path().join("remote.git");
        std::fs::create_dir(&bare).unwrap();
        run(&bare, &["init", "--bare"]).unwrap();
        let c = initialized(&d.path().join("one"));
        let branch = execute(&c, Action::Refresh).branch;
        std::fs::write(c.path.join("one.req"), "initial\n").unwrap();
        execute(&c, Action::Stage);
        execute(&c, Action::Commit("Initial".into()));
        execute(
            &c,
            Action::AddRemote("origin".into(), bare.to_string_lossy().into()),
        );
        execute(&c, Action::Push("origin".into()));
        run(
            &bare,
            &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
        )
        .unwrap();
        let second = d.path().join("two");
        clone_repository(&bare.to_string_lossy(), &second, &AtomicBool::new(false)).unwrap();
        assert!(clone_repository("irrelevant", &second, &AtomicBool::new(false)).is_err());
        let c2 = collection(&second);
        execute(
            &c2,
            Action::Identity("Second".into(), "second@example.invalid".into()),
        );
        std::fs::write(second.join("two.req"), "second\n").unwrap();
        execute(&c2, Action::Stage);
        execute(&c2, Action::Commit("Second commit".into()));
        execute(&c2, Action::Push("origin".into()));
        assert_eq!(execute(&c, Action::Fetch).behind, 1);
        let s = execute(&c, Action::Pull);
        assert_eq!(s.behind, 0);
        assert!(c.path.join("two.req").exists());
        assert_eq!(s.commits.len(), 2);
        assert!(
            execute(&c, Action::Preview(View::Commit(s.commits[0].hash.clone())))
                .diff
                .contains("+second")
        );
        execute(&c, Action::CreateBranch("feature/test".into()));
        execute(&c, Action::Checkout(branch));
        execute(&c, Action::DeleteBranch("feature/test".into()));
        std::fs::write(c.path.join("one.req"), "draft\n").unwrap();
        std::fs::write(c.path.join("untracked.req"), "draft\n").unwrap();
        let s = execute(&c, Action::Stash("draft".into()));
        assert!(
            s.files.is_empty(),
            "{}",
            s.files
                .iter()
                .map(|f| format!("{}{} {}", f.index, f.work, f.path))
                .collect::<Vec<_>>()
                .join("; ")
        );
        assert_eq!(s.stashes.len(), 1);
        let s = execute(&c, Action::ApplyStash(s.stashes[0].0.clone(), true));
        assert_eq!(s.files.len(), 2);
        assert!(s.stashes.is_empty());
        let s = execute(&c, Action::Stash("drop me".into()));
        execute(&c, Action::DropStash(s.stashes[0].0.clone()));
        assert!(
            execute(&c, Action::RemoveRemote("origin".into()))
                .remotes
                .is_empty()
        );
        // Collections nested inside a repository operate on its actual root.
        let nested = c.path.join("nested");
        std::fs::create_dir(&nested).unwrap();
        assert_eq!(
            execute(&collection(&nested), Action::Refresh)
                .root
                .unwrap()
                .canonicalize()
                .unwrap(),
            c.path.canonicalize().unwrap()
        );
    }
    #[test]
    fn failed_stash_merge_reports_conflicts_and_preserves_state() {
        let d = tempfile::tempdir().unwrap();
        let c = initialized(d.path());
        let p = d.path().join("request.req");
        std::fs::write(&p, "base\n").unwrap();
        execute(&c, Action::Stage);
        execute(&c, Action::Commit("base".into()));
        std::fs::write(&p, "stashed\n").unwrap();
        let s = execute(&c, Action::Stash("conflict".into()));
        let stash = s.stashes[0].0.clone();
        std::fs::write(&p, "committed\n").unwrap();
        execute(&c, Action::Stage);
        execute(&c, Action::Commit("other".into()));
        let s = execute_view(
            &c,
            Action::ApplyStash(stash, false),
            View::Home,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!s.error.is_empty());
        assert!(s.files.iter().any(File::conflict));
        assert_eq!(s.stashes.len(), 1);
        std::fs::write(&p, "resolved\n").unwrap();
        execute(&c, Action::Stage);
        assert!(
            execute(&c, Action::Commit("resolve".into()))
                .files
                .is_empty()
        );
    }
}
