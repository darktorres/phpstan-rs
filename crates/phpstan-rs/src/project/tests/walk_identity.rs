//! The walk lists each file once, under its resolved path. A link is
//! followed to its target, a target under a root is left to that root's walk,
//! and a target no root holds is listed once, with the strongest role of the
//! links that reach it.

use super::super::walk::{FileIndex, Listed, Role, Walk, path_hash};
use super::helpers::*;
use composer::dir_listings::DirListings;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

/// The packages the walks here are given: none, so no file they list has an
/// owner.
static NO_PACKAGES: LazyLock<composer::PackageIndex> =
    LazyLock::new(composer::PackageIndex::default);

/// Lists each `(root, role, excluded)` in order, and returns the listing
/// sorted by path as `(path, role, stub)`.
fn listing(roots: &[(PathBuf, Role, &[&str])]) -> Vec<(PathBuf, Role, bool)> {
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    for (path, role, excluded) in roots {
        walk.add(path, *role, excluded);
    }
    sorted(walk.finish(&crate::project::worker_pool()).files)
}

fn sorted(listed: Vec<Listed>) -> Vec<(PathBuf, Role, bool)> {
    let mut listed: Vec<(PathBuf, Role, bool)> = listed
        .into_iter()
        .map(|listed| (listed.path, listed.role, listed.stub))
        .collect();
    listed.sort();
    listed
}

/// `dir`, resolved, so that the expected paths are the ones the walk reaches.
fn resolved(dir: &TempDir) -> PathBuf {
    fs::canonicalize(&dir.0).unwrap()
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
    walk.add(&base.join("proj/src"), Role::Analysed, &[]);
    walk.add(&base.join("proj"), Role::Analysed, &[]);

    assert_eq!(
        sorted(walk.finish(&crate::project::worker_pool()).files),
        vec![
            (base.join("proj/src/a.php"), Role::Analysed, false),
            (base.join("proj/top.php"), Role::Analysed, false),
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_root_given_through_a_link_is_listed_under_its_target() {
    let dir = TempDir::new("walk_root_link");
    dir.write("proj/a.php", "<?php");
    let base = resolved(&dir);
    std::os::unix::fs::symlink("proj", base.join("link")).unwrap();

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
    by_walk.add(&file, Role::Context, &[]);
    assert!(
        by_walk
            .finish(&crate::project::worker_pool())
            .files
            .is_empty()
    );

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&file, Role::Context);
    assert_eq!(
        sorted(explicit.finish(&crate::project::worker_pool()).files),
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

#[cfg(unix)]
#[test]
fn an_explicit_link_is_listed_under_its_target() {
    let dir = TempDir::new("walk_explicit_link");
    dir.write("real/lib.php", "<?php");
    let base = resolved(&dir);
    std::os::unix::fs::symlink("real/lib.php", base.join("link.php")).unwrap();

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&base.join("link.php"), Role::Context);
    assert_eq!(
        sorted(explicit.finish(&crate::project::worker_pool()).files),
        vec![(base.join("real/lib.php"), Role::Context, false)]
    );
}

#[cfg(unix)]
#[test]
fn explicit_files_under_a_linked_directory_are_listed_under_its_target() {
    let dir = TempDir::new("walk_explicit_linked_dir");
    dir.write("real/lib.php", "<?php");
    dir.write("real/other.php", "<?php");
    let base = resolved(&dir);
    std::os::unix::fs::symlink("real", base.join("alias")).unwrap();

    let mut explicit = Walk::new(Arc::default(), &NO_PACKAGES);
    explicit.add_file(&base.join("alias/lib.php"), Role::Context);
    explicit.add_file(&base.join("alias/other.php"), Role::Context);
    assert_eq!(
        sorted(explicit.finish(&crate::project::worker_pool()).files),
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
        sorted(explicit.finish(&crate::project::worker_pool()).files),
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
        sorted(explicit.finish(&crate::project::worker_pool()).files),
        vec![(base.join("proj/missing.php"), Role::Context, false)]
    );
}

/// Walks `root` as analysed, reading its directories through `listings`, and
/// returns the listed files.
fn walk_through(root: &Path, listings: &Arc<DirListings>) -> Vec<(PathBuf, Role, bool)> {
    let mut walk = Walk::new(Arc::clone(listings), &NO_PACKAGES);
    walk.add(root, Role::Analysed, &[]);
    sorted(walk.finish(&crate::project::worker_pool()).files)
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
#[cfg(unix)]
struct OracleTree {
    app: PathBuf,
    ctx: PathBuf,
    outside: PathBuf,
    locked: PathBuf,
}

/// A link tree written out by hand, with every case one run meets: two links
/// into one outside directory from roots of different roles, a link from an
/// analysed root into a context root and one back, a self link and a mutual
/// link, a file link, a dangling link, a link to a non-PHP file, an unreadable
/// directory, an explicit file under a context directory, and a context root
/// inside an analysed one.
#[cfg(unix)]
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
    let symlink = |target: &str, at: PathBuf| std::os::unix::fs::symlink(target, at).unwrap();
    symlink("../outside", tree.app.join("to_outside"));
    symlink("../ctx", tree.app.join("to_ctx"));
    symlink(".", tree.app.join("self"));
    symlink("../outside/x.php", tree.app.join("copy.php"));
    symlink("nowhere.php", tree.app.join("dangling.php"));
    symlink("readme.txt", tree.app.join("notes.php"));
    symlink("../outside", tree.ctx.join("go"));
    symlink("../app", tree.ctx.join("back"));
    symlink("../mut_b", tree.outside.join("mut_a/back"));
    symlink("../mut_a", tree.outside.join("mut_b/back"));
    symlink(".", tree.outside.join("self"));
    tree
}

/// What `oracle_tree`'s roots list on `pool`, in the order the walk gives it.
#[cfg(unix)]
fn oracle_walk(tree: &OracleTree, pool: &rayon::ThreadPool) -> Vec<Listed> {
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES);
    walk.add(&tree.app, Role::Analysed, &[]);
    walk.add(&tree.app.join("pkg/src"), Role::Context, &[]);
    walk.add(&tree.ctx, Role::Context, &[]);
    walk.add_file(&tree.ctx.join("c.php"), Role::Context);
    walk.finish(pool).files
}

/// Sets the mode of the unreadable directory, `0o000` or `0o755`.
#[cfg(unix)]
fn set_locked(tree: &OracleTree, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&tree.locked, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_link_tree_is_listed_as_written_by_hand() {
    let dir = TempDir::new("walk_oracle");
    let tree = oracle_tree(&dir);
    set_locked(&tree, 0o000);
    // A user who can read a directory of mode 000 (root) has nothing to test.
    let unreadable = fs::read_dir(&tree.locked).is_err();
    let found = sorted(oracle_walk(&tree, &crate::project::worker_pool()));
    set_locked(&tree, 0o755);
    if !unreadable {
        return;
    }

    let (app, ctx, outside) = (&tree.app, &tree.ctx, &tree.outside);
    assert_eq!(
        found,
        vec![
            (app.join("a.php"), Role::Analysed, false),
            (app.join("dangling.php"), Role::Analysed, false),
            (app.join("pkg/src/p.php"), Role::Analysed, false),
            (ctx.join("c.php"), Role::Context, false),
            (outside.join("keep.php"), Role::Analysed, false),
            (outside.join("mut_a/m.php"), Role::Analysed, false),
            (outside.join("mut_b/n.php"), Role::Analysed, false),
            (outside.join("x.php"), Role::Analysed, false),
        ]
    );
}

/// The listing comes out in the same order on one thread, on four, on the
/// worker pool, and again on the worker pool.
#[cfg(unix)]
#[test]
fn the_listing_order_does_not_depend_on_the_thread_count() {
    let dir = TempDir::new("walk_threads");
    let tree = oracle_tree(&dir);
    set_locked(&tree, 0o000);
    let single = oracle_walk(&tree, &thread_pool(1));
    let four = oracle_walk(&tree, &thread_pool(4));
    let default = oracle_walk(&tree, &crate::project::worker_pool());
    let again = oracle_walk(&tree, &crate::project::worker_pool());
    set_locked(&tree, 0o755);

    assert!(!single.is_empty());
    assert_eq!(single, four);
    assert_eq!(single, default);
    assert_eq!(default, again);
}

#[cfg(unix)]
fn thread_pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
}

#[test]
fn a_path_hashes_by_its_components() {
    assert_eq!(path_hash(Path::new("/a/b/")), path_hash(Path::new("/a//b")));
    assert_ne!(path_hash(Path::new("/a/b")), path_hash(Path::new("/a/c")));
}

/// The hash a walk gives each listed file is the hash of its path, and the
/// index finds each file at its position.
#[cfg(unix)]
#[test]
fn a_listed_file_is_hashed_as_its_path_and_indexed_there() {
    let dir = TempDir::new("walk_hash");
    let tree = oracle_tree(&dir);
    set_locked(&tree, 0o000);
    let files = oracle_walk(&tree, &crate::project::worker_pool());
    set_locked(&tree, 0o755);

    assert!(!files.is_empty());
    let index = FileIndex::of(&files);
    for (at, file) in files.iter().enumerate() {
        assert_eq!(file.hash, path_hash(&file.path), "{}", file.path.display());
        assert_eq!(index.position(&files, &file.path), Some(at));
    }
}

/// Each file a walk lists carries the package that owns its directory. A file
/// reached through a link is owned by the package of its target, and a dangling
/// `.php` link by the package of the directory it is in.
#[cfg(unix)]
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
    std::os::unix::fs::symlink("../vendor/acme/lib/src/A.php", base.join("src/Linked.php"))
        .unwrap();
    std::os::unix::fs::symlink("nowhere.php", base.join("src/Dangling.php")).unwrap();
    let packages = composer::PackageIndex::new(
        composer::manifest::ComposerManifest::load(&base, &base)
            .into_iter()
            .collect(),
    );

    let mut walk = Walk::new(Arc::default(), &packages);
    walk.add(&base.join("src"), Role::Analysed, &[]);
    let files = walk.finish(&crate::project::worker_pool()).files;

    let owner = |name: &str| {
        let file = files
            .iter()
            .find(|file| file.path.ends_with(name))
            .expect(name);
        file.owner.flatten().and_then(|owner| packages.name(owner))
    };
    assert_eq!(files.len(), 3);
    assert_eq!(owner("B.php"), Some("acme/app"));
    assert_eq!(owner("Dangling.php"), Some("acme/app"));
    assert_eq!(owner("A.php"), Some("acme/lib"));
}
