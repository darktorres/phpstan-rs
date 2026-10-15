//! A class's templates as the checker marks them in the class's own PHPDoc.
use super::super::*;

#[test]
fn a_class_template_carries_its_bound_variance_default_and_scope() {
    let tokens = syntax::lexer::Lexer::new(
        "<?php /** @template-covariant T of \\Countable = \\ArrayObject\n * @template U\n */ \
         class Box {}",
    )
    .tokenize_with_spans();
    let program = syntax::parser::Parser::new(tokens).parse_program().unwrap();
    let registry = Registry::build(&program);
    let checker = Checker::new(&registry);
    let params = checker.class_template_params_for("Box");
    let t = &params["T"];
    assert_eq!(t.variance, types::Variance::Covariant);
    assert_eq!(t.scope.as_deref(), Some("Box"));
    assert_eq!(t.bound.to_string(), "Countable");
    assert_eq!(
        t.default.as_ref().map(|d| d.to_string()),
        Some("ArrayObject".to_string())
    );
    let u = &params["U"];
    assert_eq!(u.variance, types::Variance::Invariant);
    assert!(u.bound.is_explicit_mixed());
    assert!(u.default.is_none());
}
