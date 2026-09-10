//! Phase 36: `@param-immediately-invoked-callable`/`@param-later-invoked-callable`/
//! `@param-closure-this` declaration-time validation
//! (`checker::noop::check_declared_param_callable_tags`).

use super::helpers::*;

#[test]
fn a_param_immediately_invoked_callable_tag_referencing_unknown_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-immediately-invoked-callable $bogus */
            function f(callable $cb): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_later_invoked_callable_tag_referencing_unknown_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-later-invoked-callable $bogus */
            function f(callable $cb): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_immediately_invoked_callable_tag_on_an_int_typed_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-immediately-invoked-callable $cb */
            function f(int $cb): void {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "paramImmediatelyInvokedCallable.nonCallable"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_later_invoked_callable_tag_on_an_int_typed_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-later-invoked-callable $cb */
            function f(int $cb): void {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "paramLaterInvokedCallable.nonCallable"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_immediately_invoked_callable_tag_on_a_string_typed_param_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-immediately-invoked-callable $cb */
            function f(string $cb): void {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "paramImmediatelyInvokedCallable.nonCallable"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_immediately_invoked_callable_tag_on_an_untyped_param_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-immediately-invoked-callable $cb */
            function f($cb): void {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "paramImmediatelyInvokedCallable.nonCallable"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_immediately_invoked_callable_tag_on_a_nullable_callable_param_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-immediately-invoked-callable $cb */
            function f(?callable $cb): void {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "paramImmediatelyInvokedCallable.nonCallable"),
        "got: {d:?}"
    );
}

#[test]
fn a_phpstan_prefixed_spelling_is_recognized_identically_to_the_bare_form() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-param-immediately-invoked-callable $cb */
            function f(int $cb): void {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "paramImmediatelyInvokedCallable.nonCallable"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_closure_this_tag_referencing_unknown_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param-closure-this Foo $bogus */
            function f(Closure $cb): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_closure_this_tag_on_an_int_typed_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param-closure-this Foo $cb */
            function f(int $cb): void {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "paramClosureThis.nonClosure"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_closure_this_tag_on_a_closure_typed_param_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param-closure-this Foo $cb */
            function f(Closure $cb): void {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "paramClosureThis.nonClosure"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_closure_this_tag_on_an_untyped_param_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param-closure-this Foo $cb */
            function f($cb): void {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "paramClosureThis.nonClosure"),
        "got: {d:?}"
    );
}
