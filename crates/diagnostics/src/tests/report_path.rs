use std::borrow::Cow;
use std::path::Path;

use crate::{path_text_names, report_path, slash_spelling};

#[test]
fn a_path_with_slashes_is_spelled_as_it_is() {
    assert!(matches!(
        report_path(Path::new("src/a.php")),
        Cow::Borrowed("src/a.php")
    ));
}

#[cfg(unix)]
#[test]
fn a_backslash_is_a_character_of_a_name_outside_windows() {
    assert_eq!(report_path(Path::new("dir/a\\b.php")), "dir/a\\b.php");
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_is_spelled_as_display_spells_it() {
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"src/\xe9.php"));
    assert_eq!(report_path(path), path.display().to_string());
}

#[cfg(windows)]
#[test]
fn a_backslash_is_a_separator_on_windows() {
    assert_eq!(report_path(Path::new("src\\a.php")), "src/a.php");
    assert_eq!(report_path(Path::new("src/sub\\a.php")), "src/sub/a.php");
    assert_eq!(
        report_path(Path::new("C:\\project\\a.php")),
        "C:/project/a.php"
    );
    assert_eq!(
        report_path(Path::new("\\\\server\\share\\a.php")),
        "//server/share/a.php"
    );
}

#[test]
fn a_configured_backslash_names_a_slash_or_a_backslash() {
    assert!(path_text_names("src\\a.php", "src/a.php"));
    assert!(path_text_names("src\\a.php", "src\\a.php"));
    assert!(path_text_names("src/a.php", "src/a.php"));
    assert!(!path_text_names("src/a.php", "src\\a.php"));
    assert!(!path_text_names("src\\a.php", "src/b.php"));
    assert!(!path_text_names("src\\a.php", "src/a.php5"));
}

#[test]
fn configured_text_and_what_it_names_have_one_slash_spelling() {
    assert!(matches!(
        slash_spelling("src/a.php"),
        Cow::Borrowed("src/a.php")
    ));
    for (configured, reported) in [
        ("src\\a.php", "src/a.php"),
        ("src\\a.php", "src\\a.php"),
        ("a\\b/c\\d.php", "a/b/c\\d.php"),
    ] {
        assert!(path_text_names(configured, reported));
        assert_eq!(slash_spelling(configured), slash_spelling(reported));
    }
}
