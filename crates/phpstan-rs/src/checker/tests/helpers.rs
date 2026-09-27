use super::super::*;
use crate::checker::settings::CheckSettings;
use crate::registry::Registry;
use diagnostics::Diagnostic;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

pub(super) fn all_diagnostics_for(src: &str) -> Vec<Diagnostic> {
    all_diagnostics_for_with(src, |_| {})
}

pub(super) fn all_diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut CheckSettings),
) -> Vec<Diagnostic> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = Registry::build(&program);
    registry.merge_stubs(crate::registry::stubs::generated_stubs());
    registry.merge_stub_classes(crate::registry::stubs::generated_stub_classes());
    registry.merge_stub_constants(crate::registry::stubs::generated_stub_constants());
    // Merging stubs can add classes and `extends` edges after
    // `Registry::build`'s own precompute ran.
    registry.precompute_ancestor_names();
    registry.precompute_member_owners();
    let mut settings = CheckSettings::default();
    configure(&mut settings);
    let bodies = crate::registry::FileBodies::of(&program);
    let mut checker = Checker::new(&registry)
        .with_settings(&settings)
        .with_bodies(&bodies);
    checker.check_program(&program);
    checker.into_diagnostics()
}
