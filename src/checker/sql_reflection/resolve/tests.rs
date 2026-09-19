use super::*;

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
    assert_eq!(fields[0], ("category_id".to_string(), Type::Int, false));
}

#[test]
fn a_table_qualified_column_resolves_by_its_own_bare_name() {
    let schema = fixture_schema();
    let ty = resolve_from_static_schema(&schema, "SELECT categories.id FROM categories").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
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
    let mut registry = Registry::default();
    registry.sql_reflection_schema = Some(fixture_schema());
    let ty = resolve_sql_row_type(&registry, "SELECT id, name FROM categories").unwrap();
    let Type::ArrayShape(fields) = ty else {
        panic!("expected an ArrayShape");
    };
    assert_eq!(fields.len(), 2);
}

#[test]
fn resolve_sql_row_type_returns_none_with_nothing_configured_at_all() {
    let registry = Registry::default();
    assert!(resolve_sql_row_type(&registry, "SELECT id FROM categories").is_none());
}

#[test]
fn resolve_sql_row_type_falls_through_to_the_live_reflector_when_static_cannot_answer() {
    // No sql_reflection_config and no live reflector configured - the
    // static path can't answer (unknown table), and the live fallback
    // has nothing to connect to either, so this must cleanly degrade to
    // None rather than panic.
    let mut registry = Registry::default();
    registry.sql_reflection_schema = Some(fixture_schema());
    assert!(resolve_sql_row_type(&registry, "SELECT id FROM some_other_table").is_none());
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
    let row_type = Type::ArrayShape(vec![
        ("id".to_string(), Type::Int, false),
        ("total".to_string(), Type::String, false),
    ]);
    let refined = refine_count_columns("SELECT id, COUNT(*) AS total FROM categories", row_type);
    let Type::ArrayShape(fields) = refined else {
        panic!("expected an ArrayShape");
    };
    assert_eq!(fields[0], ("id".to_string(), Type::Int, false));
    assert_eq!(
        fields[1],
        ("total".to_string(), Type::IntRange(Some(0), None), false)
    );
}

#[test]
fn refine_count_columns_leaves_non_count_rows_untouched() {
    let row_type = Type::ArrayShape(vec![("total".to_string(), Type::String, false)]);
    let refined = refine_count_columns("SELECT SUM(id) AS total FROM categories", row_type);
    let Type::ArrayShape(fields) = refined else {
        panic!("expected an ArrayShape");
    };
    assert_eq!(fields[0], ("total".to_string(), Type::String, false));
}
