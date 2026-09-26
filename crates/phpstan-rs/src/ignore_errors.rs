//! A hand-mirrored subset of real PHPStan's own `ignoreErrors:` config
//! directive - this project has no NEON parser and one isn't otherwise
//! justified (see `baseline.rs`'s own identical judgment for a related
//! mechanism: a real PHPStan config supports arbitrary nesting,
//! `%parameters%` interpolation, `includes:`, and a full `services:` DI
//! section this project would have to parse and then discard almost all
//! of), so a project's real `phpstan.neon` `ignoreErrors:` block is
//! manually re-typed into `Config::ignore_errors` instead, the same way
//! `paths`/`scan_paths` already are.
//!
//! A common real shape this exists for: a project ignoring diagnostics
//! like `missingType.iterableValue`/`variable.undefined`/`nullCoalesce.*`
//! under a generated-code directory (compiled templates, not hand-written
//! code) - real PHPStan reports nothing there because of this config,
//! not because nothing would otherwise be found.

use rustc_hash::FxHashMap;
use std::path::PathBuf;

use diagnostics::Diagnostic;
use serde::Deserialize;

/// One `ignoreErrors:` entry. Every field is optional and AND-combined
/// when more than one is set (mirrors real PHPStan's own
/// `IgnoredError::shouldIgnore`, which checks `identifier`, then
/// `message`, then `path`, failing the whole entry the moment any
/// *specified* criterion doesn't match) - an entry with only `identifier`
/// set matches on identifier alone; adding `path` narrows it further.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct IgnoreErrorEntry {
    /// Exact match against `Diagnostic::identifier` - real PHPStan does
    /// exact string equality here too, never a glob.
    pub identifier: Option<String>,
    /// A glob against the diagnostic's own already-relative file path (the
    /// same string this project's `check-project` output already reports
    /// paths as, e.g. `_analysis/phpstan-latte/foo.php`). See
    /// `glob_matches`'s own doc comment for the exact wildcard semantics.
    pub path: Option<String>,
    /// A regex (real PHPStan's own PCRE delimiters, e.g. `#^...$#`,
    /// already stripped - store the bare pattern) against
    /// `Diagnostic::message`.
    pub message: Option<String>,
    /// An expected exact match count, mirroring real PHPStan's own
    /// `count:` key. **Parsed but not enforced yet** - most real
    /// `ignoreErrors` entries in a real `phpstan.neon` target check
    /// families this project doesn't implement at all (`dba.*`,
    /// `shipmonk.*`, `cast.useless` in Piwigo's own config), so every one
    /// of those would report as "unmatched" on every single run under
    /// real PHPStan's own `reportUnmatchedIgnoredErrors` strictness - not
    /// because this project's `ignoreErrors` support is broken, but
    /// because it doesn't implement the underlying rule at all, a
    /// permanent and unrelated gap. Revisit once identifier coverage is
    /// tracked well enough to tell "unmatched because stale" apart from
    /// "unmatched because never implemented."
    pub count: Option<u32>,
}

impl IgnoreErrorEntry {
    fn matches(&self, path: &str, d: &Diagnostic) -> bool {
        if let Some(identifier) = &self.identifier
            && *identifier != *d.identifier
        {
            return false;
        }
        if let Some(pattern) = &self.path
            && !glob_matches(pattern, path)
        {
            return false;
        }
        if let Some(message) = &self.message {
            match regex::Regex::new(message) {
                Ok(re) => {
                    if !re.is_match(&d.message) {
                        return false;
                    }
                }
                // A malformed pattern can never match - the same "don't
                // guess, don't crash" leniency this project applies to
                // every other user-supplied, unvalidated-at-parse-time
                // config string.
                Err(_) => return false,
            }
        }
        true
    }
}

/// Matches `path` against `pattern`, where `pattern` may contain any
/// number of `*` wildcards (real corpus usage never needs more than one,
/// but nothing here assumes exactly one). Mirrors real PHPStan's own
/// matcher (`fnmatch()`, called *without* `FNM_PATHNAME`) - `*` matches
/// `/` too, which is exactly what a real entry like `*/Event/*` (matching
/// an `Event` directory at any depth) needs. No `?`/`[...]` character-class
/// support - not used anywhere in a real `phpstan.neon`, and adding it
/// would just be unverified surface area.
pub(crate) fn glob_matches(pattern: &str, path: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return path == pattern;
    }
    let last = parts.len() - 1;
    let mut cursor = path;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !cursor.starts_with(part) {
                return false;
            }
            cursor = &cursor[part.len()..];
        } else if i == last {
            return cursor.ends_with(part);
        } else if !part.is_empty() {
            match cursor.find(part) {
                Some(idx) => cursor = &cursor[idx + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Drops every diagnostic matched by any entry in `entries`, in place -
/// mirrors `baseline::filter`'s own shape exactly. Entries are tried in
/// file order (matching real PHPStan's own "first matching ignore wins"
/// behavior, relevant when a narrower message-scoped entry is placed
/// ahead of a broader identifier-wide sibling to carve out an exception -
/// though since every entry here only ever *suppresses*, trying them in
/// any order produces the identical final result; order is preserved
/// anyway for fidelity with the real semantics a future `count`
/// enforcement pass would need). `Diagnostic::non_ignorable` is exempt,
/// the identical rule `Project::check_file`'s own inline
/// `@phpstan-ignore-line` suppression already applies - a config-level
/// entry must never suppress a diagnostic real PHPStan itself would
/// refuse to let any ignore mechanism touch.
pub fn filter(results: &mut FxHashMap<PathBuf, Vec<Diagnostic>>, entries: &[IgnoreErrorEntry]) {
    if entries.is_empty() {
        return;
    }
    for (file, diagnostics) in results.iter_mut() {
        let path = file.display().to_string();
        diagnostics
            .retain(|d| d.non_ignorable || !entries.iter().any(|entry| entry.matches(&path, d)));
    }
}

#[cfg(test)]
mod tests;
