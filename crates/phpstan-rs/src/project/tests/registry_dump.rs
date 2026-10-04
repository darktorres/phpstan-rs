//! The registry dump of a project: its registry over the stub base it
//! stands on, which it names, and the bases whole in a dump of their own.

use std::path::PathBuf;

use crate::project::Project;

#[test]
fn a_project_s_registry_prints_over_its_base_with_its_own_declarations_only() {
    let mut project = Project::new();
    project
        .load_analyzed_file(
            PathBuf::from("a.php"),
            "<?php\nclass Own extends \\ArrayIterator {}\nfunction own_function(): void {}\n",
        )
        .unwrap();
    project.finish_loading();
    let dump = project.registry_dump();
    assert!(dump.contains("\n== registry over stub-base\n"), "{dump}");
    assert!(dump.contains("    Own => "), "the project's class: {dump}");
    assert!(
        dump.contains("    own_function => "),
        "the project's function: {dump}"
    );
    assert!(
        !dump.contains("    ArrayIterator => ClassReflection"),
        "no base class prints with its reflection"
    );
    assert!(!dump.contains("== stub base"), "the bases are not in it");
}

#[test]
fn the_stub_bases_dump_holds_each_base_whole() {
    let dump = Project::stub_bases_dump();
    for base in ["stub-base", "stub-base-phpstan"] {
        assert!(dump.contains(&format!("\n== stub base {base}\n")), "{base}");
    }
    assert!(dump.contains("    ArrayIterator => "));
}
