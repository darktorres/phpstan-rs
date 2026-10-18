//! The directory listings of a load: a directory is read once, every later ask
//! gets the same listing, and the entries are sorted and typed.

use super::helpers::*;
use crate::canonical::canonicalize;
use crate::dir_listings::{DirListings, Entry, Listing};
use crate::hashed_path::HashedPath;
use crate::test_links::link_file;
use std::ffi::OsStr;
use std::sync::Arc;

/// The names of `listing`'s entries, in order.
fn names(listing: &Listing) -> Vec<&str> {
    listing
        .entries
        .iter()
        .map(|entry| entry.name.to_str().unwrap())
        .collect()
}

#[test]
fn a_directory_is_listed_once_and_shared() {
    let dir = TempDir::new("listed_once");
    dir.write("a.php", "<?php");
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listings = DirListings::default();
    let first = listings.listing(&root).unwrap();
    let second = listings.listing(&root).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn concurrent_asks_share_one_listing() {
    let dir = TempDir::new("concurrent");
    dir.write("a.php", "<?php");
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listings = Arc::new(DirListings::default());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let listings = Arc::clone(&listings);
            let root = root.clone();
            std::thread::spawn(move || listings.listing(&root).unwrap())
        })
        .collect();
    let listed: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(
        listed
            .windows(2)
            .all(|pair| Arc::ptr_eq(&pair[0], &pair[1]))
    );
}

#[test]
fn a_file_added_after_the_listing_is_seen_once_the_table_is_cleared() {
    let dir = TempDir::new("cleared");
    dir.write("a.php", "<?php");
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listings = DirListings::default();
    let before = listings.listing(&root).unwrap();
    dir.write("b.php", "<?php");
    assert!(Arc::ptr_eq(&before, &listings.listing(&root).unwrap()));
    listings.clear();
    let after = listings.listing(&root).unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(names(&after), ["a.php", "b.php"]);
}

#[test]
fn the_entries_are_sorted_by_name_bytes() {
    let dir = TempDir::new("sorted");
    // A capital, `_` and small letters sort in that order by bytes. No two
    // names differ in case alone: a disk that folds case holds one file for
    // both.
    for name in ["c.php", "B.php", "a.php", "_x.php"] {
        dir.write(name, "");
    }
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listing = DirListings::default().listing(&root).unwrap();
    assert_eq!(names(&listing), ["B.php", "_x.php", "a.php", "c.php"]);
    assert!(listing.complete);
}

#[test]
fn an_entry_is_found_by_name_with_its_type() {
    let dir = TempDir::new("entry_lookup");
    dir.write("a.php", "");
    dir.write("sub/b.php", "");
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listing = DirListings::default().listing(&root).unwrap();
    let kind = |name: &str| listing.entry(OsStr::new(name)).and_then(|entry| entry.kind);
    assert!(kind("a.php").is_some_and(|kind| kind.is_file()));
    assert!(kind("sub").is_some_and(|kind| kind.is_dir()));
    assert!(listing.entry(OsStr::new("missing.php")).is_none());
}

#[test]
fn a_link_is_listed_as_a_link_and_not_followed() {
    let dir = TempDir::new("link_kind");
    dir.write("a.php", "");
    link_file(dir.path().join("a.php"), dir.path().join("link.php"));
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listing = DirListings::default().listing(&root).unwrap();
    let kind = listing
        .entry(OsStr::new("link.php"))
        .and_then(|entry| entry.kind)
        .unwrap();
    assert!(kind.is_symlink());
}

#[test]
fn a_path_that_is_not_a_directory_has_no_listing() {
    let dir = TempDir::new("not_listable");
    dir.write("a.php", "");
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listings = DirListings::default();
    assert!(
        listings
            .listing(&root.child(OsStr::new("absent")))
            .is_none()
    );
    assert!(listings.listing(&root.child(OsStr::new("a.php"))).is_none());
}

#[cfg(unix)]
#[test]
fn a_directory_that_cannot_be_read_has_no_listing() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("unreadable");
    dir.write("hidden/a.php", "");
    let hidden = HashedPath::new(canonicalize(&dir.path().join("hidden")).unwrap());
    // Passable but not readable. A root reads any directory, so then the
    // test only checks that the listing and the system agree. The mode is
    // restored before any assertion can fail, so the directory is removed.
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o111)).unwrap();
    let readable = std::fs::read_dir(&hidden).is_ok();
    let listed = DirListings::default().listing(&hidden).is_some();
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(listed, readable);
}

#[test]
fn the_vendor_discovery_reads_its_directories_through_the_table() {
    let dir = TempDir::new("discovery_table");
    dir.write("composer.json", "{}");
    dir.write("vendor/acme/lib/src/Bar.php", "<?php class Bar {}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
return array(
    'Acme\\Bar' => $vendorDir . '/acme/lib/src/Bar.php',
);
"#,
    );

    let listings = DirListings::default();
    crate::vendor::discover_vendor_context(dir.path(), &listings, None);
    let src = HashedPath::new(canonicalize(&dir.path().join("vendor/acme/lib/src")).unwrap());
    // The discovery has read `src`, so its listing is kept after the file goes.
    std::fs::remove_file(src.as_path().join("Bar.php")).unwrap();
    let listing = listings.listing(&src).unwrap();
    assert_eq!(names(&listing), ["Bar.php"]);
}

/// A listing of entries called `names`, as a directory that holds them gives
/// it.
fn listing_of(names: &[&str], complete: bool) -> Listing {
    let mut entries: Vec<Entry> = names
        .iter()
        .map(|name| Entry {
            name: (*name).into(),
            kind: None,
        })
        .collect();
    entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    Listing { entries, complete }
}

#[test]
fn a_directory_folds_case_where_its_platform_does() {
    let dir = TempDir::new("folds_case_probe");
    dir.write("Src/Foo.php", "<?php");
    let root = HashedPath::new(canonicalize(dir.path()).unwrap());
    let listings = DirListings::default();
    assert_eq!(
        listings.folds_case(&root),
        cfg!(any(windows, target_os = "macos"))
    );
}

#[test]
fn a_directory_that_cannot_be_read_does_not_fold_case() {
    let dir = TempDir::new("folds_case_unreadable");
    let missing = HashedPath::new(canonicalize(dir.path()).unwrap().join("missing"));
    assert!(!DirListings::default().folds_case(&missing));
}

#[test]
fn a_directory_that_finds_the_flipped_spelling_of_an_entry_folds_case() {
    let dir = TempDir::new("folds_case_found");
    dir.write("abc.TXT", "");
    let listing = listing_of(&["ABC.txt"], true);
    assert!(listing.folds_case(dir.path()));
}

#[test]
fn a_directory_that_does_not_find_the_flipped_spelling_does_not_fold_case() {
    let dir = TempDir::new("folds_case_not_found");
    let listing = listing_of(&["ABC.txt"], true);
    assert!(!listing.folds_case(dir.path()));
}

#[test]
fn an_entry_whose_flipped_spelling_is_listed_does_not_decide() {
    let dir = TempDir::new("folds_case_twins");
    dir.write("cD", "");
    // `Ab` and `aB` are two files of a directory that keeps case, so neither
    // says anything; `Cd` is the first entry that does.
    assert!(listing_of(&["Ab", "aB", "Cd"], true).folds_case(dir.path()));
    let other = TempDir::new("folds_case_twins_not_found");
    assert!(!listing_of(&["Ab", "aB", "Cd"], true).folds_case(other.path()));
}

#[test]
fn names_without_a_letter_say_nothing_of_case() {
    let dir = TempDir::new("folds_case_no_letters");
    dir.write("123", "");
    assert!(!listing_of(&["123", "4.5"], true).folds_case(dir.path()));
}

#[test]
fn an_incomplete_listing_says_nothing_of_case() {
    let dir = TempDir::new("folds_case_incomplete");
    dir.write("abc.TXT", "");
    assert!(!listing_of(&["ABC.txt"], false).folds_case(dir.path()));
}
