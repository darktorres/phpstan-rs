//! The catalog call-result memo is local to a file and preserves distinct
//! flow-sensitive argument types at one source span.
use super::super::*;

fn checker() -> Checker<'static> {
    let tokens = syntax::lexer::Lexer::new("<?php").tokenize_with_spans();
    let program = syntax::parser::Parser::new(tokens).parse_program().unwrap();
    let registry = Box::leak(Box::new(Registry::build(
        &program.arenas,
        program.statements(),
    )));
    Checker::new(registry)
}

#[test]
fn same_span_keeps_distinct_argument_states_and_file_change_clears_them() {
    let mut checker = checker();
    let span = syntax::span::Span::default();
    let null_argument = vec![Type::null()];
    let int_argument = vec![Type::int_literal(7)];
    let null_result = Type::int_literal(0);
    let int_result = Type::int_range(Some(0), None);
    let null_evaluation = CatalogCallEvaluation {
        function: "strlen",
        return_type: null_result.clone(),
        diagnostics: vec![(
            php_semantics::DiagnosticKind::NullToInternalScalarParameterDeprecated,
            0,
        )],
        diagnostics_closed: true,
        invoked_string_arguments: vec![],
        php_throw_without_string_invocation: false,
        php_throw_invoked_string_arguments: vec![],
    };
    let int_evaluation = CatalogCallEvaluation {
        function: "strlen",
        return_type: int_result.clone(),
        diagnostics: vec![],
        diagnostics_closed: true,
        invoked_string_arguments: vec![0],
        php_throw_without_string_invocation: true,
        php_throw_invoked_string_arguments: vec![0],
    };

    checker.remember_catalog_return_type(span, null_argument.clone(), null_evaluation);
    checker.remember_catalog_return_type(span, int_argument.clone(), int_evaluation);

    assert_eq!(
        checker
            .cached_catalog_return_type(span, &null_argument)
            .map(|evaluation| evaluation.return_type),
        Some(null_result)
    );
    assert_eq!(
        checker
            .cached_catalog_return_type(span, &null_argument)
            .map(|evaluation| evaluation.diagnostics),
        Some(vec![(
            php_semantics::DiagnosticKind::NullToInternalScalarParameterDeprecated,
            0,
        )])
    );
    assert_eq!(
        checker
            .cached_catalog_return_type(span, &int_argument)
            .map(|evaluation| (evaluation.return_type, evaluation.diagnostics)),
        Some((int_result, vec![]))
    );
    assert_eq!(
        checker.catalog_string_arguments_at(span),
        Some(("strlen", vec![0]))
    );
    assert_eq!(
        checker.catalog_php_throw_invocation_at(span),
        Some(("strlen", true, vec![0]))
    );

    checker.set_current_file(std::path::Path::new("first.php"));
    assert!(
        checker
            .cached_catalog_return_type(span, &null_argument)
            .is_none()
    );
    assert!(
        checker
            .cached_catalog_return_type(span, &int_argument)
            .is_none()
    );
    assert!(checker.catalog_string_arguments_at(span).is_none());
    assert!(checker.catalog_php_throw_invocation_at(span).is_none());
}
