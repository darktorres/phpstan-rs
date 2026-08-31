//! Generates built-in function signatures by parsing real stub sources
//! with this project's own lexer/parser/registry, rather than
//! hand-transcribing approximations one function at a time. A hand-
//! curated overlay (`stubs.rs`) existed alongside this before Phase 8
//! ingested the real stub corpus below, but every name it covered turned
//! out to already be present here too (`Registry::merge_stubs`'s
//! first-registered-wins semantics meant the overlay had already become
//! dead weight, not an active correction layer) - removed rather than
//! kept as a doc-comment inaccuracy waiting to mislead the next person
//! who reads it (Finding 5 from the architecture audit).
//!
//! The vendored corpus, under `vendor/phpstorm-stubs/`, is
//! `jetbrains/phpstorm-stubs` at commit
//! 748ab87d16253a5b5d648b5fe4dae1ff4152bb03 (Apache-2.0 - see
//! `vendor/phpstorm-stubs/LICENSE`) - the stub source real PHPStan itself
//! depends on, not php-src's own `*.stub.php` files (an earlier version
//! of this pipeline vendored two hand-picked php-src extension stubs
//! instead; php-src's stubs turned out to be the wrong target, since real
//! PHPStan doesn't use them). Of phpstorm-stubs' 1052 `.php` files, 558
//! are vendored here: its own `tests/` (481 files) and a handful of
//! non-stub housekeeping files (the generated `PhpStormStubsMap.php`, 11
//! `.phpstorm.meta.php` files, and its `.php-cs-fixer.php` config) are
//! excluded, since none of them contribute function signatures and the
//! map/meta files would otherwise bloat the vendored tree by nearly 1MB
//! for zero benefit. `build.rs` generates `PHPSTORM_STUBS_SOURCES` (a
//! `&[&str]` of every vendored file's contents) at compile time, so this
//! file doesn't need 558 hand-written `include_str!` calls.
//!
//! A real implementation would refresh the vendored snapshot
//! automatically at build time against a pinned tag; here it's a fixed,
//! manually-refreshed one.

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::{ClassInfo, FunctionSig, Registry};

include!(concat!(env!("OUT_DIR"), "/phpstorm_stubs_generated.rs"));

/// Parses one stub file's source into `(name, FunctionSig)` pairs,
/// reusing the exact same pipeline real project source goes through -
/// stub files are just PHP with empty function bodies, so nothing new was
/// needed to read them.
pub fn generate_from_source(source: &str) -> Vec<(String, FunctionSig)> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    let program = Parser::new(tokens)
        .parse_program()
        .expect("vendored phpstorm-stubs source failed to parse");
    let registry = Registry::build(&program);
    registry.functions.into_iter().collect()
}

/// All signatures generated from the vendored phpstorm-stubs sources,
/// merged (first occurrence wins). A few functions are declared more than
/// once across the corpus (e.g. a version-conditional pair of signatures
/// for the same function in different files); taking the first is a
/// simplification, not a precision-critical choice, since `stubs.rs`'s
/// hand-curated overlay is what wins for anything precision-sensitive.
pub fn generated_stubs() -> Vec<(String, FunctionSig)> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for source in PHPSTORM_STUBS_SOURCES {
        for (name, sig) in generate_from_source(source) {
            if seen.insert(name.clone()) {
                result.push((name, sig));
            }
        }
    }
    result
}

/// Parses one stub file's source into `(name, ClassInfo)` pairs for every
/// class/interface/enum it declares - undefined-class detection (Phase 9)
/// needs built-in classes to actually be known, the same way
/// undefined-function detection needs `generated_stubs`' functions to be
/// known; without this, every reference to a built-in like `Exception` or
/// `ArrayObject` would look undefined.
pub fn generate_classes_from_source(source: &str) -> Vec<(String, ClassInfo)> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    let program = Parser::new(tokens)
        .parse_program()
        .expect("vendored phpstorm-stubs source failed to parse");
    let registry = Registry::build(&program);
    registry.classes.into_iter().collect()
}

/// All classes generated from the vendored phpstorm-stubs sources, merged
/// (first occurrence wins - see `generated_stubs`' equivalent note).
pub fn generated_stub_classes() -> Vec<(String, ClassInfo)> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for source in PHPSTORM_STUBS_SOURCES {
        for (name, info) in generate_classes_from_source(source) {
            if seen.insert(name.clone()) {
                result.push((name, info));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_a_real_ctype_signature() {
        let stubs = generated_stubs();
        let (_, sig) = stubs
            .iter()
            .find(|(name, _)| name == "ctype_alpha")
            .expect("ctype_alpha should be generated");
        assert_eq!(sig.params.len(), 1);
    }

    #[test]
    fn generates_a_real_json_signature() {
        let stubs = generated_stubs();
        assert!(stubs.iter().any(|(name, _)| name == "json_encode"));
        assert!(stubs.iter().any(|(name, _)| name == "json_decode"));
    }

    #[test]
    fn generated_stub_classes_cover_real_built_in_classes() {
        let classes = generated_stub_classes();
        let names: std::collections::HashSet<&str> =
            classes.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains("Exception"));
        assert!(names.contains("ArrayObject"));
        assert!(names.contains("PDO"));
        assert!(classes.len() > 100);
    }

    #[test]
    fn generated_stubs_cover_hundreds_of_functions_across_the_full_corpus() {
        let stubs = generated_stubs();
        let names: std::collections::HashSet<&str> =
            stubs.iter().map(|(n, _)| n.as_str()).collect();
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
            .find(|(name, _)| name == "array_map")
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
                .find(|(n, _)| n == name)
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
}
