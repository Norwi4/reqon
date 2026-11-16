use crate::{
    model::{self, Workspace},
    workspace_store::{self, Manifest},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
fn allowed(path: &Path) -> bool {
    path.components().all(|p| match p {
        Component::Normal(n) => !matches!(
            n.to_string_lossy().to_ascii_lowercase().as_str(),
            ".git"
                | "node_modules"
                | ".reqon"
                | ".trash"
                | "secrets.bin"
                | ".reqon-migration-backup"
        ),
        _ => false,
    })
}
fn walk(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        let path = item.path();
        let relative = path.strip_prefix(root)?.to_path_buf();
        if !allowed(&relative) || item.file_type()?.is_symlink() {
            continue;
        }
        if item.file_type()?.is_dir() {
            walk(root, &path, files)?;
        } else if item.file_type()?.is_file() {
            files.push((path, relative));
            ensure!(
                files.len() <= MAX_FILES,
                "Workspace has too many files to export"
            );
        }
    }
    Ok(())
}
pub fn export(ws: &Workspace, destination: &Path) -> Result<()> {
    let mut manifest = workspace_store::manifest(ws, Path::new(""));
    manifest.collections.clear();
    let mut files = vec![];
    let mut names = std::collections::HashSet::new();
    for c in &ws.collections {
        let mut folder = model::safe_name(&c.name);
        let base = folder.clone();
        let mut n = 2;
        while !names.insert(folder.to_lowercase()) {
            folder = format!("{base} {n}");
            n += 1;
        }
        let relative = PathBuf::from("collections").join(folder);
        manifest.collections.push(workspace_store::CollectionLink {
            name: c.name.clone(),
            path: relative.clone(),
        });
        let mut entries = vec![];
        walk(&c.path, &c.path, &mut entries)?;
        files.extend(entries.into_iter().map(|(p, r)| (p, relative.join(r))));
    }
    let temporary = destination.with_extension(format!("{}.tmp", model::id()));
    let result = (|| -> Result<()> {
        let mut archive = ZipWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?,
        );
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        archive.start_file("reqon.json", options)?;
        archive.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        let mut total = 0;
        for (source, relative) in files {
            let size = fs::metadata(&source)?.len();
            total += size;
            ensure!(
                total <= MAX_BYTES,
                "Workspace exceeds the 1 GiB export limit"
            );
            archive.start_file(relative.to_string_lossy().replace('\\', "/"), options)?;
            let environment_file = source
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == "environments");
            let request_yaml = crate::yaml::is_yaml(&source) && !environment_file && {
                let value = serde_yaml::from_slice::<serde_json::Value>(&fs::read(&source)?)
                    .unwrap_or_default();
                value.get("opencollection").is_some() || value.pointer("/info/type").is_some()
            };
            if (crate::native_format::is_source(&source) && !environment_file) || request_yaml {
                // Export a protected representation without rewriting the user's
                // original collection (which may predate the Auth vault).
                let request = if source.file_name().is_some_and(|n| {
                    matches!(
                        n.to_str(),
                        Some("collection.req" | "collection.bru" | "folder.req" | "folder.bru")
                    )
                }) {
                    crate::inheritance::parent(&source)?
                        .context("Cannot read collection metadata")?
                } else {
                    crate::yaml::read_request(&source)?
                };
                archive.write_all(crate::storage::source(&request)?.as_bytes())?;
            } else {
                std::io::copy(&mut fs::File::open(source)?, &mut archive)?;
            }
        }
        archive.finish()?.sync_all()?;
        fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn import(source: &Path, parent: &Path) -> Result<PathBuf> {
    ensure!(parent.is_dir(), "Choose an existing extract location");
    let temp = parent.join(format!(".reqon-import-{}", model::id()));
    fs::create_dir(&temp)?;
    // Only this newly created directory is cleaned up on failure.
    let result = (|| -> Result<PathBuf> {
        let mut archive = ZipArchive::new(fs::File::open(source)?)?;
        ensure!(
            archive.len() <= MAX_FILES,
            "Archive contains too many entries"
        );
        let mut total = 0u64;
        let mut names = std::collections::HashSet::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let relative = file
                .enclosed_name()
                .context("Archive contains an unsafe path")?;
            ensure!(
                relative
                    .components()
                    .all(|p| matches!(p, Component::Normal(_))),
                "Archive contains an unsafe path"
            );
            ensure!(
                !relative.to_string_lossy().contains(':'),
                "Archive contains an invalid file name"
            );
            ensure!(
                !file.is_symlink(),
                "Workspace archives cannot contain symbolic links"
            );
            if !allowed(&relative) {
                continue;
            }
            ensure!(
                names.insert(relative.to_string_lossy().to_lowercase()),
                "Archive contains duplicate file names"
            );
            total = total
                .checked_add(file.size())
                .context("Archive is too large")?;
            ensure!(
                total <= MAX_BYTES,
                "Archive exceeds the 1 GiB extraction limit"
            );
            let output = temp.join(relative);
            if file.is_dir() {
                fs::create_dir_all(&output)?;
            } else {
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent)?;
                }
                let size = file.size();
                let actual = std::io::copy(
                    &mut (&mut file).take(size + 1),
                    &mut fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(output)?,
                )?;
                ensure!(actual == size, "Archive entry has an invalid size");
            }
        }
        let workspace = if temp.join("reqon.json").exists() || temp.join("workspace.yml").exists() {
            temp.clone()
        } else {
            let children: Vec<_> = fs::read_dir(&temp)?.filter_map(Result::ok).collect();
            ensure!(
                children.len() == 1 && children[0].path().is_dir(),
                "Archive does not contain a workspace"
            );
            children[0].path()
        };
        let manifest = workspace_store::read_manifest(&workspace)?;
        validate_links(&workspace, &manifest)?;
        let base = model::safe_name(&manifest.name);
        let mut destination = parent.join(&base);
        let mut n = 2;
        while destination.exists() {
            destination = parent.join(format!("{base} ({n})"));
            n += 1;
        }
        fs::rename(&workspace, &destination)?;
        Ok(destination)
    })();
    if temp.exists() {
        let _ = fs::remove_dir_all(&temp);
    }
    result
}
fn validate_links(root: &Path, m: &Manifest) -> Result<()> {
    let root = fs::canonicalize(root)?;
    for c in &m.collections {
        ensure!(
            !c.path.is_absolute()
                && c.path
                    .components()
                    .all(|p| matches!(p, Component::Normal(_) | Component::CurDir)),
            "Imported collections must be inside the workspace archive"
        );
        let path = fs::canonicalize(root.join(&c.path))
            .with_context(|| format!("Missing collection {}", c.name))?;
        ensure!(
            path.starts_with(&root),
            "Collection path leaves the workspace archive"
        );
    }
    Ok(())
}
