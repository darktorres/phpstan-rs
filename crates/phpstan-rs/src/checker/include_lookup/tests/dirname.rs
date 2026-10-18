//! PHP's `dirname()` in each world.

use super::super::*;

#[test]
fn the_last_name_and_the_separators_around_it_go() {
    for (text, dirname) in [
        ("/a/b", "/a"),
        ("/a/b/", "/a"),
        ("/a/b/c", "/a/b"),
        ("a/b", "a"),
        ("//a//b//", "//a"),
        ("/a", "/"),
        ("/", "/"),
        ("///", "/"),
        ("a", "."),
        (".", "."),
        ("", ""),
    ] {
        assert_eq!(World::Unix.dirname(text, 1), dirname, "{text:?}");
    }
}

#[test]
fn levels_go_up_until_nothing_is_left_to_remove() {
    assert_eq!(World::Unix.dirname("/a/b/c", 2), "/a");
    assert_eq!(World::Unix.dirname("/a/b/c", 3), "/");
    assert_eq!(World::Unix.dirname("/a/b/c", 9), "/");
    assert_eq!(World::Unix.dirname("a/b", 5), ".");
}

#[test]
fn a_backslash_separates_names_in_the_windows_world_only() {
    assert_eq!(World::Windows.dirname("a\\b", 1), "a");
    assert_eq!(World::Windows.dirname("a/b\\c", 1), "a/b");
    assert_eq!(World::Unix.dirname("a\\b", 1), ".");
    assert_eq!(World::Windows.dirname("\\a\\b", 1), "\\a");
    assert_eq!(World::Windows.dirname("\\", 1), "\\");
}

#[test]
fn a_drive_is_kept_in_the_windows_world() {
    for (text, dirname) in [
        ("C:\\a\\b", "C:\\a"),
        ("C:\\a", "C:\\"),
        ("C:\\", "C:\\"),
        ("C:", "C:"),
        ("C:a", "C:."),
        ("c:/a/b", "c:/a"),
    ] {
        assert_eq!(World::Windows.dirname(text, 1), dirname, "{text:?}");
    }
    assert_eq!(World::Unix.dirname("C:/a", 1), "C:");
}
