//! Generates built-in function signatures by parsing real stub sources
//! with this project's own lexer/parser/registry, rather than
//! hand-transcribing approximations one function at a time (see
//! `stubs.rs` for the curated overlay, which still wins on conflicts).
//! This is the "generated skeleton" half of the two-tier design: a build
//! step pulls real signatures from a vendored corpus, and the hand-curated
//! overlay only needs to cover corrections/precision gains, not the
//! entire stdlib surface.
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
use crate::registry::{FunctionSig, Registry};

include!(concat!(env!("OUT_DIR"), "/phpstorm_stubs_generated.rs"));

/// Parses one stub file's source into `(name, FunctionSig)` pairs,
/// reusing the exact same pipeline real project source goes through -
/// stub files are just PHP with empty function bodies, so nothing new was
/// needed to read them.
pub fn generate_from_source(source: &str) -> Vec<(String, FunctionSig)> {
    let tokens = Lexer::new(source).tokenize();
    let program = Parser::new(tokens).parse_program();
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
}
