//! The canonical form of a path: absolute, with its links followed and its
//! `.` and `..` taken out, as the system gives it.
//!
//! Windows gives that form under the verbatim prefix (`\\?\C:\…`,
//! `\\?\UNC\server\share\…`), where `/` is no separator and `.` and `..` are
//! names, so a path written onto it (`__DIR__ . '/../x.php'`) names no file.
//! The form here drops the prefix when the path means the same without it,
//! which is when every name in it is spelled plainly (`spelled_plainly`). A
//! name that is not keeps the prefix, since without it the system reads
//! another file: `C:\a\nul.php` is the null device, and `C:\a\b.` is
//! `C:\a\b`. On other systems the form is the system's.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};

/// The canonical form of `path`. A relative `path` is read against the
/// current directory.
#[allow(clippy::disallowed_methods)]
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path).map(form)
}

/// `path`, absolute and without `.` or `..`, spelled as a canonical path is:
/// on Windows without the verbatim prefix when each of its names is spelled
/// plainly, and with it when one is not.
pub(crate) fn form(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    let path = respelled(&path).unwrap_or(path);
    path
}

/// `path` under the other prefix of its kind, when the one it has does not
/// suit its names. A device path (`\\.\`), a volume's (`\\?\Volume{…}\`), a
/// relative one, or one holding `.` or `..`, has no other.
#[cfg(windows)]
fn respelled(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return None;
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    let mut plainly = true;
    for component in components.clone() {
        match component {
            Component::Normal(name) => plainly &= spelled_plainly(name),
            _ => return None,
        }
    }
    let mut respelled = match prefix.kind() {
        Prefix::VerbatimDisk(drive) if plainly => {
            OsString::from(format!(r"{}:\", char::from(drive)))
        }
        Prefix::Disk(drive) if !plainly => OsString::from(format!(r"\\?\{}:\", char::from(drive))),
        Prefix::VerbatimUNC(server, share)
            if plainly && spelled_plainly(server) && spelled_plainly(share) =>
        {
            share_root(r"\\", server, share)
        }
        Prefix::UNC(server, share) if !plainly && !share.is_empty() => {
            share_root(r"\\?\UNC\", server, share)
        }
        _ => return None,
    };
    // Joined as text: pushed as a path, a name such as `a:b` would read as
    // a drive.
    for (i, component) in components.enumerate() {
        if i > 0 {
            respelled.push(r"\");
        }
        respelled.push(component.as_os_str());
    }
    Some(PathBuf::from(respelled))
}

/// `component`'s text, with a prefix spelled as std reads it, so that the
/// spellings `Path` calls equal (`c:` and `C:`, `//server/share` and
/// `\\server\share`) are one text.
pub(crate) fn component_text(component: Component<'_>) -> Cow<'_, OsStr> {
    let Component::Prefix(prefix) = component else {
        return Cow::Borrowed(component.as_os_str());
    };
    let (start, names) = match prefix.kind() {
        Prefix::Verbatim(name) => (r"\\?\", [Some(name), None]),
        Prefix::VerbatimUNC(server, share) => (r"\\?\UNC\", [Some(server), Some(share)]),
        Prefix::VerbatimDisk(drive) => {
            return Cow::Owned(format!(r"\\?\{}:", char::from(drive)).into());
        }
        Prefix::DeviceNS(device) => (r"\\.\", [Some(device), None]),
        Prefix::UNC(server, share) => (r"\\", [Some(server), Some(share)]),
        Prefix::Disk(drive) => return Cow::Owned(format!("{}:", char::from(drive)).into()),
    };
    let mut text = OsString::from(start);
    for (i, name) in names.into_iter().flatten().enumerate() {
        if i > 0 {
            text.push(r"\");
        }
        text.push(name);
    }
    Cow::Owned(text)
}

/// The root of `share` on `server`, after `start`.
#[cfg(windows)]
fn share_root(start: &str, server: &OsStr, share: &OsStr) -> OsString {
    let mut root = OsString::from(start);
    root.push(server);
    root.push(r"\");
    root.push(share);
    root.push(r"\");
    root
}

/// Whether a path without the verbatim prefix spells `name` as it is: it is
/// no device's name, holds no character such a path cannot, and ends in
/// neither `.` nor a space, which such a path drops.
#[cfg(windows)]
pub(crate) fn spelled_plainly(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    match bytes.last() {
        None | Some(b'.' | b' ') => false,
        Some(_) => {
            !bytes
                .iter()
                .any(|&byte| byte < 0x20 || b"<>:\"/\\|?*".contains(&byte))
                && !is_device_name(bytes)
        }
    }
}

/// Whether `name`'s stem, what comes before its first `.` with trailing
/// spaces dropped, names a DOS device, which Windows opens in place of a
/// file of that stem in any directory and with any extension.
#[cfg(windows)]
fn is_device_name(name: &[u8]) -> bool {
    const DEVICES: [&[u8]; 6] = [b"CON", b"PRN", b"AUX", b"NUL", b"CONIN$", b"CONOUT$"];
    let stem = name
        .split(|&byte| byte == b'.')
        .next()
        .unwrap_or_default()
        .trim_ascii_end();
    if DEVICES
        .iter()
        .any(|device| stem.eq_ignore_ascii_case(device))
    {
        return true;
    }
    // A serial or parallel port: a digit, or a superscript one, two or three.
    let Some((port, number)) = stem.split_at_checked(3) else {
        return false;
    };
    (port.eq_ignore_ascii_case(b"COM") || port.eq_ignore_ascii_case(b"LPT"))
        && matches!(number, [b'0'..=b'9'] | [0xC2, 0xB9 | 0xB2 | 0xB3])
}
