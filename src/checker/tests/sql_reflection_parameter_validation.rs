//! Sub-phase E of the live SQL query reflection phase (see
//! `delegated-tumbling-dongarra.md`) - end-to-end coverage of
//! `checker::sql_reflection::parameter_validation`'s two diagnostics
//! (`sqlReflection.missingParameter`/`sqlReflection.invalidParameterType`),
//! through the same minimal DBAL class hierarchy
//! `sql_reflection_dispatch.rs` already declares (real DBAL isn't
//! vendored as a stub here either). No live database or static schema
//! catalog is needed for any of these - this check only ever compares
//! the resolved SQL text's own placeholders against the PHP-side
//! bindings, never a column type.

use super::helpers::*;

const STUBS: &str = r#"
    namespace Doctrine\DBAL {
        enum ArrayParameterType {
            case INTEGER;
            case STRING;
        }
        class Connection {
            public function fetchAssociative(string $query, array $params = [], array $types = []): array|false {}
            public function executeQuery(string $sql, array $params = [], array $types = []): mixed {}
            public function executeStatement(string $sql, array $params = [], array $types = []): int|string {}
            public function createQueryBuilder(): Query\QueryBuilder {}
        }
    }
    namespace Doctrine\DBAL\Query {
        class QueryBuilder {
            public function select(string ...$select): static {}
            public function from(string $table, ?string $alias = null): static {}
            public function where(string $predicate): static {}
            public function setParameter(int|string $key, mixed $value, mixed $type = null): static {}
            public function setParameters(array $params, array $types = []): static {}
            public function fetchAssociative(): array|false {}
        }
    }
"#;

fn diagnostics_with_dbal_stubs(src: &str) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for(&format!("<?php\n{STUBS}\n{src}"))
}

#[test]
fn a_query_builder_chain_with_every_placeholder_bound_has_no_diagnostics() {
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c, string $status): void {
            $c->createQueryBuilder()
                ->select('id')
                ->from('categories')
                ->where('status = :status')
                ->setParameter('status', $status)
                ->fetchAssociative();
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_query_builder_chain_missing_a_bound_value_for_a_real_placeholder_is_flagged() {
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $c->createQueryBuilder()
                ->select('id')
                ->from('categories')
                ->where('status = :status')
                ->fetchAssociative();
        }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "sqlReflection.missingParameter"),
        "expected a missingParameter diagnostic, got: {d:?}"
    );
}

#[test]
fn binding_an_array_without_arrayparametertype_is_flagged() {
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c, array $ids): void {
            $c->createQueryBuilder()
                ->select('id')
                ->from('categories')
                ->where('id IN (:ids)')
                ->setParameter('ids', $ids)
                ->fetchAssociative();
        }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "sqlReflection.invalidParameterType"),
        "expected an invalidParameterType diagnostic, got: {d:?}"
    );
}

#[test]
fn binding_an_array_with_arrayparametertype_is_the_real_documented_pattern_and_is_not_flagged() {
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;
        use Doctrine\DBAL\ArrayParameterType;

        function f(Connection $c, array $ids): void {
            $c->createQueryBuilder()
                ->select('id')
                ->from('categories')
                ->where('id IN (:ids)')
                ->setParameter('ids', $ids, ArrayParameterType::INTEGER)
                ->fetchAssociative();
        }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "sqlReflection.invalidParameterType"),
        "did not expect an invalidParameterType diagnostic, got: {d:?}"
    );
}

#[test]
fn connections_own_literal_sql_with_a_matching_params_array_has_no_diagnostics() {
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c, int $id): void {
            $c->fetchAssociative('SELECT * FROM categories WHERE id = ?', [$id]);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn connections_own_literal_sql_missing_a_positional_value_is_flagged() {
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $c->fetchAssociative('SELECT * FROM categories WHERE id = ? AND status = ?', [1]);
        }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "sqlReflection.missingParameter"),
        "expected a missingParameter diagnostic, got: {d:?}"
    );
}

#[test]
fn a_dynamic_sql_argument_is_undecidable_and_silently_skipped() {
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c, string $dynamicSql): void {
            $c->fetchAssociative($dynamicSql, []);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_params_argument_that_is_a_plain_variable_not_a_literal_array_is_undecidable_and_skipped() {
    // Real regression: `array $ids` is genuinely unknowable at the call
    // site as a literal array, so this must skip the whole check, not
    // treat it as "zero bindings" and flag every real placeholder as
    // missing - confirmed a real false positive against the corpus
    // before this test existed (`Connection::executeStatement('DELETE
    // FROM images WHERE id IN (?, ?)', $newIds)`-shaped real Piwigo
    // code).
    let d = diagnostics_with_dbal_stubs(
        r#"
        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c, array $ids): void {
            $c->executeStatement('DELETE FROM images WHERE id IN (?, ?)', $ids);
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
