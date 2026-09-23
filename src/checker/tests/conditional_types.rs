//! PHPDoc conditional return types (`@return ($param is X ? A : B)`) -
//! `Type::Conditional`, `types::resolve_conditional`.

use super::helpers::*;

#[test]
fn a_conditional_return_type_picks_the_true_branch_for_a_matching_argument() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return $x; }
            assertType('string', f(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_conditional_return_type_picks_the_false_branch_for_a_non_matching_argument() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return $x; }
            assertType('bool', f('hello'));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_negated_conditional_return_type_inverts_the_branch_choice() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is not int ? string : bool)
             */
            function f($x) { return $x; }
            assertType('bool', f(5));
            assertType('string', f('hello'));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_conditional_return_type_with_an_untyped_argument_falls_back_to_the_union_of_both_branches() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return $x; }
            /** @var mixed $unknown */
            $unknown = null;
            assertType('string|bool', f($unknown));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_template_referenced_inside_a_conditional_branch_is_substituted_first() {
    // `infer_call_return_type` runs `substitute_template` before
    // `resolve_conditional` - a template used inside one of a
    // conditional's own branches (a real shape, Doctrine DBAL's own
    // `DriverManager::getConnection()`) must already be bound by the
    // time the branch is picked.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T
             * @param T $x
             * @return ($x is int ? T : string)
             */
            function f($x) { return $x; }
            assertType('5', f(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_class_reference_inside_a_conditional_branch_resolves_correctly_in_a_namespaced_file() {
    // Doctrine DBAL's own `DriverManager::getConnection()` real shape -
    // exercises `resolve_names_in_type`'s new `Conditional` arm (`Connection`
    // must resolve to its real, namespace-qualified FQN, not stay bare).
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            use function PHPStan\Testing\assertType;
            final class Connection {}
            /**
             * @param mixed $x
             * @return ($x is int ? Connection : string)
             */
            function f($x) { return $x; }
            assertType('App\Connection', f(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Composer's `PackageSorter::getMostCurrentVersion` shape: a template-
// typed conditional return reconciled with a nullable native hint.
const TEMPLATE_CONDITIONAL_SRC: &str = r#"<?php
    use function PHPStan\Testing\assertType;
    interface Pkg {}
    interface CompletePkg extends Pkg {}
    class Sorter {
        /** @template T of Pkg
         *  @param array<T> $packages
         *  @return ($packages is non-empty-array<T> ? T : T|null) */
        public static function most(array $packages): ?Pkg { return null; }
    }
    interface Repo {
        /** @return Pkg[] */ public function find(): array;
        /** @return non-empty-array<Pkg> */ public function findSome(): array;
        /** @return array<CompletePkg> */ public function findComplete(): array;
    }
    function f(Repo $r): void {
        assertType('Pkg|null', Sorter::most($r->find()));
        assertType('Pkg', Sorter::most($r->findSome()));
        assertType('CompletePkg|null', Sorter::most($r->findComplete()));
    }
"#;

#[test]
fn a_template_conditional_return_with_a_nullable_native_hint_resolves_per_call() {
    let d = diagnostics_for(TEMPLATE_CONDITIONAL_SRC);
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn an_undecidable_conditional_subject_keeps_both_branches() {
    // A possibly-empty `Pkg[]` is neither a subtype of nor disjoint from
    // `non-empty-array<T>` - real PHPStan's `maybe`, so both branches.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            interface Pkg {}
            class S {
                /** @param array<Pkg> $p
                 *  @return ($p is non-empty-array ? int : string) */
                public static function f(array $p) { return 1; }
                /** @param int|string $p
                 *  @return ($p is array ? int : string) */
                public static function g($p) { return ''; }
            }
            /** @return Pkg[] */ function find(): array { return []; }
            function t(): void {
                assertType('int|string', S::f(find()));
                assertType('string', S::g(1));
            }
        "#,
    );
    // `g`'s own condition can never hold for its `int|string` parameter -
    // reported as such, which isn't what this test is about.
    let d: Vec<_> = d
        .into_iter()
        .filter(|x| x.identifier != "conditionalType.alwaysFalse")
        .collect();
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Real Piwigo shapes, from a corpus-verification pass.

#[test]
fn a_distinct_literal_argument_picks_the_false_branch_and_an_omitted_one_uses_its_default() {
    // `NotificationByMailSender::sendMailNotifications`'s own shape.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Notification {}
            class Sender {
                /** @return ($action is 'send' ? list<string> : list<Notification>) */
                public function send(string $action = 'list_to_send'): array { return []; }
            }
            function f(Sender $s): void {
                assertType('list<Notification>', $s->send('list_to_send'));
                assertType('list<Notification>', $s->send());
                assertType('list<string>', $s->send('send'));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn an_uninferred_template_in_a_conditional_branch_resolves_to_its_bound() {
    // Doctrine DBAL's `DriverManager::getConnection`, called without a
    // `wrapperClass` - real PHPStan's `resolveToBounds` gives `Connection`.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Connection {}
            class DriverManager {
                /**
                 * @param array<string, mixed> $params
                 * @template T of Connection
                 * @return ($params is array{wrapperClass: class-string<T>} ? T : Connection)
                 */
                public static function getConnection(array $params): Connection { return new Connection(); }
            }
            /** @param array<string, mixed> $params */
            function f(array $params): void {
                assertType('Connection', DriverManager::getConnection($params));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn a_template_subject_conditional_resolves_once_the_template_is_bound() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T
             * @param T $value
             * @return (T is int ? 'int' : 'other')
             */
            function kind($value): string { return is_int($value) ? 'int' : 'other'; }
            function f(): void {
                assertType("'int'", kind(5));
                assertType("'other'", kind('x'));
                assertType("'int'|'other'", kind(rand() ? 1 : 'x'));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn conditional_return_declarations_are_validated() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return ($missing is int ? int : string)
             */
            function a(int $x): int|string { return 1; }
            /**
             * @return (Foo is int ? int : string)
             */
            function b(int $x): int|string { return 1; }
            /**
             * @return ($x is int ? int : string)
             */
            function c(int $x): int|string { return 1; }
            /**
             * @return ($x is not string ? int : string)
             */
            function d(int $x): int|string { return 1; }
            /**
             * @return ($x is int ? int : string)
             */
            function e(int|string $x): int|string { return 1; }
        "#,
    );
    let m: Vec<(&str, &str)> = d
        .iter()
        .filter(|x| {
            x.identifier.starts_with("conditionalType.") || x.identifier == "parameter.notFound"
        })
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect();
    assert_eq!(
        m,
        [
            (
                "parameter.notFound",
                "Conditional return type references unknown parameter $missing."
            ),
            (
                "conditionalType.subjectNotFound",
                "Conditional return type uses subject type Foo which is not part of PHPDoc @template tags."
            ),
            (
                "conditionalType.alwaysTrue",
                "Condition \"int is int\" in conditional return type is always true."
            ),
            (
                "conditionalType.alwaysTrue",
                "Condition \"int is not string\" in conditional return type is always true."
            ),
        ]
    );
}

#[test]
fn a_class_template_conditional_resolves_through_the_template_default() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class ArrayNode {}
            class ScalarNode {}
            /** @template T of 'array'|'scalar' = 'array' */
            class Builder {
                /** @return (T is 'array' ? ArrayNode : ScalarNode) */
                public function root(): object { return new ArrayNode(); }
            }
            /** @param Builder<'scalar'> $s */
            function f(Builder $s): void {
                assertType('ArrayNode', (new Builder())->root());
                assertType('ScalarNode', $s->root());
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}
