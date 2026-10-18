//! What `path_bytes` reads from a path's bytes is what `Path` reads from its
//! components, for paths of every shape.

use crate::manifest::lexically_normalized;
use crate::path_bytes;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// A repeatable mix of paths: absolute and relative, with doubled and
/// trailing separators, `.` and `..` components, and names with and without
/// a PHP extension.
fn paths() -> Vec<PathBuf> {
    let parts = [
        "a", "b.php", ".php", "..php", "x..php", "php", ".", "..", "", "é", "vendor", "a.b",
    ];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |bound: usize| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 33) as usize) % bound
    };
    (0..600)
        .map(|_| {
            let mut text = match next(4) {
                0 | 1 => String::from("/"),
                2 => String::from("./"),
                _ => String::new(),
            };
            for i in 0..next(6) {
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
fn a_name_and_a_parent_are_read_as_the_components_give_them() {
    for path in paths() {
        assert_eq!(
            path_bytes::file_name(&path),
            path.file_name(),
            "{}",
            path.display()
        );
        assert_eq!(
            path_bytes::parent(&path),
            path.parent(),
            "{}",
            path.display()
        );
    }
}

#[test]
fn a_prefix_is_read_as_the_components_give_it() {
    let paths = paths();
    for path in &paths {
        // Each path against its own ancestors, which are its prefixes, and
        // against each of the others.
        let mut bases: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
        bases.extend(path.ancestors().map(|ancestor| ancestor.join("a")));
        bases.extend(paths.iter().take(120).cloned());
        for base in &bases {
            assert_eq!(
                path_bytes::starts_with(path, base),
                path.starts_with(base),
                "{} against {}",
                path.display(),
                base.display()
            );
            assert_eq!(
                path_bytes::strip_prefix(path, base),
                path.strip_prefix(base).ok(),
                "{} against {}",
                path.display(),
                base.display()
            );
        }
    }
}

#[test]
fn a_normal_absolute_path_is_its_own_normal_form() {
    let mut normal = 0;
    for path in paths() {
        if path_bytes::is_normal_absolute(&path) {
            normal += 1;
            assert!(path.is_absolute(), "{}", path.display());
            assert_eq!(lexically_normalized(&path), path, "{}", path.display());
        }
    }
    assert!(normal > 50, "the mix holds normal paths: {normal}");
    assert!(path_bytes::is_normal_absolute(Path::new("/")));
    assert!(path_bytes::is_normal_absolute(Path::new("/a/b.php")));
    for path in ["a/b", "/a/../b", "/a/./b", "/a//b", "/a/", ""] {
        assert!(!path_bytes::is_normal_absolute(Path::new(path)), "{path}");
    }
}

#[test]
fn a_php_name_is_one_whose_extension_is_php() {
    let names = [
        "a.php",
        ".php",
        "..php",
        "x..php",
        "php",
        "a.PHP",
        "a.php.bak",
        "a.phpx",
        "",
        ".",
        "..",
        "é.php",
        ".a.php",
    ];
    for name in names {
        assert_eq!(
            path_bytes::is_php_name(OsStr::new(name)),
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "php"),
            "{name}"
        );
    }
}
