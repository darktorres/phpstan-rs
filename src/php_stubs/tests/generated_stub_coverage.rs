//! Coverage checks that the generated PHP stub corpus (functions and
//! classes, sourced from phpstorm-stubs and vendor/phpstan-src) is
//! actually wired in and produces correct, non-miscounted signatures.

use super::super::*;

#[test]
fn generates_a_real_ctype_signature() {
    let stubs = generated_stubs();
    let (_, sig) = stubs
        .iter()
        .find(|(name, _)| name.as_ref() == "ctype_alpha")
        .expect("ctype_alpha should be generated");
    assert_eq!(sig.params.len(), 1);
}

#[test]
fn generates_a_real_json_signature() {
    let stubs = generated_stubs();
    assert!(stubs.iter().any(|(name, _)| name.as_ref() == "json_encode"));
    assert!(stubs.iter().any(|(name, _)| name.as_ref() == "json_decode"));
}

#[test]
fn generated_stub_classes_cover_real_built_in_classes() {
    let classes = generated_stub_classes();
    let names: std::collections::HashSet<&str> = classes.iter().map(|(n, _)| n.as_ref()).collect();
    assert!(names.contains("Exception"));
    assert!(names.contains("ArrayObject"));
    assert!(names.contains("PDO"));
    assert!(classes.len() > 100);
}

#[test]
fn patch_spl_iterator_templates_gives_real_generic_spl_classes_their_missing_templates() {
    // Confirmed directly against a real, installed `phpstan/phpstan`
    // (not assumed): none of these carry `@template` tags in this
    // project's own vendored phpstorm-stubs, yet real PHPStan resolves
    // every one as generic with exactly this template list.
    let classes = generated_stub_classes();
    let by_name: std::collections::HashMap<&str, &Vec<std::sync::Arc<str>>> = classes
        .iter()
        .map(|(n, info)| (n.as_ref(), &info.templates))
        .collect();
    let expected: &[(&str, &[&str])] = &[
        ("FilterIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveIterator", &["TKey", "TValue"]),
        ("RegexIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveFilterIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveIteratorIterator", &["T"]),
        ("RecursiveDirectoryIterator", &["TKey", "TValue"]),
    ];
    for (name, templates) in expected {
        let got = by_name
            .get(name)
            .unwrap_or_else(|| panic!("{name} should be a known generated stub class"));
        let got_names: Vec<&str> = got.iter().map(|t| t.as_ref()).collect();
        assert_eq!(&got_names, templates, "{name}'s own template list");
    }
}

#[test]
fn generated_stub_classes_cover_real_phpstan_src_classes() {
    // vendor/phpstan-src's classes, chained in after phpstorm-stubs -
    // confirms PHPSTAN_SRC_SOURCES is actually wired in, not just
    // declared.
    let classes = generated_stub_classes();
    let names: std::collections::HashSet<&str> = classes.iter().map(|(n, _)| n.as_ref()).collect();
    assert!(names.contains("PHPStan\\Type\\StringType"));
    assert!(names.contains("PHPStan\\Rules\\Rule"));
}

#[test]
fn generated_stubs_cover_hundreds_of_functions_across_the_full_corpus() {
    let stubs = generated_stubs();
    let names: std::collections::HashSet<&str> = stubs.iter().map(|(n, _)| n.as_ref()).collect();
    // Spot-check functions from extensions far apart in the corpus,
    // not just ctype/json (the only two the old two-file pipeline
    // covered) - proof the full vendored tree is actually being read.
    assert!(names.contains("array_map"));
    assert!(names.contains("preg_match"));
    assert!(names.contains("curl_init"));
    assert!(names.contains("mysqli_query"));
    assert!(stubs.len() > 1000);
}

#[test]
fn array_map_s_real_stub_no_longer_miscounts_its_superseded_parameter() {
    let stubs = generated_stubs();
    let (_, sig) = stubs
        .iter()
        .find(|(name, _)| name.as_ref() == "array_map")
        .expect("array_map should be generated");
    let required = sig
        .params
        .iter()
        .filter(|p| !p.variadic && !p.has_default)
        .count();
    assert_eq!(required, 2, "array_map($callback, $array) is valid");
}

#[test]
fn array_keys_and_mt_rand_s_optional_trailing_params_are_not_miscounted() {
    let stubs = generated_stubs();
    for name in ["array_keys", "mt_rand"] {
        let (_, sig) = stubs
            .iter()
            .find(|(n, _)| n.as_ref() == name)
            .unwrap_or_else(|| panic!("{name} should be generated"));
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.has_default)
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
    let stubs = generated_stubs();
    let return_of = |name: &str| {
        stubs
            .iter()
            .find(|(n, _)| n.as_ref() == name)
            .map(|(_, sig)| sig.return_type.to_string())
            .unwrap_or_else(|| panic!("{name} should be generated"))
    };
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
    let classes = generated_stub_classes();
    let property_of = |class: &str, prop: &str| {
        classes
            .iter()
            .find(|(n, _)| n.as_ref() == class)
            .and_then(|(_, info)| info.properties.get(prop))
            .map(|sig| sig.ty.to_string())
            .unwrap_or_else(|| panic!("{class}::${prop} should be generated"))
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
