//! Sorting paths in the order `Path` compares them in, by a key made once
//! for each.
//!
//! `Path` compares component by component, so `a/b` comes before `a-b`
//! though `-` is a lower byte than `/`, and every comparison walks both
//! paths' components. The key is each component as a byte for its kind
//! (the order the kinds are declared in), its bytes (a prefix's as
//! `push_prefix` writes them) and a zero byte, which no file name holds:
//! comparing keys as bytes is comparing the component sequences.

use std::ffi::OsStr;
use std::path::{Component, Path, Prefix};

/// The key `Path`'s order is the byte order of.
fn key(path: &Path) -> Vec<u8> {
    let mut key = Vec::with_capacity(path.as_os_str().len() + 8);
    for component in path.components() {
        key.push(match component {
            Component::Prefix(_) => 1,
            Component::RootDir => 2,
            Component::CurDir => 3,
            Component::ParentDir => 4,
            Component::Normal(_) => 5,
        });
        match component {
            Component::Prefix(prefix) => push_prefix(&mut key, prefix.kind()),
            component => key.extend_from_slice(component.as_os_str().as_encoded_bytes()),
        }
        key.push(0);
    }
    key
}

/// `prefix` as `Path` orders prefixes: by the kind std reads, in the order the
/// kinds are declared, then by what the kind holds, each name ending in a zero
/// byte as a component does. Its text is not its order: `c:` is `C:`.
fn push_prefix(key: &mut Vec<u8>, prefix: Prefix<'_>) {
    let push_name = |key: &mut Vec<u8>, name: &OsStr| {
        key.extend_from_slice(name.as_encoded_bytes());
        key.push(0);
    };
    match prefix {
        Prefix::Verbatim(name) => {
            key.push(0);
            push_name(key, name);
        }
        Prefix::VerbatimUNC(server, share) => {
            key.push(1);
            push_name(key, server);
            push_name(key, share);
        }
        Prefix::VerbatimDisk(drive) => key.extend([2, drive]),
        Prefix::DeviceNS(device) => {
            key.push(3);
            push_name(key, device);
        }
        Prefix::UNC(server, share) => {
            key.push(4);
            push_name(key, server);
            push_name(key, share);
        }
        Prefix::Disk(drive) => key.extend([5, drive]),
    }
}

/// How many out-of-order neighbours a list may have and still be sorted by the
/// paths themselves, which merges its runs, rather than by a key for each path.
const MERGED_RUNS: usize = 16;

/// Sorts `items` stably by the path `path` gives each, in the order `Path`
/// compares paths. A list already in order is left alone; a few runs in order
/// are merged by the paths themselves; any other list is sorted by a key made
/// once for each path.
pub fn sort_by_path<T>(items: &mut [T], path: impl Fn(&T) -> &Path) {
    let descents = items
        .windows(2)
        .filter(|pair| path(&pair[1]) < path(&pair[0]))
        .count();
    if descents == 0 {
        return;
    }
    if descents < MERGED_RUNS {
        items.sort_by(|a, b| path(a).cmp(path(b)));
    } else {
        items.sort_by_cached_key(|item| key(path(item)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A repeatable mix of paths that differ where byte order and component
    /// order differ: separators against `-`, `.` and a space, doubled and
    /// trailing separators, dot components, absolute and relative.
    fn paths(count: usize) -> Vec<PathBuf> {
        let parts = [
            "a",
            "a-b",
            "a.b",
            "a b",
            "ab",
            "b",
            ".",
            "..",
            "é",
            "a\u{10348}",
            "vendor",
            "",
        ];
        let mut state = 0x853C_49E6_748F_EA9Bu64;
        let mut next = move |bound: usize| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((state >> 33) as usize) % bound
        };
        (0..count)
            .map(|_| {
                let mut text = match next(4) {
                    0 => String::from("/"),
                    1 => String::from("./"),
                    _ => String::new(),
                };
                for i in 0..1 + next(5) {
                    if i > 0 {
                        text.push_str(if next(6) == 0 { "//" } else { "/" });
                    }
                    text.push_str(parts[next(parts.len())]);
                }
                if next(5) == 0 {
                    text.push('/');
                }
                PathBuf::from(text)
            })
            .collect()
    }

    #[test]
    fn sorted_by_key_is_sorted_by_path() {
        let all = paths(3000);
        let mut by_key = all.clone();
        sort_by_path(&mut by_key, PathBuf::as_path);
        let mut by_path = all;
        by_path.sort();
        assert_eq!(by_key, by_path);
    }

    #[test]
    fn the_key_order_is_the_path_order_for_every_pair() {
        let all = paths(300);
        for a in &all {
            for b in &all {
                assert_eq!(
                    key(a).cmp(&key(b)),
                    a.cmp(b),
                    "{} against {}",
                    a.display(),
                    b.display()
                );
            }
        }
    }

    /// On Windows, paths under each kind of prefix, in the spellings `Path`
    /// reads alike, with names that order one way as text and the other as
    /// components (`server` and `server-b`).
    #[cfg(windows)]
    #[test]
    fn the_key_orders_prefixes_as_a_path_does() {
        let prefixes = [
            r"C:\",
            "c:/",
            "C:",
            "d:",
            r"\\server\share\",
            "//server/share/",
            r"\\server\share-b\",
            r"\\server-b\share\",
            r"\\?\C:\",
            r"\\?\c:\",
            r"\\?\UNC\server\share\",
            r"\\?\UNC\server-b\share\",
            r"\\?\Volume{a}\",
            r"\\?\Volume{a-b}\",
            r"\\.\pipe\",
            r"\\.\pipe-b\",
            r"\",
            "",
        ];
        let tails = ["", "a", "a-b", r"a\b", "a/b", "..", "."];
        let all: Vec<PathBuf> = prefixes
            .iter()
            .flat_map(|prefix| {
                tails
                    .iter()
                    .map(move |tail| PathBuf::from(format!("{prefix}{tail}")))
            })
            .collect();
        for a in &all {
            for b in &all {
                assert_eq!(
                    key(a).cmp(&key(b)),
                    a.cmp(b),
                    "{} against {}",
                    a.display(),
                    b.display()
                );
            }
        }
    }

    #[test]
    fn a_separator_orders_before_a_dash_as_a_path_does() {
        let mut list = vec![PathBuf::from("a-b"), PathBuf::from("a/b")];
        sort_by_path(&mut list, PathBuf::as_path);
        assert_eq!(list, [PathBuf::from("a/b"), PathBuf::from("a-b")]);
    }
}
