//! A path's hashes are its components': a path made by `new`, by `child` or as
//! an ancestor has the hash `path_hash` gives it, and so does its parent.

use crate::hashed_path::{HashedPath, hashed_ancestors, path_hash};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// A repeatable mix of paths: absolute and relative, with doubled and
/// trailing separators, dot components and names that are not ASCII. On
/// Windows the absolute ones start with each kind of prefix, spelled in each
/// way `Path` reads alike, and both separators are used. The last separator
/// listed ends a path that ends in one.
fn paths() -> Vec<PathBuf> {
    #[cfg(not(windows))]
    let (starts, parts, separators) = (
        ["/", "./", ""].as_slice(),
        ["a", "b", "a-b", ".", "..", "é", "vendor", ""].as_slice(),
        ["//", "/", "/", "/", "/"].as_slice(),
    );
    // No empty name: after a root it makes `\\a`, which pushing a name
    // turns into a share (`\\a\b`).
    #[cfg(windows)]
    let (starts, parts, separators) = (
        [
            r"C:\",
            "c:/",
            "C:",
            r"\\server\share\",
            "//server/share/",
            r"\\?\C:\",
            r"\\?\c:\",
            r"\\?\UNC\server\share\",
            r"\",
            "/",
            r".\",
            "./",
            "",
        ]
        .as_slice(),
        ["a", "b", "a-b", ".", "..", "é", "vendor"].as_slice(),
        [r"\\", "//", "/", r"\", r"\"].as_slice(),
    );
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |bound: usize| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 33) as usize) % bound
    };
    (0..400)
        .map(|_| {
            let mut text = String::from(starts[next(starts.len())]);
            for i in 0..next(5) {
                if i > 0 {
                    text.push_str(separators[next(separators.len())]);
                }
                text.push_str(parts[next(parts.len())]);
            }
            if next(4) == 0 {
                text.push_str(separators[separators.len() - 1]);
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
        assert_eq!(
            hashed.split(),
            path.parent().zip(path.file_name()).map(|(parent, name)| (
                parent,
                path_hash(parent),
                name
            )),
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

/// On Windows a prefix hashes as `Path` compares it: a drive letter in either
/// case and a share with either separator hash alike, through `path_hash`,
/// `new` and each ancestor, and prefixes `Path` tells apart hash apart.
#[cfg(windows)]
#[test]
fn prefixes_equal_as_paths_hash_alike() {
    let alike: [&[&str]; 5] = [
        &[r"C:\a\b", r"c:\a\b", "c:/a/b", r"C:/a\b\"],
        &["C:a", "c:a"],
        &[r"\\server\share\a", "//server/share/a", r"\\server/share\a"],
        &[r"\\?\C:\a", r"\\?\c:\a"],
        &[r"\\?\UNC\server\share\a", r"\\?\UNC\server\share\\a"],
    ];
    let hashes =
        |path: &Path| -> Vec<u64> { hashed_ancestors(path).map(|(_, hash, _)| hash).collect() };
    for spellings in alike {
        let first = Path::new(spellings[0]);
        for spelling in &spellings[1..] {
            let path = Path::new(spelling);
            assert_eq!(path, first, "{spelling}");
            assert_eq!(path_hash(path), path_hash(first), "{spelling}");
            assert_eq!(
                HashedPath::new(path.to_path_buf()),
                HashedPath::new(first.to_path_buf()),
                "{spelling}"
            );
            assert_eq!(
                HashedPath::new(path.to_path_buf()).hash(),
                path_hash(first),
                "{spelling}"
            );
            assert_eq!(hashes(path), hashes(first), "{spelling}");
        }
    }
    let apart = [
        r"C:\a",
        r"D:\a",
        "C:a",
        r"\\?\C:\a",
        r"\\server\share\a",
        r"\\server\other\a",
        r"\\other\share\a",
        r"\\?\UNC\server\share\a",
        r"\\?\unc\server\share\a",
        r"\\.\server\share\a",
        r"\a",
    ];
    for (i, a) in apart.iter().enumerate() {
        for b in &apart[i + 1..] {
            assert_ne!(Path::new(a), Path::new(b), "{a} against {b}");
            assert_ne!(
                path_hash(Path::new(a)),
                path_hash(Path::new(b)),
                "{a} against {b}"
            );
        }
    }
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
fn each_ancestor_has_its_own_hash_and_name_from_the_path_up() {
    for path in paths() {
        let expected: Vec<(&Path, u64, Option<&OsStr>)> = path
            .ancestors()
            .map(|ancestor| (ancestor, path_hash(ancestor), ancestor.file_name()))
            .collect();
        let found: Vec<(&Path, u64, Option<&OsStr>)> = hashed_ancestors(&path).collect();
        assert_eq!(found, expected, "{}", path.display());
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
