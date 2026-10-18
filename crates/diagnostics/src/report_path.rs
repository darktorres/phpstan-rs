//! How reports and messages spell a file's path: with `/` between its names
//! on every platform, as PHPStan's checkstyle, JUnit and baseline files and
//! its anonymous class names spell one. Windows separates names with `\`
//! too, and these spellings turn it into `/`. Elsewhere `\` is a character
//! a name can hold, so a path is spelled as it is.

use std::borrow::Cow;
use std::path::Path;

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

/// Text that names a path, or a pattern of paths, in `report_path`'s
/// spelling, so that it compares with what `report_path` gives: on Windows a
/// `\` is a separator, as PHPStan's `fnmatch` reads one there
/// (`FNM_NOESCAPE`), and becomes `/`. Elsewhere it is the text as it is.
pub fn report_spelling(text: &str) -> Cow<'_, str> {
    if cfg!(windows) && text.contains('\\') {
        Cow::Owned(text.replace('\\', "/"))
    } else {
        Cow::Borrowed(text)
    }
}
