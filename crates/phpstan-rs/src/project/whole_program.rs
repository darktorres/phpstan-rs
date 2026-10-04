//! Checks that need a fact aggregated across the *whole* project rather
//! than one file/function at a time - unlike every other check in this
//! crate, these can't run through the ordinary per-file `Checker`/
//! `check_program` path (`Project::check_file`/`check_all`): the fact
//! they need ("is this declaration referenced *anywhere*") isn't knowable
//! until every file has been visited at least once. `Project::check_
//! whole_program` is the single entry point that runs them, called once
//! per `check-project` invocation - see that method's own doc comment.

pub(crate) mod circular_namespace_deps;
pub(crate) mod dead_abstract_classes;
pub(crate) mod dead_code;
pub(crate) mod dead_enum_cases;
pub(crate) mod dead_members;
pub(crate) mod renamed_named_arguments;

use std::path::Path;
use syntax::span::Span;

/// A declaration an analysed file makes: its name as declared, and its
/// name's place in the file.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Declaration<'a> {
    pub(crate) name: &'a str,
    pub(crate) path: &'a Path,
    pub(crate) span: Span,
}
