use super::*;

fn only_tag(raw: &str) -> PhpDocTag {
    let node = parse(raw);
    assert_eq!(
        node.tags.len(),
        1,
        "expected exactly one tag, got: {node:?}"
    );
    node.tags.into_iter().next().unwrap()
}

fn type_of(raw: &str) -> TypeNode {
    match only_tag(raw).value {
        PhpDocTagValue::Type { ty: Some(ty), .. } => ty,
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_simple_param_parses() {
    let tag = only_tag("/** @param int $x */");
    assert_eq!(tag.name, "param");
    match tag.value {
        PhpDocTagValue::TypedVar {
            ty: Some(TypeNode::Identifier(n)),
            variable: Some(v),
            by_ref: false,
            variadic: false,
            ..
        } => {
            assert_eq!(n, "int");
            assert_eq!(v, "x");
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_by_ref_variadic_param_parses() {
    let tag = only_tag("/** @param int &...$xs */");
    match tag.value {
        PhpDocTagValue::TypedVar {
            by_ref: true,
            variadic: true,
            variable: Some(v),
            ..
        } => assert_eq!(v, "xs"),
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_genuine_intersection_type_in_param_position_is_not_mistaken_for_by_ref() {
    let tag = only_tag("/** @param Foo&Bar $x */");
    match tag.value {
        PhpDocTagValue::TypedVar {
            ty: Some(TypeNode::Intersection(members)),
            variable: Some(v),
            by_ref: false,
            ..
        } => {
            assert_eq!(
                members,
                vec![
                    TypeNode::Identifier("Foo".to_string()),
                    TypeNode::Identifier("Bar".to_string()),
                ]
            );
            assert_eq!(v, "x");
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_typeless_var_tag_parses() {
    let tag = only_tag("/** @var $x */");
    match tag.value {
        PhpDocTagValue::TypedVar {
            ty: None,
            variable: Some(v),
            ..
        } => assert_eq!(v, "x"),
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_nullable_union_type_parses() {
    let ty = type_of("/** @return ?int|string */");
    match ty {
        TypeNode::Union(members) => {
            assert!(
                matches!(&members[0], TypeNode::Nullable(inner) if **inner == TypeNode::Identifier("int".to_string()))
            );
            assert_eq!(members[1], TypeNode::Identifier("string".to_string()));
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn an_intersection_type_parses() {
    let ty = type_of("/** @return Foo&Bar */");
    assert_eq!(
        ty,
        TypeNode::Intersection(vec![
            TypeNode::Identifier("Foo".to_string()),
            TypeNode::Identifier("Bar".to_string()),
        ])
    );
}

#[test]
fn a_generic_type_parses() {
    let ty = type_of("/** @return Collection<int, User> */");
    assert_eq!(
        ty,
        TypeNode::Generic(
            "Collection".to_string(),
            vec![
                TypeNode::Identifier("int".to_string()),
                TypeNode::Identifier("User".to_string()),
            ]
        )
    );
}

#[test]
fn a_hyphenated_keyword_lexes_as_one_identifier() {
    assert_eq!(
        type_of("/** @return class-string */"),
        TypeNode::Identifier("class-string".to_string())
    );
    assert_eq!(
        type_of("/** @return non-empty-array */"),
        TypeNode::Identifier("non-empty-array".to_string())
    );
}

#[test]
fn a_generic_class_string_parses() {
    let ty = type_of("/** @return class-string<Foo> */");
    assert_eq!(
        ty,
        TypeNode::Generic(
            "class-string".to_string(),
            vec![TypeNode::Identifier("Foo".to_string())]
        )
    );
}

#[test]
fn a_repeated_array_suffix_parses() {
    let ty = type_of("/** @return int[][] */");
    assert_eq!(
        ty,
        TypeNode::ArrayOf(Box::new(TypeNode::ArrayOf(Box::new(TypeNode::Identifier(
            "int".to_string()
        )))))
    );
}

#[test]
fn an_array_shape_parses_keyed_and_optional_fields() {
    let ty = type_of("/** @return array{name: string, age?: int} */");
    match ty {
        TypeNode::ArrayShape {
            keyword,
            fields,
            sealed,
            ..
        } => {
            assert_eq!(keyword, "array");
            assert!(sealed);
            assert_eq!(fields.len(), 2);
            assert_eq!(
                fields[0].key,
                Some(ShapeKey::Identifier("name".to_string()))
            );
            assert!(!fields[0].optional);
            assert_eq!(fields[0].ty, TypeNode::Identifier("string".to_string()));
            assert_eq!(fields[1].key, Some(ShapeKey::Identifier("age".to_string())));
            assert!(fields[1].optional);
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn an_unsealed_array_shape_parses() {
    let ty = type_of("/** @return array{name: string, ...} */");
    match ty {
        TypeNode::ArrayShape { sealed, fields, .. } => {
            assert!(!sealed);
            assert_eq!(fields.len(), 1);
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn an_unsealed_array_shape_takes_a_key_and_value_type() {
    let node = parse("/** @return array{a: int, ...<string, mixed>} */");
    assert!(node.errors.is_empty(), "got: {node:?}");
    match type_of("/** @return array{a: int, ...<string, mixed>} */") {
        TypeNode::ArrayShape {
            sealed,
            fields,
            unsealed_type: Some((Some(key), value)),
            ..
        } => {
            assert!(!sealed);
            assert_eq!(fields.len(), 1);
            assert_eq!(*key, TypeNode::Identifier("string".to_string()));
            assert_eq!(*value, TypeNode::Identifier("mixed".to_string()));
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn an_unsealed_list_shape_takes_only_a_value_type() {
    let ok = parse("/** @return list{int, ...<string>,} */");
    assert!(ok.errors.is_empty(), "got: {ok:?}");
    let err = parse("/** @return list{int, ...<int, string>} */");
    assert!(!err.errors.is_empty(), "got: {err:?}");
}

#[test]
fn non_empty_array_and_list_shapes_parse() {
    for raw in [
        "/** @return non-empty-array{a: int} */",
        "/** @return non-empty-list{int, string} */",
    ] {
        let node = parse(raw);
        assert!(node.errors.is_empty(), "{raw}: {node:?}");
        assert!(matches!(type_of(raw), TypeNode::ArrayShape { .. }));
    }
}

#[test]
fn an_object_shape_needs_keys_and_has_no_unsealed_tail() {
    let ok = parse("/** @return object{a: int, 'b'?: string} */");
    assert!(ok.errors.is_empty(), "got: {ok:?}");
    for raw in [
        "/** @return object{a: int, ...} */",
        "/** @return object{int} */",
    ] {
        let node = parse(raw);
        assert!(!node.errors.is_empty(), "{raw}: {node:?}");
    }
}

#[test]
fn a_positional_array_shape_parses() {
    let ty = type_of("/** @return array{int, string} */");
    match ty {
        TypeNode::ArrayShape { fields, .. } => {
            assert_eq!(fields.len(), 2);
            assert!(fields[0].key.is_none());
            assert_eq!(fields[0].ty, TypeNode::Identifier("int".to_string()));
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_callable_signature_parses() {
    let ty = type_of("/** @return callable(int, string=, bool...): void */");
    match ty {
        TypeNode::Callable {
            keyword,
            params,
            return_type: Some(ret),
            ..
        } => {
            assert_eq!(keyword, "callable");
            assert_eq!(params.len(), 3);
            assert!(!params[0].optional && !params[0].variadic);
            assert!(params[1].optional);
            assert!(params[2].variadic);
            assert_eq!(*ret, TypeNode::Identifier("void".to_string()));
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_callable_signature_with_named_parameters_still_parses_its_whole_shape() {
    // Real text from Rector's own `BinaryOpManipulator::
    // validateCondition()` docblock - a callable parameter's own `$name`
    // is legal, purely-documentation syntax that must not break parsing
    // of the parameters (or the return type) that follow it.
    let ty = type_of(
        "/** @return callable(Node $firstNode, Node $secondNode): bool|class-string<Node> */",
    );
    match ty {
        TypeNode::Callable {
            params,
            return_type: Some(ret),
            ..
        } => {
            assert_eq!(params.len(), 2);
            assert_eq!(
                *ret,
                TypeNode::Union(vec![
                    TypeNode::Identifier("bool".to_string()),
                    TypeNode::Generic(
                        "class-string".to_string(),
                        vec![TypeNode::Identifier("Node".to_string())]
                    ),
                ])
            );
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_closure_signature_parses() {
    let ty = type_of("/** @return Closure(int): string */");
    assert!(matches!(ty, TypeNode::Callable { keyword, .. } if keyword == "Closure"));
}

#[test]
fn an_int_range_parses() {
    assert_eq!(
        type_of("/** @return int<0, 100> */"),
        TypeNode::IntRange(Some(0), Some(100))
    );
    assert_eq!(
        type_of("/** @return int<min, 100> */"),
        TypeNode::IntRange(None, Some(100))
    );
    assert_eq!(
        type_of("/** @return positive-int<1, max> */"),
        TypeNode::IntRange(Some(1), None)
    );
}

#[test]
fn literal_types_parse() {
    assert_eq!(type_of("/** @return 1 */"), TypeNode::IntLiteral(1));
    assert_eq!(type_of("/** @return -1 */"), TypeNode::IntLiteral(-1));
    assert_eq!(type_of("/** @return 1.5 */"), TypeNode::FloatLiteral(1.5));
    assert_eq!(type_of("/** @return 0.0 */").to_source_string(), "0.0");
    assert_eq!(
        type_of("/** @return 'foo' */"),
        TypeNode::StringLiteral("foo".into())
    );
}

#[test]
fn a_const_fetch_type_parses() {
    let ty = type_of("/** @return self::TYPE_* */");
    assert_eq!(
        ty,
        TypeNode::ConstFetch {
            class: "self".to_string(),
            const_name: "TYPE_*".to_string(),
        }
    );
}

#[test]
fn a_conditional_type_parses() {
    let ty = type_of("/** @return ($x is int ? true : false) */");
    match ty {
        TypeNode::Conditional {
            subject,
            negated,
            target,
            if_true,
            if_false,
        } => {
            assert_eq!(subject, "$x");
            assert!(!negated);
            assert_eq!(*target, TypeNode::Identifier("int".to_string()));
            assert_eq!(*if_true, TypeNode::Identifier("true".to_string()));
            assert_eq!(*if_false, TypeNode::Identifier("false".to_string()));
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_negated_conditional_type_parses() {
    let ty = type_of("/** @return ($x is not int ? true : false) */");
    assert!(matches!(ty, TypeNode::Conditional { negated: true, .. }));
}

#[test]
fn a_plain_parenthesized_type_still_parses_as_a_group() {
    let ty = type_of("/** @return (int|string) */");
    assert_eq!(
        ty,
        TypeNode::Paren(Box::new(TypeNode::Union(vec![
            TypeNode::Identifier("int".to_string()),
            TypeNode::Identifier("string".to_string()),
        ])))
    );
}

#[test]
fn a_template_tag_with_bound_and_default_parses() {
    let tag = only_tag("/** @template T of object = Foo */");
    match tag.value {
        PhpDocTagValue::Template {
            name,
            bound: Some(bound),
            default: Some(default),
        } => {
            assert_eq!(name, "T");
            assert_eq!(bound, TypeNode::Identifier("object".to_string()));
            assert_eq!(default, TypeNode::Identifier("Foo".to_string()));
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn an_extends_tag_parses_its_generic_argument() {
    let tag = only_tag("/** @extends Base<Foo> */");
    match tag.value {
        PhpDocTagValue::ExtendsLike {
            ty: TypeNode::Generic(name, args),
        } => {
            assert_eq!(name, "Base");
            assert_eq!(args, vec![TypeNode::Identifier("Foo".to_string())]);
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_method_tag_with_return_type_and_params_parses() {
    let tag = only_tag("/** @method static int foo(string $a, int $b = 1) */");
    match tag.value {
        PhpDocTagValue::Method {
            is_static: true,
            return_type: Some(ret),
            name,
            params,
            ..
        } => {
            assert_eq!(ret, TypeNode::Identifier("int".to_string()));
            assert_eq!(name, "foo");
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "a");
            assert!(!params[0].has_default);
            assert_eq!(params[1].name, "b");
            assert!(params[1].has_default);
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_method_tag_with_no_return_type_defaults_correctly() {
    let tag = only_tag("/** @method bar() */");
    match tag.value {
        PhpDocTagValue::Method {
            return_type: None,
            name,
            params,
            ..
        } => {
            assert_eq!(name, "bar");
            assert!(params.is_empty());
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn an_unmodeled_tag_falls_back_to_generic_text() {
    let tag = only_tag("/** @deprecated use Foo instead */");
    assert_eq!(tag.value, PhpDocTagValue::Generic("use Foo instead".into()));
}

#[test]
fn a_phpstan_prefixed_tag_name_is_kept_verbatim() {
    let tag = only_tag("/** @phpstan-return int */");
    assert_eq!(tag.name, "phpstan-return");
    assert!(matches!(tag.value, PhpDocTagValue::Type { .. }));
}

// The real, motivating example this whole grammar exists to catch: a
// stray unclosed generic bracket inside a type.

#[test]
fn an_unclosed_generic_bracket_is_a_real_recoverable_error() {
    let node = parse("/** @param Collection<int $x */");
    assert!(
        !node.errors.is_empty(),
        "expected a real parse error, got none: {node:?}"
    );
    // Still recovers enough to keep parsing forward - the tag itself is
    // still produced, not dropped outright.
    assert_eq!(node.tags.len(), 1);
}

#[test]
fn an_unclosed_array_shape_is_a_real_recoverable_error() {
    let node = parse("/** @return array{name: string */");
    assert!(!node.errors.is_empty(), "got: {node:?}");
}

#[test]
fn an_unclosed_paren_is_a_real_recoverable_error() {
    let node = parse("/** @return (int|string */");
    assert!(!node.errors.is_empty(), "got: {node:?}");
}

#[test]
fn a_malformed_tag_does_not_prevent_a_later_valid_tag_from_parsing() {
    let node = parse(
        r#"/**
             * @param Collection<int $x
             * @return string
             */"#,
    );
    assert!(!node.errors.is_empty(), "got: {node:?}");
    assert_eq!(node.tags.len(), 2);
    assert!(matches!(
        &node.tags[1].value,
        PhpDocTagValue::Type {
            ty: Some(TypeNode::Identifier(s)),
            ..
        } if s == "string"
    ));
}

#[test]
fn a_covariant_call_site_variance_marker_parses_and_round_trips() {
    let ty = type_of("/** @return Collection<covariant Cat> */");
    match &ty {
        TypeNode::Generic(outer, args) if outer == "Collection" => match &args[0] {
            TypeNode::VarianceArg(crate::grammar::ast::ArgVariance::Covariant, inner) => {
                assert_eq!(**inner, TypeNode::Identifier("Cat".to_string()));
            }
            other => panic!("got: {other:?}"),
        },
        other => panic!("got: {other:?}"),
    }
    assert_eq!(ty.to_source_string(), "Collection<covariant Cat>");
}

#[test]
fn a_contravariant_call_site_variance_marker_parses_and_round_trips() {
    let ty = type_of("/** @return Collection<contravariant Cat> */");
    assert_eq!(ty.to_source_string(), "Collection<contravariant Cat>");
}

#[test]
fn a_bivariant_call_site_marker_parses_and_round_trips() {
    let ty = type_of("/** @return Collection<*> */");
    match &ty {
        TypeNode::Generic(outer, args) if outer == "Collection" => match &args[0] {
            TypeNode::Wildcard => {}
            other => panic!("got: {other:?}"),
        },
        other => panic!("got: {other:?}"),
    }
    assert_eq!(ty.to_source_string(), "Collection<*>");
}

#[test]
fn a_bare_type_literally_named_covariant_is_not_mistaken_for_the_marker() {
    let ty = type_of("/** @return Collection<covariant> */");
    assert_eq!(
        ty,
        TypeNode::Generic(
            "Collection".to_string(),
            vec![TypeNode::Identifier("covariant".to_string())]
        )
    );
}

#[test]
fn a_trailing_html_paragraph_is_not_mistaken_for_a_generic_argument_list() {
    // Real text from phpstorm-stubs' own `Core.php::debug_backtrace()`.
    let ty = type_of(
        "/** @return array <p>an array of associative arrays. The possible returned elements</p> */",
    );
    assert_eq!(ty, TypeNode::Identifier("array".to_string()));
}

#[test]
fn a_genuine_generic_argument_named_like_an_html_tag_still_parses_when_unclosed() {
    // No matching `</p>` anywhere ahead - not HTML, so this stays a
    // (nonsensical but structurally real) generic argument named `p`.
    let ty = type_of("/** @return Collection<p> */");
    assert_eq!(
        ty,
        TypeNode::Generic(
            "Collection".to_string(),
            vec![TypeNode::Identifier("p".to_string())]
        )
    );
}

#[test]
fn a_fully_qualified_closure_type_still_parses_its_own_signature() {
    // Real text from Laravel's own `Container::factory()` conditional
    // return type - a leading `\` before `Closure` must not defeat the
    // callable-keyword check.
    let ty = type_of("/** @return \\Closure(): int */");
    assert!(matches!(ty, TypeNode::Callable { keyword, .. } if keyword == "\\Closure"));
}

#[test]
fn a_bare_wildcard_suffix_with_no_class_scope_keeps_its_star() {
    // Real text from symfony/console's own
    // `SignalableCommandInterface::getSubscribedSignals()`.
    let ty = type_of("/** @return list<\\SIG*> */");
    match ty {
        TypeNode::Generic(name, args) if name == "list" => {
            assert_eq!(args[0], TypeNode::Identifier("\\SIG*".to_string()));
        }
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn markdown_emphasis_in_the_description_is_not_mistaken_for_a_wildcard_suffix() {
    // Real text from Piwigo's own `StringHelper::qualifyUtf8()` - a
    // plain `int` return type, followed by a description that happens
    // to start with a space then a markdown-style `*0*` emphasis run.
    // The `*` isn't adjacent to `int`, so it must not attach to it.
    let ty = type_of("/** @return int *0* if ASCII, *1* if UTF-8 */");
    assert_eq!(ty, TypeNode::Identifier("int".to_string()));
}

#[test]
fn a_psalm_negated_type_degrades_to_its_wrapped_type() {
    // Real text from webmozart/assert's own
    // `Assert::allNullOrNotEmpty()` - Psalm's own negation syntax, not
    // real PHPStan grammar; not worth its own node, but also not a real
    // parse error the way genuinely malformed syntax is.
    let ty = type_of("/** @return iterable<!empty|null> */");
    match ty {
        TypeNode::Generic(name, args) if name == "iterable" => match &args[0] {
            TypeNode::Union(members) => {
                assert_eq!(members[0], TypeNode::Identifier("empty".to_string()));
                assert_eq!(members[1], TypeNode::Identifier("null".to_string()));
            }
            other => panic!("got: {other:?}"),
        },
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_wrapped_continuation_line_joins_onto_its_own_tag() {
    let node = parse(
        r#"/**
             * @return array{a: int,
             *   b: string}
             */"#,
    );
    assert_eq!(node.tags.len(), 1);
    assert!(node.errors.is_empty(), "got: {node:?}");
    match &node.tags[0].value {
        PhpDocTagValue::Type {
            ty: Some(TypeNode::ArrayShape { fields, .. }),
            ..
        } => assert_eq!(fields.len(), 2),
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_tag_or_type_prefix_found_in_the_memo_is_the_one_parsed_afresh() {
    let tags = [
        ("param", "int $x"),
        ("param", "int|string $x description of it"),
        ("param", "array<string, list<Foo>> $items"),
        ("param", "(callable(int): string)|null $f = null"),
        ("param-out", "int $x"),
        ("return", "void"),
        ("return", "array{a: int, b?: string}"),
        ("return", "\\Foo\\Bar<int>"),
        ("var", "string"),
        ("var", "string $name"),
        ("var", "$name string"),
        ("throws", "\\RuntimeException when it fails"),
        ("extends", "Base<T, U>"),
        ("implements", "Iface<T>"),
        ("use", "Trait1<T>"),
        ("method", "static int count(string $x, int ...$rest)"),
        ("template", "T of Bound"),
        ("return", "((("),
        ("param", ""),
    ];
    for (tag, rest) in tags {
        let fresh = parser::parse_tag_rest(tag, rest);
        assert_eq!(parse_tag_rest_shared(tag, rest), fresh, "{tag} {rest}");
        assert_eq!(
            parse_tag_rest_shared(tag, rest),
            fresh,
            "{tag} {rest} again"
        );
    }
    for rest in [
        "int $x",
        "Foo<int> trailing text",
        "array{a: int} then more\nand a second line",
        "?int",
        "\u{e9}t\u{e9} $x",
        "(",
        "",
        "|",
    ] {
        let (ty, remainder, errors) = parse_type_prefix(rest);
        let shared = parse_type_prefix_shared(rest);
        assert_eq!(shared, (ty, remainder, errors), "{rest:?}");
        assert_eq!(parse_type_prefix_shared(rest), shared, "{rest:?} again");
    }
}

fn description_of(tag: &str, rest: &str) -> Description {
    match parser::parse_tag_rest(tag, rest).0 {
        PhpDocTagValue::TypedVar { description, .. } | PhpDocTagValue::Type { description, .. } => {
            description
        }
        PhpDocTagValue::Generic(description) => description,
        other => panic!("got: {other:?}"),
    }
}

#[test]
fn a_description_is_the_tokens_text_joined_by_single_spaces() {
    for (tag, rest, text) in [
        ("param", "int $x the   count", "the count"),
        (
            "param",
            "int $x see http://example.com/a for more",
            "see http :",
        ),
        ("param", "int $x says 'it\\'s' and \"x\"", "says it's and x"),
        ("param", "int $x scale 1.50 or 007", "scale 1.5 or 7"),
        ("param", "int $x huge 99999999999999999999", "huge 0"),
        (
            "param",
            "int $x ($y) <b>bold</b>",
            "( $y ) < b > bold < / b >",
        ),
        ("param", "int $x", ""),
        ("return", "int the count", "the count"),
        ("deprecated", "use Foo instead", "use Foo instead"),
        ("deprecated", "", ""),
    ] {
        assert_eq!(description_of(tag, rest).text(), text, "{tag} {rest}");
    }
}

#[test]
fn a_description_is_read_from_where_the_parser_stopped_after_a_rewind() {
    for (rest, text) in [
        ("int $x [optional] text", "[ optional ] text"),
        ("Foo<int> $x [optional] text", "[ optional ] text"),
        ("(Foo is Bar ? A : B) $x tail words", "tail words"),
        ("int $x [ optional ]", "[ optional ]"),
    ] {
        assert_eq!(description_of("param", rest).text(), text, "{rest}");
    }
    // The array offset `[K]`, an int range and a callable are tried and
    // rolled back in places; the parser compares the description with the
    // tokens it reads on from where it stopped in each of these (debug
    // builds).
    for rest in [
        "int[ $x [optional] text",
        "int<0, max $x tail words",
        "callable(int $a) $x [optional]",
        "array{a: int $x [optional]",
        "Foo<int $x [optional]",
    ] {
        description_of("param", rest).text();
        description_of("return", rest).text();
    }
    assert!(description_of("param", "int $x [optional] text").starts_with_optional_marker());
}

#[test]
fn the_optional_marker_is_answered_as_the_joined_text_without_spaces_would_be() {
    for (text, expected) in [
        ("[optional]", true),
        ("[optional] and more", true),
        ("[ optional ]", true),
        ("[optional]x", true),
        ("[opt ional]", true),
        ("[\"optional\"]", true),
        ("[optional", false),
        ("optional]", false),
        ("[optionaL]", false),
        ("[option]", false),
        ("text [optional]", false),
        ("", false),
        ("[", false),
        ("[é]", false),
    ] {
        let description = description_of("param", &format!("int $x {text}"));
        assert_eq!(
            description.starts_with_optional_marker(),
            expected,
            "{text:?}"
        );
        assert_eq!(
            description
                .text()
                .replace(' ', "")
                .starts_with("[optional]"),
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn a_description_keeps_its_source_through_a_round_trip() {
    let description = description_of("param", "int $x see http://x 'a b' 1.50");
    let json = serde_json::to_string(&description).unwrap();
    let back: Description = serde_json::from_str(&json).unwrap();
    assert_eq!(back, description);
    assert_eq!(back.text(), description.text());
}

#[test]
fn a_tag_with_no_description_allocates_none() {
    assert_eq!(description_of("param", "int $x"), Description::default());
    assert_eq!(description_of("deprecated", ""), Description::default());
    assert_ne!(
        description_of("param", "int $x text"),
        Description::default()
    );
}

/// `raw`, which writes each line break as `\n`, written with each of the
/// three line breaks.
fn written_three_ways(raw: &str) -> [(&'static str, String); 3] {
    ["\n", "\r\n", "\r"].map(|line_break| (line_break, raw.replace('\n', line_break)))
}

#[test]
fn a_doc_comment_parses_to_the_same_tags_whatever_its_line_breaks() {
    let lf = concat!(
        "/**\n",
        " * Summary.\n",
        " * @param array{\n",
        " *   id: int, // the key\n",
        " *   name: string,\n",
        " * } $row the row\n",
        " * @return list<string>\n",
        " * @throws \\RuntimeException when it fails\n",
        " */",
    );
    let expected = parse(lf);
    assert_eq!(expected.tags.len(), 3);
    assert!(expected.errors.is_empty(), "got: {:?}", expected.errors);
    for (line_break, raw) in written_three_ways(lf) {
        assert_eq!(parse(&raw), expected, "{line_break:?}");
    }
}
