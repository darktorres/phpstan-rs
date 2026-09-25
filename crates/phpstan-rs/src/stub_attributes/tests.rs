//! The version attributes, checked against the same declarations
//! written without them.

use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn parse(src: &str) -> Vec<Stmt> {
    let mut stmts = Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .expect("parses");
    super::apply_stub_attributes(&mut stmts);
    stmts
}

/// The declarations' signatures, without spans: what the attributes
/// change.
fn shape(src: &str) -> Vec<String> {
    fn function(f: &FunctionDecl, out: &mut Vec<String>) {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                format!(
                    "{}{}{}${}{} attrs={:?} doc={:?}",
                    p.type_hint
                        .as_ref()
                        .map(|h| format!("{h} "))
                        .unwrap_or_default(),
                    if p.by_ref { "&" } else { "" },
                    if p.variadic { "..." } else { "" },
                    p.name,
                    if p.default.is_some() {
                        " = default"
                    } else {
                        ""
                    },
                    p.attrs
                        .iter()
                        .map(|a| a.name.to_string())
                        .collect::<Vec<_>>(),
                    p.doc_comment,
                )
            })
            .collect();
        out.push(format!(
            "function {}({}): {:?} attrs={:?} doc={:?}",
            f.name,
            params.join(", "),
            f.return_type.as_ref().map(ToString::to_string),
            f.attrs
                .iter()
                .map(|a| a.name.to_string())
                .collect::<Vec<_>>(),
            f.doc_comment,
        ));
    }
    fn stmts(list: &[Stmt], out: &mut Vec<String>) {
        for s in list {
            match &s.kind {
                StmtKind::FunctionDecl(f) => function(f, out),
                StmtKind::ClassDecl(c) => {
                    out.push(format!("class {} {{", c.name));
                    for m in &c.methods {
                        function(&m.function, out);
                    }
                    out.push("}".to_string());
                }
                StmtKind::Use(imports) => out.push(format!("use {}", imports.len())),
                StmtKind::Block(inner) => stmts(inner, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    stmts(&parse(src), &mut out);
    out
}

fn first_function_doc(src: &str) -> String {
    parse(src)
        .iter()
        .find_map(|s| match &s.kind {
            StmtKind::FunctionDecl(f) => Some(f.doc_comment.as_deref().unwrap_or("").to_string()),
            _ => None,
        })
        .expect("a function")
}

#[test]
fn closed_range_stub_attribute_with_no_default_drops_its_whole_parameter() {
    let src = "<?php function f(?callable $cb, #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $arrays, array ...$arrays) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php function f(?callable $cb, array ...$arrays) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_as_the_only_parameter_leaves_empty_parens() {
    let src = "<?php function f(#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $x) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php function f() {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn open_ended_stub_attribute_keeps_its_parameter() {
    let src = "<?php function f(#[PhpStormStubsElementAvailable(from: '8.0')] array $array) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php function f(array $array) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_on_a_param_with_a_real_default_keeps_it() {
    let src =
        "<?php function f(#[PhpStormStubsElementAvailable(from: '5.3', to: '5.6')] $flags = 6) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php function f($flags = 6) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_gating_a_whole_method_is_dropped_entirely() {
    // A closed range (`to:`) gating a whole method is superseded on the
    // PHP version this project models, same as a closed-range parameter,
    // and must leave no trace at all rather than surviving as an
    // ordinary, unconditional declaration (which the registry would
    // then wrongly merge return types with the real, modern
    // declaration).
    let src = "<?php class C { #[PhpStormStubsElementAvailable(from: '5.5', to: '8.2')]\n\
                   public function __construct($datetime = \"now\") {} }";
    let with_attr = shape(src);
    let empty_class = shape("<?php class C { }");
    assert_eq!(with_attr, empty_class);
}

#[test]
fn open_ended_stub_attribute_gating_a_whole_method_is_kept() {
    // The counterpart to the above - an open range (`from` only,
    // still valid on the latest PHP) gating a whole method is the
    // real, current declaration and must survive unchanged.
    let src = "<?php class C { #[PhpStormStubsElementAvailable(from: '8.3')]\n\
                   public function __construct($datetime = \"now\") {} }";
    let with_attr = shape(src);
    let without_attr =
        shape("<?php class C { public function __construct($datetime = \"now\") {} }");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn closed_range_stub_attribute_gating_a_whole_abstract_method_is_dropped_entirely() {
    // The `;`-terminated (no body) shape - an interface/abstract
    // method declaration - must be recognized as a whole-declaration
    // terminator too, not just a `{...}` body.
    let src = "<?php interface I { #[PhpStormStubsElementAvailable(to: '7.4')]\n\
                   public function old(string $x); }";
    let with_attr = shape(src);
    let empty_interface = shape("<?php interface I { }");
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
    let toks = shape(src);
    let expected = shape("<?php function version_compare(string $a, string $b): bool {}");
    assert_eq!(toks, expected);
}

// `#[LanguageLevelTypeAware]` - no native return-type hint at all, an
// existing but stale `@return` doc comment, and this attribute
// immediately above `function`.

#[test]
fn language_level_type_aware_appends_the_modern_return_type_to_the_doc_comment() {
    let src = "<?php\n/** @return string|false */\n#[LanguageLevelTypeAware([\"8.0\" => \"string\"], default: \"string|false\")]\nfunction f() {}";
    let doc = first_function_doc(src);
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
    let doc = first_function_doc(src);
    assert!(
        doc.contains("@return PgSql\\Connection|false"),
        "expected the 8.1 override, not the legacy default: {doc:?}"
    );
}

#[test]
fn language_level_type_aware_picks_the_numerically_highest_of_several_keys() {
    let src = "<?php\n/** @return int */\n#[LanguageLevelTypeAware(['8.1' => 'A', '8.10' => 'B', '8.2' => 'C'], default: 'int')]\nfunction f() {}";
    let doc = first_function_doc(src);
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
    let doc = first_function_doc(src);
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
    let with_attr = shape(src);
    let without_attr = shape("<?php\nfunction f(finfo $finfo) {}");
    assert_eq!(with_attr, without_attr);
}

// Real shape: `vendor/jetbrains/phpstorm-stubs/ldap/ldap.php` does `use
// JetBrains\PhpStorm\Internal\LanguageLevelTypeAware as PhpVersionAware;`
// and then spells every `ldap_*` function's own gated `$ldap`/`$result`
// parameter as `#[PhpVersionAware(...)]` exclusively - a plain PHP
// `use`-import alias for the identical attribute class handled above,
// not a different one. Without recognizing this second spelling, every
// LDAP stub parameter fell through to the generic `#[` attribute-token
// path, leaving it stuck on the stale pre-8.1 `@param resource $ldap`
// doc comment instead of the real `\LDAP\Connection` override.
#[test]
fn php_version_aware_is_recognized_as_an_alias_for_language_level_type_aware() {
    let src = "<?php\nfunction ldap_bind(#[PhpVersionAware(['8.1' => '\\LDAP\\Connection'], default: 'resource')] $ldap) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php\nfunction ldap_bind(\\LDAP\\Connection $ldap) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn php_version_aware_on_a_return_type_appends_to_the_doc_comment_the_same_way() {
    let src = "<?php\n/** @return resource|false */\n#[PhpVersionAware(['8.1' => '\\LDAP\\Connection|false'], default: 'resource|false')]\nfunction ldap_connect() {}";
    let doc = first_function_doc(src);
    assert!(
        doc.contains("@return \\LDAP\\Connection|false"),
        "expected the 8.1 override via the PhpVersionAware alias, not the legacy default: {doc:?}"
    );
}

#[test]
fn language_level_type_aware_on_a_parameter_with_bracket_shorthand_is_left_alone() {
    // `TYPE[]` is PHPDoc-only shorthand, not valid native type-hint
    // syntax at all, so injecting it as native tokens would crash the
    // parser outright (`expected variable, found LBracket`). Must fall
    // back to leaving the parameter untouched (the safe default) rather
    // than injecting anything.
    let src = "<?php\nfunction f(#[LanguageLevelTypeAware([\"7.4\" => \"string[]|string|null\"], default: \"string|null\")] $allowed_tags = null) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php\nfunction f($allowed_tags = null) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn language_level_type_aware_with_no_preceding_doc_comment_is_a_safe_no_op() {
    let src = "<?php\n#[LanguageLevelTypeAware([\"8.0\" => \"string\"], default: \"string|false\")]\nfunction f() {}";
    // Must not panic, and must parse identically to the attribute
    // being absent entirely (nothing to rewrite - no crash, no stray
    // synthetic doc comment materializing from nowhere).
    let with_attr = shape(src);
    let without_attr = shape("<?php\nfunction f() {}");
    assert_eq!(with_attr, without_attr);
}

// A general `#[Attribute(...)]` group is real tokens, not skipped text,
// so a string literal's content (including an unbalanced `[`/`]` inside
// it, such as an ordinary regex character class) is naturally immune to
// corrupting the rest of the parse - it's just one ordinary `StringLit`
// token regardless of what bytes it contains.

#[test]
fn a_closed_range_parameter_redeclared_by_the_next_one_is_dropped_even_with_a_default() {
    let src = "<?php function mktime(#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] int $hour = null, #[PhpStormStubsElementAvailable(from: '8.0')] int $hour, ?int $minute = null) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php function mktime(int $hour, ?int $minute = null) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn a_closed_range_parameter_with_a_default_and_no_replacement_is_kept() {
    let src = "<?php function f(int $a, #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $b = null) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php function f(int $a, $b = null) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn an_imported_alias_of_the_availability_attribute_is_honored() {
    let src = "<?php\nuse JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable as ElementAvailable;\nfunction f(#[ElementAvailable(from: '5.3', to: '5.6')] $form, #[ElementAvailable(from: '7.0')] $form = 1) {}";
    let with_attr = shape(src);
    let without_attr = shape(
        "<?php\nuse JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable as ElementAvailable;\nfunction f($form = 1) {}",
    );
    assert_eq!(with_attr, without_attr);
}

#[test]
fn an_imported_alias_of_the_type_aware_attribute_is_honored() {
    let src = "<?php\nuse JetBrains\\PhpStorm\\Internal\\LanguageLevelTypeAware as LanguageAware;\nfunction f(#[LanguageAware(['8.0' => 'string'], default: '')] $s) {}";
    let with_attr = shape(src);
    let without_attr = shape(
        "<?php\nuse JetBrains\\PhpStorm\\Internal\\LanguageLevelTypeAware as LanguageAware;\nfunction f(string $s) {}",
    );
    assert_eq!(with_attr, without_attr);
}

#[test]
fn an_attribute_that_merely_shares_an_alias_name_is_left_alone() {
    let src = "<?php\nfunction f(#[Available(from: '5.3', to: '5.6')] $x) {}";
    let shape = shape(src);
    assert!(shape[0].contains("attrs=[\"Available\"]"), "{shape:?}");
}

#[test]
fn a_closed_range_parameter_redeclaring_an_earlier_one_is_dropped_even_with_a_default() {
    let src = "<?php function pg_lo_import(#[PhpStormStubsElementAvailable(from: '8.0')] $connection, #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $connection = null, $pathname) {}";
    let with_attr = shape(src);
    let without_attr = shape("<?php function pg_lo_import($connection, $pathname) {}");
    assert_eq!(with_attr, without_attr);
}

#[test]
fn a_superseded_declarations_doc_and_attributes_go_with_it() {
    let src = "<?php\n/** @return array|false */\n#[Pure]\n#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')]\nfunction array_combine(array $k, array $v): array|false {}\n/** @return array */\n#[Pure]\n#[PhpStormStubsElementAvailable(from: '8.0')]\nfunction array_combine(array $k, array $v): array {}";
    assert_eq!(
        shape(src),
        shape(
            "<?php\n/** @return array */\n#[Pure]\nfunction array_combine(array $k, array $v): array {}"
        )
    );
}

#[test]
fn a_type_aware_attribute_sharing_a_group_is_honored() {
    let src = "<?php function f(#[Deprecated(since: '8.4'), LanguageLevelTypeAware(['8.0' => 'int'], default: '')] $mode = 0) {}";
    assert_eq!(
        shape(src),
        shape("<?php function f(#[Deprecated(since: '8.4')] int $mode = 0) {}")
    );
}
