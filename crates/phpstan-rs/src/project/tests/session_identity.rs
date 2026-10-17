//! The session classifies a path as the project keys it: the path resolved
//! through links, with the role the walk gave the file, or, for a file the
//! walk did not list, the role of the roots that hold it.

use super::super::Project;
use super::super::walk::Role;
use super::helpers::{TempDir, resolved, session};
use crate::registry::stub_set::Stubs;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn a_listed_file_is_classified_under_its_resolved_path_with_its_role() {
    let dir = TempDir::new("classify_listed");
    dir.write("proj/a.php", "<?php");
    let root = resolved(&dir).join("proj");
    let session = session(&root, Vec::new());

    let classified = session.classify(&root.join("a.php"));
    assert_eq!(classified.path, root.join("a.php"));
    assert_eq!(classified.role, Role::Analysed);
    assert!(!classified.is_stub);
}

#[cfg(unix)]
#[test]
fn a_spelling_through_a_link_is_classified_as_the_file_it_names() {
    let dir = TempDir::new("classify_link");
    dir.write("proj/real/a.php", "<?php");
    let root = resolved(&dir).join("proj");
    std::os::unix::fs::symlink("real", root.join("link")).unwrap();
    let session = session(&root, Vec::new());

    let classified = session.classify(&root.join("link/a.php"));
    assert_eq!(classified.path, root.join("real/a.php"));
    assert_eq!(classified.role, Role::Analysed);
}

#[cfg(unix)]
#[test]
fn a_spelling_whose_link_was_removed_keeps_the_file_it_named() {
    let dir = TempDir::new("classify_removed_link");
    dir.write("proj/real/a.php", "<?php");
    let root = resolved(&dir).join("proj");
    std::os::unix::fs::symlink("real", root.join("link")).unwrap();
    let session = session(&root, Vec::new());
    let spelled = root.join("link/a.php");
    assert_eq!(session.classify(&spelled).path, root.join("real/a.php"));

    fs::remove_file(root.join("link")).unwrap();
    let classified = session.classify(&spelled);
    assert_eq!(classified.path, root.join("real/a.php"));
    assert_eq!(classified.role, Role::Analysed);
}

#[test]
fn a_new_file_takes_the_role_of_the_root_that_holds_it() {
    let dir = TempDir::new("classify_new_file");
    dir.write("proj/a.php", "<?php");
    dir.write("stubs/s.php", "<?php");
    let base = resolved(&dir);
    let root = base.join("proj");
    let session = session(&root, vec![base.join("stubs")]);
    dir.write("proj/new.php", "<?php");
    dir.write("stubs/new.php", "<?php");

    let analysed = session.classify(&root.join("new.php"));
    assert_eq!(analysed.path, root.join("new.php"));
    assert_eq!(analysed.role, Role::Analysed);
    assert!(!analysed.is_stub);

    let stub = session.classify(&base.join("stubs/new.php"));
    assert_eq!(stub.path, base.join("stubs/new.php"));
    assert_eq!(stub.role, Role::Stub);
    assert!(stub.is_stub);
}

#[test]
fn a_new_file_under_a_default_excluded_directory_is_context() {
    let dir = TempDir::new("classify_excluded_new");
    dir.write("proj/a.php", "<?php");
    let root = resolved(&dir).join("proj");
    let session = session(&root, Vec::new());
    dir.write("proj/vendor/new.php", "<?php");

    let classified = session.classify(&root.join("vendor/new.php"));
    assert_eq!(classified.path, root.join("vendor/new.php"));
    assert_eq!(classified.role, Role::Context);
}

#[test]
fn a_file_no_root_holds_takes_the_default_role() {
    let dir = TempDir::new("classify_outside");
    dir.write("proj/a.php", "<?php");
    dir.write("outside/x.php", "<?php");
    dir.write("outside/vendor/y.php", "<?php");
    let base = resolved(&dir);
    let session = session(&base.join("proj"), Vec::new());

    assert_eq!(
        session.classify(&base.join("outside/x.php")).role,
        Role::Analysed
    );
    assert_eq!(
        session.classify(&base.join("outside/vendor/y.php")).role,
        Role::Context
    );
}

/// Whether each class declaration `path` holds is a stub site, in the order
/// the project lists them.
fn class_site_stubs(project: &Project, path: &Path) -> Vec<bool> {
    project
        .duplicate_class_sites
        .iter()
        .flat_map(|(_, sites)| sites.iter())
        .filter(|site| site.path.as_path() == path)
        .map(|site| site.is_stub)
        .collect()
}

#[test]
fn an_edit_of_a_stub_file_keeps_its_class_declarations_as_stub_sites() {
    let mut project = Project::from_stubs(Stubs::shared());
    let path = PathBuf::from("/stub-edit/stub.php");

    project
        .update_file_as(&path, "<?php class StubOnly {}", false, true, || false)
        .unwrap();
    assert_eq!(class_site_stubs(&project, &path), vec![true]);

    project
        .update_file_as(&path, "<?php class StubOnly {}", false, false, || false)
        .unwrap();
    assert_eq!(class_site_stubs(&project, &path), vec![false]);
}
