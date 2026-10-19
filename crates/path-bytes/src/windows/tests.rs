//! The Windows reader's answers, from the bytes alone. The tables below hold
//! on every platform; on Windows each answer in them is held to `Path`'s own,
//! and the reader is run against the mix of paths besides.

use super::*;
use std::path::PathBuf;

/// Paths, and what the reader answers for each one's name and its directory;
/// `None` where it leaves the answer to `Path`.
const SPLITS: &[(&str, Option<&str>, Option<&str>)] = &[
    ("C:\\a", Some("a"), Some("C:\\")),
    ("c:\\A", Some("A"), Some("c:\\")),
    ("C:\\a\\b.php", Some("b.php"), Some("C:\\a")),
    ("C:\\a\\b c.php", Some("b c.php"), Some("C:\\a")),
    ("C:\\a:b", Some("a:b"), Some("C:\\")),
    ("C:\\a\\b ", Some("b "), Some("C:\\a")),
    ("C:\\é\\ü.php", Some("ü.php"), Some("C:\\é")),
    ("a", Some("a"), Some("")),
    ("a\\b", Some("b"), Some("a")),
    ("a\\b\\c", Some("c"), Some("a\\b")),
    ("..\\a", Some("a"), Some("..")),
    ("é\\ü.php", Some("ü.php"), Some("é")),
    ("ab:\\c", Some("c"), Some("ab:")),
    ("1:\\c", Some("c"), Some("1:")),
    // The `/` is a separator too, and the last name is read around it.
    ("C:\\a/b", Some("b"), Some("C:\\a")),
    ("a/b", Some("b"), Some("a")),
    // A directory that ends in a separator or a `.` component is read by `Path`
    // without them.
    ("C:\\a\\\\b", Some("b"), None),
    ("C:\\a\\/b", Some("b"), None),
    ("C:\\a\\.\\b", Some("b"), None),
    ("C:\\a/.\\b", Some("b"), None),
    (".\\a", Some("a"), None),
    // A last component that is not a name.
    ("C:\\", None, None),
    ("C:\\a\\", None, None),
    ("C:\\a\\.", None, None),
    ("C:\\a\\..", None, None),
    ("..", None, None),
    (".", None, None),
    ("", None, None),
    // A start that is not a drive and a root, or names.
    ("C:", None, None),
    ("C:a", None, None),
    ("C:/a", None, None),
    ("c:a\\b", None, None),
    ("a:b", None, None),
    ("\\a", None, None),
    ("/a", None, None),
    ("\\\\server\\share", None, None),
    ("\\\\server\\share\\a", None, None),
    ("//server/share/a", None, None),
    ("\\\\?\\C:\\a", None, None),
    ("\\\\?\\UNC\\server\\share\\a", None, None),
    ("\\\\.\\COM1", None, None),
];

#[test]
fn a_name_and_a_directory_are_read_from_the_bytes_where_they_are_plain() {
    for &(path, name, dir) in SPLITS {
        let path = Path::new(path);
        assert_eq!(
            file_name(path),
            name.map(OsStr::new),
            "the name of {}",
            path.display()
        );
        assert_eq!(
            parent(path),
            dir.map(Path::new),
            "the directory of {}",
            path.display()
        );
        assert_eq!(
            split(path),
            dir.map(Path::new).zip(name.map(OsStr::new)),
            "the directory and name of {}",
            path.display()
        );
        if let Some(name) = name {
            for other in ["a", "b", "b.php", "ü.php", "c"] {
                // Only where the path does not end in a separator or a dot is
                // the name after the last separator the file name.
                let ends_plainly = !path.as_os_str().as_encoded_bytes().ends_with(b".")
                    && !path.as_os_str().as_encoded_bytes().ends_with(b"\\");
                if ends_plainly {
                    assert_eq!(
                        has_file_name(path, other),
                        Some(other == name),
                        "{} named {other}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn a_name_is_not_attached_to_a_drive_or_a_share() {
    for path in ["C:a", "C:", "\\\\server\\share", "\\\\?\\C:", "//s/h"] {
        assert_eq!(has_file_name(Path::new(path), "a"), None, "{path}");
        assert_eq!(has_file_name(Path::new(path), "share"), None, "{path}");
    }
    assert_eq!(has_file_name(Path::new("C:\\a"), "a"), Some(true));
    assert_eq!(has_file_name(Path::new("C:\\ba"), "a"), Some(false));
    assert_eq!(has_file_name(Path::new("C:\\a\\"), "a"), None);
    assert_eq!(has_file_name(Path::new("a/b"), "b"), Some(true));
}

/// A path, a base, and what the reader answers: whether the path starts with
/// the base, and the path without it; `None` where it leaves the answer to
/// `Path`.
type Prefix = (
    &'static str,
    &'static str,
    Option<bool>,
    Option<Option<&'static str>>,
);

const PREFIXES: &[Prefix] = &[
    ("C:\\a\\b", "C:\\a", Some(true), Some(Some("b"))),
    ("C:\\a\\b\\c", "C:\\a", Some(true), Some(Some("b\\c"))),
    ("C:\\a", "C:\\a", Some(true), Some(Some(""))),
    ("C:\\a", "C:\\", Some(true), Some(Some("a"))),
    ("C:\\", "C:\\", Some(true), Some(Some(""))),
    ("C:\\a\\b", "C:\\", Some(true), Some(Some("a\\b"))),
    // The drive is the same in either case; the names are not.
    ("c:\\a\\b", "C:\\a", Some(true), Some(Some("b"))),
    ("C:\\a\\b", "c:\\a", Some(true), Some(Some("b"))),
    ("c:\\a", "C:\\", Some(true), Some(Some("a"))),
    ("C:\\a\\b", "c:\\A", Some(false), Some(None)),
    ("C:\\A", "C:\\a", Some(false), Some(None)),
    ("C:\\a\\b", "D:\\a", Some(false), Some(None)),
    ("C:\\a", "D:\\", Some(false), Some(None)),
    // A name that only starts like the base's.
    ("C:\\ab", "C:\\a", Some(false), Some(None)),
    ("C:\\a.b", "C:\\a", Some(false), Some(None)),
    ("C:\\a", "C:\\ab", Some(false), Some(None)),
    ("C:\\a", "C:\\a\\b", Some(false), Some(None)),
    // An absolute path and a relative one share no component.
    ("C:\\a", "a", Some(false), Some(None)),
    ("a", "C:\\a", Some(false), Some(None)),
    ("a\\b", "C:\\", Some(false), Some(None)),
    // Names alone.
    ("a\\b", "a", Some(true), Some(Some("b"))),
    ("a\\b\\c", "a\\b", Some(true), Some(Some("c"))),
    ("a", "a", Some(true), Some(Some(""))),
    ("a", "a\\b", Some(false), Some(None)),
    ("ab", "a", Some(false), Some(None)),
    ("b\\a", "a", Some(false), Some(None)),
    ("..\\a", "..", Some(true), Some(Some("a"))),
    ("é\\ü", "é", Some(true), Some(Some("ü"))),
    // Either separator follows the base; what is left is read only when it is
    // plain.
    ("C:\\a/b", "C:\\a", Some(true), Some(Some("b"))),
    ("a/b", "a", Some(true), Some(Some("b"))),
    ("C:\\a\\\\b", "C:\\a", Some(true), None),
    ("C:\\a\\b/c", "C:\\a", Some(true), None),
    ("C:\\a\\.\\b", "C:\\a", Some(true), None),
    ("C:\\a\\", "C:\\a", Some(true), Some(Some(""))),
    ("C:\\a\\\\", "C:\\", Some(true), None),
    // A component `Path` passes over makes the answer `Path`'s.
    ("C:\\.\\a", "C:\\a", None, None),
    ("C:\\\\a", "C:\\a", None, None),
    ("C:\\a\\.\\b", "C:\\a\\b", None, None),
    (".\\a", "a", None, None),
    // A base that is not spelled as its components are.
    ("C:\\a\\b", "C:\\a\\", None, None),
    ("C:\\a\\b", "C:\\.\\a", None, None),
    ("C:\\a\\b", "C:\\a\\.", None, None),
    ("C:\\a", "", None, None),
    ("C:\\a\\b", "C:", None, None),
    ("C:\\a\\b", "C:/a", None, None),
    // A start that is not a drive and a root, or names.
    ("C:a", "C:a", None, None),
    ("\\a\\b", "\\a", None, None),
    ("\\\\server\\share\\a", "\\\\server\\share", None, None),
    ("\\\\?\\C:\\a", "\\\\?\\C:\\", None, None),
    ("C:\\a", "\\\\?\\C:\\a", None, None),
];

#[test]
fn a_prefix_is_read_from_the_bytes_where_both_are_plain() {
    for &(path, base, starts, stripped) in PREFIXES {
        let (path, base) = (Path::new(path), Path::new(base));
        assert_eq!(
            starts_with(path, base),
            starts,
            "{} starts with {}",
            path.display(),
            base.display()
        );
        assert_eq!(
            strip_prefix(path, base),
            stripped.map(|rest| rest.map(Path::new)),
            "{} without {}",
            path.display(),
            base.display()
        );
    }
}

#[test]
fn a_path_is_plain_when_it_is_spelled_as_its_components_are() {
    for (path, plain_as) in [
        ("C:\\", Some((3, false))),
        ("c:\\", Some((3, false))),
        ("C:\\a", Some((3, false))),
        ("C:\\a\\b.php", Some((3, false))),
        ("C:\\a\\..", Some((3, true))),
        ("C:\\..\\a", Some((3, true))),
        ("a", Some((0, false))),
        ("a\\b", Some((0, false))),
        ("..", Some((0, true))),
        ("..\\a", Some((0, true))),
        ("é\\ü", Some((0, false))),
        ("ab:\\c", Some((0, false))),
        ("", None),
        (".", None),
        (".\\a", None),
        ("a\\.", None),
        ("a\\.\\b", None),
        ("a\\", None),
        ("a\\\\b", None),
        ("a/b", None),
        ("C:\\a\\", None),
        ("C:\\a\\\\b", None),
        ("C:\\.", None),
        ("C:\\a/b", None),
        ("C:", None),
        ("C:a", None),
        ("C:/", None),
        ("\\", None),
        ("\\a", None),
        ("\\\\server\\share", None),
        ("\\\\?\\C:\\a", None),
    ] {
        assert_eq!(plain(path.as_bytes()), plain_as, "{path}");
    }
}

#[test]
fn a_normal_absolute_path_is_a_drive_a_root_and_names() {
    for path in ["C:\\", "c:\\", "C:\\a", "z:\\a\\b.php", "C:\\é"] {
        assert!(is_normal_absolute(Path::new(path)), "{path}");
    }
    for path in [
        "",
        "a",
        "a\\b",
        "C:",
        "C:a",
        "C:\\..",
        "C:\\a\\..\\b",
        "C:\\.",
        "C:\\a\\",
        "C:\\a/b",
        "\\a",
        "\\\\s\\h\\a",
        "\\\\?\\C:\\a",
    ] {
        assert!(!is_normal_absolute(Path::new(path)), "{path}");
    }
}

#[test]
fn the_components_of_a_relative_path_end_each_in_the_path_up_to_it() {
    let path = Path::new("..\\a\\b.php");
    let got: Vec<(Component<'_>, &Path)> = components(path).unwrap().collect();
    assert_eq!(
        got,
        [
            (Component::ParentDir, Path::new("..")),
            (Component::Normal(OsStr::new("a")), Path::new("..\\a")),
            (
                Component::Normal(OsStr::new("b.php")),
                Path::new("..\\a\\b.php")
            ),
        ]
    );
    for path in [
        "", ".", "a\\", "a\\.\\b", "a/b", "\\a", "C:", "C:a", "\\\\s\\h",
    ] {
        assert!(components(Path::new(path)).is_none(), "{path}");
    }
}

/// `head`, a character that is not Unicode (an unpaired surrogate, an invalid
/// byte), and `tail`, as one path.
#[cfg(any(unix, windows))]
fn not_unicode(head: &str, tail: &str) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let wide: Vec<u16> = head
            .encode_utf16()
            .chain([0xD800])
            .chain(tail.encode_utf16())
            .collect();
        PathBuf::from(std::ffi::OsString::from_wide(&wide))
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut bytes = head.as_bytes().to_vec();
        bytes.push(0xFF);
        bytes.extend_from_slice(tail.as_bytes());
        PathBuf::from(OsStr::from_bytes(&bytes))
    }
}

#[cfg(any(unix, windows))]
#[test]
fn a_part_that_is_not_unicode_is_left_to_path() {
    // The name is not Unicode: the directory is still read.
    let odd_name = not_unicode("C:\\dir\\a", "b");
    assert_eq!(file_name(&odd_name), None);
    assert_eq!(parent(&odd_name), Some(Path::new("C:\\dir")));
    assert_eq!(split(&odd_name), None);
    assert!(components(&odd_name).is_none());
    assert_eq!(has_file_name(&odd_name, "b"), Some(false));
    // The directory is not Unicode: the name is still read.
    let odd_dir = not_unicode("C:\\dir\\a", "b\\c");
    assert_eq!(file_name(&odd_dir), Some(OsStr::new("c")));
    assert_eq!(parent(&odd_dir), None);
    assert_eq!(split(&odd_dir), None);
    assert_eq!(starts_with(&odd_dir, Path::new("C:\\dir")), Some(true));
    assert_eq!(starts_with(&odd_dir, Path::new("C:\\dir\\a")), Some(false));
    assert_eq!(strip_prefix(&odd_dir, Path::new("C:\\dir")), None);
    assert_eq!(strip_prefix(&odd_dir, Path::new("C:\\dir\\a")), Some(None));
    assert_eq!(
        strip_prefix(&not_unicode("C:\\dir\\a", "b"), Path::new("C:\\dir\\a")),
        Some(None)
    );
}

#[cfg(windows)]
#[test]
fn every_answer_in_the_tables_is_the_answer_of_path() {
    for &(path, name, dir) in SPLITS {
        let path = Path::new(path);
        if let Some(name) = name {
            assert_eq!(
                path.file_name(),
                Some(OsStr::new(name)),
                "{}",
                path.display()
            );
        }
        if let Some(dir) = dir {
            assert_eq!(path.parent(), Some(Path::new(dir)), "{}", path.display());
        }
    }
    for &(path, base, starts, stripped) in PREFIXES {
        let (path, base) = (Path::new(path), Path::new(base));
        if let Some(starts) = starts {
            assert_eq!(
                path.starts_with(base),
                starts,
                "{} starts with {}",
                path.display(),
                base.display()
            );
        }
        if let Some(stripped) = stripped {
            assert_eq!(
                path.strip_prefix(base).ok(),
                stripped.map(Path::new),
                "{} without {}",
                path.display(),
                base.display()
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn the_reader_answers_what_path_answers_for_the_mix_and_answers_for_most_of_it() {
    let paths = crate::tests::paths();
    let (mut names, mut dirs, mut stripped, mut normal) = (0, 0, 0, 0);
    for path in &paths {
        if let Some(name) = file_name(path) {
            names += 1;
            assert_eq!(Some(name), path.file_name(), "{}", path.display());
        }
        if let Some(dir) = parent(path) {
            dirs += 1;
            assert_eq!(Some(dir), path.parent(), "{}", path.display());
        }
        for base in paths.iter().take(60) {
            if let Some(starts) = starts_with(path, base) {
                assert_eq!(
                    starts,
                    path.starts_with(base),
                    "{} against {}",
                    path.display(),
                    base.display()
                );
            }
            if let Some(rest) = strip_prefix(path, base) {
                stripped += 1;
                assert_eq!(
                    rest,
                    path.strip_prefix(base).ok(),
                    "{} against {}",
                    path.display(),
                    base.display()
                );
            }
        }
        if is_normal_absolute(path) {
            normal += 1;
        }
    }
    assert!(
        names > 150 && dirs > 100 && stripped > 5000 && normal > 50,
        "answered: {names} names, {dirs} directories, {stripped} prefixes, {normal} normal paths"
    );
}
