//! Sub-phase D of the live SQL query reflection phase (see
//! `delegated-tumbling-dongarra.md`) - end-to-end `assertType()`
//! coverage of the real `Doctrine\DBAL\Connection`/`Query\QueryBuilder`/
//! `Result` dispatch in `checker::expr::member_resolution`, through a
//! minimal DBAL class hierarchy declared directly in each fixture (this
//! project vendors no real `doctrine/dbal` stubs). No live database is
//! used here - every test configures a static schema catalog
//! (`sql_reflection_schema::parse_ddl`) directly on the `Registry`,
//! exercising resolution-order step 3; the live path (step 4) is
//! covered separately, end to end against a real database, by
//! `sql_reflection_reflector`'s own tests.

use super::helpers::*;

const STUBS: &str = r#"
    namespace Doctrine\DBAL {
        class QueryCacheProfile {}
        class Result {
            public function fetchAssociative(): array|false {}
            public function fetchAllAssociative(): array {}
            public function fetchOne(): mixed {}
            public function fetchFirstColumn(): array {}
            public function fetchNumeric(): array|false {}
            public function fetchAllNumeric(): array {}
            public function fetchAllKeyValue(): array {}
            public function fetchAllAssociativeIndexed(): array {}
            public function iterateAssociative(): \Traversable {}
            public function iterateNumeric(): \Traversable {}
            public function iterateKeyValue(): \Traversable {}
            public function iterateAssociativeIndexed(): \Traversable {}
            public function iterateColumn(): \Traversable {}
            public function rowCount(): int|string {}
            public function columnCount(): int {}
        }
        class Connection {
            public function fetchAssociative(string $query, array $params = [], array $types = []): array|false {}
            public function fetchAllAssociative(string $query, array $params = [], array $types = []): array {}
            public function fetchOne(string $query, array $params = [], array $types = []): mixed {}
            public function fetchFirstColumn(string $query, array $params = [], array $types = []): array {}
            public function fetchNumeric(string $query, array $params = [], array $types = []): array|false {}
            public function fetchAllNumeric(string $query, array $params = [], array $types = []): array {}
            public function fetchAllKeyValue(string $query, array $params = [], array $types = []): array {}
            public function fetchAllAssociativeIndexed(string $query, array $params = [], array $types = []): array {}
            public function iterateAssociative(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateNumeric(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateKeyValue(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateAssociativeIndexed(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateColumn(string $query, array $params = [], array $types = []): \Traversable {}
            public function executeQuery(string $sql, array $params = [], array $types = []): Result {}
            public function executeStatement(string $sql, array $params = [], array $types = []): int|string {}
            public function executeCacheQuery(string $sql, array $params, array $types, QueryCacheProfile $qcp): Result {}
            public function createQueryBuilder(): Query\QueryBuilder {}
        }
    }
    namespace Doctrine\DBAL\Query {
        class QueryBuilder {
            public function select(string ...$select): static {}
            public function addSelect(string ...$select): static {}
            public function from(string $table, ?string $alias = null): static {}
            public function where(string $predicate): static {}
            public function fetchAssociative(): array|false {}
            public function fetchAllAssociative(): array {}
            public function fetchOne(): mixed {}
            public function fetchFirstColumn(): array {}
            public function executeQuery(): \Doctrine\DBAL\Result {}
            public function executeStatement(): int|string {}
        }
    }
"#;

fn categories_schema() -> crate::sql_reflection_schema::SchemaCatalog {
    crate::sql_reflection_schema::parse_ddl(
        "CREATE TABLE categories (id int NOT NULL, name varchar(255) NOT NULL);",
        crate::sql_reflection_config::SqlDriver::Mysql,
    )
}

fn diagnostics_with_dbal_schema(src: &str) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for_with(&format!("<?php\n{STUBS}\n{src}"), |registry| {
        registry.sql_reflection_schema = Some(categories_schema());
    })
}

fn diagnostics_with_dbal_no_schema(src: &str) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for(&format!("<?php\n{STUBS}\n{src}"))
}

#[test]
fn connection_fetch_associative_with_a_literal_sql_string_resolves_the_real_row_shape() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $row = $c->fetchAssociative('SELECT id, name FROM categories');
            assertType('array{id: int, name: string}|false', $row);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn connection_fetch_all_associative_resolves_a_list_of_the_real_row_shape() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $rows = $c->fetchAllAssociative('SELECT id, name FROM categories');
            assertType('list<array{id: int, name: string}>', $rows);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn connection_fetch_one_resolves_the_first_selected_columns_own_type() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $id = $c->fetchOne('SELECT id, name FROM categories');
            assertType('int|false', $id);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn connection_fetch_first_column_resolves_a_list_of_the_first_selected_columns_own_type() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $ids = $c->fetchFirstColumn('SELECT id, name FROM categories');
            assertType('list<int>', $ids);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn query_builder_fetch_associative_resolves_the_chain_folded_sql() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $row = $c->createQueryBuilder()
                ->select('id', 'name')
                ->from('categories')
                ->fetchAssociative();
            assertType('array{id: int, name: string}|false', $row);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn execute_query_then_result_fetch_associative_resolves_via_the_two_hop_propagation() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $row = $c->executeQuery('SELECT id, name FROM categories')->fetchAssociative();
            assertType('array{id: int, name: string}|false', $row);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn query_builder_execute_query_then_result_fetch_all_associative_resolves_via_the_chain() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $rows = $c->createQueryBuilder()
                ->select('id', 'name')
                ->from('categories')
                ->executeQuery()
                ->fetchAllAssociative();
            assertType('list<array{id: int, name: string}>', $rows);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unfoldable_sql_argument_falls_back_to_todays_plain_stub_type_not_a_guess() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c, string $dynamicSql): void {
            $row = $c->fetchAssociative($dynamicSql);
            assertType('array|false', $row);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn no_schema_configured_at_all_falls_back_to_todays_plain_stub_type() {
    let d = diagnostics_with_dbal_no_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $row = $c->fetchAssociative('SELECT id, name FROM categories');
            assertType('array|false', $row);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn execute_statement_is_unaffected_still_a_plain_int_or_string() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $affected = $c->executeStatement('DELETE FROM categories WHERE id = 1');
            assertType('int|string', $affected);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn fetch_numeric_resolves_a_positionally_keyed_shape() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $row = $c->fetchNumeric('SELECT id, name FROM categories');
            assertType('array{0: int, 1: string}|false', $row);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn fetch_all_numeric_resolves_a_list_of_the_positionally_keyed_shape() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $rows = $c->fetchAllNumeric('SELECT id, name FROM categories');
            assertType('list<array{0: int, 1: string}>', $rows);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn fetch_all_key_value_keys_by_the_first_column_values_by_the_second() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $map = $c->fetchAllKeyValue('SELECT id, name FROM categories');
            assertType('array<int, string>', $map);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn fetch_all_associative_indexed_keys_by_the_first_column_values_by_the_rest() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $map = $c->fetchAllAssociativeIndexed('SELECT id, name FROM categories');
            assertType('array<int, array{name: string}>', $map);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn iterate_associative_resolves_a_traversable_of_the_row_shape_keyed_by_int() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $rows = $c->iterateAssociative('SELECT id, name FROM categories');
            assertType('Traversable<int, array{id: int, name: string}>', $rows);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn iterate_key_value_resolves_a_traversable_keyed_by_the_first_column() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $rows = $c->iterateKeyValue('SELECT id, name FROM categories');
            assertType('Traversable<int, string>', $rows);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn iterate_column_resolves_a_traversable_of_the_first_columns_own_type() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $rows = $c->iterateColumn('SELECT id, name FROM categories');
            assertType('Traversable<int, int>', $rows);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn execute_cache_query_then_result_fetch_associative_resolves_via_the_two_hop_propagation() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;
        use Doctrine\DBAL\QueryCacheProfile;

        function f(Connection $c, QueryCacheProfile $qcp): void {
            $row = $c->executeCacheQuery('SELECT id, name FROM categories', [], [], $qcp)->fetchAssociative();
            assertType('array{id: int, name: string}|false', $row);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn result_row_count_and_column_count_are_unaffected_by_this_phase() {
    let d = diagnostics_with_dbal_schema(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $result = $c->executeQuery('SELECT id, name FROM categories');
            assertType('int|string', $result->rowCount());
            assertType('int', $result->columnCount());
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
