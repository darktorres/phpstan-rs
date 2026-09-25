use super::*;
use types::Shape;

#[test]
fn extracts_positional_placeholders_in_order() {
    let keys = sql_placeholder_keys("SELECT * FROM t WHERE a = ? AND b = ?").unwrap();
    assert_eq!(keys, vec![ParamKey::Positional(0), ParamKey::Positional(1)]);
}

#[test]
fn extracts_named_placeholders_deduplicated_by_first_occurrence() {
    let keys = sql_placeholder_keys("SELECT * FROM t WHERE a = :x AND b = :y AND c = :x").unwrap();
    assert_eq!(
        keys,
        vec![
            ParamKey::Named("x".to_string()),
            ParamKey::Named("y".to_string())
        ]
    );
}

#[test]
fn a_query_with_no_placeholders_at_all_returns_an_empty_list() {
    let keys = sql_placeholder_keys("SELECT * FROM t").unwrap();
    assert!(keys.is_empty());
}

#[test]
fn describe_key_formats_named_and_positional_differently() {
    assert_eq!(describe_key(&ParamKey::Named("id".to_string())), ":id");
    assert_eq!(describe_key(&ParamKey::Positional(2)), "?#2");
}

#[test]
fn value_is_definitely_array_recognizes_both_array_type_shapes() {
    assert!(value_is_definitely_array(&Type::array_of(Type::Int)));
    assert!(value_is_definitely_array(&Type::ArrayShape(
        Shape::from_named(vec![("a".to_string(), Type::Int, false)])
    )));
    assert!(!value_is_definitely_array(&Type::String));
    assert!(!value_is_definitely_array(&Type::Mixed(false)));
}
