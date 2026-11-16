use crate::{engine::Engine, model::*, storage, theme};
use eframe::egui;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct TabState {
    pub ai_completion: crate::ai_ui::Completion,
    pub request_tab: usize,
    pub response_tab: usize,
    pub post_script: bool,
    pub docs_edit: bool,
    pub collection_tab: usize,
    pub reply: Option<Reply>,
    pub pretty: String,
    pub raw_lines: Vec<usize>,
    pub pretty_lines: Vec<usize>,
    pub raw: bool,
    pub preview_image: Option<egui::ColorImage>,
    pub preview_texture: Option<egui::TextureHandle>,
    pub grpc_methods: Vec<crate::grpc::MethodInfo>,
    pub grpc_loading: bool,
    pub grpc_discovery: crate::grpc_picker::DiscoveryState,
    pub socket_connected: bool,
    pub socket_messages: VecDeque<crate::websocket::Record>,
    pub socket_selected: usize,
}
impl TabState {
    pub fn set_reply(&mut self, reply: Reply) {
        self.apply_reply(PreparedReply::new(reply));
    }
    pub fn apply_reply(&mut self, prepared: PreparedReply) {
        self.pretty = prepared.pretty;
        self.raw_lines = prepared.raw_lines;
        self.pretty_lines = prepared.pretty_lines;
        self.preview_image = prepared.preview_image;
        self.preview_texture = None;
        self.reply = Some(prepared.reply);
    }
}
pub struct PreparedReply {
    pub reply: Reply,
    pub pretty: String,
    pub raw_lines: Vec<usize>,
    pub pretty_lines: Vec<usize>,
    pub history: Reply,
    pub preview_image: Option<egui::ColorImage>,
}
impl PreparedReply {
    pub fn new(reply: Reply) -> Self {
        let preview_image = (|| {
            if !reply
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.starts_with("image/"))
            {
                return None;
            }
            let reader = image::ImageReader::new(std::io::Cursor::new(&reply.bytes))
                .with_guessed_format()
                .ok()?;
            let (w, h) = reader.into_dimensions().ok()?;
            if w > 16384 || h > 16384 || u64::from(w) * u64::from(h) > 16_000_000 {
                return None;
            }
            let image = image::load_from_memory(&reply.bytes).ok()?.into_rgba8();
            Some(egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                image.as_raw(),
            ))
        })();
        let pretty = serde_json::from_str::<serde_json::Value>(&reply.body)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| reply.body.clone());
        let raw_lines = crate::widgets::line_offsets(&reply.body);
        let pretty_lines = crate::widgets::line_offsets(&pretty);
        let mut boundary = reply.body.len().min(256 * 1024);
        while !reply.body.is_char_boundary(boundary) {
            boundary -= 1;
        }
        let mut body = reply.body[..boundary].to_owned();
        if boundary < reply.body.len() {
            body.push_str("\n[History preview truncated]");
        }
        let history = Reply {
            cancelled: reply.cancelled,
            status: reply.status,
            reason: reply.reason.clone(),
            elapsed_ms: reply.elapsed_ms,
            size: reply.size,
            headers: reply.headers.clone(),
            trailers: reply.trailers.clone(),
            body,
            url: reply.url.clone(),
            error: reply.error.clone(),
            timeline: reply.timeline.clone(),
            tests: reply.tests.clone(),
            logs: reply.logs.clone(),
            runtime_vars: reply.runtime_vars.clone(),
            script_effects: reply.script_effects.clone(),
            deleted_vars: reply.deleted_vars.clone(),
            redaction_values: reply.redaction_values.clone(),
            bytes: vec![],
        };
        Self {
            preview_image,
            reply,
            pretty,
            raw_lines,
            pretty_lines,
            history,
        }
    }
}
pub struct LoadedCollection {
    pub collection: Collection,
    pub requests: Vec<Request>,
    pub warnings: Vec<String>,
    pub saved: HashMap<String, String>,
    pub fingerprints: BTreeMap<String, String>,
}
impl LoadedCollection {
    pub fn new((collection, requests, warnings): (Collection, Vec<Request>, Vec<String>)) -> Self {
        let saved: HashMap<String, String> = requests
            .iter()
            .map(|r| (r.id.clone(), storage::to_bru(r)))
            .collect();
        let fingerprints = saved
            .iter()
            .map(|(id, source)| (id.clone(), crate::disk_sync::fingerprint(source)))
            .collect();
        Self {
            collection,
            requests,
            warnings,
            saved,
            fingerprints,
        }
    }
}
pub enum Event {
    Git(String, Result<crate::git::State, String>),
    Migrated(String, Result<crate::yaml::Migration, String>),
    Collection(Result<LoadedCollection, String>),
    CollectionTransferFinished(Result<Option<String>, String>),
    Reply(String, u64, PreparedReply),
    Terminal(String),
    GrpcMethods(String, u64, Result<Vec<crate::grpc::MethodInfo>, String>),
    Socket(String, u64, crate::websocket::SocketEvent),
}
#[derive(Clone, PartialEq)]
pub enum Dialog {
    None,
    NewCollection,
    NewRequest(String, String),
    Environments,
    Settings,
    Snippet,
    ImportCurl,
    Runner,
    History,
    Cookies,
    About,
    ApiSpecs,
    MockServers,
    CloseTab(String),
    TreeRename(crate::tree_ops::Subject),
    TreeNewFolder(String, String),
    TreeInfo(crate::tree_ops::Subject),
    TreeDelete(crate::tree_ops::Subject),
    ExportCollections,
    RemoveCollection(String),
}
pub enum Action {
    NewRequest(String, String),
    NewTab(String, Option<RequestKind>),
    ChangeRequestType(String, RequestKind),
    GenerateGrpcExample(String),
    NewCollection,
    OpenCollection,
    ImportPostman,
    ExportPostman(Option<String>),
    ExportCollections(Vec<String>),
    MoveRequest(String, crate::tree_move::Target),
    TreeItem(crate::tree_ops::Subject, crate::tree_menu::Command),
    TreeOperation(crate::tree_ops::Operation),
    Select(String),
    Close(String),
    Save,
    Send,
    Cancel,
    NewFolder(String),
    RunCollection,
    Dialog(Dialog),
    ToggleTheme,
    ToggleSplit,
    Home,
}
pub struct App {
    pub context: egui::Context,
    pub git: HashMap<String, crate::git::State>,
    pub git_ui: HashMap<String, crate::git_ui::State>,
    pub git_pending: VecDeque<(String, crate::git::Action)>,
    pub git_clone: Option<crate::git::CloneJob>,
    pub ws: Workspace,
    pub workspaces: crate::workspaces::State,
    pub root: PathBuf,
    pub writer: crate::persistence::Writer,
    pub save_error_generation: u64,
    pub tree: crate::tree::Cache,
    pub file_explorer: crate::file_explorer::State,
    pub tree_moving: bool,
    pub tree_menu: crate::tree_menu::State,
    pub collection_ui: crate::collection_ui::State,
    pub disk_sync: Option<crate::disk_sync::DiskSync>,
    pub tabs: HashMap<String, TabState>,
    pub saved: HashMap<String, String>,
    pub dirty_cache: std::cell::RefCell<HashMap<String, bool>>,
    pub pending_close: HashSet<String>,
    pub persisted_requests: HashSet<String>,
    pub modified_requests: HashSet<String>,
    pub persisted_history: (usize, u64),
    pub runtime: tokio::runtime::Runtime,
    pub engine: Engine,
    pub tx: mpsc::Sender<Event>,
    pub rx: mpsc::Receiver<Event>,
    pub running: HashMap<String, (u64, crate::cancellation::Task)>,
    pub running_context: HashMap<String, (String, String, String)>,
    pub sockets: HashMap<String, tokio::sync::mpsc::UnboundedSender<crate::websocket::Command>>,
    pub sequence: u64,
    pub runtime_vars: BTreeMap<String, BTreeMap<String, String>>,
    pub auth_migrations: HashMap<String, crate::auth_migration_ui::State>,
    pub actions: Vec<Action>,
    pub dialog: Dialog,
    pub entry: String,
    pub entry2: String,
    pub entry3: String,
    pub new_request_kind: usize,
    pub dialog_first_frame: bool,
    pub snippet: String,
    pub curl: crate::curl_editor::State,
    pub env_index: usize,
    pub home_tab: usize,
    pub preferences_shown: bool,
    pub preferences_tab: usize,
    pub preference_ui: crate::preferences::State,
    pub ai_ui: crate::ai_ui::State,
    pub env_search: String,
    pub environment_tree: crate::environment_tree::State,
    pub environment_collection: String,
    pub search: String,
    pub search_open: bool,
    pub collapsed: HashSet<String>,
    pub sidebar_visible: bool,
    pub sidebar_panel: crate::navigation::Panel,
    pub devtools_open: bool,
    pub devtools_tab: usize,
    pub devtools_height: f32,
    pub terminal: crate::terminal::Manager,
    pub terminal_output: String,
    pub terminal_running: bool,
    pub terminal_cwd: Option<PathBuf>,
    pub frame_times: VecDeque<f32>,
    pub started: Instant,
    pub capture_path: Option<PathBuf>,
    pub capture_exit: bool,
    pub status: String,
    pub status_at: Instant,
    pub dirty_at: Option<Instant>,
    pub runner_results: Vec<(String, u16, bool, u128)>,
    pub runner_active: bool,
    pub runner_cancelled: bool,
    pub runner_execution: Option<crate::collection_runner::Plan>,
    pub runner_iterations: u32,
    pub runner_iteration: u32,
    pub runner_delay: u64,
    pub runner_stop_on_error: bool,
    pub runner_current: Option<String>,
    pub runner_wait: Option<(Instant, String)>,
    pub theme: theme::Theme,
    pub mock_task: Option<tokio::task::AbortHandle>,
    pub mock_port: u16,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf) -> anyhow::Result<Self> {
        Self::from_context(&cc.egui_ctx, root)
    }
    pub fn from_context(ctx: &egui::Context, root: PathBuf) -> anyhow::Result<Self> {
        let mut registry = crate::workspace_store::Registry::load(&root)?;
        let entry = registry
            .entries
            .iter()
            .find(|e| e.id == registry.active)
            .cloned()
            .unwrap();
        let loaded = crate::workspace_store::prepare(&entry)
            .and_then(|_| Self::from_workspace(ctx, entry.state_root()));
        let mut app = match loaded {
            Ok(app) => app,
            Err(error) if !entry.default => {
                registry.active = "default".into();
                let mut app = Self::from_workspace(ctx, root.clone())?;
                app.notice(format!("Could not open {}: {error:#}", entry.name));
                app
            }
            Err(error) => return Err(error),
        };
        registry.save(&root)?;
        app.workspaces.data = root;
        app.workspaces.registry = registry;
        app.workspaces.updates.restore_outcome(&app.workspaces.data);
        Ok(app)
    }
    pub(crate) fn from_workspace(ctx: &egui::Context, root: PathBuf) -> anyhow::Result<Self> {
        Self::from_workspace_preferences(ctx, root, None)
    }
    pub(crate) fn from_workspace_preferences(
        ctx: &egui::Context,
        root: PathBuf,
        preferences: Option<&Preferences>,
    ) -> anyhow::Result<Self> {
        theme::fonts(ctx);
        egui_extras::install_image_loaders(ctx);
        let mut ws = storage::load(&root)?;
        if let Some(preferences) = preferences {
            ws.preferences = preferences.clone();
        }
        let (migrated, migration_warnings) = crate::native_format::migrate_workspace(&mut ws);
        if migrated {
            storage::save(&root, &ws)?;
        }
        let startup_status = migration_warnings.join("\n");
        crate::file_cache::configure(&root, ws.preferences.cache.file);
        crate::git::configure_provider(&ws.preferences);
        theme::editor_fonts(ctx, &ws.preferences.appearance.font);
        ctx.set_zoom_factor(f32::from(ws.preferences.appearance.zoom.clamp(50, 150)) / 100.);
        let theme = crate::theme_registry::current(&ws.preferences, ws.dark, ctx);
        theme.apply(ctx);
        let sidebar_panel = if ws
            .active
            .as_deref()
            .and_then(crate::environment_tabs::Target::parse)
            .is_some()
        {
            crate::navigation::Panel::Environments
        } else {
            crate::navigation::Panel::Collections
        };
        let mut saved: HashMap<String, String> = ws
            .requests
            .iter()
            .filter_map(|r| {
                r.path
                    .as_ref()
                    .and_then(|p| crate::yaml::read_request(p).ok())
                    .map(|disk| (r.id.clone(), storage::to_bru(&disk)))
            })
            .collect();
        for (id, draft) in &ws.collection_drafts {
            if let Some(path) = &draft.path {
                if let Ok(original) = crate::inheritance::parent(path) {
                    let mut original = original.unwrap_or_default();
                    original.protocol = draft.protocol.clone();
                    saved.insert(id.clone(), storage::to_bru(&original));
                }
            }
        }
        ws.saved_fingerprints = saved
            .iter()
            .map(|(id, source)| (id.clone(), crate::disk_sync::fingerprint(source)))
            .collect();
        let (tx, rx) = mpsc::channel();
        let mut disk_sync = crate::disk_sync::DiskSync::new(ctx.clone()).ok();
        if let Some(sync) = &mut disk_sync {
            sync.watch(&ws.collections);
        }
        let persisted_requests = ws.requests.iter().map(|r| r.id.clone()).collect();
        let persisted_history = (
            ws.history.len(),
            ws.history.last().map(|h| h.at).unwrap_or(0),
        );
        let mut engine = Engine::default();
        engine.cookies = std::sync::Arc::new(crate::cookies::Jar::load(&root)?);
        engine.configure(ws.preferences.clone());
        let dirty_at = (ws.preferences.autosave
            && ws.open_tabs.iter().any(|id| {
                ws.requests
                    .iter()
                    .find(|r| &r.id == id)
                    .or_else(|| ws.collection_drafts.get(id))
                    .is_some_and(|r| saved.get(id).is_none_or(|disk| disk != &storage::to_bru(r)))
            }))
        .then(Instant::now);
        Ok(Self {
            ws,
            workspaces: Default::default(),
            writer: crate::persistence::Writer::new(root.clone(), ctx.clone()),
            tree: crate::tree::Cache::default(),
            file_explorer: Default::default(),
            tree_moving: false,
            tree_menu: Default::default(),
            collection_ui: Default::default(),
            disk_sync,
            root,
            tabs: HashMap::new(),
            saved,
            dirty_cache: std::cell::RefCell::new(HashMap::new()),
            pending_close: HashSet::new(),
            persisted_requests,
            modified_requests: HashSet::new(),
            persisted_history,
            runtime: tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
            engine,
            tx,
            rx,
            running: HashMap::new(),
            running_context: HashMap::new(),
            sockets: HashMap::new(),
            sequence: 0,
            runtime_vars: BTreeMap::new(),
            auth_migrations: HashMap::new(),
            actions: vec![],
            dialog: Dialog::None,
            entry: String::new(),
            entry2: String::new(),
            entry3: String::new(),
            new_request_kind: 0,
            dialog_first_frame: false,
            snippet: String::new(),
            curl: Default::default(),
            env_index: 0,
            home_tab: 0,
            preferences_shown: false,
            preferences_tab: 0,
            preference_ui: Default::default(),
            save_error_generation: 0,
            ai_ui: Default::default(),
            env_search: String::new(),
            environment_tree: Default::default(),
            environment_collection: String::new(),
            search: String::new(),
            search_open: false,
            collapsed: HashSet::new(),
            sidebar_visible: true,
            sidebar_panel,
            devtools_open: false,
            devtools_tab: 0,
            devtools_height: 300.,
            terminal: crate::terminal::Manager::default(),
            terminal_output: String::new(),
            terminal_running: false,
            terminal_cwd: None,
            frame_times: VecDeque::with_capacity(120),
            context: ctx.clone(),
            git: HashMap::new(),
            git_ui: HashMap::new(),
            git_pending: VecDeque::new(),
            git_clone: None,
            started: Instant::now(),
            capture_path: std::env::args()
                .skip_while(|a| a != "--capture")
                .nth(1)
                .map(PathBuf::from),
            capture_exit: std::env::args().any(|a| a == "--capture-exit"),
            status: startup_status,
            status_at: Instant::now(),
            dirty_at,
            runner_results: vec![],
            runner_active: false,
            runner_cancelled: false,
            runner_execution: None,
            runner_iterations: 1,
            runner_iteration: 0,
            runner_delay: 0,
            runner_stop_on_error: false,
            runner_current: None,
            runner_wait: None,
            theme,
            mock_task: None,
            mock_port: 9473,
        })
    }
    pub fn active(&self) -> Option<&Request> {
        let id = self.ws.active.as_ref()?;
        self.ws.requests.iter().find(|r| &r.id == id)
    }
    pub fn active_collection_id(&self) -> String {
        if let Some(target) = self
            .ws
            .active
            .as_deref()
            .and_then(crate::environment_tabs::Target::parse)
        {
            if let Some(cid) = target.collection() {
                return cid.into();
            }
            if self
                .ws
                .collections
                .iter()
                .any(|c| c.id == self.environment_collection)
            {
                return self.environment_collection.clone();
            }
        }
        if let Some((cid, _)) = self
            .ws
            .active
            .as_deref()
            .and_then(crate::file_explorer::parse_id)
        {
            return cid.into();
        }
        if let Some(draft) = self
            .ws
            .active
            .as_ref()
            .and_then(|id| self.ws.collection_drafts.get(id))
        {
            return draft.collection.clone();
        }
        if let Some(cid) = self.ws.active.as_deref().and_then(|id| {
            id.strip_prefix("__environments__")
                .or_else(|| id.strip_prefix("__collection__"))
                .or_else(|| id.strip_prefix("__git__"))
        }) {
            return cid.into();
        }
        self.active()
            .map(|r| r.collection.clone())
            .or_else(|| self.ws.collections.first().map(|c| c.id.clone()))
            .unwrap_or_default()
    }
    pub fn collection_name(&self, id: &str) -> String {
        self.ws
            .collections
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "Scratchpad".into())
    }
    pub fn vars(&self, r: &Request) -> BTreeMap<String, String> {
        let collection = self.ws.collections.iter().find(|c| c.id == r.collection);
        let env = collection.and_then(|c| {
            c.environments
                .iter()
                .find(|e| e.name == self.ws.selected_environment)
        });
        let mut v = variables(
            &self.workspace_globals(),
            collection.map(|c| c.vars.as_slice()).unwrap_or(&[]),
            env.map(|e| e.vars.as_slice()).unwrap_or(&[]),
            &r.vars,
        );
        v.extend(
            self.runtime_vars
                .get(&r.collection)
                .cloned()
                .unwrap_or_default(),
        );
        v
    }
    pub fn dirty(&self, id: &str) -> bool {
        if let Some(dirty) = self.dirty_cache.borrow().get(id) {
            return *dirty;
        }
        let dirty = self
            .ws
            .requests
            .iter()
            .find(|r| r.id == id)
            .or_else(|| self.ws.collection_drafts.get(id))
            .is_some_and(|r| self.saved.get(id).is_none_or(|s| storage::to_bru(r) != *s));
        self.dirty_cache.borrow_mut().insert(id.to_owned(), dirty);
        dirty
    }
    pub fn mark_changed(&mut self) {
        self.tree.stale = true;
        self.mark_edited();
    }
    pub fn mark_edited(&mut self) {
        if let Some(id) = &self.ws.active {
            self.dirty_cache.borrow_mut().remove(id);
            self.modified_requests.insert(id.clone());
        }
        self.dirty_at = Some(Instant::now());
    }
    pub fn mark_workspace_changed(&mut self) {
        self.dirty_at = Some(Instant::now());
    }
    pub fn notice(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_at = Instant::now();
    }
    pub fn open(&mut self, mut id: String) {
        if let Some(old) = self.ws.active.clone() {
            let next = self.commit_environment_name(&old);
            if id == old {
                id = next;
            }
        }
        self.workspaces.manage = false;
        if let Some(request) = self.ws.requests.iter().find(|r| r.id == id) {
            self.ws
                .last_request_kind
                .insert(request.collection.clone(), RequestKind::of(request));
        }
        if !self.ws.open_tabs.contains(&id) {
            self.ws.open_tabs.push(id.clone());
        }
        self.ws.active = Some(id);
        self.mark_workspace_changed();
        self.context.request_repaint();
    }
    pub fn close(&mut self, id: &str) {
        let committed = self.commit_environment_name(id);
        let id = committed.as_str();
        if self.ws.open_tabs.iter().any(|x| x == id) {
            self.preference_ui.closed_tabs.push(id.into());
            if self.preference_ui.closed_tabs.len() > 20 {
                self.preference_ui.closed_tabs.remove(0);
            }
        }
        self.ws.open_tabs.retain(|x| x != id);
        if self.ws.active.as_deref() == Some(id) {
            self.ws.active = self.ws.open_tabs.last().cloned();
        }
        self.mark_workspace_changed();
    }
    pub fn discard_and_close(&mut self, id: &str) {
        if self.ws.collection_drafts.remove(id).is_none() {
            if let Some(index) = self.ws.requests.iter().position(|r| r.id == id) {
                if let Some(saved) = self.saved.get(id) {
                    match storage::parse_bru(saved) {
                        Ok(mut restored) => {
                            let current = &self.ws.requests[index];
                            restored.id = current.id.clone();
                            restored.path = current.path.clone();
                            restored.collection = current.collection.clone();
                            restored.folder = current.folder.clone();
                            restored.file_format = current.file_format.clone();
                            restored.original = current.original.clone();
                            self.ws.requests[index] = restored;
                            self.modified_requests.insert(id.into());
                        }
                        Err(e) => {
                            self.notice(format!("Could not restore saved request: {e:#}"));
                            return;
                        }
                    }
                } else {
                    self.ws.requests.remove(index);
                }
            }
        }
        self.tabs.remove(id);
        self.dirty_cache.borrow_mut().remove(id);
        self.tree.stale = true;
        self.close(id);
    }
    pub fn show_dialog(&mut self, d: Dialog) {
        if d == Dialog::Settings {
            self.open(crate::preferences::TAB_ID.into());
            self.preference_ui.job(&self.context, || {
                crate::preferences::JobResult::FileSize(crate::file_cache::size())
            });
            self.dialog = Dialog::None;
            return;
        }
        if d == Dialog::Environments {
            let cid = self.active_collection_id();
            self.open_environment_manager(&cid);
            self.dialog = Dialog::None;
            return;
        }
        self.entry.clear();
        self.entry2.clear();
        self.entry3.clear();
        if d == Dialog::NewCollection {
            self.entry2 = self.ws.preferences.default_location.clone();
        }
        self.new_request_kind = 0;
        self.dialog_first_frame = true;
        self.dialog = d;
    }
    pub fn finish_saves(&mut self) {
        while let Ok((id, source)) = self.writer.saved.try_recv() {
            self.ws
                .saved_fingerprints
                .insert(id.clone(), crate::disk_sync::fingerprint(&source));
            self.saved.insert(id.clone(), source);
            self.dirty_cache.borrow_mut().remove(&id);
            self.mark_workspace_changed();
            self.notice("Request saved");
            if self.pending_close.remove(&id) && !self.dirty(&id) {
                self.close(&id);
                self.dialog = Dialog::None;
            }
        }
    }
    pub fn save_active(&mut self) {
        if let Some(id) = self.ws.active.clone() {
            self.commit_environment_name(&id);
        }
        if let Some(target) = self
            .ws
            .active
            .as_deref()
            .and_then(crate::environment_tabs::Target::parse)
        {
            if let Some(cid) = target.collection() {
                if let Some(c) = self.ws.collections.iter().find(|c| c.id == cid) {
                    self.writer.save_environments(c.clone());
                }
            }
            self.mark_workspace_changed();
            self.flush();
            self.notice("Environment saved");
            return;
        }
        if let Some(id) = self.ws.active.as_ref() {
            if let Some(request) = self.ws.collection_drafts.get(id) {
                self.writer.save_request(request.clone());
                self.notice("Saving collection…");
                return;
            }
        }
        let Some(index) = self
            .ws
            .active
            .as_ref()
            .and_then(|id| self.ws.requests.iter().position(|r| &r.id == id))
        else {
            return;
        };
        if self.ws.requests[index].path.is_none() {
            let c = self
                .ws
                .collections
                .iter()
                .find(|c| c.id == self.ws.requests[index].collection)
                .or_else(|| self.ws.collections.first());
            if let Some(c) = c {
                self.ws.requests[index].collection = c.id.clone();
                let r = &self.ws.requests[index];
                let path = self.unique_path(&c.path, &r.folder, &r.name);
                self.ws.requests[index].path = Some(path);
            } else {
                self.show_dialog(Dialog::NewCollection);
                return;
            }
        }
        self.writer.save_request(self.ws.requests[index].clone());
        self.notice("Saving request…");
    }
    pub fn unique_path(&self, root: &std::path::Path, folder: &str, name: &str) -> PathBuf {
        let stem = safe_name(name);
        let extension = if root.join("opencollection.yml").is_file() {
            "yml"
        } else {
            "req"
        };
        let mut path = root.join(folder).join(format!("{stem}.{extension}"));
        let mut i = 2;
        while path.exists()
            || self
                .ws
                .requests
                .iter()
                .any(|r| r.path.as_ref() == Some(&path))
        {
            path = root.join(folder).join(format!("{stem} {i}.{extension}"));
            i += 1;
        }
        path
    }
    pub fn add_request(
        &mut self,
        collection: String,
        folder: String,
        name: String,
        method: String,
    ) {
        let mut r = Request {
            name,
            method,
            seq: self.ws.next_request_seq(&collection, &folder),
            collection: collection.clone(),
            folder,
            ..Default::default()
        };
        if let Some(c) = self.ws.collections.iter().find(|c| c.id == collection) {
            r.path = Some(self.unique_path(&c.path, &r.folder, &r.name));
        }
        let id = r.id.clone();
        self.ws.requests.push(r);
        self.tree.stale = true;
        self.open(id);
        self.save_active();
    }
    pub fn send(&mut self, id: String, ctx: &egui::Context) {
        if self.running.contains_key(&id) {
            return;
        }
        let Some(r) = self.ws.requests.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        if r.protocol == "ws" && !self.runner_active {
            self.connect_socket(r, ctx);
            return;
        }
        let globals = self.workspace_globals();
        let collection_values = self
            .ws
            .collections
            .iter()
            .find(|c| c.id == r.collection)
            .map(|c| typed_variables(&c.vars))
            .unwrap_or_default();
        let env = self
            .ws
            .collections
            .iter()
            .find(|c| c.id == r.collection)
            .and_then(|c| {
                c.environments
                    .iter()
                    .find(|e| e.name == self.ws.selected_environment)
            })
            .map(|e| e.vars.clone())
            .unwrap_or_default();
        let runtime_vars = self
            .runtime_vars
            .get(&r.collection)
            .cloned()
            .unwrap_or_default();
        self.running_context.insert(
            id.clone(),
            (
                r.collection.clone(),
                self.ws.selected_environment.clone(),
                self.ws.workspace_environment.clone(),
            ),
        );
        self.engine.configure(self.ws.preferences.clone());
        let token = crate::cancellation::CancellationToken::new();
        let engine = self.engine.clone().with_cancellation(token.clone());
        let tx = self.tx.clone();
        let context = ctx.clone();
        self.sequence += 1;
        let sequence = self.sequence;
        let key = id.clone();
        let handle = self.runtime.spawn(async move {
            let reply = engine
                .execute_with_collection(r, globals, env, runtime_vars, collection_values)
                .await;
            let prepared = tokio::task::spawn_blocking(move || PreparedReply::new(reply)).await;
            if let Ok(prepared) = prepared {
                let _ = tx.send(Event::Reply(key, sequence, prepared));
            }
            context.request_repaint();
        });
        self.running.insert(
            id,
            (
                sequence,
                crate::cancellation::Task::new(handle.abort_handle(), token),
            ),
        );
        ctx.request_repaint();
    }
    pub fn cancel(&mut self, id: &str) {
        self.running_context.remove(id);
        if self.runner_current.as_deref() == Some(id) {
            self.runner_cancelled = true;
            self.runner_active = false;
            self.runner_execution = None;
            self.runner_current = None;
            self.runner_wait = None;
        }
        if let Some(sender) = self.sockets.remove(id) {
            let _ = sender.send(crate::websocket::Command::Close);
            let connected = self.tabs.get(id).is_some_and(|s| s.socket_connected);
            if let Some(state) = self.tabs.get_mut(id) {
                state.socket_connected = false;
            }
            if !connected {
                if let Some((_, handle)) = self.running.remove(id) {
                    handle.abort();
                }
                self.tabs
                    .entry(id.into())
                    .or_default()
                    .socket_messages
                    .push_back(crate::websocket::Record::new(
                        false,
                        "Disconnected",
                        "Connection cancelled".into(),
                    ));
            }
            return;
        }
        if let Some((_, handle)) = self.running.remove(id) {
            handle.abort();
            self.tabs
                .entry(id.into())
                .or_default()
                .apply_reply(PreparedReply::new(crate::cancellation::reply()));
            self.notice("Request cancelled");
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.curl_tick(ctx);
        if self
            .git_clone
            .as_ref()
            .is_some_and(|job| job.finished.load(std::sync::atomic::Ordering::Relaxed))
        {
            self.git_clone = None;
        }
        self.terminal.poll();
        if self.engine.cookies.take_dirty() {
            let jar = self.engine.cookies.clone();
            let root = self.root.clone();
            let tx = self.tx.clone();
            self.runtime.spawn_blocking(move || {
                if let Err(error) = jar.save(&root) {
                    let _ = tx.send(Event::Terminal(format!(
                        "Could not save cookies: {error:#}"
                    )));
                }
            });
        }
        if self
            .runner_wait
            .as_ref()
            .is_some_and(|(due, _)| Instant::now() >= *due)
        {
            let (_, id) = self.runner_wait.take().unwrap();
            if self.runner_active {
                self.send(id, ctx);
            }
        }
        while let Ok(error) = self.writer.errors.try_recv() {
            self.save_error_generation = self.save_error_generation.wrapping_add(1);
            self.notice(error);
        }
        self.finish_saves();
        self.finish_tree_moves();
        self.finish_tree_operations();
        let updates: Vec<_> = self
            .disk_sync
            .as_ref()
            .map(|sync| sync.updates.try_iter().collect())
            .unwrap_or_default();
        for mut update in updates {
            if !self.ws.collections.iter().any(|c| c.id == update.id) {
                continue;
            }
            let (count, conflicts, modified) = crate::disk_sync::merge_prepared(
                &mut self.ws,
                &update.id,
                update.requests,
                &mut self.saved,
            );
            self.modified_requests.extend(modified);
            if let Some(c) = self.ws.collections.iter_mut().find(|c| c.id == update.id) {
                c.folders = update.collection.folders;
                // The environment editor keeps its draft until Save/Reset.
                if !self.ws.active.as_deref().is_some_and(|id| {
                    id == format!("__environments__{}", c.id)
                        || crate::environment_tabs::Target::parse(id)
                            .is_some_and(|target| target.collection() == Some(c.id.as_str()))
                }) {
                    for env in &mut update.collection.environments {
                        if let Some(old) = c.environments.iter().find(|e| e.name == env.name) {
                            for p in &mut env.vars {
                                if p.secret && p.value.is_empty() {
                                    if let Some(value) =
                                        old.vars.iter().find(|v| v.key == p.key && v.secret)
                                    {
                                        p.value = value.value.clone();
                                    }
                                }
                            }
                        }
                    }
                    c.environments = update.collection.environments;
                    c.vars = update.collection.vars;
                }
            }
            self.dirty_cache.borrow_mut().clear();
            self.mark_changed();
            if conflicts > 0 {
                self.notice(format!("Files changed on disk: {count} reloaded, {conflicts} local drafts preserved. Save a draft to replace its disk version."));
            } else if !update.warnings.is_empty() {
                self.notice(update.warnings.join("; "));
            } else if count > 0 {
                self.notice(format!("Reloaded {count} changes from disk"));
            }
        }
        while let Ok(event) = self.rx.try_recv() {
            let (id, sequence, mut prepared) = match event {
                Event::CollectionTransferFinished(result) => {
                    self.collection_ui.transferring = false;
                    match result {
                        Ok(Some(message)) => self.notice(message),
                        Ok(None) => self.status.clear(),
                        Err(error) => self.notice(error),
                    }
                    continue;
                }
                Event::Git(cid, result) => {
                    match result {
                        Ok(mut state) => {
                            if let Some(message) = &state.committed_message {
                                if let Some(form) = self.git_ui.get_mut(&cid) {
                                    if &form.message == message {
                                        form.message.clear();
                                    }
                                    form.amend = false;
                                    form.selected.clear();
                                }
                                if let Some(draft) = self.ws.git_drafts.get_mut(&cid) {
                                    if &draft.message == message {
                                        draft.message.clear();
                                    }
                                    draft.amend = false;
                                    draft.selected.clear();
                                    self.mark_workspace_changed();
                                }
                            }
                            if let Some(old) = self.git.get(&cid) {
                                if old.refreshing && state.error.is_empty() {
                                    state.error = old.error.clone();
                                }
                            }
                            if !state.error.is_empty() {
                                self.notice(&state.error);
                            }
                            self.git.insert(cid.clone(), state);
                        }
                        Err(error) => {
                            let state = self.git.entry(cid.clone()).or_default();
                            state.busy = false;
                            state.updated = Some(Instant::now());
                            state.error = error.clone();
                            self.notice(error);
                        }
                    }
                    if let Some(index) = self.git_pending.iter().position(|(id, _)| id == &cid) {
                        if let Some((id, action)) = self.git_pending.remove(index) {
                            self.git_action(&id, action);
                        }
                    }
                    continue;
                }
                Event::Migrated(cid, result) => {
                    match result {
                        Ok(files) => {
                            for (old, new, canonical, source) in files {
                                for request in self
                                    .ws
                                    .requests
                                    .iter_mut()
                                    .chain(self.ws.collection_drafts.values_mut())
                                    .filter(|r| {
                                        r.collection == cid && r.path.as_ref() == Some(&old)
                                    })
                                {
                                    request.path = Some(new.clone());
                                    request.file_format = "yml".into();
                                    request.original = source.clone();
                                    self.saved.insert(request.id.clone(), canonical.clone());
                                    self.ws.saved_fingerprints.insert(
                                        request.id.clone(),
                                        crate::disk_sync::fingerprint(&canonical),
                                    );
                                    self.modified_requests.insert(request.id.clone());
                                }
                            }
                            self.dirty_cache.borrow_mut().clear();
                            self.mark_changed();
                            self.notice("Converted to YAML. Original REQ files retained in .reqon/req-backup.");
                        }
                        Err(error) => self.notice(error),
                    }
                    continue;
                }
                Event::Collection(result) => {
                    match result {
                        Ok(loaded) => {
                            if self
                                .ws
                                .collections
                                .iter()
                                .any(|c| c.path == loaded.collection.path)
                            {
                                self.notice("This collection is already open");
                                continue;
                            }
                            let first = loaded.requests.first().map(|r| r.id.clone());
                            self.saved.extend(loaded.saved);
                            self.ws.saved_fingerprints.extend(loaded.fingerprints);
                            self.ws.collections.push(loaded.collection);
                            self.ws.requests.extend(loaded.requests);
                            if let Some(id) = first {
                                self.open(id);
                            }
                            self.mark_changed();
                            if loaded.warnings.is_empty() {
                                self.notice("Collection opened");
                            } else {
                                self.notice(format!(
                                    "Collection opened with {} file warnings",
                                    loaded.warnings.len()
                                ));
                                self.terminal_output.push_str(&loaded.warnings.join("\n"));
                            }
                        }
                        Err(error) => self.notice(error),
                    }
                    continue;
                }
                Event::Reply(id, sequence, reply) => (id, sequence, reply),
                Event::Terminal(text) => {
                    self.terminal_output.push_str(&text);
                    self.terminal_running = false;
                    continue;
                }
                Event::GrpcMethods(id, generation, result) => {
                    self.receive_grpc_methods(&id, generation, result);
                    continue;
                }
                Event::Socket(id, sequence, event) => {
                    self.receive_socket(id, sequence, event);
                    continue;
                }
            };
            if self.running.get(&id).is_none_or(|(s, _)| *s != sequence) {
                continue;
            }
            self.running.remove(&id);
            let reply = &prepared.reply;
            let is_runner = self.runner_active && self.runner_current.as_deref() == Some(&id);
            let (cid, env, workspace_env) = self.running_context.remove(&id).unwrap_or_default();
            self.apply_script_effects(&cid, &env, &workspace_env, &reply.script_effects);
            let runtime = self.runtime_vars.entry(cid.clone()).or_default();
            for key in &reply.deleted_vars {
                runtime.remove(key);
            }
            runtime.extend(reply.runtime_vars.clone());
            if is_runner {
                if let Some(plan) = &mut self.runner_execution {
                    if let Err(error) = plan.complete(reply) {
                        self.runner_execution = None;
                        self.runner_active = false;
                        self.runner_current = None;
                        self.runner_results
                            .push((format!("Runner: {error}"), 0, false, 0));
                        self.notice(format!("Runner: {error}"));
                    }
                }
            }
            if let Some(r) = self.ws.requests.iter().find(|r| r.id == id) {
                self.ws.history.push(History {
                    at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    request: r.clone(),
                    reply: std::mem::take(&mut prepared.history),
                });
                if self.ws.history.len() > 100 {
                    self.ws.history.remove(0);
                }
                if is_runner {
                    if self.runner_results.len() >= 10000 {
                        self.runner_results.remove(0);
                    }
                    self.runner_results.push((
                        r.name.clone(),
                        reply.status,
                        reply.error.is_empty() && reply.tests.iter().all(|t| t.passed),
                        reply.elapsed_ms,
                    ));
                }
            }
            self.tabs
                .entry(id.clone())
                .or_default()
                .apply_reply(prepared);
            self.mark_workspace_changed();
            if is_runner {
                self.advance_runner(ctx, true);
            }
        }
        if self.dirty_at.is_some_and(|at| {
            at.elapsed()
                > Duration::from_millis(if self.ws.preferences.autosave {
                    self.ws.preferences.autosave_delay.max(100)
                } else {
                    700
                })
        }) {
            self.flush();
        }
        if self.running.keys().any(|id| !self.sockets.contains_key(id)) || self.dirty_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn flush(&mut self) {
        if self.tree_moving {
            return;
        }
        if self.ws.preferences.autosave {
            for id in self.ws.open_tabs.clone() {
                if self.dirty(&id) {
                    // New tabs already belong to a collection but have no file
                    // until their first save. Reserve a unique path for them too.
                    if let Some(index) = self
                        .ws
                        .requests
                        .iter()
                        .position(|r| r.id == id && r.path.is_none())
                    {
                        if let Some(collection) = self
                            .ws
                            .collections
                            .iter()
                            .find(|c| c.id == self.ws.requests[index].collection)
                        {
                            let request = &self.ws.requests[index];
                            let path =
                                self.unique_path(&collection.path, &request.folder, &request.name);
                            self.ws.requests[index].path = Some(path);
                            self.modified_requests.insert(id.clone());
                        }
                    }
                    if let Some(r) = self
                        .ws
                        .requests
                        .iter()
                        .find(|r| r.id == id)
                        .or_else(|| self.ws.collection_drafts.get(&id))
                    {
                        if r.path.is_some() {
                            self.writer.save_request(r.clone());
                        }
                    }
                }
            }
        }
        if let Some(sync) = &mut self.disk_sync {
            sync.watch(&self.ws.collections);
        }
        let order: Vec<_> = self.ws.requests.iter().map(|r| r.id.clone()).collect();
        let patches = self
            .ws
            .requests
            .iter()
            .filter(|r| {
                self.modified_requests.contains(&r.id) || !self.persisted_requests.contains(&r.id)
            })
            .cloned()
            .collect();
        let history_stamp = (
            self.ws.history.len(),
            self.ws.history.last().map(|h| h.at).unwrap_or(0),
        );
        let history = (history_stamp != self.persisted_history).then(|| self.ws.history.clone());
        self.persisted_history = history_stamp;
        self.persisted_requests = order.iter().cloned().collect();
        self.modified_requests.clear();
        self.writer
            .submit(self.ws.metadata_snapshot(), order, patches, history);
        self.dirty_at = None;
    }
    pub fn handle_actions(&mut self, ctx: &egui::Context) {
        for a in std::mem::take(&mut self.actions) {
            if self.tree_moving {
                self.actions.push(a);
                continue;
            }
            match a {
                Action::TreeItem(subject, command) => self.tree_item_action(subject, command),
                Action::TreeOperation(operation) => self.start_tree_operation(operation),
                Action::MoveRequest(id, target) => self.move_tree_request(id, target),
                Action::Select(id) => self.open(id),
                Action::Home => {
                    self.workspaces.manage = false;
                    self.ws.active = None;
                    self.home_tab = 0;
                    self.mark_changed();
                }
                Action::NewRequest(c, f) => {
                    self.show_dialog(Dialog::NewRequest(c, f));
                    self.entry.clear();
                    self.entry2 = "GET".into();
                }
                Action::NewTab(collection, kind) => self.new_transient_request(collection, kind),
                Action::ChangeRequestType(id, kind) => self.switch_request_type(&id, kind),
                Action::GenerateGrpcExample(id) => self.generate_grpc_example(&id, ctx),
                Action::NewCollection => self.create_collection_inline(),
                Action::Close(id) => {
                    if self.dirty(&id) {
                        self.show_dialog(Dialog::CloseTab(id));
                    } else {
                        self.close(&id);
                    }
                }
                Action::Save => self.save_active(),
                Action::Send => {
                    if let Some(id) = self.ws.active.clone() {
                        self.send(id, ctx);
                    }
                }
                Action::Cancel => {
                    if let Some(id) = self.ws.active.clone() {
                        self.cancel(&id);
                    }
                }
                Action::ToggleTheme => {
                    self.ws.dark = !self.ws.dark;
                    self.ws.preferences.appearance.mode =
                        if self.ws.dark { "dark" } else { "light" }.into();
                    self.theme =
                        crate::theme_registry::current(&self.ws.preferences, self.ws.dark, ctx);
                    self.theme.apply(ctx);
                    self.mark_changed();
                }
                Action::ToggleSplit => {
                    self.ws.side_by_side = !self.ws.side_by_side;
                    self.mark_workspace_changed();
                    ctx.request_repaint();
                }
                Action::Dialog(d) => self.show_dialog(d),
                Action::OpenCollection => {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Open collection")
                        .pick_folder()
                    {
                        let tx = self.tx.clone();
                        let ctx = ctx.clone();
                        self.notice("Opening collection…");
                        self.runtime.spawn_blocking(move || {
                            let result = storage::open_collection(&path)
                                .map(LoadedCollection::new)
                                .map_err(|e| format!("Open failed: {e:#}"));
                            let _ = tx.send(Event::Collection(result));
                            ctx.request_repaint();
                        });
                    }
                }
                Action::ImportPostman => self.import_collections(ctx),
                Action::ExportPostman(collection) => {
                    let cid = collection.unwrap_or_else(|| self.active_collection_id());
                    self.export_collections(vec![cid], false, ctx);
                }
                Action::ExportCollections(ids) => self.export_collections(ids, true, ctx),
                Action::NewFolder(cid) => {
                    self.show_dialog(Dialog::TreeNewFolder(cid, String::new()));
                    self.entry.clear();
                }
                Action::RunCollection => {
                    self.show_dialog(Dialog::Runner);
                }
            }
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        for id in self.ws.open_tabs.clone() {
            self.commit_environment_name(&id);
        }
        if let Some(job) = &self.git_clone {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        for state in self.git.values() {
            if let Some(cancel) = &state.cancel {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if self.tree_moving {
            if self.tree_menu.operating {
                if let Ok(result) = self.writer.operated.recv() {
                    self.finish_tree_operation(result);
                }
            } else if let Ok(result) = self.writer.moved.recv() {
                self.finish_tree_move(result);
            }
        }
        for (_, (_, handle)) in self.running.drain() {
            handle.abort();
        }
        self.curl_flush();
        self.flush();
        if let Err(error) = self.engine.cookies.save(&self.root) {
            eprintln!("Could not save cookies: {error:#}");
        }
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll(ctx);
        for app in self.workspaces.inactive.values_mut() {
            app.poll(ctx);
        }
        if !self.workspaces.updates.locked() {
            self.workspace_tick(ctx);
            self.preferences_tick(ctx);
        }
        self.updates_tick(ctx);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let frame_start = Instant::now();
        let ctx = ui.ctx().clone();
        if !self.workspaces.updates.locked() {
            self.handle_shortcuts(&ctx);
        }
        let scope = self.workspaces.registry.active.clone();
        ui.add_enabled_ui(!self.workspaces.updates.locked(), |ui| {
            ui.push_id(("workspace", scope), |ui| self.draw(ui));
        });
        if !self.workspaces.updates.locked() {
            self.dialogs(&ctx);
            self.workspace_dialogs(&ctx);
            self.ai_dialog(&ctx);
            self.handle_actions(&ctx);
        }
        self.updates_dialog(&ctx);
        crate::window_frame::resize(ui);
        if self.frame_times.len() >= 120 {
            self.frame_times.pop_front();
        }
        self.frame_times
            .push_back(frame_start.elapsed().as_secs_f32() * 1000.);
        if self.capture_path.is_some() {
            if self.started.elapsed() > Duration::from_secs(2) {
                let path = self.capture_path.take().unwrap();
                let context = ctx.clone();
                let exit = self.capture_exit;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::ScreenshotCallback::new(move |image| {
                        let pixels = image
                            .pixels
                            .iter()
                            .flat_map(|c| c.to_array())
                            .collect::<Vec<_>>();
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(error) = image::save_buffer(
                            &path,
                            &pixels,
                            image.size[0] as u32,
                            image.size[1] as u32,
                            image::ColorType::Rgba8,
                        ) {
                            eprintln!("Screenshot failed: {error}");
                        }
                        if exit {
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }),
                ));
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }
}

impl App {
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.devtools_open && self.devtools_tab == 3 && self.terminal.has_focus(ctx) {
            return;
        }
        if self.preference_ui.key_capture.is_some() {
            return;
        }
        self.tree_menu_shortcuts(ctx);
        self.collection_menu_shortcuts(ctx);
        self.configurable_shortcuts(ctx);
        let escape = ctx.input_mut(|i| {
            let previous = self.ws.preferences.keybindings.enabled
                && i.consume_key(
                    egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    egui::Key::Tab,
                );
            let next = self.ws.preferences.keybindings.enabled
                && !previous
                && i.consume_key(egui::Modifiers::CTRL, egui::Key::Tab);
            if (previous || next) && !self.ws.open_tabs.is_empty() {
                let n = self.ws.open_tabs.len();
                let current = self
                    .ws
                    .active
                    .as_ref()
                    .and_then(|id| self.ws.open_tabs.iter().position(|s| s == id))
                    .unwrap_or(0);
                let index = if previous {
                    (current + n - 1) % n
                } else {
                    (current + 1) % n
                };
                self.actions
                    .push(Action::Select(self.ws.open_tabs[index].clone()));
            }
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
        });
        // Context access must happen after input_mut releases its write lock.
        if escape {
            self.preference_ui.closing_tabs.clear();
            if let Some(id) = ctx.data_mut(|d| {
                let key = egui::Id::unique("ai-active-ghost");
                let value = d.get_temp::<egui::Id>(key);
                d.remove::<egui::Id>(key);
                value
            }) {
                ctx.data_mut(|d| d.remove::<crate::ai_ui::Ghost>(id));
            }
            self.dialog = Dialog::None;
            egui::Popup::close_all(ctx);
            crate::tree_drag::cancel_pending(ctx);
            crate::request_tabs::cancel_drag(ctx);
        }
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn escape_closes_popups_and_dialogs_without_locking_the_ui() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, root.path().to_owned()).unwrap();
        let popup = egui::Id::unique("escape-regression");
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {
            egui::Popup::open_id(&ctx, popup);
            app.dialog = Dialog::Environments;
        });
        output.textures_delta.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: Some(egui::Key::Escape),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |_| {
                app.handle_shortcuts(&ctx);
                assert!(app.dialog == Dialog::None);
                assert!(!egui::Popup::is_id_open(&ctx, popup));
                // Access both input and temporary data again in the same frame.
                assert!(!ctx.input(|i| i.key_pressed(egui::Key::Escape)));
                ctx.data_mut(|d| d.insert_temp(popup, true));
            },
        );
        output.textures_delta.clear();
    }
}
