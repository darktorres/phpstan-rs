use super::*;
use crate::checker::doctrine::dql::parse_select_items;
use crate::checker::doctrine::query_builder::{JoinClause, QueryBuilderChain};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::Registry;

const ENTITIES: &str = r#"<?php
    use Doctrine\ORM\Mapping as ORM;

    #[ORM\Entity]
    class TagEntity
    {
        #[ORM\Id]
        public int $id;

        public function __construct(
            #[ORM\Column]
            public string $name,
        ) {}
    }

    #[ORM\Entity]
    class ImageTagEntity
    {
        public function __construct(
            #[ORM\Id]
            #[ORM\ManyToOne]
            public TagEntity $tag,
            #[ORM\Id]
            public int $imageId,
        ) {}
    }
"#;

fn build(src: &str) -> Registry {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    Registry::build(&program)
}

#[test]
fn a_bare_entity_select_is_the_entity_object_directly() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(ty, Type::Object("TagEntity".to_string(), Vec::new()));
}

#[test]
fn count_never_needs_its_operand_to_resolve() {
    // `COUNT`'s own return type (`int<0, max>`) never depends on its
    // operand at all (matches real Doctrine's own `walkAggregateExpression`
    // exactly) - an unresolvable join alias inside a `COUNT(...)` operand
    // doesn't make the whole query undecidable, unlike `SUM`/`AVG`/`MIN`/
    // `MAX`, whose own result type *is* the operand's type.
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: vec![JoinClause {
            target: "t.imageTags".to_string(),
            alias: "it".to_string(),
        }],
        select_args: Vec::new(),
    };
    let items =
        parse_select_items(&["t".to_string(), "COUNT(it.imageId) AS counter".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            (
                "0".to_string(),
                Type::Object("TagEntity".to_string(), Vec::new()),
                false
            ),
            ("counter".to_string(), Type::IntRange(Some(0), None), false),
        ])
    );
}

#[test]
fn sum_falls_back_to_mixed_when_its_operand_alias_never_resolves() {
    // Unlike a real DQL syntax failure (168b/168c's own "undecidable"
    // contract), a metadata gap here localizes to `Mixed` for this one
    // field rather than discarding the whole row - matches real
    // `phpstan-doctrine`'s own walker.
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: vec![JoinClause {
            target: "t.imageTags".to_string(),
            alias: "it".to_string(),
        }],
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["SUM(it.imageId)".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("0".to_string(), Type::Mixed, false)])
    );
}

#[test]
fn an_entity_plus_scalar_field_from_a_real_join_alias() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: vec![JoinClause {
            target: "ImageTagEntity".to_string(),
            alias: "it".to_string(),
        }],
        select_args: Vec::new(),
    };
    let items =
        parse_select_items(&["t".to_string(), "COUNT(it.imageId) AS counter".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            (
                "0".to_string(),
                Type::Object("TagEntity".to_string(), Vec::new()),
                false
            ),
            ("counter".to_string(), Type::IntRange(Some(0), None), false),
        ])
    );
}

#[test]
fn identity_resolves_the_target_entitys_own_id_field_type() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("ImageTagEntity".to_string(), "it".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["IDENTITY(it.tag) AS tagId".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("tagId".to_string(), Type::Int, false)])
    );
}

#[test]
fn a_scalar_only_select_positions_every_unaliased_item_by_index() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t.id".to_string(), "t.name".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            ("0".to_string(), Type::Int, false),
            ("1".to_string(), Type::String, false),
        ])
    );
}

#[test]
fn a_reference_to_an_unknown_alias_falls_back_to_mixed_for_that_field() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["nope.field".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("0".to_string(), Type::Mixed, false)])
    );
}

#[test]
fn a_bare_alias_select_on_an_unknown_alias_falls_back_to_plain_mixed() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["nope".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(ty, Type::Mixed);
}

#[test]
fn an_empty_select_is_undecidable() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    assert_eq!(build_result_row_type(&checker, &chain, &[]), None);
}

#[test]
fn single_scalar_column_type_resolves_a_lone_scalar_field() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t.name".to_string()]).unwrap();
    assert_eq!(
        single_scalar_column_type(&checker, &chain, &items),
        Some(Type::String)
    );
}

#[test]
fn single_scalar_column_type_rejects_a_whole_entity_select() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t".to_string()]).unwrap();
    assert_eq!(single_scalar_column_type(&checker, &chain, &items), None);
}

#[test]
fn single_scalar_column_type_rejects_more_than_one_item() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t.id".to_string(), "t.name".to_string()]).unwrap();
    assert_eq!(single_scalar_column_type(&checker, &chain, &items), None);
}

#[test]
fn array_result_converts_a_bare_entity_select_into_an_object_shape() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t".to_string()]).unwrap();
    let ty = build_array_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ObjectShape(vec![
            ("id".to_string(), Type::Int, false),
            ("name".to_string(), Type::String, false),
        ])
    );
}

#[test]
fn array_result_converts_a_to_one_association_into_an_identifier_only_shape() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("ImageTagEntity".to_string(), "it".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["it".to_string()]).unwrap();
    let ty = build_array_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ObjectShape(vec![
            ("imageId".to_string(), Type::Int, false),
            (
                "tag".to_string(),
                Type::ObjectShape(vec![("id".to_string(), Type::Int, false)]),
                false
            ),
        ])
    );
}

#[test]
fn array_result_converts_an_entity_plus_aggregate_select_field_by_field() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t".to_string(), "COUNT(t.id) AS cnt".to_string()]).unwrap();
    let ty = build_array_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            (
                "0".to_string(),
                Type::ObjectShape(vec![
                    ("id".to_string(), Type::Int, false),
                    ("name".to_string(), Type::String, false),
                ]),
                false
            ),
            ("cnt".to_string(), Type::IntRange(Some(0), None), false),
        ])
    );
}

#[test]
fn scalar_result_flattens_entity_fields_under_alias_prefixed_keys() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
    };
    let items = parse_select_items(&["t".to_string(), "COUNT(t.id) AS cnt".to_string()]).unwrap();
    let ty = build_scalar_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            ("t_id".to_string(), Type::Int, false),
            ("t_name".to_string(), Type::String, false),
            ("cnt".to_string(), Type::IntRange(Some(0), None), false),
        ])
    );
}
