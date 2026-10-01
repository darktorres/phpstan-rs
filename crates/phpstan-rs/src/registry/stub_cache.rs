//! The built-in stubs' indexed and resolved forms, stored on disk once
//! built, for every later process to load rather than index and resolve
//! the stub sources again. Each file is keyed by the build's source hash
//! (`build.rs`), so a build never reads what different code or stubs
//! stored; one that can't be read or written is built in memory as
//! before. A file starts with a checksum of the rest, so a truncated or
//! damaged one is rebuilt rather than read. `PHPSTAN_RS_NO_STUB_CACHE`
//! turns the cache off.

use std::fs;
use std::hash::Hasher;
use std::path::PathBuf;

/// The key every cache file carries in its name.
const KEY: &str = env!("PHPSTAN_RS_SOURCE_HASH");

/// How deep a stored value may nest: types nest far deeper than the CBOR
/// reader's default allows.
pub(super) const RECURSION_LIMIT: usize = 1 << 16;

/// The cache directory: `PHPSTAN_RS_CACHE_DIR`, else `phpstan-rs` under
/// `XDG_CACHE_HOME` or `~/.cache`; `None` when the cache is off.
fn cache_dir() -> Option<PathBuf> {
    if std::env::var_os("PHPSTAN_RS_NO_STUB_CACHE").is_some() {
        return None;
    }
    if let Some(dir) = std::env::var_os("PHPSTAN_RS_CACHE_DIR") {
        return Some(PathBuf::from(dir));
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("phpstan-rs"))
}

fn path(name: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(format!("{name}-{KEY}.cbor")))
}

fn checksum(body: &[u8]) -> [u8; 8] {
    let mut hasher = std::hash::DefaultHasher::new();
    hasher.write(body);
    hasher.finish().to_le_bytes()
}

/// What `store` stored under `name` for this build, if it can be read.
pub(crate) fn load<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    let bytes = fs::read(path(name)?).ok()?;
    let (sum, body) = bytes.split_at_checked(8)?;
    if sum != checksum(body) {
        return None;
    }
    ciborium::de::from_reader_with_recursion_limit(body, RECURSION_LIMIT).ok()
}

/// Stores `value` under `name` for this build, replacing what other
/// builds stored under it. Writes a temporary file first and renames it,
/// so a concurrent `load` never reads a partial file.
pub(crate) fn store<T: serde::Serialize>(name: &str, value: &T) {
    let Some(target) = path(name) else {
        return;
    };
    let Some(dir) = target.parent() else {
        return;
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let temporary = dir.join(format!("{name}-{KEY}.{}.tmp", std::process::id()));
    let mut body = Vec::new();
    let written = ciborium::ser::into_writer(value, &mut body).is_ok()
        && fs::write(&temporary, [checksum(&body).as_slice(), &body].concat()).is_ok();
    if !written || fs::rename(&temporary, &target).is_err() {
        let _ = fs::remove_file(&temporary);
        return;
    }
    let stale = format!("{name}-");
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with(&stale) && entry.path() != target && file_name.ends_with(".cbor") {
            let _ = fs::remove_file(entry.path());
        }
    }
}
