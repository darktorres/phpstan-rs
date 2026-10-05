//! The directory table: each directory listed once, its entries sorted by
//! the bytes of their names, with the kind the directory gives them.

use super::helpers::*;
use crate::dir_table::*;
use std::ffi::OsStr;

#[test]
fn entries_come_sorted_by_name_bytes_with_their_kinds() {
    let dir = TempDir::new("table_sorted");
    dir.write("b.php", "");
    dir.write("a/inner.php", "");
    dir.write("a.php", "");
    let listing = DirTable::default().list(dir.path()).unwrap();
    let seen: Vec<(&OsStr, EntryKind)> = listing
        .entries()
        .iter()
        .map(|entry| (entry.name.as_os_str(), entry.kind))
        .collect();
    assert_eq!(
        seen,
        vec![
            (OsStr::new("a"), EntryKind::Dir),
            (OsStr::new("a.php"), EntryKind::File),
            (OsStr::new("b.php"), EntryKind::File),
        ]
    );
    assert_eq!(listing.kind_of(OsStr::new("a")), Some(EntryKind::Dir));
    assert_eq!(listing.kind_of(OsStr::new("c")), None);
}

#[test]
fn a_directory_is_listed_once() {
    let dir = TempDir::new("table_once");
    dir.write("a.php", "");
    let table = DirTable::default();
    let first = table.list(dir.path()).unwrap();
    // A file made after the listing is not in the second answer: it is the
    // first listing again.
    dir.write("b.php", "");
    let second = table.list(dir.path()).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(second.entries().len(), 1);
}

#[test]
fn a_directory_that_cannot_be_read_has_no_listing() {
    let dir = TempDir::new("table_missing");
    assert!(DirTable::default().list(&dir.path().join("none")).is_none());
}

#[cfg(unix)]
#[test]
fn a_link_is_a_link_whatever_it_leads_to() {
    let dir = TempDir::new("table_link");
    dir.write("target/x.php", "");
    std::os::unix::fs::symlink(dir.path().join("target"), dir.path().join("alias")).unwrap();
    let listing = DirTable::default().list(dir.path()).unwrap();
    assert_eq!(listing.kind_of(OsStr::new("alias")), Some(EntryKind::Link));
    assert_eq!(listing.kind_of(OsStr::new("target")), Some(EntryKind::Dir));
}
