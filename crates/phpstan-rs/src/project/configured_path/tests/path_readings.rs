//! How a path the configuration names reads, with the disk's answer
//! injected.

use super::super::*;
use std::collections::BTreeSet;

fn existing<'a>(paths: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
    let names: BTreeSet<&str> = paths.iter().copied().collect();
    move |path| path.to_str().is_some_and(|path| names.contains(path))
}

#[test]
fn text_without_a_backslash_has_one_reading() {
    assert_eq!(
        readings(Path::new("src/Http")),
        vec![PathBuf::from("src/Http")]
    );
}

#[cfg(unix)]
#[test]
fn text_with_a_backslash_has_the_separator_reading_first_and_the_name_reading_after() {
    assert_eq!(
        readings(Path::new("src\\Http")),
        vec![PathBuf::from("src/Http"), PathBuf::from("src\\Http")]
    );
}

#[cfg(windows)]
#[test]
fn a_backslash_is_a_separator_alone_on_windows() {
    assert_eq!(
        readings(Path::new("src\\Http")),
        vec![PathBuf::from("src\\Http")]
    );
}

#[cfg(unix)]
#[test]
fn a_list_entry_takes_every_reading_that_exists_the_separator_reading_first() {
    let both = existing(&["src/Http", "src\\Http"]);
    assert_eq!(
        roots(Path::new("src\\Http"), both),
        vec![PathBuf::from("src/Http"), PathBuf::from("src\\Http")]
    );
    let only_the_name = existing(&["src\\Http"]);
    assert_eq!(
        roots(Path::new("src\\Http"), only_the_name),
        vec![PathBuf::from("src\\Http")]
    );
    let only_the_separator = existing(&["src/Http"]);
    assert_eq!(
        roots(Path::new("src\\Http"), only_the_separator),
        vec![PathBuf::from("src/Http")]
    );
}

#[cfg(unix)]
#[test]
fn a_list_entry_that_exists_in_no_reading_is_named_in_the_separator_reading() {
    assert_eq!(
        roots(Path::new("src\\Missing"), existing(&[])),
        vec![PathBuf::from("src/Missing")]
    );
}

#[test]
fn a_list_entry_without_a_backslash_is_itself_whether_it_exists_or_not() {
    assert_eq!(
        roots(Path::new("src"), existing(&["src"])),
        vec![PathBuf::from("src")]
    );
    assert_eq!(
        roots(Path::new("src"), existing(&[])),
        vec![PathBuf::from("src")]
    );
}

#[cfg(unix)]
#[test]
fn a_single_file_takes_the_reading_that_exists() {
    assert_eq!(
        single(
            "baseline",
            Path::new("ci\\base.json"),
            existing(&["ci/base.json"])
        ),
        Ok(PathBuf::from("ci/base.json"))
    );
    assert_eq!(
        single(
            "baseline",
            Path::new("ci\\base.json"),
            existing(&["ci\\base.json"])
        ),
        Ok(PathBuf::from("ci\\base.json"))
    );
}

#[cfg(unix)]
#[test]
fn a_single_file_that_exists_in_no_reading_is_named_in_the_separator_reading() {
    assert_eq!(
        single("baseline", Path::new("ci\\base.json"), existing(&[])),
        Ok(PathBuf::from("ci/base.json"))
    );
}

#[cfg(unix)]
#[test]
fn a_single_file_that_exists_in_both_readings_is_an_error_naming_both() {
    let error = single(
        "sql_reflection.path",
        Path::new("db\\app.sqlite"),
        existing(&["db/app.sqlite", "db\\app.sqlite"]),
    )
    .unwrap_err();
    assert!(error.starts_with("sql_reflection.path db\\app.sqlite names two files"));
    assert!(
        error.contains("db/app.sqlite and db\\app.sqlite"),
        "{error}"
    );
}

#[test]
fn a_single_file_without_a_backslash_is_never_ambiguous() {
    assert_eq!(
        single(
            "baseline",
            Path::new("ci/base.json"),
            existing(&["ci/base.json"])
        ),
        Ok(PathBuf::from("ci/base.json"))
    );
}

#[test]
fn text_with_one_reading_is_not_asked_about() {
    let never = |path: &Path| -> bool { panic!("{} was looked up", path.display()) };
    assert_eq!(roots(Path::new("src"), never), vec![PathBuf::from("src")]);
    assert_eq!(
        single("baseline", Path::new("ci/base.json"), never),
        Ok(PathBuf::from("ci/base.json"))
    );
}
