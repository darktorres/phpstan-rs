//! How a path the configuration names reads.
//!
//! A configuration travels between platforms with the repository, so a `\` in
//! a path it names is a separator wherever it is read, as it is on Windows.
//! Elsewhere a `\` can also be a character of a name. Text that holds one has
//! two readings: the separator reading, which takes each `\` as `/`, and the
//! name reading, which takes the text as it is. A `/` is a character of no
//! platform's names, so text without a `\` has one reading.
//!
//! Which reading a path takes is a question for the disk, and the callers ask
//! it through `exists`. Text with one reading is not asked about.

use std::path::{Path, PathBuf};

/// The readings of `text`, the separator reading first.
pub(crate) fn readings(text: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        return vec![text.to_path_buf()];
    }
    match text.to_str() {
        Some(spelled) if spelled.contains('\\') => vec![
            PathBuf::from(diagnostics::slash_spelling(spelled).into_owned()),
            text.to_path_buf(),
        ],
        _ => vec![text.to_path_buf()],
    }
}

/// The roots one entry of a list of paths names: each reading that exists,
/// the separator reading first. When none exists, the separator reading, in
/// which a root that is missing is named.
pub(crate) fn roots(text: &Path, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let readings = readings(text);
    if readings.len() == 1 {
        return readings;
    }
    let found: Vec<PathBuf> = readings
        .iter()
        .filter(|reading| exists(reading))
        .cloned()
        .collect();
    if found.is_empty() {
        readings.into_iter().take(1).collect()
    } else {
        found
    }
}

/// The one file `text` names, for the setting `what`: the reading that
/// exists. When none does, the separator reading, in which a missing file is
/// named. When both exist the text is ambiguous, and it is an error: a name
/// reading cannot be told from a separator one by its spelling, so the user
/// spells the separator with `/`.
pub(crate) fn single(
    what: &str,
    text: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    let readings = readings(text);
    if readings.len() == 1 {
        return Ok(readings[0].clone());
    }
    let mut found = readings.iter().filter(|reading| exists(reading));
    match (found.next(), found.next()) {
        (Some(first), Some(second)) => Err(format!(
            "{what} {} names two files, {} and {}: a `\\` is a separator and also a character of \
             a name, so spell the separator `/`",
            text.display(),
            first.display(),
            second.display()
        )),
        (Some(only), None) => Ok(only.clone()),
        (None, _) => Ok(readings[0].clone()),
    }
}

#[cfg(test)]
mod tests;
