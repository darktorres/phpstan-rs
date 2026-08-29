//! Generates built-in function signatures by parsing php-src's own
//! `*.stub.php` files with this project's own lexer/parser/registry,
//! rather than hand-transcribing approximations one function at a time
//! (see `stubs.rs` for the curated fallback, still used for anything not
//! covered here). This is the "generated skeleton" half of the two-tier
//! design discussed early on: a build step pulls real signatures from
//! php-src at a pinned version, and a hand-curated overlay only needs to
//! cover corrections/precision gains, not the entire stdlib surface.
//!
//! The vendored sources, under `vendor/php-src-stubs/`, are
//! `ext/ctype/ctype.stub.php` and `ext/json/json.stub.php`, fetched
//! 2026-08-29 from php-src at commit
//! 278d290410b022c2abd5b875bbd65d30b1b53c76. They were chosen because they
//! parse cleanly as-is, unlike most other stub files (for example
//! `ext/standard/basic_functions.stub.php`), which use PHP 8.1 `enum`
//! declarations - a distinct, sizeable feature this parser doesn't support
//! anywhere, and not attempted here, the same way class-level generics or
//! incrementality are scoped out elsewhere in this project. A real
//! implementation would refresh these automatically at build time against
//! a pinned tag; here they are a fixed, manually-refreshed snapshot.

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::{FunctionSig, Registry};

const VENDORED_SOURCES: &[&str] = &[
    include_str!("../vendor/php-src-stubs/ctype.stub.php"),
    include_str!("../vendor/php-src-stubs/json.stub.php"),
];

/// Parses one php-src stub file's source into `(name, FunctionSig)` pairs,
/// reusing the exact same pipeline real project source goes through -
/// stub files are just PHP with empty function bodies, so nothing new was
/// needed to read them.
pub fn generate_from_source(source: &str) -> Vec<(String, FunctionSig)> {
    let tokens = Lexer::new(source).tokenize();
    let program = Parser::new(tokens).parse_program();
    let registry = Registry::build(&program);
    registry.functions.into_iter().collect()
}

/// All signatures generated from the vendored php-src sources, merged
/// (first occurrence wins - not expected to matter, since the vendored
/// files don't currently declare any of the same function twice).
pub fn generated_stubs() -> Vec<(String, FunctionSig)> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for source in VENDORED_SOURCES {
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
    use crate::types::Type;

    #[test]
    fn generates_a_real_ctype_signature() {
        let stubs = generated_stubs();
        let (_, sig) = stubs
            .iter()
            .find(|(name, _)| name == "ctype_alpha")
            .expect("ctype_alpha should be generated");
        assert_eq!(sig.params[0].ty, Type::Mixed);
        assert_eq!(sig.return_type, Type::Bool);
    }

    #[test]
    fn generates_a_real_json_signature_with_union_return_type() {
        let stubs = generated_stubs();
        let (_, sig) = stubs
            .iter()
            .find(|(name, _)| name == "json_encode")
            .expect("json_encode should be generated");
        assert_eq!(
            sig.return_type,
            Type::Union(vec![Type::String, Type::False])
        );
    }

    #[test]
    fn generates_a_signature_with_a_nullable_param() {
        let stubs = generated_stubs();
        let (_, sig) = stubs
            .iter()
            .find(|(name, _)| name == "json_decode")
            .expect("json_decode should be generated");
        assert_eq!(sig.params[1].ty, Type::Union(vec![Type::Bool, Type::Null]));
    }

    #[test]
    fn generated_stubs_cover_multiple_vendored_files() {
        let stubs = generated_stubs();
        let names: std::collections::HashSet<&str> =
            stubs.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains("ctype_digit"));
        assert!(names.contains("json_last_error"));
    }
}
