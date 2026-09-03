//! Tests for parsing `@param`, `@return`, `@var`, and `@property*` docblock tags.
use super::super::*;

#[test]
fn parses_param_and_return() {
    let block = parse(
        r#"/**
         * Adds one to a number.
         *
         * @param int $x the input
         * @return int
         */"#,
    );
    assert_eq!(block.params.get("x"), Some(&"int".to_string()));
    assert_eq!(block.return_type, Some("int".to_string()));
}

#[test]
fn parses_optional_marker_on_a_param_with_no_native_default() {
    let block = parse(
        r#"/**
         * @param int $min
         * @param int $max [optional] <p>Optional highest value.</p>
         */"#,
    );
    assert!(!block.optional_params.contains("min"));
    assert!(block.optional_params.contains("max"));
}

#[test]
fn a_by_ref_param_s_ampersand_prefixed_name_is_still_recognized() {
    let block = parse(
        r#"/**
         * @param string $pattern
         * @param string[] &$matches [optional] <p>Set on match.</p>
         */"#,
    );
    assert_eq!(block.params.get("matches"), Some(&"string[]".to_string()));
    assert!(block.optional_params.contains("matches"));
}

#[test]
fn a_documented_variadic_param_s_ellipsis_prefixed_name_is_still_recognized() {
    let block = parse("/** @param int ...$nums */");
    assert_eq!(block.params.get("nums"), Some(&"int".to_string()));
}

#[test]
fn parses_property_property_read_and_property_write_tags() {
    let block = parse(
        r#"/**
         * @property int $magic
         * @property-read string $ro
         * @property-write bool $wo
         */"#,
    );
    assert_eq!(
        block.properties,
        vec![
            PropertyTag {
                name: "magic".to_string(),
                ty: "int".to_string(),
                readable: true,
                writable: true,
            },
            PropertyTag {
                name: "ro".to_string(),
                ty: "string".to_string(),
                readable: true,
                writable: false,
            },
            PropertyTag {
                name: "wo".to_string(),
                ty: "bool".to_string(),
                readable: false,
                writable: true,
            },
        ]
    );
}

#[test]
fn a_malformed_property_tag_with_no_name_is_skipped() {
    let block = parse("/** @property int */");
    assert!(block.properties.is_empty());
}

#[test]
fn parses_var() {
    let block = parse("/** @var string|null */");
    assert_eq!(block.var_type, Some("string|null".to_string()));
    assert_eq!(block.var_name, None);
}

#[test]
fn parses_var_with_a_name() {
    let block = parse("/** @var SplFileInfo $file */");
    assert_eq!(block.var_type, Some("SplFileInfo".to_string()));
    assert_eq!(block.var_name, Some("file".to_string()));
}

#[test]
fn parses_multiple_params() {
    let block = parse(
        r#"/**
         * @param int $a
         * @param string $b
         */"#,
    );
    assert_eq!(block.params.get("a"), Some(&"int".to_string()));
    assert_eq!(block.params.get("b"), Some(&"string".to_string()));
}

#[test]
fn ignores_unknown_tags_and_prose() {
    let block = parse(
        r#"/**
         * This function does a thing.
         *
         * @throws \RuntimeException
         * @deprecated use something else
         */"#,
    );
    assert!(block.params.is_empty());
    assert!(block.return_type.is_none());
    assert!(block.var_type.is_none());
}

#[test]
fn single_line_doc_comment() {
    let block = parse("/** @return bool */");
    assert_eq!(block.return_type, Some("bool".to_string()));
}

#[test]
fn return_type_with_internal_spaces_is_not_truncated() {
    let block = parse("/** @return array{count: int, first: string} */");
    assert_eq!(
        block.return_type,
        Some("array{count: int, first: string}".to_string())
    );
}

#[test]
fn param_type_with_internal_spaces_still_finds_the_variable_name() {
    let block = parse("/** @param array{count: int, first: string} $x the input */");
    assert_eq!(
        block.params.get("x"),
        Some(&"array{count: int, first: string}".to_string())
    );
}

#[test]
fn a_return_type_wrapped_across_multiple_lines_is_not_truncated() {
    // Real, common style for documenting a large DTO's shape (found
    // via a genuine false-positive return-type mismatch this exact
    // bug caused on real code): line-by-line parsing used to hand
    // take_type_token only the first physical line's text, silently
    // truncating at the line break with an unclosed `{` - producing
    // a malformed type string that then got treated as a bogus class
    // name and namespace-mangled downstream.
    let block = parse(
        r#"/**
         * @return array{id: int, file: string, date_available: ?string,
         *   name: ?string, comment: ?string, hit: int}
         */"#,
    );
    assert_eq!(
        block.return_type,
        Some(
            "array{id: int, file: string, date_available: ?string, name: ?string, comment: ?string, hit: int}"
                .to_string()
        )
    );
}

#[test]
fn a_param_type_wrapped_across_multiple_lines_still_finds_the_variable_name() {
    let block = parse(
        r#"/**
         * @param array{id: int,
         *   name: string} $row the input row
         */"#,
    );
    assert_eq!(
        block.params.get("row"),
        Some(&"array{id: int, name: string}".to_string())
    );
}

#[test]
fn phpstan_prefixed_return_and_param_win_over_the_plain_tag_regardless_of_order() {
    // Real Doctrine ORM/DBAL convention: a loose native-friendly
    // @return/@param alongside a more precise @phpstan-return/-param
    // meant for this tool specifically.
    let block = parse(
        r#"/**
         * @return array
         * @phpstan-return list<string>
         * @phpstan-param int $count
         * @param string $count
         */"#,
    );
    assert_eq!(block.return_type, Some("list<string>".to_string()));
    assert_eq!(block.params.get("count"), Some(&"int".to_string()));
}

#[test]
fn phpstan_template_extends_and_implements_are_recognized() {
    let block = parse(
        r#"/**
         * @phpstan-template T of object
         * @phpstan-extends Collection<T>
         * @phpstan-implements Iterable<T>
         */"#,
    );
    assert_eq!(block.templates, vec!["T".to_string()]);
    assert_eq!(block.template_bounds.get("T"), Some(&"object".to_string()));
    assert_eq!(block.extends, Some("Collection<T>".to_string()));
    assert_eq!(block.implements, vec!["Iterable<T>".to_string()]);
}

#[test]
fn a_real_phpstan_conditional_return_type_is_skipped_rather_than_parsed_into_garbage() {
    // Doctrine DBAL's own DriverManager::getConnection() - `T` is never
    // meant to be resolved by this project's own simple parser; leaving
    // `return_type` unset (falling back to the native hint elsewhere)
    // is the correct "doesn't parse this" behavior, not a bogus class
    // name built from whatever balanced-parens token happened to be
    // grabbed.
    let block = parse(
        r#"/**
         * @template T of Connection
         * @param Params $params
         * @phpstan-return ($params is array{wrapperClass: class-string<T>} ? T : Connection)
         */"#,
    );
    assert_eq!(block.return_type, None);
}
