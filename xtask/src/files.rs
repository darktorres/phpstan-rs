//! File helpers shared by the tasks.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Collects every `.php` file below `dir` as a `/`-separated path relative
/// to `base`, sorted in byte order of that spelling.
pub(crate) fn php_files(base: &Path, dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    walk(base, dir, &mut out)?;
    out.sort_unstable();
    Ok(out)
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|error| format!("{}: {error}", dir.display()))?;
    for path in entries {
        if path.is_dir() {
            walk(base, &path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "php") {
            let relative = path.strip_prefix(base).expect("walked below base");
            let parts: Vec<_> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.push(parts.join("/"));
        }
    }
    Ok(())
}

/// Writes `bytes` to a temporary file beside `path`, then renames it over
/// `path`, so a failed write never leaves a truncated file.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    fs::write(&temporary, bytes).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The platform's per-user cache directory: `$XDG_CACHE_HOME` or
/// `~/.cache` on Linux and the BSDs, `~/Library/Caches` on macOS and
/// `%LOCALAPPDATA%` on Windows.
pub(crate) fn cache_dir() -> Result<PathBuf, String> {
    let var = |name: &str| {
        env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME")
            .filter(|dir| dir.is_absolute())
            .or_else(|| var("HOME").map(|home| home.join(".cache")))
    };
    dir.ok_or_else(|| "no cache directory: set HOME (LOCALAPPDATA on Windows)".to_owned())
}
