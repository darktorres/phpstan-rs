use std::borrow::Cow;
use std::path::Path;

use crate::{CaseRule, path_text_names, report_path, slash_spelling};

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

#[test]
fn an_exact_rule_compares_text_as_it_is() {
    assert!(matches!(
        CaseRule::Exact.key("Src/Foo.php"),
        Cow::Borrowed("Src/Foo.php")
    ));
    assert_ne!(CaseRule::Exact.key("Src"), CaseRule::Exact.key("src"));
}

#[test]
fn a_folded_rule_gives_one_key_to_the_spellings_of_a_name() {
    for (a, b) in [
        ("Src/Foo.PHP", "src/foo.php"),
        ("École", "école"),
        ("ÉCOLE", "école"),
        ("ΟΔΟΣ", "οδος"),
        ("ΟΔΟΣ", "οδοσ"),
        ("\u{212A}", "k"),
    ] {
        assert_eq!(CaseRule::Folded.key(a), CaseRule::Folded.key(b), "{a} {b}");
    }
}

#[test]
fn a_folded_rule_keeps_apart_what_simple_mappings_keep_apart() {
    for (a, b) in [
        ("straße", "strasse"),
        ("İ", "i"),
        ("a", "b"),
        ("src/a", "src\\a"),
    ] {
        assert_ne!(CaseRule::Folded.key(a), CaseRule::Folded.key(b), "{a} {b}");
    }
}

#[test]
fn a_folded_key_of_text_that_is_folded_already_is_borrowed() {
    assert!(matches!(
        CaseRule::Folded.key("src/foo.php"),
        Cow::Borrowed("src/foo.php")
    ));
    assert!(matches!(CaseRule::Folded.key("Src/Foo.php"), Cow::Owned(_)));
}

#[test]
fn the_rule_of_a_directory_follows_whether_it_folds_case() {
    assert_eq!(CaseRule::of_folding(true), CaseRule::Folded);
    assert_eq!(CaseRule::of_folding(false), CaseRule::Exact);
    assert_eq!(CaseRule::default(), CaseRule::Exact);
}
