//! The canonical form of a path: the system's, and on Windows spelled without
//! the verbatim prefix wherever the path means the same without it.

use super::helpers::*;
use crate::canonical::canonicalize;
use crate::hashed_path::HashedPath;

#[test]
fn a_canonical_path_is_its_own_resolved_path() {
    let dir = TempDir::new("canonical_resolved");
    dir.write("a/f.php", "<?php");
    let canonical = canonicalize(&dir.path().join("a/../a/f.php")).unwrap();
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let child = root.child("a".as_ref()).child("f.php".as_ref());
    assert_eq!(canonical, child.as_path());
    assert_eq!(crate::manifest::resolve(&canonical), canonical);
    // A file that is gone resolves through its directory, in the same form.
    assert_eq!(
        crate::manifest::resolve(&dir.path().join("a/gone/../g.php")),
        child.as_path().with_file_name("g.php")
    );
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::canonical::form;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    fn formed(path: &str) -> PathBuf {
        form(PathBuf::from(path))
    }

    /// The device names, and names whose stem is one.
    const DEVICES: [&str; 16] = [
        "CON",
        "con",
        "PRN",
        "AUX",
        "NUL",
        "nul.php",
        "NUL.tar.gz",
        "COM0",
        "com9.txt",
        "LPT1",
        "COM\u{b9}",
        "LPT\u{b3}.x",
        "CONIN$",
        "conout$",
        "CON .txt",
        "AUX  ",
    ];

    #[test]
    fn a_drive_path_with_plain_names_has_no_prefix() {
        assert_eq!(
            formed(r"\\?\C:\Users\a\b.php"),
            PathBuf::from(r"C:\Users\a\b.php")
        );
        assert_eq!(formed(r"\\?\C:\").as_os_str(), r"C:\");
        assert_eq!(formed(r"C:\Users\a").as_os_str(), r"C:\Users\a");
    }

    #[test]
    fn the_drive_letter_is_spelled_as_std_reads_it() {
        assert_eq!(formed(r"\\?\c:\a").as_os_str(), r"C:\a");
    }

    #[test]
    fn a_share_path_with_plain_names_has_no_prefix() {
        assert_eq!(
            formed(r"\\?\UNC\server\share\a\b.php").as_os_str(),
            r"\\server\share\a\b.php"
        );
        assert_eq!(
            formed(r"\\?\UNC\server\share\").as_os_str(),
            r"\\server\share\"
        );
        assert_eq!(formed(r"\\server\share\a").as_os_str(), r"\\server\share\a");
    }

    #[test]
    fn a_device_name_keeps_or_takes_the_prefix() {
        for name in DEVICES {
            let verbatim = format!(r"\\?\C:\a\{name}\b");
            assert_eq!(formed(&verbatim).as_os_str(), verbatim.as_str(), "{name}");
            assert_eq!(
                formed(&format!(r"C:\a\{name}\b")).as_os_str(),
                verbatim.as_str(),
                "{name}"
            );
            let share = format!(r"\\?\UNC\server\share\{name}");
            assert_eq!(formed(&share).as_os_str(), share.as_str(), "{name}");
            assert_eq!(
                formed(&format!(r"\\server\share\{name}")).as_os_str(),
                share.as_str(),
                "{name}"
            );
        }
    }

    #[test]
    fn a_name_that_only_starts_like_a_device_is_plain() {
        for name in [
            "CONSOLE", "NULL.php", "COM10", "LPT", "xCON", " CON", "COM1x", ".CON", "CON_",
        ] {
            let verbatim = format!(r"\\?\C:\a\{name}");
            assert_eq!(formed(&verbatim).as_os_str(), &verbatim[4..], "{name}");
        }
    }

    #[test]
    fn a_name_ending_in_a_dot_or_a_space_keeps_or_takes_the_prefix() {
        for name in ["a.", "a ", "a.php.", "...", "a. "] {
            let verbatim = format!(r"\\?\C:\{name}");
            assert_eq!(formed(&verbatim).as_os_str(), verbatim.as_str(), "{name:?}");
            assert_eq!(
                formed(&format!(r"C:\{name}")).as_os_str(),
                verbatim.as_str(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn a_name_with_a_character_a_plain_path_cannot_hold_keeps_or_takes_the_prefix() {
        for name in ["a<b", "a>b", "a\"b", "a|b", "a?b", "a*b", "a:b", "a\u{1}b"] {
            let verbatim = format!(r"\\?\C:\x\{name}");
            assert_eq!(formed(&verbatim).as_os_str(), verbatim.as_str(), "{name:?}");
            assert_eq!(
                formed(&format!(r"C:\x\{name}")).as_os_str(),
                verbatim.as_str(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn a_long_path_has_no_prefix_all_the_same() {
        let long = format!(r"\\?\C:\{}", ["dir"; 100].join(r"\"));
        assert!(long.len() > 260);
        assert_eq!(formed(&long).as_os_str(), &long[4..]);
    }

    #[test]
    fn paths_of_other_kinds_are_kept() {
        for path in [
            r"\\?\Volume{0b5d0f3a-0000-0000-0000-100000000000}\a",
            r"\\.\pipe\name",
            r"\\?\C:\a\..\b",
            r"\\?\C:\a\.\b",
            r"\\?\UNC\server\NUL\a",
            r"C:a\b",
            r"a\CON",
        ] {
            assert_eq!(formed(path).as_os_str(), path);
        }
    }

    #[test]
    fn a_canonical_path_has_no_prefix() {
        let dir = TempDir::new("canonical_plain");
        dir.write("f.php", "<?php");
        let canonical = canonicalize(&dir.path().join("f.php")).unwrap();
        assert!(
            !canonical
                .as_os_str()
                .as_encoded_bytes()
                .starts_with(br"\\?\"),
            "{}",
            canonical.display()
        );
    }

    #[test]
    fn the_entry_a_plain_path_cannot_spell_is_a_verbatim_child() {
        let dir = HashedPath::new(PathBuf::from(r"C:\a"));
        let device = dir.child(OsStr::new("nul.php"));
        assert_eq!(device.as_path().as_os_str(), r"\\?\C:\a\nul.php");
        assert_eq!(
            device.hash(),
            crate::hashed_path::path_hash(device.as_path())
        );
        let below = device.child(OsStr::new("b.php"));
        assert_eq!(below.as_path().as_os_str(), r"\\?\C:\a\nul.php\b.php");
        let plain = dir.child(OsStr::new("b.php"));
        assert_eq!(plain.as_path().as_os_str(), r"C:\a\b.php");
    }
}
