//! What `Path` reads from a path's components, read from its bytes where the
//! path's shape gives the same answer, and asked of `Path` otherwise, so
//! every answer is `Path`'s own. The paths a load carries are spelled as their
//! components are: no empty component, no `.` component and no trailing
//! separator. Read so, the byte that ends a component is a separator, and a
//! component's bytes are its name.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

use std::ffi::OsStr;
use std::path::{Component, Path};

#[cfg(test)]
mod tests;

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
    if let Some((dir, _)) = split_bytes(path.as_os_str().as_encoded_bytes()) {
        return Some(Path::new(os_str_of(dir)));
    }
    path.parent()
}

/// `path.parent()` and `path.file_name()`, when the path has both, from one
/// reading of its last component.
pub fn split(path: &Path) -> Option<(&Path, &OsStr)> {
    #[cfg(unix)]
    if let Some((dir, name)) = split_bytes(path.as_os_str().as_encoded_bytes()) {
        return Some((Path::new(os_str_of(dir)), os_str_of(name)));
    }
    path.parent().zip(path.file_name())
}

/// Whether `path.file_name()` is `name`, one name: no separator, and not `.`
/// or `..`.
pub fn has_file_name(path: &Path, name: &str) -> bool {
    #[cfg(unix)]
    {
        let bytes = path.as_os_str().as_encoded_bytes();
        // A last byte that is neither a separator nor a dot ends a name after
        // the last separator, and that name is the last component.
        if !matches!(bytes.last(), None | Some(b'/' | b'.')) {
            return bytes
                .strip_suffix(name.as_bytes())
                .is_some_and(|dir| dir.is_empty() || dir.ends_with(b"/"));
        }
    }
    path.file_name() == Some(OsStr::new(name))
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
    if let Some(end) = prefix_end(
        path.as_os_str().as_encoded_bytes(),
        base.as_os_str().as_encoded_bytes(),
    ) {
        return end.is_some();
    }
    path.starts_with(base)
}

/// `path.strip_prefix(base).ok()`.
pub fn strip_prefix<'p>(path: &'p Path, base: &Path) -> Option<&'p Path> {
    #[cfg(unix)]
    {
        let bytes = path.as_os_str().as_encoded_bytes();
        match prefix_end(bytes, base.as_os_str().as_encoded_bytes()) {
            Some(None) => return None,
            Some(Some(end)) => {
                // What is left is its bytes when they are a relative path
                // spelled as its components are.
                let rest = &bytes[end..];
                if rest.is_empty() || (rest[0] != b'/' && plain(rest).is_some()) {
                    return Some(Path::new(os_str_of(rest)));
                }
            }
            None => {}
        }
    }
    path.strip_prefix(base).ok()
}

/// Whether `path` is absolute and spelled as its components are, with no
/// `.` or `..` among them, so that normalizing it lexically, dropping each
/// `.` and resolving each `..`, gives it back as it is. `false` says only
/// that it is not known to be.
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

/// The components of `path`, from the top, as `Path::components` gives them,
/// each with the ancestor of `path` that it ends, as `Path::ancestors` gives
/// them; `None` when the bytes do not tell. A relative path's top ancestor,
/// the empty path, ends no component.
pub fn components(path: &Path) -> Option<impl Iterator<Item = (Component<'_>, &Path)>> {
    #[cfg(unix)]
    {
        let bytes = path.as_os_str().as_encoded_bytes();
        plain(bytes)?;
        let root = bytes.first() == Some(&b'/');
        let body = &bytes[usize::from(root)..];
        let mut start = usize::from(root);
        let names = body
            .split(|&byte| byte == b'/')
            // The root alone has no body, which splits into one empty name.
            .filter(|_| !body.is_empty())
            .map(move |name| {
                let end = start + name.len();
                start = end + 1;
                let component = if name == b".." {
                    Component::ParentDir
                } else {
                    Component::Normal(os_str_of(name))
                };
                (component, Path::new(os_str_of(&bytes[..end])))
            });
        let root = root.then(|| (Component::RootDir, Path::new(os_str_of(&bytes[..1]))));
        Some(root.into_iter().chain(names))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None::<std::iter::Empty<(Component<'_>, &Path)>>
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

/// The directory and the name of a path's bytes, as `Path::parent` and
/// `Path::file_name` give them, when the last component is a name after the
/// last separator, or the whole of them, and the bytes before that separator
/// are the root, nothing, or a directory that ends in a name.
#[cfg(unix)]
fn split_bytes(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = bytes.iter().rposition(|&byte| byte == b'/');
    let name = at.map_or(bytes, |at| &bytes[at + 1..]);
    if matches!(name, b"" | b"." | b"..") {
        return None;
    }
    let dir = match at {
        None => &bytes[..0],
        Some(0) => &bytes[..1],
        Some(at) => {
            let dir = &bytes[..at];
            // `Path` reads a directory without its trailing separators and
            // `.` components.
            if dir.ends_with(b"/") || dir.ends_with(b"/.") || dir == b"." {
                return None;
            }
            dir
        }
    };
    Some((dir, name))
}

/// Where what is left of `path` after `base` starts in `path`'s bytes, past
/// the separator after `base`, read from the bytes when `base` is spelled as
/// its components are: `Some(None)` when `base` is no prefix of `path`, and
/// `None` when the bytes do not tell.
#[cfg(unix)]
fn prefix_end(bytes: &[u8], prefix: &[u8]) -> Option<Option<usize>> {
    plain(prefix)?;
    if bytes.starts_with(prefix) {
        // `base`'s components are the first of `path`'s when a component
        // starts where `base` ends.
        return Some(match bytes.get(prefix.len()) {
            None => Some(prefix.len()),
            Some(_) if prefix == b"/" => Some(1),
            Some(b'/') => Some(prefix.len() + 1),
            Some(_) => None,
        });
    }
    // The bytes before the component of `path` that holds the first byte the
    // two differ at are whole components of `base`. That component differs
    // from `base`'s, unless it is one that `Path` passes over: an empty one or
    // a `.`.
    let shared = bytes
        .iter()
        .zip(prefix)
        .take_while(|(byte, other)| byte == other)
        .count();
    let start = bytes[..shared]
        .iter()
        .rposition(|&byte| byte == b'/')
        .map_or(0, |at| at + 1);
    let rest = &bytes[start..];
    if rest.starts_with(b"/") || rest == b"." || rest.starts_with(b"./") {
        return None;
    }
    Some(None)
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
