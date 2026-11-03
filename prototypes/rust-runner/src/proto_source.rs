use anyhow::{Result, bail};
use protox::file::{File, FileResolver, GoogleFileResolver, IncludeFileResolver};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

struct BoundedResolver {
    includes: Vec<PathBuf>,
    loaded: Arc<Mutex<(usize, Vec<PathBuf>)>>,
}
impl FileResolver for BoundedResolver {
    fn resolve_path(&self, path: &Path) -> Option<String> {
        let path = path.canonicalize().ok()?;
        self.includes
            .iter()
            .find_map(|dir| IncludeFileResolver::new(dir.clone()).resolve_path(&path))
    }
    fn open_file(&self, name: &str) -> Result<File, protox::Error> {
        let open = || -> Result<Option<File>> {
            if Path::new(name)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            {
                bail!("invalid proto import path");
            }
            for root in &self.includes {
                let path = root.join(name);
                if !path.exists() {
                    continue;
                }
                let path = path.canonicalize()?;
                if !path.starts_with(root) {
                    bail!("proto import escapes include directory");
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)?
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                let mut loaded = self.loaded.lock().unwrap();
                if bytes.len() > 1024 * 1024
                    || loaded.0 + bytes.len() > 8 * 1024 * 1024
                    || loaded.1.len() >= 512
                {
                    bail!("protobuf source budget exceeded (1 MiB/file, 8 MiB total, 512 files)");
                }
                loaded.0 += bytes.len();
                loaded.1.push(path);
                return Ok(Some(File::from_source(name, std::str::from_utf8(&bytes)?)?));
            }
            Ok(None)
        };
        match open() {
            Ok(Some(file)) => Ok(file),
            Ok(None) => GoogleFileResolver::new().open_file(name),
            Err(e) => Err(protox::Error::new(std::io::Error::other(e.to_string()))),
        }
    }
}

pub fn compile(
    files: &[PathBuf],
    includes: &[PathBuf],
) -> Result<(prost_types::FileDescriptorSet, Vec<PathBuf>)> {
    let loaded = Arc::new(Mutex::new((0, Vec::new())));
    let includes = includes
        .iter()
        .map(|p| p.canonicalize())
        .collect::<std::io::Result<Vec<_>>>()?;
    let resolver = BoundedResolver {
        includes,
        loaded: loaded.clone(),
    };
    let mut compiler = protox::Compiler::with_file_resolver(resolver);
    compiler
        .include_imports(true)
        .include_source_info(false)
        .open_files(files)?;
    let files = loaded.lock().unwrap().1.clone();
    Ok((compiler.file_descriptor_set(), files))
}
