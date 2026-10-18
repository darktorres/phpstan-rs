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
    for function in include_values::GUARD_FUNCTIONS {
        assert!(
            mentions::WATCH.contains(&(function, mentions::GUARD)),
            "{function}"
        );
    }
}

#[test]
fn every_function_that_sets_the_include_path_is_watched() {
    for function in ["set_include_path", "ini_set", "ini_alter"] {
        assert!(
            mentions::WATCH.contains(&(function, mentions::INCLUDE_PATH)),
            "{function}"
        );
    }
}

#[test]
fn chdir_is_watched() {
    assert!(mentions::WATCH.contains(&("chdir", mentions::CHDIR)));
    assert_eq!(
        mentions::LOOKUP,
        mentions::INCLUDE_PATH | mentions::CHDIR,
        "the walk that reads both is gated by both"
    );
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
