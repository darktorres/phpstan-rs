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

use crate::ast::*;
use crate::checker::{Checker, DiagnosticBuilder};

impl<'a> Checker<'a> {
    /// `DuplicateDeclarationHelper::checkClassLike`'s equivalent for a
    /// `class`/`interface`/`trait`. Enums use `check_duplicate_enum_members`
    /// instead - `EnumDecl` is its own AST node, not a `ClassDecl` with
    /// `ClassKind::Enum` (see this project's own `ast.rs`).
    pub(crate) fn check_duplicate_class_members(&mut self, fqn: &str, c: &ClassDecl) {
        let kind = class_kind_identifier(&c.kind);
        self.check_duplicate_consts(fqn, kind, &[], &c.consts);
        self.check_duplicate_properties(fqn, kind, &c.properties, &c.methods);
        self.check_duplicate_methods(fqn, kind, &c.methods);
    }

    pub(crate) fn check_duplicate_enum_members(&mut self, fqn: &str, e: &EnumDecl) {
        self.check_duplicate_consts(fqn, "enum", &e.cases, &e.consts);
        self.check_duplicate_properties(fqn, "enum", &[], &e.methods);
        self.check_duplicate_methods(fqn, "enum", &e.methods);
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
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
            if let Some(prev) = seen.insert(p.name.as_ref(), p)
                && !property_shapes_differ(prev, p)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
        // `$declaredProperties` set for exactly this reason. Only ever
        // compared against another promoted collision, since a plain
        // property's shape isn't directly comparable to a `Param`'s.
        let mut seen_promoted: rustc_hash::FxHashMap<&str, &Param> =
            rustc_hash::FxHashMap::default();
        for m in methods {
            if !m.function.name.eq_ignore_ascii_case("__construct") {
                continue;
            }
            for param in &m.function.params {
                if param.promoted_visibility.is_none() {
                    continue;
                }
                if seen.contains_key(param.name.as_ref()) {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!("Cannot redeclare property {fqn}::${}.", param.name),
                            param.span,
                            0,
                            format!("{kind}.duplicateProperty"),
                        )
                        .non_ignorable(),
                    );
                    continue;
                }
                if let Some(prev) = seen_promoted.insert(param.name.as_ref(), param)
                    && prev.type_hint == param.type_hint
                {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!("Cannot redeclare property {fqn}::${}.", param.name),
                            param.span,
                            0,
                            format!("{kind}.duplicateProperty"),
                        )
                        .non_ignorable(),
                    );
                }
            }
        }
    }

    fn check_duplicate_methods(&mut self, fqn: &str, kind: &str, methods: &[MethodDecl]) {
        // Case-insensitive, matching real PHP's own method-name lookup
        // rules (and real PHPStan's own `strtolower` in the same spot).
        let mut seen: rustc_hash::FxHashMap<String, &MethodDecl> = rustc_hash::FxHashMap::default();
        for m in methods {
            let lower = m.function.name.to_ascii_lowercase();
            if let Some(prev) = seen.insert(lower, m)
                && !method_signatures_differ(prev, m)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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

/// phpstorm-stubs represents "this member's own shape changed across PHP
/// versions" by declaring the same conceptual member twice, once per
/// era (e.g. `ReflectionMethod::setAccessible()`'s pre-8.0/8.1+
/// signature split, `Directory::$path`'s untyped/`readonly
/// string`-typed split) - each gated by its own `#[PhpStormStubsElementAvailable(
/// from: X, to: Y)]` attribute in the source, but this project's own
/// lexer (`skip_stub_availability_attribute`) consumes that attribute
/// entirely before the parser ever sees it (a deliberate, pre-existing
/// design for the *parameter*-level version-split idiom - see
/// `registry::function_sig::dedup_versioned_params`'s own doc comment -
/// that happens to erase the same signal a *member*-level check like
/// this one would otherwise want to read back). Detected structurally
/// instead: a real version-gated split always changes the member's own
/// native shape (a different param count/type, return type, or
/// readonly-ness) - an actual copy-paste duplicate bug never does,
/// since it's definitionally the same declaration twice. A signature
/// difference is treated as proof of the versioning idiom rather than a
/// second, independent bug to also report - conservative, matching this
/// project's own "don't guess, don't double up" precedent elsewhere.
fn method_signatures_differ(a: &MethodDecl, b: &MethodDecl) -> bool {
    function_decl_signatures_differ(&a.function, &b.function)
}

/// `pub(crate)`: also reused by `checker::duplicate_functions` for the
/// identical top-level-function shape of this same "is this a real bug or
/// a version-gated split" question.
pub(crate) fn function_decl_signatures_differ(af: &FunctionDecl, bf: &FunctionDecl) -> bool {
    if af.params.len() != bf.params.len() {
        return true;
    }
    if af.return_type != bf.return_type {
        return true;
    }
    af.params
        .iter()
        .zip(&bf.params)
        .any(|(ap, bp)| ap.type_hint != bp.type_hint)
}

fn property_shapes_differ(a: &PropertyDecl, b: &PropertyDecl) -> bool {
    a.type_hint != b.type_hint || a.is_readonly != b.is_readonly
}
