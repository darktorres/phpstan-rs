//! Direct unit tests for `DbalQueryBuilderChain::render()` and its own
//! free-standing helpers - the SQL-text-generation logic itself, which
//! needs no `Checker`/`Locals` at all. The chain-*walking* half
//! (`resolve_dbal_query_builder_sql`, an AST backward-walk needing a
//! real `Checker`) mirrors the DQL side's
//! `resolve_query_builder_chain` mechanism closely enough that it's left
//! to the call sites' end-to-end `#[PHPStan\Testing\assertType]` tests
//! once a real call site actually surfaces its result - this project's
//! own established convention for checker-level logic (see
//! `checker/tests/doctrine_query_result.rs`), not a standalone Rust
//! harness for a bare `Checker`.

use super::*;

#[test]
fn renders_a_simple_select() {
    let chain = DbalQueryBuilderChain {
        select: vec!["id".to_string(), "name".to_string()],
        from: vec![FromClause {
            table: "users".to_string(),
            alias: None,
        }],
        ..Default::default()
    };
    assert_eq!(
        chain.render().as_deref(),
        Some("SELECT id, name FROM users")
    );
}

#[test]
fn renders_distinct_and_a_real_alias() {
    let chain = DbalQueryBuilderChain {
        distinct: true,
        select: vec!["u.id".to_string()],
        from: vec![FromClause {
            table: "users".to_string(),
            alias: Some("u".to_string()),
        }],
        ..Default::default()
    };
    assert_eq!(
        chain.render().as_deref(),
        Some("SELECT DISTINCT u.id FROM users u")
    );
}

#[test]
fn an_alias_identical_to_the_table_name_is_not_rendered() {
    let chain = DbalQueryBuilderChain {
        select: vec!["id".to_string()],
        from: vec![FromClause {
            table: "users".to_string(),
            alias: Some("users".to_string()),
        }],
        ..Default::default()
    };
    assert_eq!(chain.render().as_deref(), Some("SELECT id FROM users"));
}

#[test]
fn renders_joins_where_group_by_having_order_by_limit_and_offset() {
    let chain = DbalQueryBuilderChain {
        select: vec!["u.id".to_string()],
        from: vec![FromClause {
            table: "users".to_string(),
            alias: Some("u".to_string()),
        }],
        joins: vec![
            JoinClause {
                kind: JoinKind::Inner,
                table: "profiles".to_string(),
                alias: "p".to_string(),
                condition: Some("p.user_id = u.id".to_string()),
            },
            JoinClause {
                kind: JoinKind::Left,
                table: "avatars".to_string(),
                alias: "a".to_string(),
                condition: None,
            },
        ],
        where_parts: vec![
            (false, "u.active = 1".to_string()),
            (true, "u.is_admin = 1".to_string()),
        ],
        group_by: vec!["u.id".to_string()],
        having_parts: vec![(false, "COUNT(*) > 1".to_string())],
        order_by: vec!["u.id DESC".to_string()],
        limit: Some(10),
        offset: Some(5),
        ..Default::default()
    };
    assert_eq!(
        chain.render().as_deref(),
        Some(
            "SELECT u.id FROM users u JOIN profiles p ON p.user_id = u.id LEFT JOIN avatars a \
             WHERE u.active = 1 OR u.is_admin = 1 GROUP BY u.id HAVING COUNT(*) > 1 \
             ORDER BY u.id DESC LIMIT 10 OFFSET 5"
        )
    );
}

#[test]
fn a_select_chain_with_no_select_or_from_is_undecidable() {
    assert_eq!(DbalQueryBuilderChain::default().render(), None);
    let only_select = DbalQueryBuilderChain {
        select: vec!["id".to_string()],
        ..Default::default()
    };
    assert_eq!(only_select.render(), None);
}

#[test]
fn renders_an_insert() {
    let chain = DbalQueryBuilderChain {
        dml: Some(Dml::Insert {
            table: "users".to_string(),
            values: vec![
                ("name".to_string(), "?".to_string()),
                ("email".to_string(), "?".to_string()),
            ],
        }),
        ..Default::default()
    };
    assert_eq!(
        chain.render().as_deref(),
        Some("INSERT INTO users (name, email) VALUES(?, ?)")
    );
}

#[test]
fn renders_an_update_with_a_where_clause() {
    let chain = DbalQueryBuilderChain {
        dml: Some(Dml::Update {
            table: "users".to_string(),
            set: vec![("name".to_string(), "?".to_string())],
        }),
        where_parts: vec![(false, "id = ?".to_string())],
        ..Default::default()
    };
    assert_eq!(
        chain.render().as_deref(),
        Some("UPDATE users SET name = ? WHERE id = ?")
    );
}

#[test]
fn renders_a_delete_without_a_where_clause() {
    let chain = DbalQueryBuilderChain {
        dml: Some(Dml::Delete {
            table: "users".to_string(),
        }),
        ..Default::default()
    };
    assert_eq!(chain.render().as_deref(), Some("DELETE FROM users"));
}

#[test]
fn combine_predicate_parts_joins_with_the_right_keyword_per_entry() {
    assert_eq!(combine_predicate_parts(&[]), None);
    assert_eq!(
        combine_predicate_parts(&[(false, "a = 1".to_string())]),
        Some("a = 1".to_string())
    );
    assert_eq!(
        combine_predicate_parts(&[
            (false, "a = 1".to_string()),
            (false, "b = 2".to_string()),
            (true, "c = 3".to_string()),
        ]),
        Some("a = 1 AND b = 2 OR c = 3".to_string())
    );
}

#[test]
fn order_by_entry_appends_a_direction_only_when_given() {
    assert_eq!(order_by_entry("id".to_string(), None), "id");
    assert_eq!(
        order_by_entry("id".to_string(), Some("DESC".to_string())),
        "id DESC"
    );
}

#[test]
fn join_kind_keywords_match_real_sql() {
    assert_eq!(JoinKind::Inner.keyword(), "JOIN");
    assert_eq!(JoinKind::Left.keyword(), "LEFT JOIN");
    assert_eq!(JoinKind::Right.keyword(), "RIGHT JOIN");
}
