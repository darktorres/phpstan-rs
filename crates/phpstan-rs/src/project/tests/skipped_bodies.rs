//! A context file parsed with its function bodies skipped: what it gives
//! at the load, and what a project asked what depends on a name registers.

use super::super::*;

const CONTEXT: &str = "<?php
namespace Lib;
function viaHelper(int $n): int { return \\helper($n) + CONSTANT_IN_BODY; }
class Holder { public function run(): int { return \\helper(2); } }
";
const APP: &str = "<?php function helper(int $x): int { return $x; }";

fn parsed(skip: BodyText) -> ParsedFile {
    parse_file_with_names(Path::new("ctx.php"), CONTEXT, false, skip, None).expect("parses")
}

#[test]
fn a_file_whose_bodies_are_skipped_declares_what_the_whole_parse_declares() {
    let (skipped, whole) = (parsed(BodyText::Skipped), parsed(BodyText::Parsed));
    assert!(skipped.facts.has_skimmed_bodies && !whole.facts.has_skimmed_bodies);
    assert_eq!(skipped.functions, whole.functions);
    assert_eq!(skipped.classes, whole.classes);
    assert_eq!(skipped.constants, whole.constants);
    assert_eq!(
        format!("{:?}", skipped.global_facts),
        format!("{:?}", whole.global_facts)
    );
    // Only what the bodies refer to is not in the file's references.
    let helper = deps::RefKey::function_text("helper");
    assert!(!skipped.references.contains(&helper));
    assert!(whole.references.contains(&helper));
    assert!(skipped.bodies.text_references().contains(&helper));
    assert_eq!(skipped.declaration_references, whole.declaration_references);
}

#[test]
fn a_project_asked_what_depends_on_a_name_has_what_the_skipped_bodies_refer_to() {
    let helper = deps::RefKey::function_text("helper");
    let depends = |project: &Project| {
        let id = project.files.id_of(Path::new("ctx.php")).expect("loaded");
        project
            .dependents
            .get(&helper)
            .is_some_and(|files| files.contains(&id))
    };
    let names = |project: &Project| {
        let id = project.files.id_of(Path::new("ctx.php")).expect("loaded");
        let mut names: Vec<String> = project.files.get_by_id(id).map_or_else(Vec::new, |file| {
            file.references
                .iter()
                .map(|key| format!("{key:?}"))
                .collect()
        });
        names.sort();
        names
    };
    let load = |skip: BodyText| {
        let mut project = Project::new();
        let source = CONTEXT.to_string();
        project.insert_parsed_file(PathBuf::from("ctx.php"), parsed(skip), source, false);
        project
            .load_file(PathBuf::from("app.php"), APP)
            .expect("parses");
        project
    };
    let (mut skipped, mut whole) = (load(BodyText::Skipped), load(BodyText::Parsed));
    assert!(depends(&whole) && !depends(&skipped));
    // The finishing passes after an edit look names up, so they complete it.
    skipped.complete_skipped_references();
    assert!(depends(&skipped));
    assert_eq!(names(&skipped), names(&whole));
    skipped.finish_loading();
    whole.finish_loading();
    assert!(depends(&skipped) && depends(&whole));
    assert!(skipped.registry().functions.contains_key("Lib\\viaHelper"));
}

#[test]
fn a_constant_defined_in_a_body_is_declared_whether_or_not_the_parse_skips_bodies() {
    let source = "<?php function boot() { define('FROM_BODY', 1); } function quiet() { return 1; }";
    let load = |skip: BodyText| {
        parse_file_with_names(Path::new("boot.php"), source, false, skip, None).expect("parses")
    };
    let (skipped, whole) = (load(BodyText::Skipped), load(BodyText::Parsed));
    assert_eq!(skipped.constants, whole.constants);
    assert_eq!(skipped.constants.len(), 1);
    // `quiet` is skipped; `boot` is not, for the `define`.
    assert!(skipped.facts.has_skimmed_bodies);
}
