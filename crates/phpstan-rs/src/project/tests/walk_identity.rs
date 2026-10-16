//! The walk lists each file once, under the path it is reached by. A link is
//! followed to its target, a target under a root is left to that root's walk,
//! and a target no root holds is listed once, with the strongest role of the
//! links that reach it.

use super::super::walk::{Listed, Role, Walk};
use super::helpers::*;
use std::fs;
use std::path::{Path, PathBuf};

/// Lists each `(root, role, excluded)` in order, and returns the listing
/// sorted by path as `(path, role, stub)`.
fn listing(roots: &[(PathBuf, Role, &[&str])]) -> Vec<(PathBuf, Role, bool)> {
    let mut walk = Walk::new();
    for (path, role, excluded) in roots {
        walk.add(path, path, *role, excluded);
    }
    sorted(walk.finish())
}

fn sorted(listed: Vec<Listed>) -> Vec<(PathBuf, Role, bool)> {
    let mut listed: Vec<(PathBuf, Role, bool)> = listed
        .into_iter()
        .map(|Listed { path, role, stub }| (path, role, stub))
        .collect();
    listed.sort();
    listed
}

/// `dir`, resolved, so that the expected paths are the ones the walk reaches.
fn resolved(dir: &TempDir) -> PathBuf {
    fs::canonicalize(&dir.0).unwrap()
}

#[test]
fn a_tree_without_links_is_listed_under_its_roots_spelling() {
    let dir = TempDir::new("walk_plain");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/nested/b.php", "<?php");
    dir.write("proj/notes.txt", "not php");
    let root = resolved(&dir).join("proj");

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (root.join("a.php"), Role::Analysed, false),
            (root.join("nested/b.php"), Role::Analysed, false),
        ]
    );
}

#[test]
fn a_directory_root_is_spelled_as_it_was_given() {
    let dir = TempDir::new("walk_spelling");
    dir.write("proj/src/a.php", "<?php");
    dir.write("proj/top.php", "<?php");
    let base = resolved(&dir);
    let mut walk = Walk::new();
    walk.add(
        &base.join("proj/src"),
        Path::new("src"),
        Role::Analysed,
        &[],
    );
    walk.add(&base.join("proj"), Path::new("proj"), Role::Analysed, &[]);

    // The file under both roots is listed once, under the first root's spelling.
    assert_eq!(
        sorted(walk.finish()),
        vec![
            (PathBuf::from("proj/top.php"), Role::Analysed, false),
            (PathBuf::from("src/a.php"), Role::Analysed, false),
        ]
    );
}

#[test]
fn an_excluded_directory_name_is_not_entered() {
    let dir = TempDir::new("walk_excluded");
    dir.write("proj/src/a.php", "<?php");
    dir.write("proj/vendor/v.php", "<?php");
    let root = resolved(&dir).join("proj");

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &["vendor"])]),
        vec![(root.join("src/a.php"), Role::Analysed, false)]
    );
}

#[test]
fn a_context_root_inside_an_excluding_root_is_walked_with_its_own_exclusions() {
    let dir = TempDir::new("walk_context_inside_root");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/vendor/pkg/src/x.php", "<?php");
    let root = resolved(&dir).join("proj");
    let package = root.join("vendor/pkg");

    assert_eq!(
        listing(&[
            (root.clone(), Role::Analysed, &["vendor"]),
            (package.clone(), Role::Context, &[]),
        ]),
        vec![
            (root.join("a.php"), Role::Analysed, false),
            (package.join("src/x.php"), Role::Context, false),
        ]
    );
}

#[test]
fn a_root_named_like_an_excluded_name_is_walked_and_the_names_exclude_only_below_it() {
    let dir = TempDir::new("walk_root_named_vendor");
    dir.write("vendor/x.php", "<?php");
    dir.write("vendor/vendor/y.php", "<?php");
    let vendor = resolved(&dir).join("vendor");

    assert_eq!(
        listing(&[(vendor.clone(), Role::Context, &[])]),
        vec![
            (vendor.join("vendor/y.php"), Role::Context, false),
            (vendor.join("x.php"), Role::Context, false),
        ]
    );
    assert_eq!(
        listing(&[(vendor.clone(), Role::Context, &["vendor"])]),
        vec![(vendor.join("x.php"), Role::Context, false)]
    );
}

#[test]
fn a_directory_root_lists_only_php_files_but_an_explicit_file_is_listed_whatever_its_extension() {
    let dir = TempDir::new("walk_explicit_file");
    dir.write("config/bootstrap.inc", "<?php");
    let file = resolved(&dir).join("config/bootstrap.inc");

    let mut by_walk = Walk::new();
    by_walk.add(&file, &file, Role::Context, &[]);
    assert!(by_walk.finish().is_empty());

    let mut explicit = Walk::new();
    explicit.add_file(&file, &file, Role::Context);
    assert_eq!(
        sorted(explicit.finish()),
        vec![(file.clone(), Role::Context, false)]
    );
}

#[cfg(unix)]
#[test]
fn a_link_into_a_listed_root_is_left_to_that_root() {
    let dir = TempDir::new("walk_link_in_root");
    dir.write("proj/src/a.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    std::os::unix::fs::symlink("src", root.join("alias")).unwrap();

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![(root.join("src/a.php"), Role::Analysed, false)]
    );
}

#[cfg(unix)]
#[test]
fn a_file_link_into_a_listed_root_lists_its_target_once() {
    let dir = TempDir::new("walk_file_link_in_root");
    dir.write("proj/src/a.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    std::os::unix::fs::symlink("src/a.php", root.join("copy.php")).unwrap();

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![(root.join("src/a.php"), Role::Analysed, false)]
    );
}

#[cfg(unix)]
#[test]
fn a_target_outside_every_root_is_listed_once_under_its_resolved_path() {
    let dir = TempDir::new("walk_outside_target");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    std::os::unix::fs::symlink("../outside", root.join("one")).unwrap();
    std::os::unix::fs::symlink("../outside", root.join("two")).unwrap();

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (base.join("outside/x.php"), Role::Analysed, false),
            (root.join("a.php"), Role::Analysed, false),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_link_target_is_walked_with_the_exclusions_of_each_walk_that_meets_it() {
    let dir = TempDir::new("walk_link_exclusions");
    dir.write("proj/a.php", "<?php");
    dir.write("ctx/c.php", "<?php");
    dir.write("outside/keep.php", "<?php");
    dir.write("outside/vendor/v.php", "<?php");
    let base = resolved(&dir);
    let proj = base.join("proj");
    let ctx = base.join("ctx");
    let outside = base.join("outside");
    std::os::unix::fs::symlink("../outside", proj.join("out")).unwrap();
    std::os::unix::fs::symlink("../outside", ctx.join("out")).unwrap();

    // The excluding walk skips `vendor` inside the target; the context walk,
    // which excludes nothing, lists it. Each file is listed once.
    assert_eq!(
        listing(&[
            (proj.clone(), Role::Analysed, &["vendor"]),
            (ctx.clone(), Role::Context, &[]),
        ]),
        vec![
            (ctx.join("c.php"), Role::Context, false),
            (outside.join("keep.php"), Role::Analysed, false),
            (outside.join("vendor/v.php"), Role::Context, false),
            (proj.join("a.php"), Role::Analysed, false),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_file_reached_through_analysed_and_context_links_is_analysed() {
    let dir = TempDir::new("walk_role_by_link");
    dir.write("ctx/c.php", "<?php");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    let base = resolved(&dir);
    let ctx = base.join("ctx");
    let proj = base.join("proj");
    std::os::unix::fs::symlink("../outside", ctx.join("go")).unwrap();
    std::os::unix::fs::symlink("../outside", proj.join("go")).unwrap();
    let expected = vec![
        (ctx.join("c.php"), Role::Context, false),
        (base.join("outside/x.php"), Role::Analysed, false),
        (proj.join("a.php"), Role::Analysed, false),
    ];

    // The role does not depend on which root is listed first.
    assert_eq!(
        listing(&[
            (ctx.clone(), Role::Context, &[]),
            (proj.clone(), Role::Analysed, &[])
        ]),
        expected
    );
    assert_eq!(
        listing(&[
            (proj.clone(), Role::Analysed, &[]),
            (ctx.clone(), Role::Context, &[])
        ]),
        expected
    );
}

#[cfg(unix)]
#[test]
fn a_context_root_reached_through_an_analysed_link_keeps_its_role() {
    let dir = TempDir::new("walk_context_root_via_link");
    dir.write("ctx/c.php", "<?php");
    dir.write("proj/a.php", "<?php");
    let base = resolved(&dir);
    let proj = base.join("proj");
    std::os::unix::fs::symlink("../ctx", proj.join("c")).unwrap();

    assert_eq!(
        listing(&[
            (proj.clone(), Role::Analysed, &[]),
            (base.join("ctx"), Role::Context, &[])
        ]),
        vec![
            (base.join("ctx/c.php"), Role::Context, false),
            (proj.join("a.php"), Role::Analysed, false),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_link_cycle_ends_and_lists_each_file_once() {
    let dir = TempDir::new("walk_cycle");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    dir.write("other/y.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    std::os::unix::fs::symlink("../outside", root.join("go")).unwrap();
    std::os::unix::fs::symlink(".", base.join("outside/self")).unwrap();
    std::os::unix::fs::symlink("../other", base.join("outside/back")).unwrap();
    std::os::unix::fs::symlink("../outside", base.join("other/loop")).unwrap();

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (base.join("other/y.php"), Role::Analysed, false),
            (base.join("outside/x.php"), Role::Analysed, false),
            (root.join("a.php"), Role::Analysed, false),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_dangling_php_link_is_listed_by_its_path_and_a_link_to_a_non_php_file_is_not() {
    let dir = TempDir::new("walk_dangling");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/readme.txt", "not php");
    let root = resolved(&dir).join("proj");
    std::os::unix::fs::symlink("nowhere.php", root.join("dangling.php")).unwrap();
    std::os::unix::fs::symlink("readme.txt", root.join("notes.php")).unwrap();

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (root.join("a.php"), Role::Analysed, false),
            (root.join("dangling.php"), Role::Analysed, false),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_link_named_like_an_excluded_directory_is_not_followed() {
    let dir = TempDir::new("walk_excluded_link");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    std::os::unix::fs::symlink("../outside", root.join("vendor")).unwrap();

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &["vendor"])]),
        vec![(root.join("a.php"), Role::Analysed, false)]
    );
}

#[cfg(unix)]
#[test]
fn hard_links_stay_two_files() {
    let dir = TempDir::new("walk_hard_link");
    dir.write("proj/a.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    fs::hard_link(root.join("a.php"), root.join("b.php")).unwrap();

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (root.join("a.php"), Role::Analysed, false),
            (root.join("b.php"), Role::Analysed, false),
        ]
    );
}

#[test]
fn a_stub_root_marks_its_files_and_an_analysed_listing_stays_analysed() {
    let dir = TempDir::new("walk_stubs");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/stubs/s.php", "<?php");
    dir.write("extra/x.php", "<?php");
    let base = resolved(&dir);
    let proj = base.join("proj");

    assert_eq!(
        listing(&[
            (proj.clone(), Role::Analysed, &[]),
            (proj.join("stubs"), Role::Stub, &[]),
            (base.join("extra"), Role::Stub, &[]),
        ]),
        vec![
            (base.join("extra/x.php"), Role::Stub, true),
            (proj.join("a.php"), Role::Analysed, false),
            (proj.join("stubs/s.php"), Role::Analysed, true),
        ]
    );
}
