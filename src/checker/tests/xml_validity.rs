//! `DOMDocument::createElement()`'s tag-name validation and
//! `SimpleXMLElement`/`simplexml_load_string()`'s well-formedness
//! validation.

use super::helpers::*;

#[test]
fn create_element_with_a_malformed_tag_name_is_flagged() {
    let d = diagnostics_for(r#"<?php (new DOMDocument())->createElement('1bad');"#);
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "expected argument.type, got: {d:?}"
    );
}

#[test]
fn create_element_with_a_valid_tag_name_is_not_flagged() {
    let d = diagnostics_for(r#"<?php (new DOMDocument())->createElement('valid-tag');"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn create_element_with_a_computed_argument_is_not_flagged_by_the_argument_type_check() {
    // Only a literal string argument can be conclusively proven
    // malformed - a computed one is left alone by this check (though it
    // still contributes a real throw type, see throws.rs).
    let d = diagnostics_for(
        r#"<?php
            function test(string $name): void {
                (new DOMDocument())->createElement($name);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn new_simplexml_element_with_malformed_xml_is_flagged() {
    let d = diagnostics_for(r#"<?php new SimpleXMLElement('<a><b></a>');"#);
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "expected argument.type, got: {d:?}"
    );
}

#[test]
fn new_simplexml_element_with_well_formed_xml_is_not_flagged() {
    let d = diagnostics_for(r#"<?php new SimpleXMLElement('<a><b></b></a>');"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn simplexml_load_string_with_malformed_xml_is_flagged() {
    let d = diagnostics_for(r#"<?php simplexml_load_string('<a><b></a>');"#);
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "expected argument.type, got: {d:?}"
    );
}

#[test]
fn simplexml_load_string_with_well_formed_xml_is_not_flagged() {
    let d = diagnostics_for(r#"<?php simplexml_load_string('<a><b></b></a>');"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
