//! What `path_bytes` reads from a path's bytes is what `Path` reads from its
//! components, for paths of every shape, spelled as this platform spells
//! them.

use crate as path_bytes;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// The names a path of the mix is made of: with and without a PHP extension,
/// `.` and `..`, an empty one, and one that is not ASCII.
#[cfg(unix)]
const PARTS: &[&str] = &[
    "a", "b.php", ".php", "..php", "x..php", "php", ".", "..", "", "é", "vendor", "a.b",
];
#[cfg(windows)]
const PARTS: &[&str] = &[
    "a", "b.php", ".php", "..php", "x..php", "php", ".", "..", "", "é", "vendor", "a.b", "a:b",
    "CON", "a.", "C:",
];

/// What a path of the mix starts with, besides a root and nothing: a `.`
/// component, and on Windows every kind of prefix, in either case, and the
/// forms `Path` reads another way.
#[cfg(windows)]
const ODD_HEADS: &[&str] = &[
    "C:/",
    "c:/",
    "C:",
    "c:a",
    "\\",
    "/",
    "\\\\server\\share\\",
    "//server/share/",
    "\\\\?\\C:\\",
    "\\\\?\\UNC\\server\\share\\",
    "\\\\.\\COM1\\",
    "a:b\\",
    "ab:\\",
    ".\\",
    "./",
    "..\\",
    "\\\\",
];

/// The text of a path of the mix, from the numbers `next` gives, each below
/// its bound.
#[cfg(unix)]
fn path_text(next: &mut impl FnMut(usize) -> usize) -> String {
    let mut text = match next(4) {
        0 | 1 => String::from("/"),
        2 => String::from("./"),
        _ => String::new(),
    };
    for i in 0..next(6) {
        if i > 0 {
            text.push_str(if next(6) == 0 { "//" } else { "/" });
        }
        text.push_str(PARTS[next(PARTS.len())]);
    }
    if next(5) == 0 {
        text.push('/');
    }
    text
}

#[cfg(windows)]
fn path_text(next: &mut impl FnMut(usize) -> usize) -> String {
    let mut text = match next(10) {
        0..=4 => ["C:\\", "c:\\", "D:\\", "C:\\", "z:\\"][next(5)].to_string(),
        5 | 6 => String::new(),
        _ => ODD_HEADS[next(ODD_HEADS.len())].to_string(),
    };
    for i in 0..next(6) {
        if i > 0 {
            text.push_str(match next(8) {
                0 => "\\\\",
                1 => "/",
                _ => "\\",
            });
        }
        text.push_str(PARTS[next(PARTS.len())]);
    }
    if next(5) == 0 {
        text.push_str(["\\", "/"][next(2)]);
    }
    text
}

/// A repeatable mix of paths: absolute and relative, with doubled and
/// trailing separators, `.` and `..` components, and names with and without a
/// PHP extension; on Windows, with every kind of prefix and both separators.
pub(crate) fn paths() -> Vec<PathBuf> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |bound: usize| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 33) as usize) % bound
    };
    (0..600)
        .map(|_| PathBuf::from(path_text(&mut next)))
        .collect()
}

/// `path`, and, when it starts with a drive, `path` with the drive letter in
/// the other case: the two spellings of one path to `Path`.
fn drive_cases(path: &Path) -> Vec<PathBuf> {
    let text = path.to_string_lossy();
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => {
            let flipped = if drive.is_ascii_uppercase() {
                drive.to_ascii_lowercase()
            } else {
                drive.to_ascii_uppercase()
            };
            vec![
                path.to_path_buf(),
                PathBuf::from(format!("{flipped}{}", &text[1..])),
            ]
        }
        _ => vec![path.to_path_buf()],
    }
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
        // Each component with the path made of the components up to it: the
        // ancestors `Path` gives, from the top, and for a prefix its own text,
        // which ends no ancestor.
        let mut ancestors: Vec<&Path> = path.ancestors().collect();
        ancestors.reverse();
        if !path.has_root() {
            ancestors.remove(0);
        }
        let mut ancestors = ancestors.into_iter();
        let expected: Vec<(Component<'_>, &Path)> = path
            .components()
            .map(|component| match component {
                Component::Prefix(prefix) => (component, Path::new(prefix.as_os_str())),
                _ => (component, ancestors.next().unwrap()),
            })
            .collect();
        assert_eq!(ancestors.next(), None, "{}", path.display());
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
        let bases: Vec<PathBuf> = bases.iter().flat_map(|base| drive_cases(base)).collect();
        let spellings = drive_cases(path);
        for base in &bases {
            for path in &spellings {
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
}

/// Prefixes to read a path against, and ways to go on after one.
#[cfg(unix)]
const BASES: &[&str] = &[
    "/", "/a", "/a/b", "/a/..", "/a/.b", "a", "a/b", "..", "../a",
];
#[cfg(unix)]
const TAILS: &[&str] = &[
    "", "/", "//", "/x", "//x", "/./x", "/.", "x", "/x/", "/x//y", "/.x",
];
/// Ways to spell a prefix with components that `Path` passes over.
#[cfg(unix)]
const EXTRAS: &[&str] = &["/", "/.", "./"];
#[cfg(unix)]
const SEPARATOR: char = '/';

#[cfg(windows)]
const BASES: &[&str] = &[
    "C:\\",
    "C:\\a",
    "C:\\a\\b",
    "C:\\a\\..",
    "C:\\a\\.b",
    "c:\\a",
    "D:\\",
    "a",
    "a\\b",
    "..",
    "..\\a",
];
#[cfg(windows)]
const TAILS: &[&str] = &[
    "", "\\", "\\\\", "\\x", "\\\\x", "\\.\\x", "\\.", "x", "\\x\\", "\\x\\\\y", "\\.x", "/", "/x",
    "/.", "\\..", "\\..\\x",
];
#[cfg(windows)]
const EXTRAS: &[&str] = &["\\", "\\.", ".\\", "/", "/.", "./"];
#[cfg(windows)]
const SEPARATOR: char = '\\';

#[test]
fn a_prefix_spelled_another_way_is_read_as_the_components_give_it() {
    for base in BASES {
        let mut spellings = vec![base.to_string()];
        for (at, _) in base.match_indices(SEPARATOR) {
            for extra in EXTRAS {
                let mut spelled = base.to_string();
                spelled.insert_str(at + 1, extra);
                spellings.push(spelled);
            }
        }
        spellings.push(format!("{}{base}", EXTRAS[2]));
        spellings.push(base[..base.len() - 1].to_string());
        for spelled in &spellings {
            for tail in TAILS {
                let path = PathBuf::from(format!("{spelled}{tail}"));
                let base = Path::new(base);
                for path in drive_cases(&path) {
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
}

/// Paths that are not their own normal form, or are not absolute.
#[cfg(unix)]
const NOT_NORMAL: &[&str] = &["a/b", "/a/../b", "/a/./b", "/a//b", "/a/", ""];
#[cfg(windows)]
const NOT_NORMAL: &[&str] = &[
    "a\\b",
    "C:\\a\\..\\b",
    "C:\\a\\.\\b",
    "C:\\a\\\\b",
    "C:\\a\\",
    "C:\\a/b",
    // `PathBuf::push` takes a name that starts as a drive does for a path of
    // its own, so rebuilding the path from its components does not give it
    // back.
    "C:\\a:b",
    "C:\\a\\b:c",
    "C:/a",
    "C:",
    "C:a",
    "\\a",
    "",
];
/// Absolute paths that are their own normal form.
#[cfg(unix)]
const NORMAL: &[&str] = &["/", "/a/b.php"];
#[cfg(windows)]
const NORMAL: &[&str] = &["C:\\", "c:\\", "C:\\a\\b.php", "z:\\é\\ab.c", "C:\\ab:c"];

#[test]
fn a_normal_absolute_path_is_its_own_normal_form() {
    let mut normal = 0;
    for path in paths() {
        if path_bytes::is_normal_absolute(&path) {
            normal += 1;
            assert!(path.is_absolute(), "{}", path.display());
            // A lexical normalization drops each `.` and resolves each `..`,
            // and rebuilds what is left from the components.
            assert!(
                !path
                    .components()
                    .any(|c| matches!(c, Component::CurDir | Component::ParentDir)),
                "{}",
                path.display()
            );
            assert_eq!(
                path.components().collect::<PathBuf>(),
                path,
                "{}",
                path.display()
            );
        }
    }
    assert!(normal > 50, "the mix holds normal paths: {normal}");
    for path in NORMAL {
        assert!(path_bytes::is_normal_absolute(Path::new(path)), "{path}");
    }
    for path in NOT_NORMAL {
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
