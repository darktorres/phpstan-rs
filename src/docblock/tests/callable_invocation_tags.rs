//! `@param-immediately-invoked-callable`/`@param-later-invoked-callable`/
//! `@param-closure-this` tags (Phase 36).
use super::super::*;

#[test]
fn parses_an_immediately_invoked_callable_tag() {
    let block = parse("/** @param-immediately-invoked-callable $cb */");
    assert_eq!(
        block.param_immediately_invoked_callable.get("cb"),
        Some(&true)
    );
}

#[test]
fn parses_a_later_invoked_callable_tag() {
    let block = parse("/** @param-later-invoked-callable $cb */");
    assert_eq!(
        block.param_immediately_invoked_callable.get("cb"),
        Some(&false)
    );
}

#[test]
fn a_later_tag_on_the_same_parameter_overwrites_an_earlier_immediately_tag() {
    let block = parse(
        "/** @param-immediately-invoked-callable $cb\n * @param-later-invoked-callable $cb */",
    );
    assert_eq!(
        block.param_immediately_invoked_callable.get("cb"),
        Some(&false)
    );
}

#[test]
fn the_phpstan_prefixed_spelling_is_recognized_identically() {
    let block = parse("/** @phpstan-param-immediately-invoked-callable $cb */");
    assert_eq!(
        block.param_immediately_invoked_callable.get("cb"),
        Some(&true)
    );
}

#[test]
fn parses_a_param_closure_this_tag() {
    let block = parse("/** @param-closure-this Foo $cb */");
    assert_eq!(block.param_closure_this.get("cb"), Some(&"Foo".to_string()));
}
