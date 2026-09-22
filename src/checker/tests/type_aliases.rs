//! Tests for `@phpstan-type Name TypeExpr` local class type aliases,
//! resolved into every referencing member's own type
//! (`registry::function_sig::build_type_aliases`/`substitute_type_aliases`),
//! and its cross-class sibling `@phpstan-import-type Name from Class [as
//! Alias]` (`Registry::resolve_deferred_type_imports`).
use super::helpers::*;

#[test]
fn a_return_type_referencing_a_local_alias_is_expanded() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type Error array{code: int, message: string}
             */
            class ErrorBag {
                /**
                 * @return Error
                 */
                public function firstError(): array {
                    return ['code' => 1, 'message' => 'oops'];
                }
            }
            function run(ErrorBag $bag): void {
                $error = $bag->firstError();
                $code = $error['code'];
                $message = $error['message'];
                $missing = $error['nope'];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.notFound");
}

#[test]
fn an_alias_referencing_a_sibling_alias_is_expanded_one_level() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type Error array{code: int, message: string}
             * @phpstan-type ErrorList list<Error>
             */
            class ErrorBag {
                /**
                 * @return ErrorList
                 */
                public function errors(): array {
                    return [];
                }
            }
            function needsInt(int $x): void {}
            function run(ErrorBag $bag): void {
                foreach ($bag->errors() as $error) {
                    needsInt($error['code']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_alias_referencing_a_sibling_alias_still_enforces_the_inner_shape() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type Error array{code: int, message: string}
             * @phpstan-type ErrorList list<Error>
             */
            class ErrorBag {
                /**
                 * @return ErrorList
                 */
                public function errors(): array {
                    return [];
                }
            }
            function needsInt(int $x): void {}
            function run(ErrorBag $bag): void {
                foreach ($bag->errors() as $error) {
                    needsInt($error['message']);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn a_param_referencing_a_local_alias_is_enforced() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type Options array{verbose: bool}
             */
            class Runner {
                /**
                 * @param Options $options
                 */
                public function run(array $options): void {}
            }
            function run(Runner $r): void {
                $r->run(['verbose' => 'yes']);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn an_inline_var_annotation_referencing_a_local_alias_is_expanded() {
    // `/** @var ComponentMap $components */` on a local variable is a
    // separate, check-time resolution path from the registry-build-time
    // member handling the other tests here cover.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type ComponentMap array{scheme: ?string, host: ?string}
             */
            class UriString {
                public function parse(): void {
                    /** @var ComponentMap $components */
                    $components = ['scheme' => null, 'host' => null];
                    $missing = $components['nope'];
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.notFound");
}

#[test]
fn an_alias_narrowed_override_is_compared_by_its_native_type_not_the_expanded_alias() {
    // `ParamSig::ty_native` (used by the override-compatibility check,
    // see oop.rs's own doc comment on why it's native-only) must stay
    // the plain `Node` native hint on both sides here, unaffected by
    // `SpecificNode`'s own alias expansion into an unrelated array shape
    // - if alias substitution ever leaked into `ty_native`, this would
    // wrongly flag `SpecificNode` as an incompatible override of `Node`.
    let d = diagnostics_for(
        r#"<?php
            interface Node {}
            interface Rule {
                public function process(Node $node): void;
            }
            /**
             * @phpstan-type SpecificNode array{kind: string}
             */
            class MyRule implements Rule {
                /**
                 * @param SpecificNode $node
                 */
                public function process(Node $node): void {}
            }
        "#,
    );
    // Only the alias's own disagreement with the native hint is
    // reported, not an override incompatibility.
    let ids: Vec<&str> = d.iter().map(|x| x.identifier.as_str()).collect();
    assert_eq!(ids, ["parameter.phpDocType"], "got: {d:?}");
}

#[test]
fn a_type_imported_from_another_class_is_expanded_the_same_as_a_local_alias() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type Error array{code: int, message: string}
             */
            class Types {}

            /**
             * @phpstan-import-type Error from Types
             */
            class ErrorBag {
                /**
                 * @return Error
                 */
                public function firstError(): array {
                    return ['code' => 1, 'message' => 'oops'];
                }
            }
            function run(ErrorBag $bag): void {
                $error = $bag->firstError();
                $missing = $error['nope'];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.notFound");
}

#[test]
fn an_import_can_rename_the_alias_via_as() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type Error array{code: int, message: string}
             */
            class Types {}

            /**
             * @phpstan-import-type Error from Types as ImportedError
             */
            class ErrorBag {
                /**
                 * @return ImportedError
                 */
                public function firstError(): array {
                    return ['code' => 1, 'message' => 'oops'];
                }
            }
            function run(ErrorBag $bag): void {
                $error = $bag->firstError();
                $missing = $error['nope'];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.notFound");
}

#[test]
fn an_import_resolves_correctly_across_namespaces() {
    let d = diagnostics_for(
        r#"<?php
            namespace App\Source;

            /** @phpstan-type Error array{code: int, message: string} */
            class Types {}

            namespace App\Consumer;

            use App\Source\Types;

            /** @phpstan-import-type Error from Types as ImportedError */
            class ErrorBag {
                /** @return ImportedError */
                public function firstError(): array {
                    return ['code' => 1, 'message' => 'oops'];
                }
            }
            function run(ErrorBag $bag): void {
                $error = $bag->firstError();
                $missing = $error['nope'];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.notFound");
}

#[test]
fn an_import_from_a_class_with_no_such_alias_is_left_unresolved_not_guessed() {
    let d = diagnostics_for(
        r#"<?php
            class Types {}

            /**
             * @phpstan-import-type Nonexistent from Types
             */
            class ErrorBag {
                /**
                 * @return Nonexistent
                 */
                public function firstError(): array {
                    return [];
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "expected the unresolved alias to surface as an ordinary unknown-class reference, got: {d:?}"
    );
}
