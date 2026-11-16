//! Portable workspaces and a local registry. Removing an entry never deletes its files.
use crate::{
    model::{self, Environment, Pair, Workspace},
    storage,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub pinned: bool,
}
impl Entry {
    pub fn state_root(&self) -> PathBuf {
        if self.default {
            self.path.clone()
        } else {
            self.path.join(".reqon")
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    pub active: String,
    pub entries: Vec<Entry>,
}
impl Registry {
    pub fn load(data: &Path) -> Result<Self> {
        let mut registry: Self = if data.join("workspaces.json").is_file() {
            serde_json::from_slice(&fs::read(data.join("workspaces.json"))?)?
        } else {
            Self::default()
        };
        if !registry.entries.iter().any(|e| e.default) {
            let name = storage::load(data)?.name;
            registry.entries.insert(
                0,
                Entry {
                    id: "default".into(),
                    name,
                    path: fs::canonicalize(data)?,
                    default: true,
                    pinned: false,
                },
            );
        }
        if !registry.entries.iter().any(|e| e.id == registry.active) {
            registry.active = "default".into();
        }
        Ok(registry)
    }
    pub fn save(&self, data: &Path) -> Result<()> {
        storage::atomic_write(
            &data.join("workspaces.json"),
            serde_json::to_vec_pretty(self)?,
        )
    }
    pub fn register(&mut self, path: &Path) -> Result<String> {
        let path = fs::canonicalize(path)?;
        if let Some(e) = self.entries.iter().find(|e| e.path == path) {
            return Ok(e.id.clone());
        }
        let manifest = read_manifest(&path)?;
        let id = model::id();
        self.entries.push(Entry {
            id: id.clone(),
            name: manifest.name,
            path,
            default: false,
            pinned: false,
        });
        Ok(id)
    }
    pub fn ordered(&self) -> Vec<Entry> {
        let mut entries = self.entries.clone();
        entries.sort_by_key(|e| (!e.default, !e.pinned));
        entries
    }
    pub fn reorder(&mut self, source: &str, target: &str) {
        let Some(a) = self.entries.iter().position(|e| e.id == source) else {
            return;
        };
        let Some(b) = self.entries.iter().position(|e| e.id == target) else {
            return;
        };
        if a == b
            || self.entries[a].default
            || self.entries[b].default
            || self.entries[a].pinned != self.entries[b].pinned
        {
            return;
        }
        let entry = self.entries.remove(a);
        self.entries.insert(b, entry);
    }
    pub fn remove(&mut self, id: &str) -> Result<()> {
        ensure!(
            !self.entries.iter().any(|e| e.id == id && e.default),
            "The default workspace cannot be removed"
        );
        self.entries.retain(|e| e.id != id);
        if self.active == id {
            self.active = "default".into();
        }
        Ok(())
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub version: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub collections: Vec<CollectionLink>,
    pub docs: String,
    pub globals: Vec<Pair>,
    pub environments: Vec<Environment>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CollectionLink {
    pub name: String,
    pub path: PathBuf,
}
pub fn read_manifest(folder: &Path) -> Result<Manifest> {
    let native = folder.join("reqon.json");
    if native.is_file() {
        let m: Manifest = serde_json::from_slice(&fs::read(native)?)?;
        ensure!(
            m.kind == "workspace",
            "This reqon.json describes a collection, not a workspace"
        );
        return Ok(m);
    }
    let file = folder.join("workspace.yml");
    ensure!(
        file.is_file(),
        "Choose a workspace folder containing reqon.json or workspace.yml"
    );
    let value: serde_json::Value = serde_yaml::from_slice(&fs::read(file)?)?;
    ensure!(
        value["info"]["type"].as_str() == Some("workspace"),
        "Invalid workspace.yml"
    );
    let mut m = Manifest {
        version: 1,
        kind: "workspace".into(),
        name: value["info"]["name"].as_str().unwrap_or("Workspace").into(),
        docs: value["docs"].as_str().unwrap_or_default().into(),
        ..Default::default()
    };
    if let Some(collections) = value["collections"].as_array() {
        for c in collections {
            if let Some(path) = c["path"].as_str() {
                m.collections.push(CollectionLink {
                    name: c["name"].as_str().unwrap_or_default().into(),
                    path: path.into(),
                });
            }
        }
    }
    // Bruno stores workspace environments as .bru/.yml files beside its config.
    let environments = folder.join("environments");
    if environments.is_dir() {
        for entry in fs::read_dir(environments)? {
            let path = entry?.path();
            if path.is_file() {
                if let Ok(env) = crate::environments::import_environment(&path) {
                    m.environments.push(env);
                }
            }
        }
    }
    Ok(m)
}
pub fn manifest(ws: &Workspace, folder: &Path) -> Manifest {
    Manifest {
        version: 1,
        kind: "workspace".into(),
        name: ws.name.clone(),
        docs: ws.docs.clone(),
        globals: ws.globals.clone(),
        environments: ws.environments.clone(),
        collections: ws
            .collections
            .iter()
            .map(|c| CollectionLink {
                name: c.name.clone(),
                path: c.path.strip_prefix(folder).unwrap_or(&c.path).into(),
            })
            .collect(),
    }
}
pub fn write_manifest(folder: &Path, manifest: &Manifest) -> Result<()> {
    storage::atomic_write(
        &folder.join("reqon.json"),
        serde_json::to_vec_pretty(manifest)?,
    )
}
pub fn save_portable(root: &Path, ws: &Workspace) -> Result<()> {
    if root.file_name().is_some_and(|n| n == ".reqon") {
        if let Some(folder) = root.parent() {
            write_manifest(folder, &manifest(ws, folder))?;
        }
    }
    Ok(())
}
pub fn create(parent: &Path, folder: &str, name: &str) -> Result<PathBuf> {
    ensure!(!name.trim().is_empty(), "Enter a workspace name");
    ensure!(
        !folder.trim().is_empty()
            && model::safe_name(folder) == folder
            && folder != "."
            && folder != "..",
        "Enter a valid folder name"
    );
    ensure!(parent.is_dir(), "Choose an existing location");
    let destination = parent.join(folder);
    ensure!(
        !destination.exists(),
        "A folder with this name already exists"
    );
    fs::create_dir(&destination)?;
    let ws = Workspace {
        name: name.trim().into(),
        ..Default::default()
    };
    storage::save(&destination.join(".reqon"), &ws)?;
    fs::create_dir(destination.join("collections"))?;
    Ok(fs::canonicalize(destination)?)
}
pub fn prepare(entry: &Entry) -> Result<()> {
    if entry.default {
        return Ok(());
    }
    let manifest = read_manifest(&entry.path)?;
    let root = entry.state_root();
    if root.join("workspace.json").exists() {
        return Ok(());
    }
    let mut ws = Workspace {
        name: manifest.name,
        docs: manifest.docs,
        globals: manifest.globals,
        environments: manifest.environments,
        ..Default::default()
    };
    for link in manifest.collections {
        let path = if link.path.is_absolute() {
            link.path
        } else {
            entry.path.join(link.path)
        };
        let (collection, requests, warnings) = storage::open_collection(&path)
            .with_context(|| format!("Cannot open collection {}", path.display()))?;
        ensure!(
            warnings.is_empty(),
            "Collection {} has unreadable files: {}",
            collection.name,
            warnings.join("; ")
        );
        ws.collections.push(collection);
        ws.requests.extend(requests);
    }
    let (_, warnings) = crate::native_format::migrate_workspace(&mut ws);
    ensure!(warnings.is_empty(), "{}", warnings.join("; "));
    storage::save(&root, &ws)
}
