use super::*;

fn column(ty: ColumnType, nullable: bool) -> Column {
    Column {
        name: "c".to_string(),
        ty,
        nullable,
    }
}

#[test]
fn a_nullable_column_adds_null_to_its_type() {
    assert_eq!(
        column_value_type(&column(ColumnType::Int, true)),
        Type::union_of(vec![Type::int(), Type::null()])
    );
    assert_eq!(
        column_value_type(&column(ColumnType::Int, false)),
        Type::int()
    );
}

#[test]
fn a_nullable_number_column_is_one_flat_union() {
    assert_eq!(
        column_value_type(&column(ColumnType::Number, true)),
        Type::union_of(vec![Type::int(), Type::float(), Type::null()])
    );
}

#[test]
fn an_enum_column_is_the_union_of_its_labels() {
    assert_eq!(
        column_php_type(&ColumnType::Enum(vec!["a".to_string(), "b".to_string()])),
        Type::union_of(vec![
            Type::string_literal("a".into()),
            Type::string_literal("b".into()),
        ])
    );
}

#[test]
fn a_nullable_enum_column_is_one_flat_union() {
    assert_eq!(
        column_value_type(&column(
            ColumnType::Enum(vec!["a".to_string(), "b".to_string()]),
            true
        )),
        Type::union_of(vec![
            Type::string_literal("a".into()),
            Type::string_literal("b".into()),
            Type::null(),
        ])
    );
}
