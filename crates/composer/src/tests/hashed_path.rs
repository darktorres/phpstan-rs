//! A path's hashes are its components': a path made by `new`, by `child` or as
//! an ancestor has the hash `path_hash` gives it, and so does its parent.

use crate::hashed_path::{HashedPath, hashed_ancestors, path_hash};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// A repeatable mix of paths: absolute and relative, with doubled and
/// trailing separators, dot components and names that are not ASCII.
fn paths() -> Vec<PathBuf> {
    let parts = ["a", "b", "a-b", ".", "..", "é", "vendor", ""];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |bound: usize| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 33) as usize) % bound
    };
    (0..400)
        .map(|_| {
            let mut text = match next(3) {
                0 => String::from("/"),
                1 => String::from("./"),
                _ => String::new(),
            };
            for i in 0..next(5) {
                if i > 0 {
                    text.push_str(if next(5) == 0 { "//" } else { "/" });
                }
                text.push_str(parts[next(parts.len())]);
            }
            if next(4) == 0 {
                text.push('/');
            }
            PathBuf::from(text)
        })
        .collect()
}

#[test]
fn a_path_and_its_parent_have_the_hashes_of_their_components() {
    for path in paths() {
        let hashed = HashedPath::new(path.clone());
        assert_eq!(hashed.hash(), path_hash(&path), "{}", path.display());
        assert_eq!(
            hashed.parent(),
            path.parent().map(|parent| (parent, path_hash(parent))),
            "{}",
            path.display()
        );
    }
}

#[test]
fn paths_equal_as_paths_are_equal_and_hash_alike() {
    let a = HashedPath::new(PathBuf::from("/a/b/"));
    let b = HashedPath::new(PathBuf::from("/a//b"));
    assert_eq!(a, b);
    assert_eq!(a.hash(), b.hash());
    assert_ne!(a, HashedPath::new(PathBuf::from("/a/c")));
    assert_ne!(path_hash(Path::new("/a/b")), path_hash(Path::new("/a/c")));
}

#[test]
fn an_entry_made_from_its_directory_is_the_path_made_whole() {
    for dir in paths() {
        let dir = HashedPath::new(dir);
        for name in ["a", "b.php", "é"] {
            let child = dir.child(OsStr::new(name));
            let whole = HashedPath::new(dir.as_path().join(name));
            assert_eq!(child, whole, "{}", whole.as_path().display());
            assert_eq!(child.parent(), whole.parent());
        }
    }
}

#[test]
fn each_ancestor_has_its_own_hash_from_the_top_down() {
    for path in paths() {
        let mut expected: Vec<(&Path, u64)> = path
            .ancestors()
            .map(|ancestor| (ancestor, path_hash(ancestor)))
            .collect();
        expected.reverse();
        assert_eq!(hashed_ancestors(&path), expected, "{}", path.display());
    }
}

#[test]
fn paths_order_as_paths_do() {
    let mut paths = paths();
    let mut hashed: Vec<HashedPath> = paths.iter().cloned().map(HashedPath::new).collect();
    paths.sort();
    hashed.sort();
    let sorted: Vec<&Path> = hashed.iter().map(HashedPath::as_path).collect();
    let expected: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    assert_eq!(sorted, expected);
}
