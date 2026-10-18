//! Links for the tests that build a tree with links in it. A link to a
//! directory is a symbolic link on Unix and a junction on Windows, which
//! needs no privilege and is what Composer makes for a path repository. A
//! link to a file is a symbolic link on both; Windows makes one in Developer
//! Mode or with the symlink privilege, as its CI runners have.

use std::io;
use std::path::Path;

/// Makes `link` a link to the directory `target`. A relative `target` is read
/// from the directory `link` is in, as a symbolic link reads it.
pub(crate) fn link_dir(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    let (target, link) = (target.as_ref(), link.as_ref());
    create_dir_link(target, link).unwrap_or_else(|error| failed(target, link, &error));
}

/// Makes `link` a link to the file `target`, which need not exist. A relative
/// `target` is read from the directory `link` is in.
pub(crate) fn link_file(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    let (target, link) = (target.as_ref(), link.as_ref());
    create_file_link(target, link).unwrap_or_else(|error| failed(target, link, &error));
}

/// Removes `link`, a link to a directory, and leaves its target.
pub(crate) fn unlink_dir(link: impl AsRef<Path>) {
    let link = link.as_ref();
    remove_dir_link(link)
        .unwrap_or_else(|error| panic!("cannot remove the link {}: {error}", link.display()));
}

fn failed(target: &Path, link: &Path, error: &io::Error) -> ! {
    // ERROR_PRIVILEGE_NOT_HELD
    let hint = if cfg!(windows) && error.raw_os_error() == Some(1314) {
        " (a symbolic link needs Developer Mode or the symlink privilege)"
    } else {
        ""
    };
    panic!(
        "cannot link {} to {}: {error}{hint}",
        link.display(),
        target.display()
    );
}

#[cfg(unix)]
fn create_dir_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
fn remove_dir_link(link: &Path) -> io::Result<()> {
    std::fs::remove_file(link)
}

/// A junction holds an absolute target, so a relative one is joined to the
/// link's directory, and the system resolves its `..` when it makes the
/// junction.
#[cfg(windows)]
fn create_dir_link(target: &Path, link: &Path) -> io::Result<()> {
    let absolute = link
        .parent()
        .map_or_else(|| target.to_path_buf(), |parent| parent.join(target));
    junction::create(absolute, link)
}

/// A symbolic link's relative target is stored as it is written, and the
/// system reads it with `\` alone.
#[cfg(windows)]
fn create_file_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target.components().collect::<std::path::PathBuf>(), link)
}

/// A directory link is removed as a directory is.
#[cfg(windows)]
fn remove_dir_link(link: &Path) -> io::Result<()> {
    std::fs::remove_dir(link)
}
