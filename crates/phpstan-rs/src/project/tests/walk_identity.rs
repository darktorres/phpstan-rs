//! The walk lists each file once, under its resolved path. A link is
//! followed to its target, a target under a root is left to that root's walk,
//! and a target no root holds is listed once, with the strongest role of the
//! links that reach it.

use super::super::exclude_paths::Exclusions;
use super::super::walk::{FileIndex, FinalFiles, Listed, PathCut, Role, Unstreamed, Walk};
use super::helpers::*;
use crate::test_links::{link_dir, link_file};
use composer::dir_listings::DirListings;
use composer::hashed_path::path_hash;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// The packages the walks here are given: none, so no file they list has an
/// owner.
static NO_PACKAGES: LazyLock<composer::PackageIndex> =
    LazyLock::new(composer::PackageIndex::default);

/// The exclusions that leave out each of `names` below `root`, at any depth,
/// as a configuration's `exclude_paths` would. A name needs no file on disk.
fn leaving_out(names: &[&str]) -> PathCut {
    if names.is_empty() {
        return PathCut::None;
    }
    let patterns = names
        .iter()
        .flat_map(|name| [name.to_string(), format!("*/{name}")]);
    PathCut::Analysis(Arc::new(Exclusions::anywhere(patterns)))
}

/// Lists each `(root, role, excluded)` in order, and returns the listing
/// sorted by path as `(path, role, stub)`.
fn listing(roots: &[(PathBuf, Role, &[&str])]) -> Vec<(PathBuf, Role, bool)> {
    let mut listed = in_walk_order(roots);
    listed.sort();
    listed
}

/// Lists each `(root, role, excluded)` in order, and returns the listing as
/// `(path, role, stub)` in the order the walk gives it, without a sort.
fn in_walk_order(roots: &[(PathBuf, Role, &[&str])]) -> Vec<(PathBuf, Role, bool)> {
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    for (path, role, excluded) in roots {
        walk.add(path, *role, leaving_out(excluded));
    }
    walk.finish(&crate::project::worker_pool(), &Unstreamed)
        .files
        .into_iter()
        .map(|listed| (listed.path.into_path_buf(), listed.role, listed.stub))
        .collect()
}

fn sorted(listed: Vec<Listed>) -> Vec<(PathBuf, Role, bool)> {
    let mut listed: Vec<(PathBuf, Role, bool)> = listed
        .into_iter()
        .map(|listed| (listed.path.into_path_buf(), listed.role, listed.stub))
        .collect();
    listed.sort();
    listed
}

/// `dir`, resolved, so that the expected paths are the ones the walk reaches.
fn resolved(dir: &TempDir) -> PathBuf {
    composer::canonical::canonicalize(&dir.0).unwrap()
}

#[test]
fn a_tree_without_links_is_listed_under_its_resolved_path() {
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
fn a_file_under_two_roots_is_listed_once_under_its_resolved_path() {
    let dir = TempDir::new("walk_two_roots");
    dir.write("proj/src/a.php", "<?php");
    dir.write("proj/top.php", "<?php");
    let base = resolved(&dir);
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&base.join("proj/src"), Role::Analysed, PathCut::None);
    walk.add(&base.join("proj"), Role::Analysed, PathCut::None);

    assert_eq!(
        sorted(
            walk.finish(&crate::project::worker_pool(), &Unstreamed)
                .files
        ),
        vec![
            (base.join("proj/src/a.php"), Role::Analysed, false),
            (base.join("proj/top.php"), Role::Analysed, false),
        ]
    );
}

#[test]
fn a_root_given_through_a_link_is_listed_under_its_target() {
    let dir = TempDir::new("walk_root_link");
    dir.write("proj/a.php", "<?php");
    let base = resolved(&dir);
    link_dir("proj", base.join("link"));

    assert_eq!(
        listing(&[(base.join("link"), Role::Analysed, &[])]),
        vec![(base.join("proj/a.php"), Role::Analysed, false)]
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

    let mut by_walk = Walk::new(Arc::default(), &NO_PACKAGES);
    by_walk.add(&file, Role::Context, PathCut::None);
    assert!(
        by_walk
            .finish(&crate::project::worker_pool(), &Unstreamed)
            .files
            .is_empty()
    );

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&file, Role::Context);
    assert_eq!(
        sorted(
            explicit
                .finish(&crate::project::worker_pool(), &Unstreamed)
                .files
        ),
        vec![(file.clone(), Role::Context, false)]
    );
}

#[test]
fn a_link_into_a_listed_root_is_left_to_that_root() {
    let dir = TempDir::new("walk_link_in_root");
    dir.write("proj/src/a.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    link_dir("src", root.join("alias"));

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![(root.join("src/a.php"), Role::Analysed, false)]
    );
}

#[test]
fn a_file_link_into_a_listed_root_lists_its_target_once() {
    let dir = TempDir::new("walk_file_link_in_root");
    dir.write("proj/src/a.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    link_file("src/a.php", root.join("copy.php"));

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![(root.join("src/a.php"), Role::Analysed, false)]
    );
}

#[test]
fn a_target_outside_every_root_is_listed_once_under_its_resolved_path() {
    let dir = TempDir::new("walk_outside_target");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    link_dir("../outside", root.join("one"));
    link_dir("../outside", root.join("two"));

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (base.join("outside/x.php"), Role::Analysed, false),
            (root.join("a.php"), Role::Analysed, false),
        ]
    );
}

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
    link_dir("../outside", proj.join("out"));
    link_dir("../outside", ctx.join("out"));

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

#[test]
fn a_file_reached_through_analysed_and_context_links_is_analysed() {
    let dir = TempDir::new("walk_role_by_link");
    dir.write("ctx/c.php", "<?php");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    let base = resolved(&dir);
    let ctx = base.join("ctx");
    let proj = base.join("proj");
    link_dir("../outside", ctx.join("go"));
    link_dir("../outside", proj.join("go"));
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

#[test]
fn a_context_root_reached_through_an_analysed_link_keeps_its_role() {
    let dir = TempDir::new("walk_context_root_via_link");
    dir.write("ctx/c.php", "<?php");
    dir.write("proj/a.php", "<?php");
    let base = resolved(&dir);
    let proj = base.join("proj");
    link_dir("../ctx", proj.join("c"));

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

#[test]
fn a_link_cycle_ends_and_lists_each_file_once() {
    let dir = TempDir::new("walk_cycle");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    dir.write("other/y.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    link_dir("../outside", root.join("go"));
    link_dir(".", base.join("outside/self"));
    link_dir("../other", base.join("outside/back"));
    link_dir("../outside", base.join("other/loop"));

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (base.join("other/y.php"), Role::Analysed, false),
            (base.join("outside/x.php"), Role::Analysed, false),
            (root.join("a.php"), Role::Analysed, false),
        ]
    );
}

#[test]
fn a_dangling_php_link_is_listed_by_its_path_and_a_link_to_a_non_php_file_is_not() {
    let dir = TempDir::new("walk_dangling");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/readme.txt", "not php");
    let root = resolved(&dir).join("proj");
    link_file("nowhere.php", root.join("dangling.php"));
    link_file("readme.txt", root.join("notes.php"));

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &[])]),
        vec![
            (root.join("a.php"), Role::Analysed, false),
            (root.join("dangling.php"), Role::Analysed, false),
        ]
    );
}

#[test]
fn a_link_named_like_an_excluded_directory_is_not_followed() {
    let dir = TempDir::new("walk_excluded_link");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    link_dir("../outside", root.join("vendor"));

    assert_eq!(
        listing(&[(root.clone(), Role::Analysed, &["vendor"])]),
        vec![(root.join("a.php"), Role::Analysed, false)]
    );
}

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

#[test]
fn an_explicit_link_is_listed_under_its_target() {
    let dir = TempDir::new("walk_explicit_link");
    dir.write("real/lib.php", "<?php");
    let base = resolved(&dir);
    link_file("real/lib.php", base.join("link.php"));

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&base.join("link.php"), Role::Context);
    assert_eq!(
        sorted(
            explicit
                .finish(&crate::project::worker_pool(), &Unstreamed)
                .files
        ),
        vec![(base.join("real/lib.php"), Role::Context, false)]
    );
}

#[test]
fn explicit_files_under_a_linked_directory_are_listed_under_its_target() {
    let dir = TempDir::new("walk_explicit_linked_dir");
    dir.write("real/lib.php", "<?php");
    dir.write("real/other.php", "<?php");
    let base = resolved(&dir);
    link_dir("real", base.join("alias"));

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&base.join("alias/lib.php"), Role::Context);
    explicit.add_file(&base.join("alias/other.php"), Role::Context);
    assert_eq!(
        sorted(
            explicit
                .finish(&crate::project::worker_pool(), &Unstreamed)
                .files
        ),
        vec![
            (base.join("real/lib.php"), Role::Context, false),
            (base.join("real/other.php"), Role::Context, false),
        ]
    );
}

#[test]
fn an_explicit_file_spelled_through_dot_dot_is_listed_under_its_resolved_path() {
    let dir = TempDir::new("walk_explicit_dot_dot");
    dir.write("proj/config/keep.txt", "");
    dir.write("proj/lib.php", "<?php");
    let base = resolved(&dir);

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&base.join("proj/config/../lib.php"), Role::Context);
    assert_eq!(
        sorted(
            explicit
                .finish(&crate::project::worker_pool(), &Unstreamed)
                .files
        ),
        vec![(base.join("proj/lib.php"), Role::Context, false)]
    );
}

#[test]
fn a_missing_explicit_file_is_listed_under_its_path() {
    let dir = TempDir::new("walk_explicit_missing");
    dir.write("proj/keep.txt", "");
    let base = resolved(&dir);

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&base.join("proj/missing.php"), Role::Context);
    assert_eq!(
        sorted(
            explicit
                .finish(&crate::project::worker_pool(), &Unstreamed)
                .files
        ),
        vec![(base.join("proj/missing.php"), Role::Context, false)]
    );
}

/// Walks `root` as analysed, reading its directories through `listings`, and
/// returns the listed files.
fn walk_through(root: &Path, listings: &Arc<DirListings>) -> Vec<(PathBuf, Role, bool)> {
    let mut walk = Walk::new(Arc::clone(listings), &NO_PACKAGES);
    walk.add(root, Role::Analysed, PathCut::None);
    sorted(
        walk.finish(&crate::project::worker_pool(), &Unstreamed)
            .files,
    )
}

#[test]
fn a_walk_reads_a_directory_once_through_its_listings() {
    let dir = TempDir::new("walk_listings");
    dir.write("a.php", "<?php");
    let root = resolved(&dir);
    let listings = Arc::new(DirListings::default());
    let first = walk_through(&root, &listings);
    dir.write("b.php", "<?php");
    assert_eq!(walk_through(&root, &listings), first);
    listings.clear();
    assert_eq!(walk_through(&root, &listings).len(), 2);
}

/// The directories of the link tree `oracle_tree` builds.
struct OracleTree {
    app: PathBuf,
    ctx: PathBuf,
    outside: PathBuf,
    locked: PathBuf,
}

/// A link tree written out by hand, with every case one run meets: two links
/// into one outside directory from roots of different roles, a link from an
/// analysed root into a context root and one back, a self link and a mutual
/// link, a file link, a dangling link, a link to a non-PHP file, a directory
/// `set_locked` can make unreadable, an explicit file under a context
/// directory, and a context root inside an analysed one.
fn oracle_tree(dir: &TempDir) -> OracleTree {
    dir.write("app/a.php", "<?php");
    dir.write("app/readme.txt", "not php");
    dir.write("app/locked/secret.php", "<?php");
    dir.write("app/pkg/src/p.php", "<?php");
    dir.write("ctx/c.php", "<?php");
    dir.write("outside/keep.php", "<?php");
    dir.write("outside/x.php", "<?php");
    dir.write("outside/mut_a/m.php", "<?php");
    dir.write("outside/mut_b/n.php", "<?php");
    let base = resolved(dir);
    let tree = OracleTree {
        app: base.join("app"),
        ctx: base.join("ctx"),
        outside: base.join("outside"),
        locked: base.join("app/locked"),
    };
    link_dir("../outside", tree.app.join("to_outside"));
    link_dir("../ctx", tree.app.join("to_ctx"));
    link_dir(".", tree.app.join("self"));
    link_file("../outside/x.php", tree.app.join("copy.php"));
    link_file("nowhere.php", tree.app.join("dangling.php"));
    link_file("readme.txt", tree.app.join("notes.php"));
    link_dir("../outside", tree.ctx.join("go"));
    link_dir("../app", tree.ctx.join("back"));
    link_dir("../mut_b", tree.outside.join("mut_a/back"));
    link_dir("../mut_a", tree.outside.join("mut_b/back"));
    link_dir(".", tree.outside.join("self"));
    tree
}

/// What `oracle_tree`'s roots list on `pool`, in the order the walk gives it.
fn oracle_walk(tree: &OracleTree, pool: &rayon::ThreadPool) -> Vec<Listed> {
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&tree.app, Role::Analysed, PathCut::None);
    walk.add(&tree.app.join("pkg/src"), Role::Context, PathCut::None);
    walk.add(&tree.ctx, Role::Context, PathCut::None);
    walk.add_file(&tree.ctx.join("c.php"), Role::Context);
    walk.finish(pool, &Unstreamed).files
}

/// Makes the tree's `locked` directory unreadable, or readable again. A Unix
/// mode does it; elsewhere the directory stays readable.
#[cfg(unix)]
fn set_locked(tree: &OracleTree, locked: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if locked { 0o000 } else { 0o755 };
    fs::set_permissions(&tree.locked, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_locked(_tree: &OracleTree, _locked: bool) {}

#[test]
fn a_link_tree_is_listed_as_written_by_hand() {
    let dir = TempDir::new("walk_oracle");
    let tree = oracle_tree(&dir);
    set_locked(&tree, true);
    // A user who can read a directory of mode 000 (root) lists what is in it,
    // as does a platform that cannot lock one.
    let readable = fs::read_dir(&tree.locked).is_ok();
    let found = sorted(oracle_walk(&tree, &crate::project::worker_pool()));
    set_locked(&tree, false);

    let (app, ctx, outside) = (&tree.app, &tree.ctx, &tree.outside);
    let mut expected = vec![
        (app.join("a.php"), Role::Analysed, false),
        (app.join("dangling.php"), Role::Analysed, false),
        (app.join("pkg/src/p.php"), Role::Analysed, false),
        (ctx.join("c.php"), Role::Context, false),
        (outside.join("keep.php"), Role::Analysed, false),
        (outside.join("mut_a/m.php"), Role::Analysed, false),
        (outside.join("mut_b/n.php"), Role::Analysed, false),
        (outside.join("x.php"), Role::Analysed, false),
    ];
    if readable {
        expected.push((tree.locked.join("secret.php"), Role::Analysed, false));
        expected.sort();
    }
    assert_eq!(found, expected);
}

/// The listing comes out in the same order on one thread, on four, on the
/// worker pool, and again on the worker pool.
#[test]
fn the_listing_order_does_not_depend_on_the_thread_count() {
    let dir = TempDir::new("walk_threads");
    let tree = oracle_tree(&dir);
    set_locked(&tree, true);
    let single = oracle_walk(&tree, &thread_pool(1));
    let four = oracle_walk(&tree, &thread_pool(4));
    let default = oracle_walk(&tree, &crate::project::worker_pool());
    let again = oracle_walk(&tree, &crate::project::worker_pool());
    set_locked(&tree, false);

    assert!(!single.is_empty());
    assert_eq!(single, four);
    assert_eq!(single, default);
    assert_eq!(default, again);
}

fn thread_pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
}

/// The hash a walk gives each listed file is the hash of its path, and the
/// index finds each file at its position.
#[test]
fn a_listed_file_is_hashed_as_its_path_and_indexed_there() {
    let dir = TempDir::new("walk_hash");
    let tree = oracle_tree(&dir);
    set_locked(&tree, true);
    let files = oracle_walk(&tree, &crate::project::worker_pool());
    set_locked(&tree, false);

    assert!(!files.is_empty());
    let index = FileIndex::of(&files);
    for (at, file) in files.iter().enumerate() {
        let path = file.path.as_path();
        assert_eq!(file.path.hash(), path_hash(path), "{}", path.display());
        assert_eq!(index.position(&files, path), Some(at));
    }
}

/// Each file a walk lists carries the package that owns its directory. A file
/// reached through a link is owned by the package of its target, and a dangling
/// `.php` link by the package of the directory it is in.
#[test]
fn a_listed_file_carries_the_package_that_owns_its_directory() {
    let dir = TempDir::new("walk_owners");
    dir.write("composer.json", r#"{"name": "acme/app"}"#);
    dir.write(
        "vendor/composer/installed.json",
        r#"{"packages": [{"name": "acme/lib", "type": "library", "install-path": "../acme/lib"}]}"#,
    );
    dir.write("vendor/acme/lib/src/A.php", "<?php");
    dir.write("src/B.php", "<?php");
    let base = resolved(&dir);
    link_file("../vendor/acme/lib/src/A.php", base.join("src/Linked.php"));
    link_file("nowhere.php", base.join("src/Dangling.php"));
    let packages = composer::PackageIndex::new(
        composer::manifest::ComposerManifest::load(
            &base.join("composer.json"),
            &base,
            &base,
            Some(&base.join("vendor")),
            &composer::dir_listings::DirListings::default(),
        )
        .into_iter()
        .collect(),
    );

    let mut walk = Walk::new(Arc::default(), &packages);
    walk.add(&base.join("src"), Role::Analysed, PathCut::None);
    let files = walk
        .finish(&crate::project::worker_pool(), &Unstreamed)
        .files;

    let owner = |name: &str| {
        let file = files
            .iter()
            .find(|file| file.path.as_path().ends_with(name))
            .expect(name);
        file.owner.flatten().and_then(|owner| packages.name(owner))
    };
    assert_eq!(files.len(), 3);
    assert_eq!(owner("B.php"), Some("acme/app"));
    assert_eq!(owner("Dangling.php"), Some("acme/app"));
    assert_eq!(owner("A.php"), Some("acme/lib"));
}

/// The walk gives its files in path order, which compares components, not the
/// text: `a/x.php` comes before `a-b.php`. That holds across nested roots (a
/// context root inside an excluding root, a stub root inside an analysed one),
/// a link whose target holds a root, and a dangling link. A file's role is the
/// strongest that lists it.
#[test]
fn the_listing_is_in_path_order_across_nested_roots_links_and_stubs() {
    let dir = TempDir::new("walk_order");
    dir.write("app/a/x.php", "<?php");
    dir.write("app/a-b.php", "<?php");
    dir.write("app/a.php", "<?php");
    dir.write("app/stubs/s.php", "<?php");
    dir.write("app/vendor/lib/v.php", "<?php");
    dir.write("app/z.php", "<?php");
    dir.write("shared/a.php", "<?php");
    dir.write("shared/n/k.php", "<?php");
    dir.write("shared/z.php", "<?php");
    let base = resolved(&dir);
    link_dir(base.join("shared"), base.join("app/linked"));
    link_file(base.join("nowhere.php"), base.join("app/dangling.php"));
    let app = base.join("app");
    let excluded: &[&str] = &["vendor"];

    assert_eq!(
        in_walk_order(&[
            (app.clone(), Role::Analysed, excluded),
            (app.join("vendor/lib"), Role::Context, &[]),
            (app.join("stubs"), Role::Stub, &[]),
            (base.join("shared/n"), Role::Context, &[]),
        ]),
        vec![
            (app.join("a/x.php"), Role::Analysed, false),
            (app.join("a-b.php"), Role::Analysed, false),
            (app.join("a.php"), Role::Analysed, false),
            (app.join("dangling.php"), Role::Analysed, false),
            (app.join("stubs/s.php"), Role::Analysed, true),
            (app.join("vendor/lib/v.php"), Role::Context, false),
            (app.join("z.php"), Role::Analysed, false),
            (base.join("shared/a.php"), Role::Analysed, false),
            (base.join("shared/n/k.php"), Role::Context, false),
            (base.join("shared/z.php"), Role::Analysed, false),
        ]
    );
}

/// The order does not depend on the order the roots are added in.
#[test]
fn the_listing_does_not_depend_on_the_order_the_roots_are_added_in() {
    let dir = TempDir::new("walk_root_order");
    dir.write("app/a.php", "<?php");
    dir.write("app/sub/b.php", "<?php");
    dir.write("app/vendor/c.php", "<?php");
    dir.write("app/vendor/lib/d.php", "<?php");
    let app = resolved(&dir).join("app");
    let excluded: &[&str] = &["vendor"];
    let expected = vec![
        (app.join("a.php"), Role::Analysed, false),
        (app.join("sub/b.php"), Role::Analysed, false),
        (app.join("vendor/lib/d.php"), Role::Context, false),
    ];

    let forward = in_walk_order(&[
        (app.clone(), Role::Analysed, excluded),
        (app.join("vendor/lib"), Role::Context, &[]),
    ]);
    let backward = in_walk_order(&[
        (app.join("vendor/lib"), Role::Context, &[]),
        (app.clone(), Role::Analysed, excluded),
    ]);
    assert_eq!(forward, expected);
    assert_eq!(backward, expected);
}

/// A file the tree cannot place, because the directory it is in cannot be
/// listed, is merged into the listing in path order.
#[cfg(unix)]
#[test]
fn a_file_the_tree_cannot_place_keeps_the_listing_in_path_order() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("walk_unplaced");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/locked/x.php", "<?php");
    dir.write("proj/z.php", "<?php");
    let base = resolved(&dir).join("proj");
    let locked = base.join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o311)).unwrap();
    // A user who can read a directory of mode 311 (root) has nothing to test.
    let unreadable = fs::read_dir(&locked).is_err();
    let listed = in_walk_order(&[
        (base.clone(), Role::Analysed, &[]),
        (locked.join("x.php"), Role::Context, &[]),
    ]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if !unreadable {
        return;
    }

    assert_eq!(
        listed,
        vec![
            (base.join("a.php"), Role::Analysed, false),
            (locked.join("x.php"), Role::Context, false),
            (base.join("z.php"), Role::Analysed, false),
        ]
    );
}

/// A sink that keeps the files it is handed.
#[derive(Default)]
struct Handed(Mutex<Vec<Listed>>);

impl FinalFiles for Handed {
    fn take<'s>(&'s self, _scope: &rayon::Scope<'s>, files: Vec<Listed>) {
        self.0.lock().unwrap().extend(files);
    }
}

/// Runs `walk` with a sink that keeps the files it hands over. Returns those
/// files, in path order, and the listing.
fn handed_over(walk: Walk<'_>) -> (Vec<Listed>, Vec<Listed>) {
    let handed = Handed::default();
    let listing = walk.finish(&crate::project::worker_pool(), &handed).files;
    let mut handed = handed.0.into_inner().unwrap();
    handed.sort_by(|a, b| a.path.cmp(&b.path));
    (handed, listing)
}

#[test]
fn a_file_is_handed_over_by_the_one_walk_of_its_strongest_role_as_it_is_listed() {
    let dir = TempDir::new("walk_handed");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/sub/b.php", "<?php");
    dir.write("proj/vendor/pkg/src/x.php", "<?php");
    dir.write("proj/vendor/pkg/y.php", "<?php");
    dir.write("proj/ctx/c.php", "<?php");
    dir.write("proj/stubs/s.php", "<?php");
    let root = resolved(&dir).join("proj");
    let package = root.join("vendor/pkg");

    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&root, Role::Analysed, leaving_out(&["vendor"]));
    walk.add(&package, Role::Context, PathCut::None);
    walk.add(&root.join("ctx"), Role::Context, PathCut::None);
    walk.add(&root.join("stubs"), Role::Stub, PathCut::None);
    let (handed, listing) = handed_over(walk);

    // The analysed root excludes `vendor`, so only the package root lists the
    // package's files. The files of `ctx` and `stubs` are listed by two roots,
    // and the analysed one alone has their strongest role, so it hands them
    // over, those of `stubs` with the stub root's mark.
    let expected = vec![
        (root.join("a.php"), Role::Analysed, false),
        (root.join("ctx/c.php"), Role::Analysed, false),
        (root.join("stubs/s.php"), Role::Analysed, true),
        (root.join("sub/b.php"), Role::Analysed, false),
        (package.join("src/x.php"), Role::Context, false),
        (package.join("y.php"), Role::Context, false),
    ];
    assert_eq!(sorted(handed), expected);
    assert_eq!(sorted(listing), expected);
}

#[test]
fn a_file_two_walks_of_its_strongest_role_list_is_handed_over_by_neither() {
    let dir = TempDir::new("walk_handed_two_strongest");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/src/s.php", "<?php");
    let root = resolved(&dir).join("proj");

    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&root, Role::Stub, leaving_out(&["vendor"]));
    walk.add(&root.join("src"), Role::Stub, leaving_out(&["vendor"]));
    walk.add(&root.join("src"), Role::Context, PathCut::None);
    let (handed, listing) = handed_over(walk);

    // Both stub roots list `src/s.php`, so its role is known when the walk
    // ends; the stub root of `proj` alone lists `a.php`.
    assert_eq!(sorted(handed), vec![(root.join("a.php"), Role::Stub, true)]);
    assert_eq!(
        sorted(listing),
        vec![
            (root.join("a.php"), Role::Stub, true),
            (root.join("src/s.php"), Role::Stub, true),
        ]
    );
}

#[test]
fn a_link_target_is_listed_but_never_handed_over() {
    let dir = TempDir::new("walk_link_handed");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/o.php", "<?php");
    let base = resolved(&dir);
    let proj = base.join("proj");
    link_dir("../outside", proj.join("out"));

    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&proj, Role::Analysed, PathCut::None);
    let (handed, listing) = handed_over(walk);

    assert_eq!(
        sorted(handed),
        vec![(proj.join("a.php"), Role::Analysed, false)]
    );
    assert_eq!(
        sorted(listing),
        vec![
            (base.join("outside/o.php"), Role::Analysed, false),
            (proj.join("a.php"), Role::Analysed, false),
        ]
    );
}

#[test]
fn an_explicit_file_is_handed_over_by_the_one_walk_of_its_strongest_role() {
    let dir = TempDir::new("walk_handed_explicit");
    dir.write("proj/a.php", "<?php");
    dir.write("other/b.php", "<?php");
    dir.write("other/c.php", "<?php");
    let base = resolved(&dir);
    let proj = base.join("proj");

    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&proj, Role::Analysed, PathCut::None);
    walk.add_file(&proj.join("a.php"), Role::Context);
    walk.add_file(&base.join("other/b.php"), Role::Context);
    walk.add_file(&base.join("other/b.php"), Role::Stub);
    walk.add_file(&base.join("other/c.php"), Role::Context);
    walk.add_file(&base.join("other/c.php"), Role::Context);
    let (handed, listing) = handed_over(walk);

    // The analysed walk lists `a.php` with the strongest role, and the stub
    // file walk `b.php`; two context file walks list `c.php`.
    assert_eq!(
        sorted(handed),
        vec![
            (base.join("other/b.php"), Role::Stub, true),
            (proj.join("a.php"), Role::Analysed, false),
        ]
    );
    assert_eq!(listing.len(), 3);
}

#[test]
fn a_context_root_under_a_covering_root_is_handed_over_under_the_covering_role() {
    let dir = TempDir::new("walk_handed_covered");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/ctx/c.php", "<?php");
    let root = resolved(&dir).join("proj");

    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&root, Role::Analysed, PathCut::None);
    walk.add(&root.join("ctx"), Role::Context, PathCut::None);
    let (handed, listing) = handed_over(walk);

    // The analysed root excludes no names, so it covers `ctx`, whose walk
    // lists nothing. The analysed root alone lists `ctx/c.php`, so the file
    // is handed over with the analysed role it is listed with.
    let expected = vec![
        (root.join("a.php"), Role::Analysed, false),
        (root.join("ctx/c.php"), Role::Analysed, false),
    ];
    assert_eq!(sorted(handed), expected);
    assert_eq!(sorted(listing), expected);
}

#[test]
fn a_context_walk_stops_at_a_covering_root_below_it() {
    let dir = TempDir::new("walk_handed_partly_covered");
    dir.write("proj/x.php", "<?php");
    dir.write("proj/src/s.php", "<?php");
    let root = resolved(&dir).join("proj");

    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&root, Role::Context, PathCut::None);
    walk.add(&root.join("src"), Role::Analysed, PathCut::None);
    let (handed, listing) = handed_over(walk);

    // The context walk lists `x.php` and stops at `src`, which the analysed
    // root covers. `src/s.php` is listed by the analysed root alone.
    let expected = vec![
        (root.join("src/s.php"), Role::Analysed, false),
        (root.join("x.php"), Role::Context, false),
    ];
    assert_eq!(sorted(handed), expected);
    assert_eq!(sorted(listing), expected);
}

/// What a walk hands over, in path order, what it lists, and its roots, in
/// path order.
type Outcome = (Vec<Listed>, Vec<Listed>, Vec<(PathBuf, Role)>);

/// Runs `walk` as `handed_over` does, and adds its roots.
fn walked_with_roots(walk: Walk<'_>) -> Outcome {
    let handed = Handed::default();
    let walked = walk.finish(&crate::project::worker_pool(), &handed);
    let mut handed = handed.0.into_inner().unwrap();
    handed.sort_by(|a, b| a.path.cmp(&b.path));
    let mut roots: Vec<(PathBuf, Role)> = walked
        .roots
        .iter()
        .map(|root| (root.path.clone(), root.role))
        .collect();
    roots.sort();
    (handed, walked.files, roots)
}

#[test]
fn a_path_that_discovery_resolved_is_walked_as_when_the_walk_resolves_it() {
    use composer::vendor::VendorPath;

    let dir = TempDir::new("walk_vendor_resolved");
    let base = resolved(&dir);
    dir.write("composer.json", "{}");
    dir.write("vendor/composer/ClassLoader.php", "<?php");
    dir.write("vendor/acme/a/src/A.php", "<?php");
    dir.write("vendor/acme/a/src/deep/D.php", "<?php");
    dir.write("vendor/acme/a/Extra.php", "<?php");
    dir.write("vendor/acme/a/Target.php", "<?php");
    link_file(
        base.join("vendor/acme/a/Target.php"),
        base.join("vendor/acme/a/Linked.php"),
    );
    // A path-repository package, linked into `vendor/`.
    dir.write("packages/b/src/B.php", "<?php");
    dir.write("packages/b/Other.php", "<?php");
    link_dir(base.join("packages/b"), base.join("vendor/acme/b"));
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'A' => $vendorDir . '/acme/a/src/A.php',
    'Extra' => $vendorDir . '/acme/a/Extra.php',
    'Linked' => $vendorDir . '/acme/a/Linked.php',
    'Other' => $vendorDir . '/acme/b/Other.php',
);
"#,
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'A\\' => array($vendorDir . '/acme/a/src'),
    'B\\' => array($vendorDir . '/acme/b/src'),
);
"#,
    );

    let listings = Arc::new(DirListings::default());
    let found = composer::vendor::discover_vendor_context(&base.join("vendor"), &listings, None);
    let resolved_ones = |paths: &[VendorPath]| {
        paths
            .iter()
            .filter(|path| matches!(path, VendorPath::Resolved(_)))
            .count()
    };
    // `a/src`, `Extra.php` and `ClassLoader.php` are resolved; `b/src` and
    // `Other.php` are kept as written, and `Linked.php` was resolved through
    // its link.
    assert_eq!(
        (resolved_ones(&found.dirs), found.dirs.len()),
        (1, 2),
        "{:?}",
        found.dirs
    );
    assert_eq!(
        (resolved_ones(&found.files), found.files.len()),
        (2, 4),
        "{:?}",
        found.files
    );

    let mut taken = Walk::new(Arc::clone(&listings), &NO_PACKAGES);
    let mut resolving = Walk::new(Arc::clone(&listings), &NO_PACKAGES);
    for dir in &found.dirs {
        match dir {
            VendorPath::Resolved(dir) => {
                taken.add_resolved_dir(dir.clone(), Role::Context, PathCut::None)
            }
            VendorPath::Unresolved(dir) => taken.add(dir, Role::Context, PathCut::None),
        }
        resolving.add(dir.as_path(), Role::Context, PathCut::None);
    }
    for file in &found.files {
        match file {
            VendorPath::Resolved(file) => taken.add_resolved_file(file.clone(), Role::Context),
            VendorPath::Unresolved(file) => taken.add_file(file, Role::Context),
        }
        resolving.add_file(file.as_path(), Role::Context);
    }
    let taken = walked_with_roots(taken);
    assert_eq!(taken, walked_with_roots(resolving));
    // The files under the two directories are handed over, and so are the
    // explicit files, each listed by its own walk alone.
    let handed: Vec<PathBuf> = taken
        .0
        .iter()
        .map(|listed| listed.path.as_path().to_path_buf())
        .collect();
    assert_eq!(
        handed,
        [
            "packages/b/Other.php",
            "packages/b/src/B.php",
            "vendor/acme/a/Extra.php",
            "vendor/acme/a/Target.php",
            "vendor/acme/a/src/A.php",
            "vendor/acme/a/src/deep/D.php",
            "vendor/composer/ClassLoader.php",
        ]
        .map(|path| base.join(path))
    );
}

#[test]
fn context_roots_given_while_the_stronger_walks_run_are_walked_as_when_given_first() {
    let dir = TempDir::new("walk_context_later");
    dir.write("proj/a.php", "<?php");
    dir.write("proj/src/S.php", "<?php");
    dir.write("proj/src/Kept.php", "<?php");
    dir.write("proj/ctx/C.php", "<?php");
    dir.write("proj/stubs/St.php", "<?php");
    dir.write("proj/vendor/acme/lib/Beside.php", "<?php");
    dir.write("proj/vendor/acme/lib/src/L.php", "<?php");
    dir.write("proj/vendor/acme/lib/src/inner/I.php", "<?php");
    dir.write("scan/Sc.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    let lib = root.join("vendor/acme/lib");
    let analysed = |walk: &mut Walk<'_>| walk.add(&root, Role::Analysed, leaving_out(&["vendor"]));
    let stub = |walk: &mut Walk<'_>| walk.add(&root.join("stubs"), Role::Stub, PathCut::None);
    let context_dirs = [lib.join("src"), root.join("ctx"), base.join("scan")];
    let context_files = [lib.join("Beside.php"), root.join("src/Kept.php")];

    // Every root given before the walk starts, the stub root last.
    let mut first = Walk::new(Arc::default(), &NO_PACKAGES);
    analysed(&mut first);
    for dir in &context_dirs {
        first.add(dir, Role::Context, PathCut::None);
    }
    for file in &context_files {
        first.add_file(file, Role::Context);
    }
    stub(&mut first);
    let first = walked_with_roots(first);

    // The analysed and stub roots given first, and the context roots while
    // their walks run.
    let mut later = Walk::new(Arc::default(), &NO_PACKAGES);
    analysed(&mut later);
    stub(&mut later);
    let handed = Handed::default();
    let walked = later.finish_with(&crate::project::worker_pool(), &handed, |context| {
        for dir in &context_dirs {
            context.add(dir);
        }
        for file in &context_files {
            context.add_file(file);
        }
    });
    let mut later_handed = handed.0.into_inner().unwrap();
    later_handed.sort_by(|a, b| a.path.cmp(&b.path));
    let mut later_roots: Vec<(PathBuf, Role)> = walked
        .roots
        .iter()
        .map(|root| (root.path.clone(), root.role))
        .collect();
    later_roots.sort();
    assert_eq!((later_handed, walked.files, later_roots), first);

    // Each file is handed over once, by the one walk of its strongest role:
    // `Kept.php`, a context file root in an analysed directory, and `ctx`, a
    // context root in the analysed root, by the analysed walk; `stubs` by the
    // analysed walk too, with the stub mark; the vendor files by their own
    // walks.
    assert_eq!(
        sorted(first.0),
        vec![
            (root.join("a.php"), Role::Analysed, false),
            (root.join("ctx/C.php"), Role::Analysed, false),
            (root.join("src/Kept.php"), Role::Analysed, false),
            (root.join("src/S.php"), Role::Analysed, false),
            (root.join("stubs/St.php"), Role::Analysed, true),
            (lib.join("Beside.php"), Role::Context, false),
            (lib.join("src/L.php"), Role::Context, false),
            (lib.join("src/inner/I.php"), Role::Context, false),
            (base.join("scan/Sc.php"), Role::Context, false),
        ]
    );
}

/// A Composer project under `proj` whose vendor directory is `vendor_dir`: a
/// file of the project, an installed.json and two files of one package.
/// Returns the project's resolved path.
fn composer_project(dir: &TempDir, vendor_dir: &str) -> PathBuf {
    dir.write("proj/a.php", "<?php");
    dir.write(&format!("proj/{vendor_dir}/composer/installed.json"), "{}");
    dir.write(&format!("proj/{vendor_dir}/pkg/src/x.php"), "<?php");
    dir.write(&format!("proj/{vendor_dir}/pkg/y.php"), "<?php");
    resolved(dir).join("proj")
}

#[test]
fn a_degrade_root_leaves_out_a_composer_vendor_directory_and_its_context_walk_lists_it() {
    for vendor_dir in ["vendor", "libs", "libs/vendor"] {
        let dir = TempDir::new("walk_vendor_handed");
        let root = composer_project(&dir, vendor_dir);
        let vendor = root.join(vendor_dir);
        let composer = |path: &Path| path == vendor.as_path();

        let mut walk = Walk::new(Arc::default(), &NO_PACKAGES).with_composer(&composer);
        walk.add_leaving_out_composer(&root, Role::Analysed, PathCut::None);
        walk.add(&vendor, Role::Context, PathCut::None);
        let (handed, listing) = handed_over(walk);

        // Only the context walk lists the vendor directory's files, so it
        // hands them over, and each is listed once.
        let expected = vec![
            (root.join("a.php"), Role::Analysed, false),
            (vendor.join("pkg/src/x.php"), Role::Context, false),
            (vendor.join("pkg/y.php"), Role::Context, false),
        ];
        assert_eq!(sorted(handed), expected.clone(), "{vendor_dir}");
        assert_eq!(sorted(listing), expected, "{vendor_dir}");
    }
}

#[test]
fn a_configured_root_and_a_degrade_root_over_one_composer_project_list_each_file_once() {
    let dir = TempDir::new("walk_vendor_overlap");
    let root = composer_project(&dir, "vendor");
    let vendor = root.join("vendor");
    let composer = |path: &Path| path == vendor.as_path();

    // The configured root names the vendor directory, so its files are
    // analysed; the degrade root leaves the directory out, and the two walks
    // meet over the same files.
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES).with_composer(&composer);
    walk.add_leaving_out_composer(&root, Role::Analysed, PathCut::None);
    walk.add(&vendor, Role::Analysed, PathCut::None);
    let (handed, listing) = handed_over(walk);

    let expected = vec![
        (root.join("a.php"), Role::Analysed, false),
        (vendor.join("pkg/src/x.php"), Role::Analysed, false),
        (vendor.join("pkg/y.php"), Role::Analysed, false),
    ];
    assert_eq!(sorted(handed), expected);
    assert_eq!(sorted(listing), expected);
}
