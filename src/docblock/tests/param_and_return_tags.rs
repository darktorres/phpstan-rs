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

// A Piwigo docblock running its own description text straight into the
// parameter name with no separating space
// (`NotificationByMailSubController::doTimeoutTreatment`'s own `@param
// array<int|string, mixed> $post: handle()'s own local post working
// copy...`) must not leave the trailing `:` attached to the extracted
// name - `split_whitespace` alone can't tell a real word-boundary from
// `:`, which would otherwise store the type under `"post:"`, a key the
// real parameter (`"post"`) never looks up.
#[test]
fn a_param_tag_whose_description_runs_into_the_name_with_a_colon_is_still_recognized() {
    let block = parse(
        r#"/**
         * @param array<int|string, mixed> $post: some description text
         */"#,
    );
    assert_eq!(
        block.params.get("post"),
        Some(&"array<int|string, mixed>".to_string())
    );
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
fn a_property_tag_whose_description_runs_into_the_name_with_a_colon_is_still_recognized() {
    let block = parse("/** @property int $magic: some description text */");
    assert_eq!(
        block.properties,
        vec![PropertyTag {
            name: "magic".to_string(),
            ty: "int".to_string(),
            readable: true,
            writable: true,
        }]
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
fn a_var_tag_whose_description_runs_into_the_name_with_a_colon_is_still_recognized() {
    let block = parse("/** @var SplFileInfo $file: some description text */");
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
    // A real, common style for documenting a large DTO's shape,
    // spanning multiple physical lines - `take_type_token` must see the
    // whole multi-line type, not just the first physical line (which
    // would leave an unclosed `{`).
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
fn a_var_type_wrapped_across_multiple_lines_with_trailing_line_comments_strips_them() {
    // pestphp/pest-plugin-browser's own
    // JavaScriptSerializer::parseValue: each wrapped shape field carries
    // its own trailing `// description`, which must not get spliced into
    // the joined type string - `take_type_token` would otherwise parse
    // "// null" as a bare class-like name, corrupting the whole shape.
    let block = parse(
        r#"/**
         * @var array{
         *      v?: string, // null, undefined
         *      b?: bool,   // boolean
         *  } $value
         */"#,
    );
    assert_eq!(
        block.var_type,
        Some("array{v?: string, b?: bool}".to_string())
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
fn a_real_phpstan_conditional_return_type_is_captured_as_its_own_raw_token() {
    // Doctrine DBAL's own DriverManager::getConnection().
    // `Type::Conditional`/`resolve_conditional` do real per-call-site
    // binding, so this raw token is captured like any other `@return`
    // type - `take_type_token`'s own balanced-parens grab already
    // produces the exact substring `types::parse_type_string` needs.
    let block = parse(
        r#"/**
         * @template T of Connection
         * @param Params $params
         * @phpstan-return ($params is array{wrapperClass: class-string<T>} ? T : Connection)
         */"#,
    );
    assert_eq!(
        block.return_type,
        Some("($params is array{wrapperClass: class-string<T>} ? T : Connection)".to_string())
    );
}
