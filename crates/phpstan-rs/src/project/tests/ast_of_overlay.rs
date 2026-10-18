//! `ast_of` parses a file that keeps no tree from the bytes the project holds
//! for it now: its open document's, and not those of an earlier parse.

use super::super::*;
use crate::project::session::load_test_file;

const EDITOR: &str = "<?php function in_the_editor() {}";
const EDITED_AGAIN: &str = "<?php function in_the_editor() {} function edited_again() {}";

/// A project built once, with `open.php` loaded as a context file, which
/// drops its tree.
fn project_with_a_context_file(path: &Path) -> Project {
    let mut project = Project::new();
    project.built_once = true;
    load_test_file(
        &mut project,
        path.to_path_buf(),
        "<?php function loaded_from_the_disk() {}",
        false,
    );
    let id = project.files.id_of(path).expect("loaded");
    let file = project.files.get_by_id(id).expect("loaded");
    assert!(file.tree().is_none(), "the context file dropped its tree");
    project
}

#[test]
fn a_file_with_no_kept_tree_is_parsed_from_its_open_document() {
    let path = PathBuf::from("open.php");
    let mut project = project_with_a_context_file(&path);
    project.set_document(&path, EDITOR.as_bytes());
    let id = project.number_of(&path);
    assert_eq!(project.ast_of(id).map(|ast| ast.len()), Some(1));
}

#[test]
fn a_file_edited_after_a_parse_is_parsed_again_from_its_new_text() {
    let path = PathBuf::from("open.php");
    let mut project = project_with_a_context_file(&path);
    let id = project.number_of(&path);
    project.set_document(&path, EDITOR.as_bytes());
    assert_eq!(project.ast_of(id).map(|ast| ast.len()), Some(1));
    project.set_document(&path, EDITED_AGAIN.as_bytes());
    assert_eq!(project.ast_of(id).map(|ast| ast.len()), Some(2));
}
