//! Coverage checks that the generated PHP stub corpus (functions and
//! classes, sourced from phpstorm-stubs and vendor/phpstan-src) is
//! actually wired in and produces correct, non-miscounted signatures.

use super::super::*;
use std::sync::Arc;

use crate::registry::stub_set::Stubs;

fn corpus() -> Arc<Corpus> {
    Stubs::shared().corpus()
}

/// The first declaration of the function `name`, the one the full list
/// holds.
fn function(name: &str) -> FunctionReflection {
    corpus()
        .generated_stubs_named(name, true)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{name} should be generated"))
        .1
}

/// The first declaration of the class `name`, the one the full list holds.
fn class(name: &str) -> ClassReflection {
    corpus()
        .generated_stub_classes_named(name, true)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{name} should be a known generated stub class"))
        .1
}

/// Whether the stub index lists the class `name` (any case).
fn has_class(name: &str) -> bool {
    corpus()
        .index
        .class_names
        .contains(&name.to_ascii_lowercase())
}

/// Whether the stub index lists the function `name` (any case).
fn has_function(name: &str) -> bool {
    corpus()
        .index
        .function_names
        .contains(&name.to_ascii_lowercase())
}

/// The one test that decodes every stored entry, so a damaged one is
/// found here; the others read what they check by name.
#[test]
fn every_generated_stub_entry_decodes() {
    assert!(corpus().generated_stubs().len() > 1000);
    assert!(corpus().generated_stub_classes().len() > 100);
}

#[test]
fn generates_a_real_ctype_signature() {
    assert_eq!(function("ctype_alpha").params.len(), 1);
}

#[test]
fn generates_a_real_json_signature() {
    assert!(has_function("json_encode"));
    assert!(has_function("json_decode"));
}

#[test]
fn generated_stub_classes_cover_real_built_in_classes() {
    assert!(has_class("Exception"));
    assert!(has_class("ArrayObject"));
    assert!(has_class("PDO"));
    assert!(corpus().index.class_names.records_len() > 100);
}

#[test]
fn patch_spl_iterator_templates_gives_real_generic_spl_classes_their_missing_templates() {
    // Confirmed directly against a real, installed `phpstan/phpstan`
    // (not assumed): none of these carry `@template` tags in this
    // project's own vendored phpstorm-stubs, yet real PHPStan resolves
    // every one as generic with exactly this template list.
    let expected: &[(&str, &[&str])] = &[
        ("FilterIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveIterator", &["TKey", "TValue"]),
        ("RegexIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveFilterIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveIteratorIterator", &["T"]),
        ("RecursiveDirectoryIterator", &["TKey", "TValue"]),
    ];
    for (name, templates) in expected {
        let info = class(name);
        let got_names: Vec<&str> = info.template_names().collect();
        assert_eq!(&got_names, templates, "{name}'s own template list");
    }
}

#[test]
fn generated_stub_classes_cover_real_phpstan_src_classes() {
    // vendor/phpstan-src's classes, chained in after phpstorm-stubs -
    // confirms PHPSTAN_SRC_SOURCES is actually wired in, not just
    // declared.
    assert!(has_class("PHPStan\\Type\\StringType"));
    assert!(has_class("PHPStan\\Rules\\Rule"));
}

#[test]
fn generated_stubs_cover_hundreds_of_functions_across_the_full_corpus() {
    // Spot-check functions from extensions far apart in the corpus,
    // not just ctype/json (the only two the old two-file pipeline
    // covered) - proof the full vendored tree is actually being read.
    assert!(has_function("array_map"));
    assert!(has_function("preg_match"));
    assert!(has_function("curl_init"));
    assert!(has_function("mysqli_query"));
    assert!(corpus().index.function_names.records_len() > 1000);
}

#[test]
fn array_map_s_real_stub_no_longer_miscounts_its_superseded_parameter() {
    let sig = function("array_map");
    let required = sig
        .params
        .iter()
        .filter(|p| !p.variadic() && !p.has_default())
        .count();
    assert_eq!(required, 2, "array_map($callback, $array) is valid");
}

#[test]
fn array_keys_and_mt_rand_s_optional_trailing_params_are_not_miscounted() {
    for name in ["array_keys", "mt_rand"] {
        let sig = function(name);
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic() && !p.has_default())
            .count();
        assert!(
            required <= 1,
            "{name} should allow its highest-arity-only overload's \
             trailing params to be omitted, got {required} required"
        );
    }
}

#[test]
fn patched_return_shapes_follow_the_newest_function_map_delta() {
    let return_of = |name: &str| function(name).return_type.to_string();
    let proc_status = return_of("proc_get_status");
    assert!(!proc_status.contains("false"), "got: {proc_status}");
    let cookie_params = return_of("session_get_cookie_params");
    assert!(
        cookie_params.contains("partitioned: bool"),
        "got: {cookie_params}"
    );
}

#[test]
fn phpstan_stub_property_types_override_phpstorm_stubs_ones() {
    let property_of = |name: &str, prop: &str| {
        class(name)
            .properties
            .get(prop)
            .map(|sig| sig.ty.to_string())
            .unwrap_or_else(|| panic!("{name}::${prop} should be generated"))
    };
    assert_eq!(
        property_of("DOMDocument", "documentElement"),
        "DOMElement|null"
    );
    assert_eq!(
        property_of("DOMDocument", "doctype"),
        "DOMDocumentType|null"
    );
}
