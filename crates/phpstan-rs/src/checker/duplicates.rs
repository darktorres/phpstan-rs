//! `DuplicateDeclarationHelper`'s own core case - a single
//! class/interface/trait/enum body literally redeclaring the same
//! constant/enum-case/property/method name twice. Narrower value than it
//! sounds for an ordinary class (`php -l` already rejects a literal
//! duplicate property/method/constant as a hard fatal error), but real,
//! provable value survives regardless: this project's own parser (like
//! real PHPStan's, for the identical reason - a static analyzer has to
//! tolerate parsing code that might never actually run, e.g. a
//! work-in-progress edit) accepts the duplicate into the AST without
//! complaint, so nothing else in the checker would ever catch it. Enum
//! cases specifically are also rejected by real PHP itself (`Cannot
//! redefine class constant`), so this isn't "the one shape PHP lets
//! through" the way it might first look - it's "the one shape this
//! project's own checker would otherwise stay silent about, regardless
//! of what real PHP does with it."

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;

impl<'a> Checker<'a> {
    /// `DuplicateDeclarationHelper::checkClassLike`'s equivalent.
    pub(crate) fn check_duplicate_class_members(&mut self, fqn: &str, c: &ClassDef) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let methods = arenas.methods(c.methods.clone());
        let kind = class_kind_identifier(&c.kind);
        self.check_duplicate_consts(fqn, kind, c.enum_cases(arenas), c.consts(arenas));
        self.check_duplicate_properties(fqn, kind, c.properties(arenas), methods);
        self.check_duplicate_methods(fqn, kind, methods);
    }

    /// A class constant and an enum case share one real namespace (both
    /// are class constants under the hood - a case's own backing is just
    /// syntax sugar for one) - checked together, matching real PHPStan's
    /// own `DuplicateDeclarationHelper` exactly.
    fn check_duplicate_consts(
        &mut self,
        fqn: &str,
        kind: &str,
        cases: &[EnumCase],
        consts: &[ClassConst],
    ) {
        let mut seen: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        for case in cases {
            if !seen.insert(case.name.as_ref()) {
                report_builder!(self, 
                    DiagnosticBuilder::new(
                        format!("Cannot redeclare enum case {fqn}::{}.", case.name),
                        case.span,
                        0,
                        format!("{kind}.duplicateEnumCase"),
                    )
                    .non_ignorable(),
                );
            }
        }
        for c in consts {
            if !seen.insert(c.name.as_ref()) {
                report_builder!(self, 
                    DiagnosticBuilder::new(
                        format!("Cannot redeclare constant {fqn}::{}.", c.name),
                        c.span,
                        0,
                        format!("{kind}.duplicateConstant"),
                    )
                    .non_ignorable(),
                );
            }
        }
    }

    fn check_duplicate_properties(
        &mut self,
        fqn: &str,
        kind: &str,
        properties: &[PropertyDecl],
        methods: &[MethodDecl],
    ) {
        let mut seen: rustc_hash::FxHashMap<&str, &PropertyDecl> = rustc_hash::FxHashMap::default();
        for p in properties {
            if seen.insert(p.name.as_ref(), p).is_some() {
                report_builder!(self, 
                    DiagnosticBuilder::new(
                        format!("Cannot redeclare property {fqn}::${}.", p.name),
                        p.span,
                        0,
                        format!("{kind}.duplicateProperty"),
                    )
                    .non_ignorable(),
                );
            }
        }
        // A promoted constructor parameter is a property declaration in
        // disguise - PHPStan's own helper checks it against the same
        // `$declaredProperties` set for exactly this reason.
        let mut seen_promoted: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        let promotions: Vec<_> = self
            .syntax_arenas()
            .map(|arenas| {
                methods
                    .iter()
                    .filter(|m| m.function.is_constructor())
                    .flat_map(|m| arenas.params(m.function.params.clone()))
                    .filter(|param| param.promotion.is_some())
                    .map(|param| (param.name.to_string(), param.span))
                    .collect()
            })
            .unwrap_or_default();
        for (name, span) in promotions {
            if seen.contains_key(name.as_str()) || !seen_promoted.insert(name.clone()) {
                report_builder!(self, 
                    DiagnosticBuilder::new(
                        format!("Cannot redeclare property {fqn}::${name}."),
                        span,
                        0,
                        format!("{kind}.duplicateProperty"),
                    )
                    .non_ignorable(),
                );
            }
        }
    }

    fn check_duplicate_methods(&mut self, fqn: &str, kind: &str, methods: &[MethodDecl]) {
        // Case-insensitive, matching real PHP's own method-name lookup
        // rules (and real PHPStan's own `strtolower` in the same spot).
        let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for m in methods {
            if !seen.insert(m.function.name.to_ascii_lowercase()) {
                report_builder!(self, 
                    DiagnosticBuilder::new(
                        format!("Cannot redeclare method {fqn}::{}().", m.function.name),
                        m.span,
                        0,
                        format!("{kind}.duplicateMethod"),
                    )
                    .non_ignorable(),
                );
            }
        }
    }
}

pub(crate) fn class_kind_identifier(kind: &ClassKind) -> &'static str {
    match kind {
        ClassKind::Class => "class",
        ClassKind::Interface => "interface",
        ClassKind::Trait => "trait",
        ClassKind::Enum => "enum",
    }
}
