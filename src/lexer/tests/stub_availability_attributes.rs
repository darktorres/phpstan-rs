//! Stub-only availability attributes and language-level type-aware annotations.
use super::super::*;

// `skip_stub_availability_attribute` - see its own doc comment for the
// real-world motivation (array_map's superseded, closed-range,
// no-default `$arrays` parameter).

#[test]
fn closed_range_stub_attribute_with_no_default_drops_its_whole_parameter() {
    let src = "<?php function f(?callable $cb, #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $arrays, array ...$arrays) {}";
    let with_attr = Lexer::new(src).tokenize();
    let without_attr =
        Lexer::new("<?php function f(?callable $cb, array ...$arrays) {}").tokenize();
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_as_the_only_parameter_leaves_empty_parens() {
    let src = "<?php function f(#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $x) {}";
    let with_attr = Lexer::new(src).tokenize();
    let without_attr = Lexer::new("<?php function f() {}").tokenize();
    assert_eq!(with_attr, without_attr);
}

#[test]
fn open_ended_stub_attribute_keeps_its_parameter() {
    let src = "<?php function f(#[PhpStormStubsElementAvailable(from: '8.0')] array $array) {}";
    let with_attr = Lexer::new(src).tokenize();
    let without_attr = Lexer::new("<?php function f(array $array) {}").tokenize();
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_on_a_param_with_a_real_default_keeps_it() {
    let src =
        "<?php function f(#[PhpStormStubsElementAvailable(from: '5.3', to: '5.6')] $flags = 6) {}";
    let with_attr = Lexer::new(src).tokenize();
    let without_attr = Lexer::new("<?php function f($flags = 6) {}").tokenize();
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_gating_a_whole_method_is_dropped_entirely() {
    // Round 15 of the Piwigo corpus cleanup: a closed range (`to:`)
    // gating a whole method - real shape:
    // `DateTimeImmutable::__construct`'s `to: '8.2'`/`from: '8.3'`
    // pair - is superseded on the PHP version this project models,
    // same as a closed-range *parameter*, and must leave no trace at
    // all rather than surviving as an ordinary, unconditional
    // declaration (which the registry would then wrongly merge
    // return types with the real, modern declaration).
    let src = "<?php class C { #[PhpStormStubsElementAvailable(from: '5.5', to: '8.2')]\n\
                   public function __construct($datetime = \"now\") {} }";
    let with_attr = Lexer::new(src).tokenize();
    let empty_class = Lexer::new("<?php class C { }").tokenize();
    assert_eq!(with_attr, empty_class);
}

#[test]
fn open_ended_stub_attribute_gating_a_whole_method_is_kept() {
    // The counterpart to the above - an open range (`from` only,
    // still valid on the latest PHP) gating a whole method is the
    // real, current declaration and must survive unchanged.
    let src = "<?php class C { #[PhpStormStubsElementAvailable(from: '8.3')]\n\
                   public function __construct($datetime = \"now\") {} }";
    let with_attr = Lexer::new(src).tokenize();
    let without_attr =
        Lexer::new("<?php class C { public function __construct($datetime = \"now\") {} }")
            .tokenize();
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_gating_a_whole_abstract_method_is_dropped_entirely() {
    // The `;`-terminated (no body) shape - an interface/abstract
    // method declaration - must be recognized as a whole-declaration
    // terminator too, not just a `{...}` body.
    let src = "<?php interface I { #[PhpStormStubsElementAvailable(to: '7.4')]\n\
                   public function old(string $x); }";
    let with_attr = Lexer::new(src).tokenize();
    let empty_interface = Lexer::new("<?php interface I { }").tokenize();
    assert_eq!(with_attr, empty_interface);
}

#[test]
fn closed_range_stub_attribute_gating_a_whole_free_function_is_dropped_entirely() {
    // The real version_compare() shape: two whole *free function*
    // declarations (not methods), the closed-range one superseded.
    let src = "<?php #[PhpStormStubsElementAvailable(to: '7.4')]\n\
                   function version_compare(string $a, string $b): ?bool {}\n\
                   #[PhpStormStubsElementAvailable(from: '8.0')]\n\
                   function version_compare(string $a, string $b): bool {}";
    let toks = Lexer::new(src).tokenize();
    let expected =
        Lexer::new("<?php function version_compare(string $a, string $b): bool {}").tokenize();
    assert_eq!(toks, expected);
}

// Phase 24: `#[LanguageLevelTypeAware]` - a real shape from
// `substr()`'s own stub: no native return-type hint at all, an
// existing but stale `@return` doc comment, and this attribute
// immediately above `function`.

#[test]
fn language_level_type_aware_appends_the_modern_return_type_to_the_doc_comment() {
    let src = "<?php\n/** @return string|false */\n#[LanguageLevelTypeAware([\"8.0\" => \"string\"], default: \"string|false\")]\nfunction f() {}";
    let toks = Lexer::new(src).tokenize();
    let Token::DocComment(doc) = &toks[0] else {
        panic!("expected a doc comment first, got: {toks:?}");
    };
    assert!(
        doc.contains("@return string\n"),
        "modern @return not appended: {doc:?}"
    );
    assert!(
        doc.contains("@return string|false"),
        "original @return line should still be present: {doc:?}"
    );
}

#[test]
fn language_level_type_aware_picks_the_highest_versioned_key_over_default() {
    let src = "<?php\n/** @return resource|false */\n#[LanguageLevelTypeAware(['8.1' => 'PgSql\\Connection|false'], default: 'resource|false')]\nfunction pg_connect() {}";
    let toks = Lexer::new(src).tokenize();
    let Token::DocComment(doc) = &toks[0] else {
        panic!("expected a doc comment first, got: {toks:?}");
    };
    assert!(
        doc.contains("@return PgSql\\Connection|false"),
        "expected the 8.1 override, not the legacy default: {doc:?}"
    );
}

#[test]
fn language_level_type_aware_picks_the_numerically_highest_of_several_keys() {
    let src = "<?php\n/** @return int */\n#[LanguageLevelTypeAware(['8.1' => 'A', '8.10' => 'B', '8.2' => 'C'], default: 'int')]\nfunction f() {}";
    let toks = Lexer::new(src).tokenize();
    let Token::DocComment(doc) = &toks[0] else {
        panic!("expected a doc comment first, got: {toks:?}");
    };
    assert!(
        doc.contains("@return B"),
        "8.10 should numerically outrank 8.2/8.1, not string-sort behind them: {doc:?}"
    );
}

#[test]
fn language_level_type_aware_on_a_parameter_is_not_treated_as_a_return_override() {
    // Real shape: GD's `#[LanguageLevelTypeAware(['8.1' => 'GdFont|int'],
    // default: 'int')] $font` - immediately followed by `$name`, not
    // `function`. Must not append a spurious `@return` override -
    // this shape becomes injected native tokens instead (see the
    // dedicated `_injects_a_real_native_type_hint` test below).
    let src = "<?php\n/** @param int $font */\nfunction f(#[LanguageLevelTypeAware(['8.1' => 'GdFont|int'], default: 'int')] $font) {}";
    let toks = Lexer::new(src).tokenize();
    let Token::DocComment(doc) = &toks[0] else {
        panic!("expected a doc comment first, got: {toks:?}");
    };
    assert!(
        !doc.contains("@return"),
        "a parameter-gating attribute must not append a @return override: {doc:?}"
    );
}

#[test]
fn language_level_type_aware_on_a_parameter_injects_a_real_native_type_hint() {
    // Real shape: `finfo_file`'s own `#[LanguageLevelTypeAware(['8.1'
    // => 'finfo'], default: 'resource')] $finfo` - no separate native
    // hint of its own, so the modern type has to arrive as a real,
    // synthetic type-hint token sequence right before `$finfo`.
    let src = "<?php\nfunction f(#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')] $finfo) {}";
    let with_attr = Lexer::new(src).tokenize();
    let without_attr = Lexer::new("<?php\nfunction f(finfo $finfo) {}").tokenize();
    assert_eq!(with_attr, without_attr);
}

#[test]
fn language_level_type_aware_on_a_parameter_with_bracket_shorthand_is_left_alone() {
    // Real shape (found via a crash while verifying this fix against
    // this project's own bundled stub corpus): `strip_tags()`'s own
    // `#[LanguageLevelTypeAware(["7.4" => "string[]|string|null"],
    // default: "string|null")] $allowed_tags = null` - `TYPE[]` is
    // PHPDoc-only shorthand, not valid native type-hint syntax at
    // all, so injecting it as native tokens crashed the parser
    // outright (`expected variable, found LBracket`) before this
    // guard existed. Must fall back to leaving the parameter
    // untouched (today's pre-existing, safe default) rather than
    // injecting anything.
    let src = "<?php\nfunction f(#[LanguageLevelTypeAware([\"7.4\" => \"string[]|string|null\"], default: \"string|null\")] $allowed_tags = null) {}";
    let with_attr = Lexer::new(src).tokenize();
    let without_attr = Lexer::new("<?php\nfunction f($allowed_tags = null) {}").tokenize();
    assert_eq!(with_attr, without_attr);
}

#[test]
fn language_level_type_aware_with_no_preceding_doc_comment_is_a_safe_no_op() {
    let src = "<?php\n#[LanguageLevelTypeAware([\"8.0\" => \"string\"], default: \"string|false\")]\nfunction f() {}";
    // Must not panic, and must tokenize identically to the attribute
    // being absent entirely (nothing to rewrite - no crash, no stray
    // synthetic doc comment materializing from nowhere).
    let with_attr = Lexer::new(src).tokenize();
    let without_attr = Lexer::new("<?php\nfunction f() {}").tokenize();
    assert_eq!(with_attr, without_attr);
}

// Finding 6 from the architecture audit - the #[Attribute(...)] skip
// logic wasn't string-aware, so an unbalanced `[`/`]` inside an
// argument string (a completely ordinary regex character class)
// ended the skip early and corrupted the rest of the parse.

#[test]
fn attribute_skip_is_not_fooled_by_an_unbalanced_bracket_in_a_string_argument() {
    let src = "<?php #[Assert\\Regex('/[^\\]]+/')]\nclass C {}\n";
    let toks = Lexer::new(src).tokenize();
    // The attribute is skipped entirely - the next real token is the
    // `class` keyword, not garbage from inside the regex string.
    assert_eq!(toks[0], Token::Keyword(Keyword::Class));
}
