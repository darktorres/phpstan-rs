//! Whether the friendly report is written with colour.

use super::super::check_project::wants_colour;
use std::ffi::OsStr;

#[test]
fn a_terminal_gets_colour_unless_no_color_is_set_to_something() {
    assert!(wants_colour(true, None));
    // The convention counts an empty value as unset.
    assert!(wants_colour(true, Some(OsStr::new(""))));
    assert!(!wants_colour(true, Some(OsStr::new("1"))));
    assert!(!wants_colour(true, Some(OsStr::new("0"))));
}

#[test]
fn what_is_not_a_terminal_gets_none() {
    assert!(!wants_colour(false, None));
    assert!(!wants_colour(false, Some(OsStr::new(""))));
    assert!(!wants_colour(false, Some(OsStr::new("1"))));
}
