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
//!
//! `PHPSTAN_SRC_SOURCES` (Finding 4): a second vendored corpus,
//! `vendor/phpstan-src/src/` - real PHPStan's own MIT-licensed runtime
//! source (already vendored in this project, previously only used to
//! derive test fixtures - see the many `// Derived from vendor/
//! phpstan-src/tests/...` comments throughout `checker.rs`), reused here
//! for the identical reason `phpstorm-stubs` is: a real corpus vendor
//! package authoring a PHPStan rule/extension references `PHPStan\Type\*`/
//! `PHPStan\Rules\Rule`/etc., none of which exist as scannable source in
//! `phpstan/phpstan`'s own phar distribution. `generated_stubs`/
//! `generated_stub_classes` chain `PHPSTAN_SRC_SOURCES` in after
//! `PHPSTORM_STUBS_SOURCES`, so real PHP builtins keep their existing
//! first-occurrence-wins priority - phpstan-src can only add names, never
//! take priority over one phpstorm-stubs already claimed. Confirmed
//! before landing this: `phpstan-rs check vendor/phpstan-src/src` (0 of
//! 1793 files failed to parse) and `phpstan-rs check-project
//! vendor/phpstan-src/src` (the full `Registry::build`/`ClassInfo::
//! from_decl` path this pipeline actually exercises, a strict superset of
//! what stub generation needs) both completed cleanly with no panic. Not
//! closed by this: `vendor/phpstan-src/src` itself references classes
//! from its own dependency, `nikic/php-doc-parser` (`PHPStan\PhpDocParser//! Ast\Type\*`), not vendored here - a smaller, disclosed residual (see
//! `ROADMAP.md`'s "Known limitations").

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
    let registry = Registry::index_and_flatten(&program);
    registry
        .functions
        .into_iter()
        .map(|(name, mut sig)| {
            // See `FunctionSig::is_stub`'s own doc comment: a stub file's
            // own body is never real - `Registry::index_and_flatten` has
            // no way to know that on its own, so this is set here
            // instead, the one place that does.
            sig.is_stub = true;
            (name, sig)
        })
        .collect()
}

/// Phase 89: phpstorm-stubs' own real (and, per real PHP's own C-level `arg_info`,
/// genuinely accurate) declaration of `array_multisort`'s variadic tail
/// is `&...$rest` - every "rest" argument marked by-reference uniformly.
/// Real PHP's own actual runtime behavior for this specific function is
/// looser than that declaration suggests, though: a call like
/// `array_multisort($a, SORT_DESC, SORT_NUMERIC, $b)` legitimately
/// passes literal sort-order/sort-flag constants into by-ref-declared
/// positions, which every *other* by-ref parameter in the entire
/// language would reject outright ("Only variables can be passed by
/// reference"). Real PHPStan's own bundled `resources/functionMap.php`
/// (confirmed directly) declares this same function with a *plain*,
/// non-`&`-prefixed signature (`SignatureMapParser`'s own convention:
/// a `&`-prefixed parameter *name* is what marks by-ref, and this
/// project's entry has none) - i.e. real PHPStan deliberately treats
/// none of `array_multisort`'s arguments as reference-checked at all,
/// the exact trade-off (losing precision on the genuinely-by-ref array
/// slots to avoid a false positive on the interleaved sort-flag ones)
/// mirrored here. This project has no general hand-curated stub-overlay
/// mechanism any more (see this module's own doc comment - the last one
/// was dead code, removed before Phase 8) - a single, narrow, explicitly
/// justified post-generation patch for the one function this project's
/// own real corpus actually evidenced the gap on is more honest than
/// reintroducing a whole overlay system for it.
fn patch_array_multisort_by_ref(stubs: &mut [(String, FunctionSig)]) {
    for (name, sig) in stubs.iter_mut() {
        if name != "array_multisort" {
            continue;
        }
        for param in &mut sig.params {
            param.by_ref = false;
        }
    }
}

/// All signatures generated from the vendored phpstorm-stubs sources,
/// merged (first occurrence wins). A few functions are declared more than
/// once across the corpus (e.g. a version-conditional pair of signatures
/// for the same function in different files); taking the first is a
/// simplification, not a precision-critical choice, since `stubs.rs`'s
/// hand-curated overlay is what wins for anything precision-sensitive.
pub fn generated_stubs() -> Vec<(String, FunctionSig)> {
    // Chaining PHPSTAN_SRC_SOURCES in (Finding 4) took the combined
    // corpus from 558 to 2351 files - re-parsing that from scratch on
    // every call (as this used to do) is fine for the real CLI binary,
    // which calls this exactly once per process, but made the test suite
    // (hundreds of `diagnostics_for`/`Project::new` calls, each
    // previously re-parsing the whole corpus independently) measurably
    // slower - confirmed directly: the lib test suite went from ~40s to
    // ~143s. `LazyLock` computes the expensive part (parsing every file)
    // exactly once per process regardless of caller count; each call
    // here still returns a fresh, independently-owned `Vec` (cloning
    // already-built `FunctionSig`s, not re-parsing anything) so this
    // function's own public signature - and every one of its call sites,
    // which all consume the returned `Vec` by value - stays unchanged.
    static CACHE: std::sync::LazyLock<Vec<(String, FunctionSig)>> =
        std::sync::LazyLock::new(|| {
            let mut seen = std::collections::HashSet::new();
            let mut result = Vec::new();
            for source in PHPSTORM_STUBS_SOURCES
                .iter()
                .chain(PHPSTAN_SRC_SOURCES.iter())
            {
                for (name, sig) in generate_from_source(source) {
                    if seen.insert(name.clone()) {
                        result.push((name, sig));
                    }
                }
            }
            patch_array_multisort_by_ref(&mut result);
            result
        });
    CACHE.clone()
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
    let registry = Registry::index_and_flatten(&program);
    registry
        .classes
        .into_iter()
        .map(|(name, mut info)| {
            // Same reasoning as `generate_from_source`'s own
            // `FunctionSig::is_stub` marking, one level down: every
            // method a stub class declares has an equally empty,
            // unreal body.
            for sig in info.methods.values_mut() {
                sig.is_stub = true;
            }
            (name, info)
        })
        .collect()
}

/// All classes generated from the vendored phpstorm-stubs sources, merged
/// (first occurrence wins - see `generated_stubs`' equivalent note).
pub fn generated_stub_classes() -> Vec<(String, ClassInfo)> {
    // See `generated_stubs`' own doc comment - identical `LazyLock`
    // caching, same reason.
    static CACHE: std::sync::LazyLock<Vec<(String, ClassInfo)>> = std::sync::LazyLock::new(|| {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for source in PHPSTORM_STUBS_SOURCES
            .iter()
            .chain(PHPSTAN_SRC_SOURCES.iter())
        {
            for (name, info) in generate_classes_from_source(source) {
                if seen.insert(name.clone()) {
                    result.push((name, info));
                }
            }
        }
        result
    });
    CACHE.clone()
}

#[cfg(test)]
mod tests;
