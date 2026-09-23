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

const INHERITANCE_ENTITIES: &str = r#"<?php
    use Doctrine\ORM\Mapping as ORM;

    #[ORM\Entity]
    class CatEntity
    {
        #[ORM\Id]
        public int $id;
    }
    #[ORM\Entity]
    class DogEntity
    {
        #[ORM\Id]
        public int $id;
    }

    #[ORM\Entity]
    #[ORM\DiscriminatorMap(['cat' => CatEntity::class, 'dog' => DogEntity::class])]
    class AnimalEntity
    {
        #[ORM\Id]
        public int $id;
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
        dml: false,
        group_by: false,
        index_by: None,
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
            left: false,
        }],
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
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
            left: false,
        }],
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["SUM(it.imageId)".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("0".to_string(), Type::Mixed(false), false)])
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
            left: false,
        }],
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
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
fn identity_falls_back_to_mixed_never_the_targets_own_id_field_type() {
    // Real Doctrine's `IdentityFunction` bypasses the target's own
    // id-column type conversion entirely (see `field_ref_type`'s own
    // `FieldRef::Identity` doc comment) - claiming its declared type here
    // would be a real, confirmed mismatch with what a live query actually
    // hydrates.
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("ImageTagEntity".to_string(), "it".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["IDENTITY(it.tag) AS tagId".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("tagId".to_string(), Type::Mixed(false), false)])
    );
}

#[test]
fn a_scalar_only_select_keys_each_unaliased_field_by_its_own_name() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["t.id".to_string(), "t.name".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            ("id".to_string(), Type::Int, false),
            ("name".to_string(), Type::String, false),
        ])
    );
}

#[test]
fn an_unaliased_aggregate_keys_by_its_own_scalar_counter_not_its_list_position() {
    // `t.name AS n` never advances `scalarResultCounter` (only a real
    // Doctrine generic-expression item does, when unaliased) - the
    // `COUNT(...)` right after it is the *first* such item, so it keys
    // `"0"` despite being at list position 1.
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items =
        parse_select_items(&["t.name AS n".to_string(), "COUNT(t.id)".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            ("n".to_string(), Type::String, false),
            ("0".to_string(), Type::IntRange(Some(0), None), false),
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
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["nope.field".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("field".to_string(), Type::Mixed(false), false)])
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
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["nope".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(ty, Type::Mixed(false));
}

#[test]
fn an_empty_select_is_undecidable() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
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
        dml: false,
        group_by: false,
        index_by: None,
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
        dml: false,
        group_by: false,
        index_by: None,
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
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["t.id".to_string(), "t.name".to_string()]).unwrap();
    assert_eq!(single_scalar_column_type(&checker, &chain, &items), None);
}

#[test]
fn array_result_converts_a_bare_entity_select_into_an_array_shape() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["t".to_string()]).unwrap();
    let ty = build_array_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
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
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["it".to_string()]).unwrap();
    let ty = build_array_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            ("imageId".to_string(), Type::Int, false),
            (
                "tag".to_string(),
                Type::ArrayShape(vec![("id".to_string(), Type::Int, false)]),
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
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["t".to_string(), "COUNT(t.id) AS cnt".to_string()]).unwrap();
    let ty = build_array_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            (
                "0".to_string(),
                Type::ArrayShape(vec![
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
        dml: false,
        group_by: false,
        index_by: None,
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

#[test]
fn a_declared_custom_function_resolves_to_its_own_category_type() {
    let mut registry = build(ENTITIES);
    registry.doctrine_custom_functions.insert(
        "date_format_year_month".to_string(),
        crate::doctrine_functions::DoctrineFunctionKind::String,
    );
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["DATE_FORMAT_YEAR_MONTH(t.name) AS ym".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("ym".to_string(), Type::String, false)])
    );
}

#[test]
fn an_undeclared_custom_function_falls_back_to_mixed() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["SOME_UNDECLARED_FN(t.name) AS x".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![("x".to_string(), Type::Mixed(false), false)])
    );
}

#[test]
fn a_polymorphic_from_produces_a_union_across_every_mapped_subclass() {
    let registry = build(INHERITANCE_ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("AnimalEntity".to_string(), "a".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["a".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::Union(vec![
            Type::Object("CatEntity".to_string(), Vec::new()),
            Type::Object("DogEntity".to_string(), Vec::new()),
        ])
    );
}

#[test]
fn an_ordinary_from_never_produces_a_union() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("TagEntity".to_string(), "t".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["t".to_string()]).unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(ty, Type::Object("TagEntity".to_string(), Vec::new()));
}

#[test]
fn a_scalar_subquery_select_item_resolves_its_own_single_column_type() {
    let registry = build(ENTITIES);
    let checker = Checker::new(&registry);
    let (chain, items) = crate::checker::doctrine::dql::parse_select_statement(
        "SELECT t, (SELECT it.imageId FROM ImageTagEntity it) AS firstImageId FROM TagEntity t",
    )
    .unwrap();
    let ty = build_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::ArrayShape(vec![
            (
                "0".to_string(),
                Type::Object("TagEntity".to_string(), Vec::new()),
                false
            ),
            ("firstImageId".to_string(), Type::Int, false),
        ])
    );
}

#[test]
fn array_result_converts_a_polymorphic_row_member_by_member() {
    let registry = build(INHERITANCE_ENTITIES);
    let checker = Checker::new(&registry);
    let chain = QueryBuilderChain {
        from: Some(("AnimalEntity".to_string(), "a".to_string())),
        joins: Vec::new(),
        select_args: Vec::new(),
        dml: false,
        group_by: false,
        index_by: None,
    };
    let items = parse_select_items(&["a".to_string()]).unwrap();
    let ty = build_array_result_row_type(&checker, &chain, &items).unwrap();
    assert_eq!(
        ty,
        Type::Union(vec![
            Type::ArrayShape(vec![("id".to_string(), Type::Int, false)]),
            Type::ArrayShape(vec![("id".to_string(), Type::Int, false)]),
        ])
    );
}
