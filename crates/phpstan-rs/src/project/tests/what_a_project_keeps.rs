//! What a project built for editing keeps of each file: an analysed file's
//! tree always, a context file's only while an editor has it open, and the
//! text of an open document only.

use super::super::session::{Overrides, Policy, Session};
use super::super::*;
use super::helpers::TempDir;

const CONTEXT: &str = "<?php function lib(): void {}\n";

/// The project of a directory with an analysed file and a context one, and
/// the paths it knows them by.
fn editing_project(dir: &TempDir) -> (Project, PathBuf, PathBuf) {
    dir.write("src/A.php", "<?php function a(): void { lib(); }\n");
    dir.write("lib/L.php", CONTEXT);
    let schema = crate::app::config_schema();
    let config = config::ConfigFile::parse("", &schema).unwrap();
    let session = Session::resolve(
        &config,
        None,
        dir.0.clone(),
        Overrides {
            paths: vec![PathBuf::from("src")],
            scan_paths: vec![PathBuf::from("lib")],
            no_composer: true,
            ..Overrides::default()
        },
        Policy::Fail,
        worker_pool(),
    )
    .unwrap()
    .for_editing();
    let project = session.load().unwrap().project;
    (
        project,
        PathBuf::from("src/A.php"),
        PathBuf::from("lib/L.php"),
    )
}

fn kept_tree(project: &Project, path: &Path) -> bool {
    project.files.get(path).unwrap().tree().is_some()
}

#[test]
fn a_dropped_project_tree_returns_arenas_for_the_next_parse() {
    let mut parsed = super::super::parse_file(
        Path::new("context.php"),
        b"<?php $value = 1;".as_slice(),
        false,
    )
    .unwrap();
    let mut scratch = parsed.drop_tree().expect("the parsed tree is present");
    let expr_capacity = scratch.expr_spans.capacity();
    let stmt_capacity = scratch.stmt_spans.capacity();
    assert!(expr_capacity > 0);
    assert!(stmt_capacity > 0);

    let reparsed = syntax::parse_watching_transient_with_scratch(
        b"<?php $next = 2;",
        None,
        None,
        &mut scratch,
    )
    .unwrap();

    assert!(reparsed.source.is_empty());
    assert!(reparsed.names.is_empty());
    assert!(reparsed.arenas.expr_spans.capacity() >= expr_capacity);
    assert!(reparsed.arenas.stmt_spans.capacity() >= stmt_capacity);
}

#[test]
fn a_context_file_keeps_no_tree_and_no_text_until_an_editor_opens_it() {
    let dir = TempDir::new("keeps_context_until_open");
    let (mut project, analysed, context) = editing_project(&dir);

    assert!(kept_tree(&project, &analysed));
    assert!(!kept_tree(&project, &context));
    assert_eq!(project.source(&context), None);
    assert!(
        project.ast_of(&context).is_some(),
        "a tree that is not kept is parsed again"
    );

    project.open_file(&context, CONTEXT);
    assert!(kept_tree(&project, &context));
    assert_eq!(project.source(&context), Some(CONTEXT));

    project.close_file(&context);
    assert!(!kept_tree(&project, &context));
    assert_eq!(project.source(&context), None);
}

#[test]
fn an_analysed_file_keeps_its_tree_but_only_an_open_one_keeps_its_text() {
    let dir = TempDir::new("keeps_analysed_tree");
    let (mut project, analysed, _) = editing_project(&dir);
    let text = "<?php function a(): void { lib(); }\n";

    assert_eq!(project.source(&analysed), None);
    project.open_file(&analysed, text);
    assert_eq!(project.source(&analysed), Some(text));
    project.close_file(&analysed);
    assert!(kept_tree(&project, &analysed));
    assert_eq!(project.source(&analysed), None);
}

#[test]
fn an_edit_keeps_the_tree_of_a_context_file_only_while_it_is_open() {
    let dir = TempDir::new("keeps_edited_context");
    let (mut project, _, context) = editing_project(&dir);
    let edited = "<?php function lib(): int { return 1; }\n";

    project.update_file(&context, edited).unwrap();
    assert!(!kept_tree(&project, &context));

    project.open_file(&context, edited);
    project.update_file(&context, CONTEXT).unwrap();
    assert!(kept_tree(&project, &context));
    assert_eq!(project.source(&context), Some(CONTEXT));
}

#[test]
fn a_tree_is_not_parsed_again_from_a_file_that_changed_since_it_was_read() {
    let dir = TempDir::new("keeps_changed_on_disk");
    let (project, _, context) = editing_project(&dir);

    dir.write("lib/L.php", "<?php function other(): void {}\n");
    assert!(project.ast_of(&context).is_none());
}
