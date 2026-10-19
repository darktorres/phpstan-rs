//! The reader for paths spelled as Windows spells them. A path is read from
//! its bytes when it is a drive and a root (`C:\`) followed by names, or
//! names alone, separated by `\`: the shape of every path a load carries.
//! Any other path is left to `Path`: one with a `/` where the answer depends
//! on it, a share, a device or a verbatim prefix, a root without a drive
//! (`\a`), a drive without a root (`C:a`), and a name that is not Unicode.
//!
//! A function returns `None` where the bytes do not tell, and otherwise what
//! `Path` answers. The reader is made of the bytes alone, so it runs, and is
//! tested, on every platform; `Path` reads these bytes as Windows does only
//! on Windows.
//!
//! A part of a path is a `&str` cut from its bytes, which is checked to be
//! Unicode first: it is the one view of part of a path that is made without
//! unsafe code. The drive letter is read apart from the names: `c:\a` and
//! `C:\a` are one path to `Path`, and `C:\a` and `C:\A` are two.

use std::ffi::OsStr;
use std::path::{Component, Path};

#[cfg(test)]
mod tests;

/// The bytes of `path`, which are its text where it is Unicode.
fn bytes_of(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// `bytes` as text, when it is Unicode.
fn text_of(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// Whether `byte` separates two components.
fn is_separator(byte: u8) -> bool {
    matches!(byte, b'\\' | b'/')
}

/// How many bytes of a path's start are its root: 3 for the drive and root of
/// `C:\`, and 0 for a path made of names whose first is not a drive. `None`
/// for a path that starts as another kind: a drive without a root (`C:a`), or
/// a separator first, which is a root without a drive, a share, a device or a
/// verbatim path.
fn root_len(bytes: &[u8]) -> Option<usize> {
    match bytes {
        [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic() => Some(3),
        [drive, b':', ..] if drive.is_ascii_alphabetic() => None,
        [b'\\' | b'/', ..] => None,
        _ => Some(0),
    }
}

/// Whether `body` is names separated by `\`, each one a name `Path` reads as
/// a component: not empty, not `.`, and with no `/`. `Some` says whether one
/// of them is `..`.
fn body_has_parent_dir(body: &[u8]) -> Option<bool> {
    fn name_is_parent_dir(name: &[u8]) -> Option<bool> {
        match name {
            b"" | b"." => None,
            b".." => Some(true),
            _ => Some(false),
        }
    }
    let mut parent_dir = false;
    let mut name = 0;
    for (at, &byte) in body.iter().enumerate() {
        match byte {
            b'/' => return None,
            b'\\' => {
                parent_dir |= name_is_parent_dir(&body[name..at])?;
                name = at + 1;
            }
            _ => {}
        }
    }
    Some(parent_dir | name_is_parent_dir(&body[name..])?)
}

/// Whether `bytes` spell a path as its components do: no empty component, no
/// `.` component, no `/` and no trailing separator, except for the root
/// itself. `Some` gives the length of the root and whether one of the
/// components is `..`.
fn plain(bytes: &[u8]) -> Option<(usize, bool)> {
    let root = root_len(bytes)?;
    let body = &bytes[root..];
    if body.is_empty() {
        return (root != 0).then_some((root, false));
    }
    Some((root, body_has_parent_dir(body)?))
}

/// Where the last name of a path's bytes starts, when the last component is
/// that name: it is after the last separator, or is all of them, and is not
/// empty, `.` or `..`.
fn name_start(bytes: &[u8]) -> Option<usize> {
    root_len(bytes)?;
    let start = bytes
        .iter()
        .rposition(|&byte| is_separator(byte))
        .map_or(0, |at| at + 1);
    (!matches!(&bytes[start..], b"" | b"." | b"..")).then_some(start)
}

/// Where the directory of a path's bytes ends and its name starts, as
/// `Path::parent` and `Path::file_name` give them, when the last component is
/// a name and the bytes before its separator are the root, nothing, or a
/// directory that ends in a name.
fn split_offsets(bytes: &[u8]) -> Option<(usize, usize)> {
    let root = root_len(bytes)?;
    let at = bytes.iter().rposition(|&byte| is_separator(byte));
    let name = at.map_or(0, |at| at + 1);
    if matches!(&bytes[name..], b"" | b"." | b"..") {
        return None;
    }
    let dir = match at {
        None => 0,
        // The separator that ends the root is part of the directory.
        Some(at) if root != 0 && at == root - 1 => root,
        Some(at) => {
            let dir = &bytes[..at];
            // `Path` reads a directory without its trailing separators and
            // `.` components.
            if dir.last().is_some_and(|&byte| is_separator(byte))
                || dir.ends_with(b"\\.")
                || dir.ends_with(b"/.")
                || dir == b"."
            {
                return None;
            }
            at
        }
    };
    Some((dir, name))
}

/// Where what is left of `bytes` after `prefix` starts in `bytes`, past the
/// separator after `prefix`, read from the bytes when `prefix` is spelled as
/// its components are: `Some(None)` when `prefix` is no prefix of `bytes`, and
/// `None` when the bytes do not tell.
fn prefix_end(bytes: &[u8], prefix: &[u8]) -> Option<Option<usize>> {
    let (prefix_root, _) = plain(prefix)?;
    // An absolute path and a relative one have no component in common.
    if root_len(bytes)? != prefix_root {
        return Some(None);
    }
    // The drive is the same in either case, and the names that follow it are
    // read as they are written.
    let skip = usize::from(prefix_root != 0);
    if skip == 1 && !bytes[0].eq_ignore_ascii_case(&prefix[0]) {
        return Some(None);
    }
    let (bytes, prefix_names) = (&bytes[skip..], &prefix[skip..]);
    // `prefix` is spelled with `\` alone, and a `/` of `bytes` is the same
    // separator.
    let shared = bytes
        .iter()
        .zip(prefix_names)
        .take_while(|&(&byte, &other)| byte == other || (byte == b'/' && other == b'\\'))
        .count();
    if shared == prefix_names.len() {
        // `prefix`'s components are the first of `bytes` when a component
        // starts where `prefix` ends.
        let end = match bytes.get(shared) {
            None => Some(shared),
            // The root ends with its separator.
            Some(_) if prefix_root != 0 && prefix.len() == prefix_root => Some(shared),
            Some(&byte) if is_separator(byte) => Some(shared + 1),
            Some(_) => None,
        };
        return Some(end.map(|end| end + skip));
    }
    // The bytes before the component of `bytes` that holds the first byte the
    // two differ at are whole components of `prefix`. That component differs
    // from `prefix`'s, unless it is one that `Path` passes over: an empty one
    // or a `.`.
    let start = bytes[..shared]
        .iter()
        .rposition(|&byte| is_separator(byte))
        .map_or(0, |at| at + 1);
    let rest = &bytes[start..];
    if rest.first().is_some_and(|&byte| is_separator(byte))
        || rest == b"."
        || rest.starts_with(b".\\")
        || rest.starts_with(b"./")
    {
        return None;
    }
    Some(None)
}

/// `path.file_name()`.
pub(crate) fn file_name(path: &Path) -> Option<&OsStr> {
    let bytes = bytes_of(path);
    let name = name_start(bytes)?;
    Some(OsStr::new(text_of(&bytes[name..])?))
}

/// `path.parent()`.
pub(crate) fn parent(path: &Path) -> Option<&Path> {
    let bytes = bytes_of(path);
    let (dir, _) = split_offsets(bytes)?;
    Some(Path::new(text_of(&bytes[..dir])?))
}

/// `path.parent()` and `path.file_name()`.
pub(crate) fn split(path: &Path) -> Option<(&Path, &OsStr)> {
    let bytes = bytes_of(path);
    let (dir, name) = split_offsets(bytes)?;
    let text = text_of(bytes)?;
    Some((Path::new(text.get(..dir)?), OsStr::new(text.get(name..)?)))
}

/// Whether `path.file_name()` is `name`, one name.
pub(crate) fn has_file_name(path: &Path, name: &str) -> Option<bool> {
    let bytes = bytes_of(path);
    root_len(bytes)?;
    // A last byte that is neither a separator nor a dot ends a name after the
    // last separator, or all of the bytes, and that name is the last
    // component.
    if matches!(bytes.last(), None | Some(b'\\' | b'/' | b'.')) {
        return None;
    }
    Some(
        bytes
            .strip_suffix(name.as_bytes())
            .is_some_and(|dir| dir.last().is_none_or(|&byte| is_separator(byte))),
    )
}

/// `path.starts_with(base)`.
pub(crate) fn starts_with(path: &Path, base: &Path) -> Option<bool> {
    prefix_end(bytes_of(path), bytes_of(base)).map(|end| end.is_some())
}

/// `path.strip_prefix(base).ok()`.
pub(crate) fn strip_prefix<'p>(path: &'p Path, base: &Path) -> Option<Option<&'p Path>> {
    let bytes = bytes_of(path);
    let Some(end) = prefix_end(bytes, bytes_of(base))? else {
        return Some(None);
    };
    // What is left is its bytes when they are a relative path spelled as its
    // components are.
    let rest = &bytes[end..];
    if rest.is_empty() || (!is_separator(rest[0]) && body_has_parent_dir(rest).is_some()) {
        return Some(Some(Path::new(text_of(rest)?)));
    }
    None
}

/// Whether `name` is a name `PathBuf::push` adds to a path as it is: not
/// empty, not `.` or `..`, and not starting as a drive does, which `push`
/// takes for a path of its own and makes the whole of the buffer.
fn is_pushed_as_it_is(name: &[u8]) -> bool {
    match name {
        b"" | b"." | b".." => false,
        [drive, b':', ..] => !drive.is_ascii_alphabetic(),
        _ => true,
    }
}

/// Whether `path` is absolute and spelled as its components are, with no `.`
/// or `..` among them, and so that rebuilding it from them gives it back.
pub(crate) fn is_normal_absolute(path: &Path) -> bool {
    let [drive, b':', b'\\', body @ ..] = bytes_of(path) else {
        return false;
    };
    if !drive.is_ascii_alphabetic() {
        return false;
    }
    let mut name = 0;
    for (at, &byte) in body.iter().enumerate() {
        match byte {
            b'/' => return false,
            b'\\' => {
                if !is_pushed_as_it_is(&body[name..at]) {
                    return false;
                }
                name = at + 1;
            }
            _ => {}
        }
    }
    body.is_empty() || is_pushed_as_it_is(&body[name..])
}

/// The components of `path`, from the top, each with the path made of the
/// components up to it, as `Path::components` gives them; `None` when the
/// bytes do not tell. The drive's prefix comes first and ends no ancestor:
/// `Path::ancestors` of `C:\a` is `C:\a` and `C:\`.
pub(crate) fn components(path: &Path) -> Option<impl Iterator<Item = (Component<'_>, &Path)>> {
    let text = text_of(bytes_of(path))?;
    let (root, _) = plain(text.as_bytes())?;
    let head = if root == 0 {
        None
    } else {
        // `PrefixComponent` has no constructor: `Path` makes it from the drive.
        let drive = Path::new(text.get(..2)?);
        let Some(Component::Prefix(prefix)) = drive.components().next() else {
            return None;
        };
        Some([
            (Component::Prefix(prefix), drive),
            (Component::RootDir, Path::new(text.get(..root)?)),
        ])
    };
    let body = text.get(root..)?;
    let mut start = root;
    let names = body
        .split('\\')
        // The root alone has no body, which splits into one empty name.
        .filter(|_| !body.is_empty())
        .map(move |name| {
            let end = start + name.len();
            start = end + 1;
            let component = if name == ".." {
                Component::ParentDir
            } else {
                Component::Normal(OsStr::new(name))
            };
            (component, Path::new(&text[..end]))
        });
    Some(head.into_iter().flatten().chain(names))
}
