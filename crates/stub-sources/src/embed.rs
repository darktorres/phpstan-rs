//! What the build script decides about the data it embeds, so that a build
//! on any platform embeds the same data. The build script includes this file
//! by path, and the crate's tests include it to run the same functions.

use std::path::Path;

/// `relative` spelled with `/` between its names, which is how the checked-in
/// index spells a path, whatever separator the platform writes.
pub(crate) fn slash_spelling(relative: &Path) -> String {
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `bytes` with each carriage return and line feed as a line feed, or `None`
/// when it holds none. A checkout that converts line breaks (Git's
/// `core.autocrlf` on Windows) gives a source the break it does not have in
/// the repository, and the data embedded is the repository's.
pub(crate) fn with_lf_breaks(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.windows(2).any(|pair| pair == b"\r\n") {
        return None;
    }
    let mut lf = Vec::with_capacity(bytes.len());
    for (at, &byte) in bytes.iter().enumerate() {
        if byte == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
            continue;
        }
        lf.push(byte);
    }
    Some(lf)
}
