//! How reports and messages spell a file's path, and how configured path
//! text names one.
//!
//! A report spells a path with `/` between its names on every platform, so
//! the files that tools read (JSON, checkstyle, JUnit, a baseline) and the
//! messages a baseline matches name a file alike wherever they were made.
//! Windows separates names with `\` too, and the spelling turns it into
//! `/`. Elsewhere `\` is a character a name can hold, so a path is spelled
//! as it is.
//!
//! Configured path text, such as an `ignoreErrors` path or a baseline
//! entry's file, is compared with that spelling. It travels between
//! platforms with the repository, so a `\` in it is a separator on every
//! platform and text written on Windows names the same files elsewhere.
//! Outside Windows a `\` can also be a character of a name, and it names
//! that as well.
//!
//! Letters follow the case rule of the directory the paths lie in
//! ([`CaseRule`]): configured text names a file by any case of its letters
//! where the directory's file system folds case, and by its own spelling
//! elsewhere.

use std::borrow::Cow;
use std::path::Path;

/// How the letters of configured path text name the letters of a path.
///
/// A directory whose file system folds case (NTFS, the default APFS, and a
/// casefold directory of a Unix file system) holds one file under every
/// spelling of its letters, so configured text that spells a name in another
/// case still names it. In any other directory the spelling is exact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaseRule {
    /// A letter names only itself.
    #[default]
    Exact,
    /// A letter names each case of itself.
    Folded,
}

impl CaseRule {
    /// The rule of a directory whose file system folds case when `folds`.
    pub fn of_folding(folds: bool) -> CaseRule {
        if folds {
            CaseRule::Folded
        } else {
            CaseRule::Exact
        }
    }

    /// `text` as this rule compares it: as it is under `Exact`, and with each
    /// letter in its simple lower case under `Folded`. Two texts name alike
    /// under the rule when their keys are equal. A letter whose case mapping
    /// is more than one letter (`ß`) is not folded, as a file system that
    /// folds case by simple mappings does not fold it.
    pub fn key(self, text: &str) -> Cow<'_, str> {
        match self {
            CaseRule::Exact => Cow::Borrowed(text),
            CaseRule::Folded if text.chars().all(|c| fold_letter(c) == c) => Cow::Borrowed(text),
            CaseRule::Folded => Cow::Owned(text.chars().map(fold_letter).collect()),
        }
    }
}

/// `c`'s simple lower case, taken through its simple upper case so that
/// letters with one upper case (`ς` and `σ`) have one key.
fn fold_letter(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let upper = simple(c.to_uppercase(), c);
    simple(upper.to_lowercase(), upper)
}

/// The one letter `mapped` yields, else `otherwise`.
fn simple(mut mapped: impl Iterator<Item = char>, otherwise: char) -> char {
    match (mapped.next(), mapped.next()) {
        (Some(only), None) => only,
        _ => otherwise,
    }
}

/// `path`'s text with `/` between its names: on Windows each `\` becomes
/// `/`, and elsewhere it is the path's own text. Bytes that are not UTF-8
/// are replaced, as `Path::display` replaces them.
pub fn report_path(path: &Path) -> Cow<'_, str> {
    let text = path.to_string_lossy();
    if cfg!(windows) && text.contains('\\') {
        Cow::Owned(text.replace('\\', "/"))
    } else {
        text
    }
}

/// Whether the byte `configured` of configured path text names the byte
/// `reported` of a report path (`report_path`) at the same place. A `\`
/// names a `/`, and also a `\`, which a report path holds only outside
/// Windows, as a character of a name. Any other byte names itself.
pub fn path_byte_names(configured: u8, reported: u8) -> bool {
    configured == reported || (configured == b'\\' && reported == b'/')
}

/// Whether configured path text names the report path `reported`, each
/// byte as `path_byte_names` reads it.
pub fn path_text_names(configured: &str, reported: &str) -> bool {
    configured.len() == reported.len()
        && configured
            .bytes()
            .zip(reported.bytes())
            .all(|(c, r)| path_byte_names(c, r))
}

/// `text` with each `\` spelled `/`. Configured text and every report path
/// it names (`path_text_names`) are equal in this spelling, so it can key a
/// table whose matches `path_text_names` then confirms.
pub fn slash_spelling(text: &str) -> Cow<'_, str> {
    if text.contains('\\') {
        Cow::Owned(text.replace('\\', "/"))
    } else {
        Cow::Borrowed(text)
    }
}
