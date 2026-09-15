use super::super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::Registry;

/// `missingType.*` (this project's `MissingTypehintCheck` family -
/// untyped params/properties/returns, bare `array`/`iterable`,
/// non-generic use of a generic class) fires on almost any declaration
/// that isn't fully, precisely typed - which describes the overwhelming
/// majority of this whole suite's own minimal, deliberately-untyped
/// fixtures, none of which were written with this family in mind.
/// PHPStan's own test suite never hits this problem, since
/// `RuleTestCase` instantiates exactly one rule at a time; this
/// project's `Checker` runs every check unconditionally (see
/// `checker::Diagnostic::level`'s own doc comment - level is metadata
/// for the CLI's own post-hoc `--level` filtering, not something that
/// gates whether a check runs at all), so
/// `diagnostics_for`/`diagnostics_for_with` filtering this one family
/// out by default is what keeps every pre-existing test's own
/// "diagnostics_for returns exactly what this fixture is actually
/// testing" assumption true, without touching any of those tests' own
/// content. CLI usage is unaffected either way - `main.rs`'s own
/// default (`unwrap_or(MAX_LEVEL)`) already surfaces every level-6
/// diagnostic, `missingType.*` included, with no equivalent test-only
/// exclusion. A test that specifically wants to see this family
/// (`checker/tests/missing_typehint.rs`) uses `all_diagnostics_for`
/// instead.
fn is_missing_typehint_diagnostic(d: &Diagnostic) -> bool {
    d.identifier.starts_with("missingType.")
}

/// The transitive-purity payoff (`checker::noop::
/// check_pure_call_without_effect`) fires on a bare, discarded call to
/// any declaration this project's own registry can prove effect-free -
/// which, unlike the family it sits alongside
/// (`logicalXor.resultUnused`/`booleanAnd.resultUnused`/
/// `booleanOr.resultUnused`/`ternary.resultUnused`, deliberately left
/// unfiltered here - `checker/tests/noop.rs` relies on seeing those by
/// default), describes an enormous number of this whole suite's own
/// tiny helper functions/methods, called as a bare statement purely as
/// test setup incidental to whatever each fixture actually means to
/// exercise (the identical `missingType.*` problem above, for a
/// different diagnostic family). A test that specifically wants to see
/// this one (`checker/tests/purity.rs`) uses `all_diagnostics_for`
/// instead.
fn is_pure_call_result_unused_diagnostic(d: &Diagnostic) -> bool {
    matches!(
        d.identifier.as_str(),
        "function.resultUnused"
            | "method.resultUnused"
            | "staticMethod.resultUnused"
            | "new.resultUnused"
    )
}

/// `TooWideFunctionReturnTypehintRule`'s own
/// `return.unusedType` fires unconditionally for a plain function (no
/// opt-in gate the way the method-side variant has) whenever a declared
/// `Union` return-type member is never actually produced by the analyzed
/// body - which describes a large share of this whole suite's own
/// minimal fixtures (a `array|false`-declared helper whose trivial body
/// only ever returns the array half, an `int|string`-declared one that
/// only exercises one branch, ...), none of which were written with this
/// check in mind. The identical `missingType.*`/`noop` problem above, for
/// a third family - see `is_missing_typehint_diagnostic`'s own doc
/// comment. A test that specifically wants to see this
/// (`checker/tests/return_type_too_wide.rs`) uses `all_diagnostics_for`
/// instead.
fn is_too_wide_return_type_diagnostic(d: &Diagnostic) -> bool {
    d.identifier == "return.unusedType"
}

pub(super) fn diagnostics_for(src: &str) -> Vec<Diagnostic> {
    diagnostics_for_with(src, |_| {})
}

pub(super) fn diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut Registry),
) -> Vec<Diagnostic> {
    let mut diagnostics = all_diagnostics_for_with(src, configure);
    diagnostics.retain(|d| {
        !is_missing_typehint_diagnostic(d)
            && !is_pure_call_result_unused_diagnostic(d)
            && !is_too_wide_return_type_diagnostic(d)
    });
    diagnostics
}

/// The unfiltered twin of `diagnostics_for` - every diagnostic
/// `Checker::check_program` actually produced, `missingType.*` included.
/// Only `checker/tests/missing_typehint.rs` itself should need this.
pub(super) fn all_diagnostics_for(src: &str) -> Vec<Diagnostic> {
    all_diagnostics_for_with(src, |_| {})
}

fn all_diagnostics_for_with(src: &str, configure: impl FnOnce(&mut Registry)) -> Vec<Diagnostic> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = Registry::build(&program);
    registry.merge_stubs(crate::php_stubs::generated_stubs());
    registry.merge_stub_classes(crate::php_stubs::generated_stub_classes());
    registry.merge_stub_constants(crate::php_stubs::generated_stub_constants());
    configure(&mut registry);
    // `merge_stub_classes`/`configure` can both add classes/extends edges
    // after `Registry::build`'s own internal precompute already ran -
    // rebuild `ancestor_names`/member-owner maps once more here, now that
    // the registry is actually settled, so `is_subclass_of`/`lookup_
    // method`/etc. see every merged-in stub class too (see `precompute_
    // ancestor_names`/`precompute_member_owners`'s own doc comments).
    registry.precompute_ancestor_names();
    registry.precompute_member_owners();
    let mut checker = Checker::new(&registry);
    checker.check_program(&program);
    checker.into_diagnostics()
}
