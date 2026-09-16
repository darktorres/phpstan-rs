use super::*;

fn items(args: &[&str]) -> Vec<SelectItem> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    parse_select_items(&args).expect("expected select items to parse")
}

#[test]
fn a_bare_alias_is_a_whole_entity_select() {
    let got = items(&["t"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Field(FieldRef::Alias("t".to_string())),
            alias: None,
        }]
    );
}

#[test]
fn a_dotted_field_is_a_scalar_field_select() {
    let got = items(&["t.name"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Field(FieldRef::Field("t".to_string(), "name".to_string())),
            alias: None,
        }]
    );
}

#[test]
fn identity_resolves_an_association_reference() {
    let got = items(&["IDENTITY(ic.category) AS categoryId"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Field(FieldRef::Identity(
                "ic".to_string(),
                "category".to_string()
            )),
            alias: Some("categoryId".to_string()),
        }]
    );
}

#[test]
fn count_star_has_no_operand() {
    let got = items(&["COUNT(*)"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Aggregate {
                func: AggregateFn::Count,
                operand: None,
            },
            alias: None,
        }]
    );
}

#[test]
fn count_distinct_field_with_alias() {
    let got = items(&["COUNT(DISTINCT it.imageId) AS counter"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Aggregate {
                func: AggregateFn::Count,
                operand: Some(FieldRef::Field("it".to_string(), "imageId".to_string())),
            },
            alias: Some("counter".to_string()),
        }]
    );
}

#[test]
fn a_single_comma_joined_argument_splits_into_multiple_items() {
    let got = items(&["t, COUNT(it.imageId) AS counter"]);
    assert_eq!(got.len(), 2);
    assert_eq!(
        got[0].kind,
        SelectItemKind::Field(FieldRef::Alias("t".to_string()))
    );
    assert_eq!(
        got[1].kind,
        SelectItemKind::Aggregate {
            func: AggregateFn::Count,
            operand: Some(FieldRef::Field("it".to_string(), "imageId".to_string())),
        }
    );
}

#[test]
fn separate_arguments_each_produce_their_own_item() {
    let got = items(&["t", "COUNT(it.imageId) AS counter"]);
    assert_eq!(got.len(), 2);
}

#[test]
fn new_dto_construction_is_recognized_with_its_args_skipped() {
    let got = items(&["NEW App\\Dto\\TagCount(t.id, t.name, COUNT(it.imageId))"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::New("App\\Dto\\TagCount".to_string()),
            alias: None,
        }]
    );
}

#[test]
fn case_when_then_else_end_collects_every_arm() {
    let got = items(&["CASE WHEN t.active = 1 THEN t.name ELSE t.id END AS label"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Case {
                then_arms: vec![FieldRef::Field("t".to_string(), "name".to_string())],
                else_arm: Some(FieldRef::Field("t".to_string(), "id".to_string())),
            },
            alias: Some("label".to_string()),
        }]
    );
}

#[test]
fn a_custom_function_name_is_recognized_as_custom() {
    let got = items(&["DATE_FORMAT_MONTH_DAY(t.lastmodified) AS period"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Custom("DATE_FORMAT_MONTH_DAY".to_string()),
            alias: Some("period".to_string()),
        }]
    );
}

#[test]
fn distinct_modifier_is_consumed_without_changing_the_items_shape() {
    let got = items(&["DISTINCT t.id"]);
    assert_eq!(
        got,
        vec![SelectItem {
            kind: SelectItemKind::Field(FieldRef::Field("t".to_string(), "id".to_string())),
            alias: None,
        }]
    );
}

#[test]
fn an_unresolvable_select_item_makes_the_whole_query_undecidable() {
    let args = vec!["t.id + 1".to_string()];
    assert!(parse_select_items(&args).is_none());
}

#[test]
fn a_nested_subquery_select_item_is_undecidable_not_misparsed() {
    let args = vec!["(SELECT COUNT(x) FROM App\\X x) AS cnt".to_string()];
    assert!(parse_select_items(&args).is_none());
}
