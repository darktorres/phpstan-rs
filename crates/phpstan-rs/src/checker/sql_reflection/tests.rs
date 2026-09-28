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
        Type::Union(vec![Type::Int, Type::Null])
    );
    assert_eq!(
        column_value_type(&column(ColumnType::Int, false)),
        Type::Int
    );
}

#[test]
fn a_nullable_number_column_is_one_flat_union() {
    assert_eq!(
        column_value_type(&column(ColumnType::Number, true)),
        Type::Union(vec![Type::Int, Type::Float, Type::Null])
    );
}

#[test]
fn an_enum_column_is_the_union_of_its_labels() {
    assert_eq!(
        column_php_type(&ColumnType::Enum(vec!["a".to_string(), "b".to_string()])),
        Type::Union(vec![
            Type::StringLiteral("a".into()),
            Type::StringLiteral("b".into()),
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
        Type::Union(vec![
            Type::StringLiteral("a".into()),
            Type::StringLiteral("b".into()),
            Type::Null,
        ])
    );
}
