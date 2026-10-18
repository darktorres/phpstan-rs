//! The spellings that gate the walks of the checker's file pass cover what
//! the walks read: a file the lexer leaves without a walk's bit has nothing
//! for that walk to find.

use super::super::*;

static WATCHED: syntax::WatchList = syntax::WatchList(&[mentions::WATCH]);

fn parsed(source: &str) -> syntax::ParsedFile {
    syntax::parse_watching(source.as_bytes(), None, Some(&WATCHED)).expect("parses")
}

#[test]
fn every_function_the_include_guard_walk_reads_is_watched() {
    let functions = includes::GUARD_FUNCTIONS
        .iter()
        .chain(&["set_include_path", "ini_set"]);
    for function in functions {
        assert!(
            mentions::WATCH.contains(&(*function, mentions::GUARD)),
            "{function}"
        );
    }
}

#[test]
fn a_file_with_a_rebind_the_scan_finds_has_the_rebind_bit() {
    for source in [
        "<?php Closure::bind(function () {}, null, A::class);",
        "<?php (function () {})->bindTo(null, A::class);",
        "<?php (function () {})->call(new A());",
    ] {
        let file = parsed(source);
        let (targets, escape_all) = unused::scan_program_for_rebind_targets(&file.stmts);
        assert!(!targets.is_empty() || escape_all, "a rebind in {source}");
        assert_ne!(file.facts.mentions & mentions::REBIND, 0, "{source}");
    }
}
