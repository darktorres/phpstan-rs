use super::super::*;
use crate::diagnostic::Diagnostic;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::Registry;

/// Families that fire on almost any minimal, deliberately untyped test
/// snippet (untyped declarations, discarded pure calls and expressions,
/// declared union members a trivial body never produces). The white-box
/// tests left here assert on other things, so `diagnostics_for` leaves
/// these out; `all_diagnostics_for` keeps everything.
fn is_incidental(d: &Diagnostic) -> bool {
    d.identifier.starts_with("missingType.")
        || matches!(
            d.identifier.as_str(),
            "function.resultUnused"
                | "method.resultUnused"
                | "staticMethod.resultUnused"
                | "new.resultUnused"
                | "expr.resultUnused"
                | "return.unusedType"
                | "property.unusedType"
                | "staticProperty.unusedType"
        )
}

pub(super) fn diagnostics_for(src: &str) -> Vec<Diagnostic> {
    diagnostics_for_with(src, |_| {})
}

pub(super) fn diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut Registry),
) -> Vec<Diagnostic> {
    let mut diagnostics = all_diagnostics_for_with(src, configure);
    diagnostics.retain(|d| !is_incidental(d));
    diagnostics
}

pub(super) fn all_diagnostics_for(src: &str) -> Vec<Diagnostic> {
    all_diagnostics_for_with(src, |_| {})
}

pub(super) fn all_diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut Registry),
) -> Vec<Diagnostic> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = Registry::build(&program);
    registry.merge_stubs(crate::php_stubs::generated_stubs());
    registry.merge_stub_classes(crate::php_stubs::generated_stub_classes());
    registry.merge_stub_constants(crate::php_stubs::generated_stub_constants());
    configure(&mut registry);
    // Merging stubs and `configure` can add classes and `extends` edges
    // after `Registry::build`'s own precompute ran.
    registry.precompute_ancestor_names();
    registry.precompute_member_owners();
    let mut checker = Checker::new(&registry);
    checker.check_program(&program);
    checker.into_diagnostics()
}
