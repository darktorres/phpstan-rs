use std::collections::HashMap;

use crate::ast::*;
use crate::checker::Checker;
use crate::registry::{FunctionSig, PropertySig};
use crate::token::Span;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Finding 46's class-level half: extending a class declared `final`
    /// is a real, unconditional fatal error in PHP at class-declaration
    /// time, regardless of whether the subclass overrides anything at
    /// all. The method-level half (overriding a `final` *method*) lives
    /// in `check_method_overrides`, sharing that function's own
    /// per-method ancestor walk instead of a second one here.
    pub(crate) fn check_final_class_extension(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let Some(parent) = info.extends.first() else {
            return;
        };
        let Some(parent_info) = self.registry.classes.get(parent) else {
            return;
        };
        if parent_info.is_final {
            self.report(
                fqn.to_string(),
                format!("class {fqn} cannot extend final class {parent}"),
                span,
                0,
            );
        }
    }

    /// Finding 58: two traits used by the same class declaring the same
    /// property (with a genuinely different type) or method (with a
    /// genuinely different signature) is a real, unconditional PHP fatal
    /// error at class-declaration time unless the class explicitly
    /// resolves the conflict (`insteadof`/`as` - Finding 30's own
    /// `resolved_trait_conflicts`, tracked regardless of whether this
    /// project models exactly which trait `insteadof` picked). Compares
    /// only pairs of *different* traits - a name declared by just one
    /// used trait is never a conflict, no matter what it looks like.
    pub(crate) fn check_trait_conflicts(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.uses.len() < 2 {
            return;
        }

        let mut seen_props: HashMap<&str, (&str, &PropertySig)> = HashMap::new();
        let mut seen_methods: HashMap<&str, (&str, &FunctionSig)> = HashMap::new();
        for trait_name in &info.uses {
            let Some(trait_info) = self.registry.classes.get(trait_name) else {
                continue;
            };
            for (prop_name, sig) in &trait_info.properties {
                if info.resolved_trait_conflicts.contains(prop_name) {
                    continue;
                }
                match seen_props.get(prop_name.as_str()) {
                    Some((first_trait, first_sig)) if first_sig.ty != sig.ty => {
                        self.report(fqn.to_string(), format!(
                                "{first_trait} and {trait_name} define the same property (${prop_name}) with incompatible types in the composition of {fqn}"
                            ), span, 0);
                    }
                    Some(_) => {}
                    None => {
                        seen_props.insert(prop_name, (trait_name, sig));
                    }
                }
            }
            for (method_name, sig) in &trait_info.methods {
                if info.resolved_trait_conflicts.contains(method_name) {
                    continue;
                }
                match seen_methods.get(method_name.as_str()) {
                    Some((first_trait, first_sig)) if !signatures_compatible(first_sig, sig) => {
                        self.report(fqn.to_string(), format!(
                                "{first_trait} and {trait_name} define the same method ({method_name}) with an incompatible signature in the composition of {fqn}"
                            ), span, 0);
                    }
                    Some(_) => {}
                    None => {
                        seen_methods.insert(method_name, (trait_name, sig));
                    }
                }
            }
        }
    }

    /// Finding 59: every case of a backed enum must have a unique backing
    /// value - a real, unconditional PHP fatal error at declaration time
    /// otherwise ("Duplicate value in enum ... for case ..."), not merely
    /// a design smell. Only compares cases whose value is a literal
    /// int/string (the overwhelming majority in practice) - a computed or
    /// otherwise non-literal case value isn't evaluated, matching this
    /// project's usual "don't guess" leniency.
    pub(crate) fn check_enum_case_values(&mut self, fqn: &str, e: &EnumDecl) {
        let mut seen: HashMap<(bool, String), &str> = HashMap::new();
        for case in &e.cases {
            let Some(value) = &case.value else {
                continue;
            };
            let key = match &value.kind {
                ExprKind::Literal(Literal::Int(n)) => (true, n.to_string()),
                ExprKind::Literal(Literal::String(s)) => (false, s.clone()),
                _ => continue,
            };
            if let Some(first_case) = seen.get(&key) {
                self.report(fqn.to_string(), format!(
                        "duplicate value in enum {fqn} for case {} (already used by case {first_case})",
                        case.name
                    ), case.span, 0);
            } else {
                seen.insert(key, &case.name);
            }
        }
    }

    /// Finding 45: a concrete class must provide a real implementation of
    /// every abstract method it inherits (from a parent class, an
    /// implemented interface, or a directly-`use`d trait) - real PHP
    /// raises a fatal error at declaration time otherwise, independent of
    /// whether anything ever calls the missing method. Interfaces,
    /// traits, and abstract classes are themselves exempt (a class that
    /// legitimately leaves a method unimplemented is required to declare
    /// itself `abstract`, and one that does is not itself required to be
    /// complete).
    pub(crate) fn check_class_completeness(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.kind != ClassKind::Class && info.kind != ClassKind::Enum {
            return;
        }
        if info.is_abstract {
            return;
        }
        let required = self.registry.required_abstract_methods(fqn);
        let missing: Vec<&String> = required
            .iter()
            .filter(|m| !self.registry.has_concrete_implementation(fqn, m))
            .collect();
        if missing.is_empty() {
            return;
        }
        let shown: Vec<&str> = missing.iter().take(3).map(|s| s.as_str()).collect();
        let mut message = format!(
            "class {fqn} does not implement abstract method(s): {}",
            shown.join(", ")
        );
        if missing.len() > shown.len() {
            message.push_str(&format!(", and {} more", missing.len() - shown.len()));
        }
        self.report(fqn.to_string(), message, span, 0);
    }

    /// Finding 50: an overriding method's signature must stay
    /// Liskov-compatible with the method it overrides - a parameter type
    /// may only be widened or kept the same (accepting more is safe),
    /// never narrowed; a return type may only be narrowed or kept the
    /// same (promising more is safe), never widened. Real PHP enforces
    /// this unconditionally at class-declaration time. Deliberately
    /// scoped to concrete parameter/return type mismatches only, not
    /// generic type-argument variance within an overridden signature -
    /// this project's own `is_assignable` is already lenient there by
    /// design (guessing wrong in either direction is worse than not
    /// checking), and that leniency is the right call here too.
    ///
    /// Also covers Finding 46's method-level half: overriding a method
    /// declared `final` anywhere in the ancestor chain, sharing this same
    /// per-method loop since both checks need the same "does this
    /// override collide with an ancestor's declaration" walk.
    pub(crate) fn check_method_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for m in &c.methods {
            // Finding 46: overriding a `final` method is a real,
            // unconditional fatal error in PHP - including for
            // __construct, unlike the signature-compatibility checks
            // below, so this runs before that exemption.
            if let Some(ancestor) = self
                .registry
                .find_final_ancestor_method(fqn, &m.function.name)
            {
                self.report(
                    fqn.to_string(),
                    format!(
                        "{fqn}::{}() cannot override final method {ancestor}::{}()",
                        m.function.name, m.function.name
                    ),
                    m.span,
                    0,
                );
            }
            // Real PHP deliberately exempts constructors from override-
            // compatibility checking - a subclass's own __construct is
            // allowed an entirely different, otherwise-incompatible
            // signature from its parent's, unlike every other method.
            if m.function.name == "__construct" {
                continue;
            }
            let Some(own_sig) = info.methods.get(&m.function.name) else {
                continue;
            };
            let Some((_, ancestor_sig)) = self
                .registry
                .lookup_ancestor_method_with_owner(fqn, &m.function.name)
            else {
                continue;
            };
            for (i, ancestor_param) in ancestor_sig.params.iter().enumerate() {
                let Some(own_param) = own_sig.params.get(i) else {
                    continue;
                };
                // `Type::Mixed` is overloaded throughout this project to
                // mean both real PHP `mixed` *and* "no type declared at
                // all, native or docblock" (see this file's own opening
                // comment) - and real PHP explicitly does NOT enforce
                // override compatibility for a parameter/return position
                // that isn't typed at all (only an explicitly *narrower*
                // declared type is a real fatal error). Since an
                // intentional, explicit `mixed` on an override is rare
                // next to "just wasn't declared", skipping either side
                // being Mixed avoids flagging the overwhelmingly common
                // case (confirmed via vendor/phpstorm-stubs: dozens of
                // real stub methods simply don't redeclare an inherited
                // native return type) at the cost of missing the rare
                // genuine `mixed`-narrows-a-real-type case - the same
                // "don't guess wrong on Mixed" tradeoff `is_assignable`
                // already makes elsewhere in this project.
                if own_param.ty == Type::Mixed || ancestor_param.ty == Type::Mixed {
                    continue;
                }
                if !self
                    .registry
                    .is_assignable(&ancestor_param.ty, &own_param.ty)
                {
                    self.report(fqn.to_string(), format!(
                            "{fqn}::{}() parameter #{} (${}) narrows the inherited parameter type: expects {}, ancestor expects {}",
                            m.function.name,
                            i + 1,
                            own_param.name,
                            own_param.ty,
                            ancestor_param.ty
                        ), m.span, 0);
                }
            }
            // An ancestor's own `static`-typed return (`self.registry`
            // has no way to resolve what `static` meant relative to the
            // *ancestor's* declaring class, only the checker's own
            // current-class-aware `resolve_pseudo`, which isn't
            // available here) is real PHP's own explicit "late static
            // binding" covariance escape hatch - a subclass narrowing it
            // to its own concrete class name (the overwhelmingly common
            // real shape, confirmed via vendor/phpstorm-stubs'
            // `Ds\Collection::copy(): static` and its own subclasses'
            // `copy(): Vector`/`copy(): Stack`/...) is always exactly
            // what real PHP allows, never a real incompatibility.
            // Checked at the *native* level only (ignoring any `@return`
            // docblock refinement on either side) - real PHP itself only
            // ever enforces override-compatibility on the native
            // signature, and by default so does real PHPStan
            // (`checkPhpDocMethodSignatures` is off unless a project
            // explicitly opts in - confirmed neither Piwigo's own corpus
            // config nor this project's own does). Using the merged,
            // docblock-aware `return_type` here wrongly flagged every
            // override that faithfully keeps the ancestor's own native
            // `array` while the ancestor's *interface* had separately
            // narrowed it to `list<...>` via docblock alone - a real
            // corpus false positive (several `HtmlRenderingInterface`
            // test fakes each declaring `getCatBreadcrumb(): array`,
            // exactly matching the interface's own native signature).
            if own_sig.return_type_native != Type::Mixed
                && ancestor_sig.return_type_native != Type::Mixed
                && ancestor_sig.return_type_native != Type::StaticRef
                && !self.registry.is_assignable(
                    &own_sig.return_type_native,
                    &ancestor_sig.return_type_native,
                )
            {
                self.report(fqn.to_string(), format!(
                        "{fqn}::{}() return type {} is not compatible with the inherited return type {}",
                        m.function.name, own_sig.return_type, ancestor_sig.return_type
                    ), m.span, 0);
            }
        }
    }
}

/// Finding 58's own notion of "two trait methods with the same name are
/// genuinely incompatible, not just written slightly differently" -
/// deliberately narrow (exact param count and exact per-position/return
/// type equality) rather than a real Liskov-style compatibility check:
/// this is comparing two *unrelated* trait declarations with no
/// inheritance relationship between them at all, so there's no
/// contravariance/covariance direction to apply - only "are these the
/// same signature" makes sense here.
pub(crate) fn signatures_compatible(a: &FunctionSig, b: &FunctionSig) -> bool {
    if a.params.len() != b.params.len() {
        return false;
    }
    if a.return_type != b.return_type {
        return false;
    }
    a.params
        .iter()
        .zip(&b.params)
        .all(|(pa, pb)| pa.ty == pb.ty)
}
