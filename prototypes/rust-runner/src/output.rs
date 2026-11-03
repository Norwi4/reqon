use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

fn normalize(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().context("report path needs a file name")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    if path.exists() && !path.is_file() {
        bail!("report destination must be a regular file");
    }
    Ok(parent.canonicalize()?.join(name))
}

fn same(a: &Path, b: &Path) -> Result<bool> {
    if a.exists() && b.exists() {
        return Ok(same_file::is_same_file(a, b)?);
    }
    #[cfg(windows)]
    {
        Ok(a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy()))
    }
    #[cfg(not(windows))]
    {
        Ok(a == b)
    }
}

/// Preflight destinations before network activity; protect input aliases/hard links.
/// This is not a lock against another process modifying paths after the preflight.
pub fn prepare(json: &Path, junit: &Path, inputs: &[PathBuf]) -> Result<(PathBuf, PathBuf)> {
    let json = normalize(json)?;
    let junit = normalize(junit)?;
    if same(&json, &junit)? {
        bail!("JSON and JUnit report paths must be different");
    }
    for source in inputs {
        if same(&json, source)? || same(&junit, source)? {
            bail!("report must not overwrite an input file");
        }
    }
    Ok((json, junit))
}

pub fn prepare_many(paths: &[PathBuf], inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in paths {
        let path = normalize(path)?;
        for other in out.iter().chain(inputs) {
            if same(&path, other)? {
                bail!("artifact paths must be distinct and must not overwrite inputs");
            }
        }
        out.push(path);
    }
    Ok(out)
}
