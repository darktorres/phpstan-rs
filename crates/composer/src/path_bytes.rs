//! What `Path` reads from a path's components, read from its bytes where the
//! path's shape gives the same answer, and asked of `Path` otherwise, so
//! every answer is `Path`'s own. The paths a load carries are spelled as their
//! components are: no empty component, no `.` component and no trailing
//! separator. Read so, the byte that ends a component is a separator, and a
//! component's bytes are its name.

use std::ffi::OsStr;
use std::path::Path;

/// `path.file_name()`.
pub fn file_name(path: &Path) -> Option<&OsStr> {
    #[cfg(unix)]
    if let Some(name) = last_name(path.as_os_str().as_encoded_bytes()) {
        return Some(os_str_of(name));
    }
    path.file_name()
}

/// `path.parent()`.
pub fn parent(path: &Path) -> Option<&Path> {
    #[cfg(unix)]
    {
        let bytes = path.as_os_str().as_encoded_bytes();
        if let Some(at) = bytes.iter().rposition(|&byte| byte == b'/')
            && !matches!(&bytes[at + 1..], b"" | b"." | b"..")
        {
            // The root, or a directory that ends in a name.
            let dir = &bytes[..at.max(1)];
            if at == 0 || !(dir.ends_with(b"/") || dir.ends_with(b"/.") || dir == b".") {
                return Some(Path::new(os_str_of(dir)));
            }
        }
    }
    path.parent()
}

/// Whether `name`, one name of a directory's listing, has the extension
/// `php`, as `Path::extension` reads it.
pub fn is_php_name(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    // The dot of the extension is not the name's first byte: `.php` has none.
    bytes.len() > 4 && bytes.ends_with(b".php")
}

/// `path.starts_with(base)`.
pub fn starts_with(path: &Path, base: &Path) -> bool {
    #[cfg(unix)]
    if let Some(rest) = plain_rest(path, base) {
        return rest.is_some();
    }
    path.starts_with(base)
}

/// `path.strip_prefix(base).ok()`.
pub fn strip_prefix<'p>(path: &'p Path, base: &Path) -> Option<&'p Path> {
    #[cfg(unix)]
    if let Some(rest) = plain_rest(path, base) {
        return rest.map(|rest| Path::new(os_str_of(rest)));
    }
    path.strip_prefix(base).ok()
}

/// Whether `path` is absolute and spelled as its components are, with no
/// `.` or `..` among them, so that `manifest::lexically_normalized` gives it
/// back as it is. `false` says only that it is not known to be.
pub fn is_normal_absolute(path: &Path) -> bool {
    #[cfg(unix)]
    {
        let bytes = path.as_os_str().as_encoded_bytes();
        bytes.first() == Some(&b'/') && plain(bytes) == Some(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// The last component of a path's bytes, when it is a name after the last
/// separator, or the whole of them when they hold no separator.
#[cfg(unix)]
fn last_name(bytes: &[u8]) -> Option<&[u8]> {
    let name = match bytes.iter().rposition(|&byte| byte == b'/') {
        Some(at) => &bytes[at + 1..],
        None => bytes,
    };
    (!matches!(name, b"" | b"." | b"..")).then_some(name)
}

/// What is left of `path` after `base`, read from the bytes when both are
/// spelled as their components are: `Some(None)` when `base` is no prefix of
/// it, and `None` when the bytes do not tell.
#[cfg(unix)]
fn plain_rest<'p>(path: &'p Path, base: &Path) -> Option<Option<&'p [u8]>> {
    let (bytes, prefix) = (
        path.as_os_str().as_encoded_bytes(),
        base.as_os_str().as_encoded_bytes(),
    );
    plain(bytes)?;
    plain(prefix)?;
    let Some(rest) = bytes.strip_prefix(prefix) else {
        return Some(None);
    };
    Some(if rest.is_empty() || prefix == b"/" {
        Some(rest)
    } else {
        rest.strip_prefix(b"/")
    })
}

/// Whether `bytes` spell a path as its components do: no empty component, no
/// `.` component, no trailing separator, except for the root itself. `Some`
/// says whether one of the components is `..`.
#[cfg(unix)]
fn plain(bytes: &[u8]) -> Option<bool> {
    if bytes == b"/" {
        return Some(false);
    }
    let body = bytes.strip_prefix(b"/").unwrap_or(bytes);
    let mut parent_dir = false;
    for component in body.split(|&byte| byte == b'/') {
        match component {
            b"" | b"." => return None,
            b".." => parent_dir = true,
            _ => {}
        }
    }
    Some(parent_dir)
}

/// The name `bytes` spell.
#[cfg(unix)]
fn os_str_of(bytes: &[u8]) -> &OsStr {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(bytes)
}
