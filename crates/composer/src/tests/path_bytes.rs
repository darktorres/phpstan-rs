//! What `path_bytes` reads from a path's bytes is what `Path` reads from its
//! components, for paths of every shape.

use crate::manifest::lexically_normalized;
use crate::path_bytes;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

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
        assert_eq!(
            path_bytes::split(&path),
            path.parent().zip(path.file_name()),
            "{}",
            path.display()
        );
        for name in ["a", "b.php", ".php", "x..php", "php", "é", "a.b"] {
            assert_eq!(
                path_bytes::has_file_name(&path, name),
                path.file_name() == Some(OsStr::new(name)),
                "{} named {name}",
                path.display()
            );
        }
    }
}

#[test]
fn the_components_and_the_ancestors_they_end_are_read_as_the_path_gives_them() {
    let mut read = 0;
    for path in paths() {
        let Some(components) = path_bytes::components(&path) else {
            continue;
        };
        read += 1;
        // The ancestors from the top, but for a relative path's empty one.
        let mut ancestors: Vec<&Path> = path.ancestors().collect();
        ancestors.reverse();
        if !path.has_root() {
            ancestors.remove(0);
        }
        let expected: Vec<(Component<'_>, &Path)> = path.components().zip(ancestors).collect();
        assert_eq!(
            components.collect::<Vec<_>>(),
            expected,
            "{}",
            path.display()
        );
    }
    assert!(read > 100, "the mix holds paths the bytes read: {read}");
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
fn a_prefix_spelled_another_way_is_read_as_the_components_give_it() {
    let bases = [
        "/", "/a", "/a/b", "/a/..", "/a/.b", "a", "a/b", "..", "../a",
    ];
    // Ways to go on after a base, and to spell the base with components that
    // `Path` passes over.
    let tails = [
        "", "/", "//", "/x", "//x", "/./x", "/.", "x", "/x/", "/x//y", "/.x",
    ];
    for base in bases {
        let mut spellings = vec![base.to_string()];
        for (at, _) in base.match_indices('/') {
            for extra in ["/", "/.", "./"] {
                let mut spelled = base.to_string();
                spelled.insert_str(at + 1, extra);
                spellings.push(spelled);
            }
        }
        spellings.push(format!("./{base}"));
        spellings.push(base[..base.len() - 1].to_string());
        for spelled in &spellings {
            for tail in tails {
                let path = PathBuf::from(format!("{spelled}{tail}"));
                let base = Path::new(base);
                assert_eq!(
                    path_bytes::starts_with(&path, base),
                    path.starts_with(base),
                    "{} against {}",
                    path.display(),
                    base.display()
                );
                assert_eq!(
                    path_bytes::strip_prefix(&path, base),
                    path.strip_prefix(base).ok(),
                    "{} against {}",
                    path.display(),
                    base.display()
                );
            }
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
