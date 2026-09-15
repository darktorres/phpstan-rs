//! `yield`/`yield from` expression typing - resolving the enclosing
//! declared `Generator<TKey, TValue, TSend, TReturn>`'s own TSend/TReturn
//! template params, instead of always inferring `Mixed`.

use super::helpers::*;

#[test]
fn yield_expression_type_is_the_declared_generator_tsend() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string, bool, void> */
            function f(): \Generator {
                assertType('bool', yield 'x');
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn yield_expression_type_is_mixed_when_declared_return_type_has_no_tsend_info() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string> */
            function f(): \Generator {
                assertType('mixed', yield 'x');
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn yield_from_expression_type_is_the_delegated_generators_treturn() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string, mixed, bool> */
            function inner(): \Generator {
                yield 'x';
                return true;
            }
            function f(): \Generator {
                assertType('bool', yield from inner());
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn yield_from_expression_type_is_mixed_for_a_plain_array_argument() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): \Generator {
                assertType('mixed', yield from [1, 2, 3]);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// `return $x;` inside a generator must be checked against TReturn, not
// the whole declared `Generator<...>` type (which would always be a
// mismatch).
#[test]
fn returning_a_value_inside_a_generator_checks_against_its_treturn_not_the_whole_generator_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string, mixed, bool> */
            function f(): \Generator {
                yield 'x';
                return true;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.type"),
        "got: {d:?}"
    );
}

#[test]
fn returning_a_mismatched_value_inside_a_generator_is_still_flagged_against_its_treturn() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string, mixed, bool> */
            function f(): \Generator {
                yield 'x';
                return "not a bool";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "got: {d:?}"
    );
}

// Generator `TReturn` completeness - falling off the end (or an
// explicit bare `return;`) inside a generator with a non-Mixed/Void
// declared TReturn is a real miss, checked against TReturn, not the
// outer `Generator<...>` type.

#[test]
fn generator_function_with_concrete_treturn_and_no_return_statement_gets_return_missing() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string, mixed, bool> */
            function f(): \Generator {
                yield 'x';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn generator_function_with_mixed_treturn_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string> */
            function f(): \Generator {
                yield 'x';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn generator_function_with_void_treturn_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string, mixed, void> */
            function f(): \Generator {
                yield 'x';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn generator_function_declared_bare_generator_with_no_type_args_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): \Generator {
                yield 'x';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn generator_function_with_concrete_treturn_and_a_bare_return_gets_return_type_mismatch() {
    // Not `return.missing`: a bare `return;` is a real, explicit
    // statement that unconditionally sets `Generator::getReturn()` to
    // `null` (the same real PHP semantics an ordinary function's own
    // bare `return;` has) - for a non-nullable, concrete TReturn like
    // `bool`, that's a genuine wrong-value mismatch (`return.type`,
    // "expected bool, got null"), the same category `check_stmt`'s own
    // `Return(Some(e))` arm already reports for an explicit-but-wrong
    // value. `return.missing` is reserved for a path that never reaches
    // any `return` statement at all.
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string, mixed, bool> */
            function f(): \Generator {
                yield 'x';
                return;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"
            && x.message.contains("expected bool, got null")),
        "got: {d:?}"
    );
}

// `yield from` compatibility checks.

#[test]
fn yield_from_a_definitely_non_iterable_value_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): \Generator {
                yield from 5;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.nonIterable"),
        "got: {d:?}"
    );
}

#[test]
fn yield_from_an_array_is_never_flagged_as_non_iterable() {
    let d = diagnostics_for(
        r#"<?php
            function f(): \Generator {
                yield from [1, 2, 3];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.nonIterable"),
        "got: {d:?}"
    );
}

#[test]
fn yield_from_key_type_mismatch_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<string, mixed> */
            function inner(): \Generator {
                yield 1 => 'x';
            }
            /** @return \Generator<int, mixed> */
            function f(): \Generator {
                yield from inner();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.keyType"),
        "got: {d:?}"
    );
}

#[test]
fn yield_from_value_type_mismatch_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, string> */
            function inner(): \Generator {
                yield 'x';
            }
            /** @return \Generator<int, int> */
            function f(): \Generator {
                yield from inner();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.valueType"),
        "got: {d:?}"
    );
}

#[test]
fn yield_from_send_type_mismatch_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, mixed, int> */
            function inner(): \Generator {
                yield 1;
            }
            /** @return \Generator<int, mixed, string> */
            function f(): \Generator {
                yield from inner();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.sendType"),
        "got: {d:?}"
    );
}

#[test]
fn yield_from_result_used_when_delegated_treturn_is_void_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, mixed, mixed, void> */
            function inner(): \Generator {
                yield 1;
            }
            /** @return \Generator<int, mixed> */
            function f(): \Generator {
                assertType('void', yield from inner());
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.void"),
        "got: {d:?}"
    );
}

#[test]
fn yield_from_result_discarded_as_a_bare_statement_is_not_flagged_even_when_void() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<int, mixed, mixed, void> */
            function inner(): \Generator {
                yield 1;
            }
            /** @return \Generator<int, mixed> */
            function f(): \Generator {
                yield from inner();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.void"),
        "got: {d:?}"
    );
}

#[test]
fn yield_from_is_not_flagged_when_enclosing_return_type_is_mixed() {
    let d = diagnostics_for(
        r#"<?php
            /** @return \Generator<string, string> */
            function inner(): \Generator {
                yield 'a' => 'x';
            }
            function f(): \Generator {
                yield from inner();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "generator.keyType" | "generator.valueType" | "generator.sendType" | "generator.void"
        )),
        "got: {d:?}"
    );
}

#[test]
fn yield_expression_type_is_mixed_inside_a_non_generic_bare_generator_declaration() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): \Generator {
                assertType('mixed', yield 'x');
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// A closure/arrow-fn literal's own declared return type (native hint
// only - no docblock merge exists for a closure literal in this project)
// still saves/restores `current_declared_return_type` correctly, so a
// generator closure with no useful generic precision degrades to `mixed`
// rather than leaking whatever the *enclosing* function's own declared
// return type happened to be.
#[test]
fn yield_expression_type_inside_a_generator_closure_literal_does_not_leak_the_enclosing_functions_declared_type()
 {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @return \Generator<int, string, bool, void> */
            function outer(): \Generator {
                $cb = function (): \Generator {
                    assertType('mixed', yield 'x');
                };
                assertType('bool', yield 'x');
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}
