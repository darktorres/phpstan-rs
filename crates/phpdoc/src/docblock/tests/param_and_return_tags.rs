//! Tests for parsing `@param`, `@return`, `@var`, and `@property*` docblock tags.
use super::super::*;
use super::node;

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
    assert_eq!(block.params.get("x"), Some(&node("int")));
    assert_eq!(block.return_type, Some(node("int")));
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
    assert_eq!(block.params.get("matches"), Some(&node("string[]")));
    assert!(block.optional_params.contains("matches"));
}

#[test]
fn a_documented_variadic_param_s_ellipsis_prefixed_name_is_still_recognized() {
    let block = parse("/** @param int ...$nums */");
    assert_eq!(block.params.get("nums"), Some(&node("int")));
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
        Some(&node("array<int|string, mixed>"))
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
                ty: node("int"),
                readable: true,
                writable: true,
            },
            PropertyTag {
                name: "ro".to_string(),
                ty: node("string"),
                readable: true,
                writable: false,
            },
            PropertyTag {
                name: "wo".to_string(),
                ty: node("bool"),
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
            ty: node("int"),
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
    assert_eq!(block.var_type, Some(node("string|null")));
    assert_eq!(block.var_name, None);
}

#[test]
fn parses_var_with_a_name() {
    let block = parse("/** @var SplFileInfo $file */");
    assert_eq!(block.var_type, Some(node("SplFileInfo")));
    assert_eq!(block.var_name, Some("file".to_string()));
}

#[test]
fn a_var_tag_whose_description_runs_into_the_name_with_a_colon_is_still_recognized() {
    let block = parse("/** @var SplFileInfo $file: some description text */");
    assert_eq!(block.var_type, Some(node("SplFileInfo")));
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
    assert_eq!(block.params.get("a"), Some(&node("int")));
    assert_eq!(block.params.get("b"), Some(&node("string")));
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
    assert_eq!(block.return_type, Some(node("bool")));
}

#[test]
fn return_type_with_internal_spaces_is_not_truncated() {
    let block = parse("/** @return array{count: int, first: string} */");
    assert_eq!(
        block.return_type,
        Some(node("array{count: int, first: string}"))
    );
}

#[test]
fn param_type_with_internal_spaces_still_finds_the_variable_name() {
    let block = parse("/** @param array{count: int, first: string} $x the input */");
    assert_eq!(
        block.params.get("x"),
        Some(&node("array{count: int, first: string}"))
    );
}

#[test]
fn a_return_type_wrapped_across_multiple_lines_is_not_truncated() {
    // A real, common style for documenting a large DTO's shape,
    // spanning multiple physical lines - the parser must see the whole
    // multi-line type, not just the first physical line (which would
    // leave an unclosed `{`).
    let block = parse(
        r#"/**
         * @return array{id: int, file: string, date_available: ?string,
         *   name: ?string, comment: ?string, hit: int}
         */"#,
    );
    assert_eq!(
        block.return_type,
        Some(node(
            "array{id: int, file: string, date_available: ?string, name: ?string, comment: ?string, hit: int}"
        ))
    );
}

#[test]
fn a_var_type_wrapped_across_multiple_lines_with_trailing_line_comments_strips_them() {
    // pestphp/pest-plugin-browser's own
    // JavaScriptSerializer::parseValue: each wrapped shape field carries
    // its own trailing `// description`, which must not get spliced into
    // the joined type - otherwise "// null" would parse as a bare
    // class-like name, corrupting the whole shape.
    let block = parse(
        r#"/**
         * @var array{
         *      v?: string, // null, undefined
         *      b?: bool,   // boolean
         *  } $value
         */"#,
    );
    assert_eq!(block.var_type, Some(node("array{v?: string, b?: bool}")));
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
        Some(&node("array{id: int, name: string}"))
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
    assert_eq!(block.return_type, Some(node("list<string>")));
    assert_eq!(block.params.get("count"), Some(&node("int")));
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
    assert_eq!(block.template_bounds.get("T"), Some(&node("object")));
    assert_eq!(block.extends, Some(node("Collection<T>")));
    assert_eq!(block.implements, vec![node("Iterable<T>")]);
}

#[test]
fn a_real_phpstan_conditional_return_type_is_captured_as_its_own_raw_token() {
    // Doctrine DBAL's own DriverManager::getConnection().
    // conditional type/`resolve_conditional` do real per-call-site
    // binding, so this type is captured like any other `@return`
    // type.
    let block = parse(
        r#"/**
         * @template T of Connection
         * @param Params $params
         * @phpstan-return ($params is array{wrapperClass: class-string<T>} ? T : Connection)
         */"#,
    );
    assert_eq!(
        block.return_type,
        Some(node(
            "($params is array{wrapperClass: class-string<T>} ? T : Connection)"
        ))
    );
}

#[test]
fn a_trailing_line_comment_inside_a_wrapped_shape_is_skipped_but_a_url_is_kept() {
    // A wrapped array shape's own `// description` after a field ends at
    // its line - the type still parses whole.
    let block = parse(
        r#"/**
         * @var array{v?: string, // null or undefined
         *   b?: bool, // a flag
         * } $opts
         * @param string $url see http://example.com/docs for details
         * @see https://example.com/page
         */"#,
    );
    assert_eq!(block.var_type, Some(node("array{v?: string, b?: bool}")));
    assert_eq!(block.params.get("url"), Some(&node("string")));
    assert_eq!(block.see, vec!["https://example.com/page".to_string()]);
    assert!(
        block.parse_errors.is_empty(),
        "got: {:?}",
        block.parse_errors
    );
}

#[test]
fn an_invalid_tag_value_containing_a_url_is_recorded_whole() {
    let block = parse("/** @param boolean with_scheme if false - does not add http://toto.com */");
    assert_eq!(block.invalid_tag_values.len(), 1);
    assert_eq!(
        block.invalid_tag_values[0].value,
        "boolean with_scheme if false - does not add http://toto.com"
    );
}

#[test]
fn a_param_closure_this_type_wrapped_across_lines_keeps_its_variable_name() {
    let block = parse(
        r#"/**
         * @param-closure-this array{a: int,
         *   b: int} $fn
         */"#,
    );
    assert_eq!(
        block.param_closure_this.get("fn"),
        Some(&node("array{a: int, b: int}"))
    );
}
