//! A configured root spelled as the listings of its directories spell it.
//!
//! A directory that folds case holds a file under every spelling of its
//! letters. The system gives one of them back as the file's resolved path on
//! macOS and Windows, but not on a Unix casefold directory, where a root
//! written in another case would be a second key for the files of the first.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use composer::dir_listings::{DirListings, Entry, Listing};
use composer::hashed_path::HashedPath;
use diagnostics::CaseRule;

/// `path`, a configured path, with each name spelled as the listing of its
/// directory spells it when `case` folds case: a name that the listing holds
/// only in another case becomes the entry's name. `base` is the resolved
/// directory that `path` is relative to or lies under. A name that the
/// listing does not hold, or holds in two cases, a link, and every component
/// that is not a plain name end the respelling, and the rest of `path` is as
/// given.
pub(crate) fn as_listed(
    base: &Path,
    path: &Path,
    case: CaseRule,
    listings: &DirListings,
) -> PathBuf {
    if case == CaseRule::Exact {
        return path.to_path_buf();
    }
    let (below, absolute) = match path.strip_prefix(base) {
        Ok(below) => (below, true),
        Err(_) if path.is_relative() => (path, false),
        Err(_) => return path.to_path_buf(),
    };
    let mut dir = HashedPath::new(base.to_path_buf());
    let mut spelled = PathBuf::new();
    let mut rest = below.components();
    loop {
        let mut ahead = rest.clone();
        let Some(Component::Normal(name)) = ahead.next() else {
            break;
        };
        let Some(listing) = listings.listing(&dir) else {
            break;
        };
        let Some(entry) = listed_as(&listing, name, case) else {
            break;
        };
        spelled.push(&entry.name);
        rest = ahead;
        if !entry.kind.is_some_and(|kind| kind.is_dir()) {
            break;
        }
        dir = dir.child(&entry.name);
    }
    spelled.extend(rest);
    if absolute {
        base.join(spelled)
    } else {
        spelled
    }
}

/// The entry of `listing` called `name`, else the one entry that `case`
/// compares as `name`.
fn listed_as<'l>(listing: &'l Listing, name: &OsStr, case: CaseRule) -> Option<&'l Entry> {
    if let Some(exact) = listing.entry(name) {
        return Some(exact);
    }
    let key = case.key(name.to_str()?);
    let mut alike = listing
        .entries
        .iter()
        .filter(|entry| entry.name.to_str().is_some_and(|n| case.key(n) == key));
    match (alike.next(), alike.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}
