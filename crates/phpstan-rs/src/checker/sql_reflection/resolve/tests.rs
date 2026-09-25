use super::*;
use crate::sql_reflection_context::SqlContext;

fn fixture_schema() -> SchemaCatalog {
    crate::sql_reflection_schema::parse_ddl(
        "CREATE TABLE categories (
            id int NOT NULL,
            name varchar(255) NOT NULL,
            comment text
        );",
        crate::sql_reflection_config::SqlDriver::Mysql,
    )
}

#[test]
fn a_wildcard_select_resolves_every_column_in_declared_order() {
    let schema = fixture_schema();
    let ty = resolve_from_static_schema(&schema, "SELECT * FROM categories").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].0, "id");
    assert_eq!(fields[1].0, "name");
    assert_eq!(fields[2].0, "comment");
}

#[test]
fn an_explicit_column_list_resolves_only_those_columns() {
    let schema = fixture_schema();
    let ty = resolve_from_static_schema(&schema, "SELECT id, name FROM categories").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0], ("id".to_string(), Type::Int, false));
    assert_eq!(fields[1], ("name".to_string(), Type::String, false));
}

#[test]
fn an_aliased_column_uses_the_alias_as_the_field_name() {
    let schema = fixture_schema();
    let ty =
        resolve_from_static_schema(&schema, "SELECT id AS category_id FROM categories").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0], ("category_id".to_string(), Type::Int, false));
}

#[test]
fn a_table_qualified_column_resolves_by_its_own_bare_name() {
    let schema = fixture_schema();
    let ty = resolve_from_static_schema(&schema, "SELECT categories.id FROM categories").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0], ("id".to_string(), Type::Int, false));
}

#[test]
fn a_computed_select_item_falls_through_rather_than_guessing() {
    let schema = fixture_schema();
    assert!(resolve_from_static_schema(&schema, "SELECT COUNT(*) FROM categories").is_none());
}

#[test]
fn a_join_falls_through_rather_than_guessing() {
    let schema = fixture_schema();
    assert!(
        resolve_from_static_schema(
            &schema,
            "SELECT id FROM categories JOIN images ON images.category_id = categories.id"
        )
        .is_none()
    );
}

#[test]
fn an_unknown_table_falls_through() {
    let schema = fixture_schema();
    assert!(resolve_from_static_schema(&schema, "SELECT id FROM does_not_exist").is_none());
}

#[test]
fn an_unknown_column_falls_through() {
    let schema = fixture_schema();
    assert!(resolve_from_static_schema(&schema, "SELECT nope FROM categories").is_none());
}

#[test]
fn resolve_sql_row_type_uses_the_static_schema_when_configured_and_no_live_reflector_exists() {
    let registry = Registry::default();
    let context = SqlContext::new(None, Some(fixture_schema()));
    let ty = resolve_sql_row_type(&registry, &context, "SELECT id, name FROM categories").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields.len(), 2);
}

#[test]
fn resolve_sql_row_type_returns_none_with_nothing_configured_at_all() {
    let registry = Registry::default();
    let context = SqlContext::default();
    assert!(resolve_sql_row_type(&registry, &context, "SELECT id FROM categories").is_none());
}

#[test]
fn resolve_sql_row_type_falls_through_to_the_live_reflector_when_static_cannot_answer() {
    // No sql_reflection_config and no live reflector configured - the
    // static path can't answer (unknown table), and the live fallback
    // has nothing to connect to either, so this must cleanly degrade to
    // None rather than panic.
    let registry = Registry::default();
    let context = SqlContext::new(None, Some(fixture_schema()));
    assert!(resolve_sql_row_type(&registry, &context, "SELECT id FROM some_other_table").is_none());
}

#[test]
fn count_column_positions_finds_a_bare_count_call() {
    let positions = count_column_positions("SELECT id, COUNT(*) FROM categories").unwrap();
    assert_eq!(positions, vec![1]);
}

#[test]
fn count_column_positions_finds_an_aliased_count_call() {
    let positions =
        count_column_positions("SELECT COUNT(*) AS total, name FROM categories").unwrap();
    assert_eq!(positions, vec![0]);
}

#[test]
fn count_column_positions_is_case_insensitive() {
    let positions = count_column_positions("SELECT count(*) FROM categories").unwrap();
    assert_eq!(positions, vec![0]);
}

#[test]
fn count_column_positions_does_not_match_sum_or_other_aggregates() {
    let positions = count_column_positions("SELECT SUM(id), AVG(id) FROM categories").unwrap();
    assert!(positions.is_empty());
}

#[test]
fn refine_count_columns_overwrites_only_the_count_field() {
    let row_type = Type::ArrayShape(Shape::from_named(vec![
        ("id".to_string(), Type::Int, false),
        ("total".to_string(), Type::String, false),
    ]));
    let refined = refine_count_columns("SELECT id, COUNT(*) AS total FROM categories", row_type);
    let Type::ArrayShape(fields) = refined else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0], ("id".to_string(), Type::Int, false));
    assert_eq!(
        fields[1],
        ("total".to_string(), Type::IntRange(Some(0), None), false)
    );
}

#[test]
fn refine_count_columns_leaves_non_count_rows_untouched() {
    let row_type = Type::ArrayShape(Shape::from_named(vec![(
        "total".to_string(),
        Type::String,
        false,
    )]));
    let refined = refine_count_columns("SELECT SUM(id) AS total FROM categories", row_type);
    let Type::ArrayShape(fields) = refined else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0], ("total".to_string(), Type::String, false));
}

#[test]
fn count_column_positions_finds_a_window_function_count_over() {
    let positions =
        count_column_positions("SELECT id, COUNT(*) OVER() AS total FROM categories").unwrap();
    assert_eq!(positions, vec![1]);
}

#[test]
fn resolve_pure_ast_resolves_integer_and_string_literals_with_no_driver_at_all() {
    let ty = resolve_pure_ast("SELECT 1, 'x'", None).unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0], ("1".to_string(), Type::Int, false));
    assert_eq!(fields[1], ("x".to_string(), Type::String, false));
}

#[test]
fn resolve_pure_ast_resolves_null_and_a_negative_integer() {
    let ty = resolve_pure_ast("SELECT NULL, -1", None).unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0], ("NULL".to_string(), Type::Null, false));
    assert_eq!(fields[1], ("-1".to_string(), Type::Int, false));
}

#[test]
fn resolve_pure_ast_uses_an_alias_over_the_source_text_when_present() {
    let ty = resolve_pure_ast("SELECT 1 AS one", None).unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0], ("one".to_string(), Type::Int, false));
}

#[test]
fn resolve_pure_ast_resolves_a_bare_count_star_with_no_from_at_all() {
    let ty = resolve_pure_ast("SELECT COUNT(*)", None).unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0].0, "COUNT(*)");
    assert_eq!(fields[0].1, Type::Int);
}

#[test]
fn resolve_pure_ast_bails_on_a_decimal_literal_with_no_driver_known() {
    assert!(resolve_pure_ast("SELECT 1.5", None).is_none());
}

#[test]
fn resolve_pure_ast_resolves_a_decimal_literal_as_string_for_mysql() {
    let ty = resolve_pure_ast("SELECT 1.5", Some(SqlDriver::Mysql)).unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0].1, Type::String);
}

#[test]
fn resolve_pure_ast_resolves_a_decimal_literal_as_float_for_sqlite() {
    let ty = resolve_pure_ast("SELECT 1.5", Some(SqlDriver::Sqlite)).unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0].1, Type::Float);
}

#[test]
fn resolve_pure_ast_bails_on_a_boolean_literal_with_no_driver_known() {
    assert!(resolve_pure_ast("SELECT true", None).is_none());
}

#[test]
fn resolve_pure_ast_resolves_a_boolean_literal_as_bool_for_postgres_but_int_for_mysql() {
    let pg = resolve_pure_ast("SELECT true", Some(SqlDriver::Pgsql)).unwrap();
    let Type::ArrayShape(pg_fields) = pg else {
        panic!("expected an ArrayShape");
    };
    assert_eq!(pg_fields.fields[0].ty, Type::Bool);

    let mysql = resolve_pure_ast("SELECT true", Some(SqlDriver::Mysql)).unwrap();
    let Type::ArrayShape(mysql_fields) = mysql else {
        panic!("expected an ArrayShape");
    };
    assert_eq!(mysql_fields.fields[0].ty, Type::Int);
}

#[test]
fn resolve_pure_ast_bails_when_any_select_item_references_a_column() {
    assert!(resolve_pure_ast("SELECT 1, name FROM categories", None).is_none());
}

#[test]
fn resolve_sql_row_type_answers_a_pure_literal_query_with_nothing_configured_at_all() {
    let registry = Registry::default();
    let context = SqlContext::default();
    let ty = resolve_sql_row_type(&registry, &context, "SELECT 1, 'x'").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields[0].1, Type::Int);
    assert_eq!(fields[1].1, Type::String);
}

#[test]
fn single_table_name_extracts_the_bare_table_name() {
    assert_eq!(
        single_table_name("SELECT id FROM categories"),
        Some("categories".to_string())
    );
}

#[test]
fn single_table_name_returns_none_for_a_join() {
    assert!(
        single_table_name("SELECT id FROM categories JOIN images ON images.id = categories.id")
            .is_none()
    );
}

#[test]
fn cross_reference_entity_fields_is_a_no_op_with_no_matching_entity() {
    let registry = Registry::default();
    let row_type = Type::ArrayShape(Shape::from_named(vec![(
        "name".to_string(),
        Type::String,
        false,
    )]));
    let result =
        cross_reference_entity_fields(&registry, "SELECT name FROM categories", row_type.clone());
    assert_eq!(result, row_type);
}

#[test]
fn a_derived_table_resolves_through_the_same_static_schema_lookup() {
    let schema = fixture_schema();
    let ty = resolve_from_static_schema(
        &schema,
        "SELECT t.id FROM (SELECT id, name FROM categories) AS t",
    )
    .unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields, vec![("id".to_string(), Type::Int, false)]);
}

#[test]
fn a_non_recursive_cte_resolves_through_the_same_static_schema_lookup() {
    let schema = fixture_schema();
    let ty = resolve_from_static_schema(
        &schema,
        "WITH t AS (SELECT id, name FROM categories) SELECT name FROM t",
    )
    .unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    let fields = fields.named_fields();
    assert_eq!(fields, vec![("name".to_string(), Type::String, false)]);
}

#[test]
fn a_recursive_cte_falls_through_rather_than_guessing() {
    let schema = fixture_schema();
    assert!(
        resolve_from_static_schema(
            &schema,
            "WITH RECURSIVE t AS (SELECT id, name FROM categories) SELECT name FROM t"
        )
        .is_none()
    );
}

#[test]
fn a_derived_table_over_an_unresolvable_inner_query_falls_through() {
    let schema = fixture_schema();
    assert!(
        resolve_from_static_schema(
            &schema,
            "SELECT t.x FROM (SELECT COUNT(*) AS x FROM categories) AS t"
        )
        .is_none()
    );
}

#[test]
fn a_nullable_static_schema_column_reads_as_nullable() {
    let schema = crate::sql_reflection_schema::parse_ddl(
        "CREATE TABLE t (id int NOT NULL, a datetime DEFAULT NULL, b varchar(10) NULL, PRIMARY KEY (id));",
        crate::sql_reflection_config::SqlDriver::Mysql,
    );
    let row = resolve_from_static_schema(&schema, "SELECT id, a, b FROM t").expect("resolves");
    assert_eq!(
        row.to_string(),
        "array{id: int, a: string|null, b: string|null}"
    );
}
