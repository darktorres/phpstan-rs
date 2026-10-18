//! The spellings whose mention in a file lets `check_program` run a walk of
//! its file pass: a walk that can only find something in a file that names
//! one of its spellings is left out of the others. The bits are those of
//! `syntax::FileFacts::mentions`, which the lexer sets for a `syntax::WatchList`
//! holding `WATCH`.

/// The bit of a file that names `bind`, `bindTo` or `call`: a closure rebind
/// (`unused::scan_program_for_rebind_targets`) needs one of them.
pub(crate) const REBIND: u32 = 1 << 1;

/// The bit of a file that names a function that tests a path, or sets the
/// include path: only such a file has an include guard to find
/// (`includes::GUARD_FUNCTIONS`).
pub(crate) const GUARD: u32 = 1 << 2;

/// The spellings that set each bit.
pub(crate) const WATCH: &[(&str, u32)] = &[
    ("bind", REBIND),
    ("bindto", REBIND),
    ("call", REBIND),
    ("file_exists", GUARD),
    ("is_file", GUARD),
    ("is_readable", GUARD),
    ("is_writable", GUARD),
    ("is_writeable", GUARD),
    ("is_executable", GUARD),
    ("set_include_path", GUARD),
    ("ini_set", GUARD),
];
