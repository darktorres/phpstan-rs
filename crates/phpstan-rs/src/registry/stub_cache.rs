//! The built-in stubs' indexed and resolved forms, stored on disk once
//! built, for every later process to load rather than index and resolve
//! the stub sources again. Each file is keyed by the build's source hash
//! (`build.rs`), so a build never reads what different code or stubs
//! stored; one that can't be read or written is built in memory as
//! before. A file (`container`) carries a checksum, so a truncated or
//! damaged one is rebuilt rather than read; one that passes it but doesn't
//! decode is a bug, and says so. `PHPSTAN_RS_NO_STUB_CACHE`
//! turns the cache off.

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use super::container::{Reader, Unreadable, Writer};

/// The key every cache file carries in its name. The crate hash covers the
/// analyzer and vendored runtime sources; `SOURCES_HASH` covers exactly the
/// PHP files embedded by `stub-sources`, including PHPStan's `.stub` files.
pub(crate) fn cache_key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        format!(
            "{}-{}",
            env!("PHPSTAN_RS_SOURCE_HASH"),
            stub_sources::SOURCES_HASH
        )
    })
}

/// `value` as the cache stores it: a compact encoding that does not
/// describe itself, so what reads it back must be what wrote it. Types nest
/// deeply, which `Type`'s own impls guard the stack for.
pub(super) fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let encode = || postcard::to_allocvec(value).map_err(|e| e.to_string());
    match syntax::name::current_cache_name_encoder() {
        Some(names) => syntax::name::with_cache_name_encoder(&names, encode),
        None => encode(),
    }
}

/// What `encode` wrote.
pub(super) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let decode = || postcard::from_bytes(bytes).map_err(|e| e.to_string());
    match syntax::name::current_cache_name_table() {
        Some(names) => syntax::name::with_cache_name_table(&names, decode),
        None => decode(),
    }
}

/// The cache directory: `PHPSTAN_RS_CACHE_DIR`, else `phpstan-rs` under
/// `XDG_CACHE_HOME`, `~/.cache`, or the platform's local app-data
/// directory; `None` when the cache is off or no such directory is known.
pub(crate) fn cache_dir() -> Option<PathBuf> {
    cache_dir_in(|name| std::env::var_os(name))
}

/// `cache_dir`, reading the environment through `var`, so the fallback
/// order can be tested without touching the real environment.
fn cache_dir_in(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if var("PHPSTAN_RS_NO_STUB_CACHE").is_some() {
        return None;
    }
    if let Some(dir) = var("PHPSTAN_RS_CACHE_DIR") {
        return Some(PathBuf::from(dir));
    }
    let base = var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .or_else(|| platform_cache_base(&var))?;
    Some(base.join("phpstan-rs"))
}

/// The local app-data directory on Windows, where a server launched by an
/// editor may have neither `HOME` nor `XDG_CACHE_HOME` set. Elsewhere the
/// `HOME` fallback above is the only one.
fn platform_cache_base(var: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if cfg!(windows) {
        var("LOCALAPPDATA").map(PathBuf::from).or_else(|| {
            var("USERPROFILE").map(|home| PathBuf::from(home).join("AppData").join("Local"))
        })
    } else {
        None
    }
}

fn path(name: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(format!("{name}-{}.cbor", cache_key())))
}

/// The file stored under `name` for this build, if it can be read: the
/// bytes of a file whole by its checksum and of this layout, else `None`,
/// for the caller to build it again.
pub(crate) fn load_container(name: &str) -> Option<Reader> {
    let path = path(name)?;
    match Reader::open(&path) {
        Ok(reader) => Some(reader),
        Err(Unreadable::Damaged) => None,
        Err(Unreadable::Inconsistent(reason)) => {
            // The checksum held, so this is not a damaged file: what was
            // stored can't be read back, which only a bug causes.
            debug_assert!(false, "the stored {name} no longer reads: {reason}");
            eprintln!(
                "phpstan-rs: the stored {name} no longer reads ({reason}); building it again"
            );
            None
        }
    }
}

/// Stores `writer`'s file under `name` for this build, pruning the oldest
/// of what other builds stored under it. Writes a temporary file first and
/// renames it, so a concurrent load never reads a partial file.
pub(crate) fn store_container(name: &str, writer: Writer) {
    let Some(target) = path(name) else {
        return;
    };
    let Some(dir) = target.parent() else {
        return;
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let temporary = dir.join(format!(
        "{name}-{}.{}.{}.tmp",
        cache_key(),
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let written = fs::write(&temporary, writer.finish()).is_ok();
    if !written || fs::rename(&temporary, &target).is_err() {
        let _ = fs::remove_file(&temporary);
        return;
    }
    prune(dir, name, &target);
}

/// The section of a stored value.
const VALUE: &str = "value";

/// What `store` stored under `name` for this build, if it can be read.
pub(crate) fn load<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    let reader = load_container(name)?;
    let names = reader.names().clone();
    let decoded = reader
        .section(VALUE)
        .map_err(|e| e.to_string())
        .and_then(|range| {
            syntax::name::with_cache_name_table(&names, || decode(&reader.buffer()[range]))
        });
    match decoded {
        Ok(value) => Some(value),
        Err(error) => {
            // The checksum held, so this is not a damaged file: what was
            // stored can't be read back, which only a bug causes.
            debug_assert!(false, "the stored {name} no longer decodes: {error}");
            eprintln!(
                "phpstan-rs: the stored {name} no longer decodes ({error}); building it again"
            );
            None
        }
    }
}

/// Stores `value` under `name` for this build, as a file of one section.
pub(crate) fn store<T: serde::Serialize>(name: &str, value: &T) {
    let mut writer = Writer::new();
    let Ok(body) = writer.encode(value) else {
        return;
    };
    if writer.section(VALUE, &body).is_ok() {
        store_container(name, writer);
    }
}

/// How many builds' files of one kind stay: a branch switch back to a
/// recent build finds its file instead of building again.
const KEPT_BUILDS: usize = 3;

/// Removes all but the `KEPT_BUILDS` most recently written files of kind
/// `name` in `dir`, `keep` among them.
fn prune(dir: &std::path::Path, name: &str, keep: &std::path::Path) {
    let prefix = format!("{name}-");
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            file_name.starts_with(&prefix) && file_name.ends_with(".cbor")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    for (_, path) in files
        .into_iter()
        .filter(|(_, path)| path != keep)
        .skip(KEPT_BUILDS - 1)
    {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod cache_dir_tests {
    use super::*;
    use rustc_hash::FxHashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: FxHashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn the_cache_is_off_when_disabled() {
        let var = env(&[("PHPSTAN_RS_NO_STUB_CACHE", "1"), ("HOME", "/home/u")]);
        assert_eq!(cache_dir_in(var), None);
    }

    #[test]
    fn an_explicit_directory_wins_over_everything() {
        let var = env(&[("PHPSTAN_RS_CACHE_DIR", "/tmp/x"), ("HOME", "/home/u")]);
        assert_eq!(cache_dir_in(var), Some(PathBuf::from("/tmp/x")));
    }

    #[test]
    fn xdg_then_home_then_the_platform_directory() {
        let xdg = env(&[("XDG_CACHE_HOME", "/xdg"), ("HOME", "/home/u")]);
        assert_eq!(cache_dir_in(xdg), Some(PathBuf::from("/xdg/phpstan-rs")));

        let home = env(&[("HOME", "/home/u")]);
        assert_eq!(
            cache_dir_in(home),
            Some(PathBuf::from("/home/u/.cache/phpstan-rs"))
        );
    }

    #[test]
    fn a_windows_server_without_home_uses_local_app_data() {
        let local = env(&[("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local")]);
        let found = cache_dir_in(local);
        if cfg!(windows) {
            assert_eq!(
                found,
                Some(PathBuf::from("C:\\Users\\u\\AppData\\Local").join("phpstan-rs"))
            );
        } else {
            assert_eq!(found, None);
        }
    }

    #[test]
    fn user_profile_is_the_last_windows_fallback() {
        let profile = env(&[("USERPROFILE", "C:\\Users\\u")]);
        let found = cache_dir_in(profile);
        if cfg!(windows) {
            assert_eq!(
                found,
                Some(
                    PathBuf::from("C:\\Users\\u")
                        .join("AppData")
                        .join("Local")
                        .join("phpstan-rs")
                )
            );
        } else {
            assert_eq!(found, None);
        }
    }
}
