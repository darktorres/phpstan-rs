use std::collections::HashSet;

use crate::ast::*;
use crate::checker::{Checker, TemplateBindings};
use crate::registry::{FunctionSig, ParamSig};
use crate::token::Span;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Walks `ty` for every `Object(class, args)` naming a class with its
    /// own `@template T of Bound` (Phase 10.5's bound-enforcement half),
    /// flagging any argument that doesn't satisfy its bound - e.g.
    /// `Collection<Foo>` where `Collection`'s own `@template T of Shape`
    /// and `Foo` doesn't implement `Shape`. Recurses into every argument
    /// too (so `Collection<Box<Foo>>` still checks `Box`'s own bound), and
    /// into `Union`/`Intersection`/`Array`/`ArrayShape` members (a bound
    /// violation doesn't stop being real just because it's wrapped in a
    /// union or is one array's element type).
    ///
    /// Bindings are computed once, positionally, before any bound is
    /// checked - not just so multiple arguments against multiple bounds
    /// each see the right value, but because a bound can itself reference
    /// another of the *same* class's own templates (`@template A`
    /// `@template B of A`), which only resolves correctly once `A`'s own
    /// concrete argument is known.
    ///
    /// Called once per declared type (a function's params/return type, a
    /// class's own properties, its `@extends`/`@implements` tags) rather
    /// than at every call/access site - a generic type only needs
    /// flagging once, where it's *written*, the same way real PHPStan's
    /// own `GenericObjectTypeCheck` documents itself.
    pub(crate) fn check_generic_bounds(&mut self, ty: &Type, label: &str, span: Span) {
        match ty {
            Type::Object(class, args) => {
                if !args.is_empty()
                    && let Some(info) = self.registry.classes.get(class)
                    && !info.template_bounds.is_empty()
                {
                    let bindings: TemplateBindings = info
                        .templates
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    for (name, arg) in info.templates.iter().zip(args.iter()) {
                        let Some(bound) = info.template_bounds.get(name) else {
                            continue;
                        };
                        let bound = types::substitute_template(bound, &bindings);
                        // Same "don't know, don't guess" leniency every
                        // other undefined-symbol-adjacent check in this
                        // project already applies to a class the registry
                        // has no real data for (an uninstalled dependency,
                        // or - the real case found verifying this against
                        // Piwigo's corpus - a PHAR-bundled one like
                        // `phpstan/phpstan` itself, which ships with no
                        // loose source files to parse at all): without it,
                        // every reference to one of its own real classes,
                        // e.g. `PHPStan\Type\ArrayType implements
                        // PHPStan\Type\Type`, reads as violating a bound
                        // this project simply has no way to verify either
                        // way, not one it's actually confirmed broken.
                        if *arg != Type::Mixed
                            && self.is_type_fully_known(arg)
                            && self.is_type_fully_known(&bound)
                            && !self.registry.is_assignable(arg, &bound)
                        {
                            self.report(
                                label.to_string(),
                                format!("{class}'s @template {name} requires {bound}, got {arg}"),
                                span,
                                6,
                            );
                        }
                    }
                }
                for arg in args {
                    self.check_generic_bounds(arg, label, span);
                }
            }
            Type::Array(inner) | Type::ClassString(inner) => {
                self.check_generic_bounds(inner, label, span)
            }
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.check_generic_bounds(m, label, span);
                }
            }
            Type::ArrayShape(fields) => {
                for (_, t, _) in fields {
                    self.check_generic_bounds(t, label, span);
                }
            }
            _ => {}
        }
    }

    /// `false` if `ty` mentions any class this project has no real data
    /// for at all - an uninstalled dependency, or a PHAR-bundled one
    /// (real `phpstan/phpstan` ships as a compiled PHAR with no loose
    /// source files, the same gap Phase 9's own undefined-symbol checks
    /// already document). `check_generic_bounds` uses this to skip a
    /// bound it has no way to actually verify, rather than guess wrong.
    pub(crate) fn is_type_fully_known(&self, ty: &Type) -> bool {
        match ty {
            Type::Object(name, args) => {
                (name == "object" || self.registry.classes.contains_key(name))
                    && args.iter().all(|a| self.is_type_fully_known(a))
            }
            Type::Array(inner) | Type::ClassString(inner) => self.is_type_fully_known(inner),
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().all(|m| self.is_type_fully_known(m))
            }
            Type::ArrayShape(fields) => fields.iter().all(|(_, t, _)| self.is_type_fully_known(t)),
            _ => true,
        }
    }

    /// `check_generic_bounds` for everything a class declares outside its
    /// own methods (which `check_function` already covers per-method):
    /// every property's type, plus its own `@extends`/`@implements`
    /// arguments - both are real places a generic bound violation can be
    /// written (`class Bad extends Collection<NotAShape> {}`) without
    /// ever showing up in a method signature at all.
    pub(crate) fn check_class_generic_bounds(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // Each property gets its own real span (declaration order, not a
        // sorted HashMap walk - the AST's `Vec<PropertyDecl>` is already
        // deterministic, unlike the registry's `HashMap`-backed lookup).
        for p in &c.properties {
            if let Some(prop_sig) = info.properties.get(&p.name) {
                self.check_generic_bounds(&prop_sig.ty, fqn, p.span);
            }
        }
        // `extends_args`/`implements_args` store just the argument list,
        // not paired with the parent/interface's own name - re-pair each
        // one into a real `Object(class, args)` here, or
        // `check_generic_bounds` has no class to look bounds up against
        // at all (it would just see the *arguments* as if they were the
        // generic reference themselves). Neither the parent name nor an
        // interface name carries its own span in the AST, so both use the
        // class declaration's own span - imprecise. but still real.
        let mut types_to_check: Vec<Type> = Vec::new();
        if let Some(parent) = info.extends.first() {
            types_to_check.push(Type::Object(parent.clone(), info.extends_args.clone()));
        }
        // `implements_args` is a `HashMap` with no guaranteed iteration
        // order - sorted by key so two diagnostics from it always come out
        // in the same relative order, not one that silently varies by
        // process (Rust's default hasher is randomly seeded per run).
        let mut iface_names: Vec<&String> = info.implements_args.keys().collect();
        iface_names.sort();
        for iface in iface_names {
            types_to_check.push(Type::Object(
                iface.clone(),
                info.implements_args[iface].clone(),
            ));
        }
        for ty in types_to_check {
            self.check_generic_bounds(&ty, fqn, c.span);
        }
    }

    /// Phase 10.5's variance-enforcement half: a class's own
    /// `@template-covariant T` may only appear in one of its methods'
    /// *return* types (an output position - covariance is only sound if
    /// nothing can ever hand a `T` back *into* the generic container,
    /// the way `Collection<Cat>` being usable where `Collection<Animal>`
    /// is expected relies on only ever reading a value out, never
    /// writing one in); `@template-contravariant T` is the mirror image,
    /// restricted to *parameter* positions only. A plain, un-annotated
    /// `@template T` (invariant, the only kind Phase 10 modeled at all)
    /// is unrestricted, same as always.
    ///
    /// Deliberately conservative in two ways, both matching this
    /// project's general preference for fewer false positives over
    /// catching everything: only checks method params/return types, not
    /// properties (this project doesn't track `readonly` at all yet, and
    /// a *readonly* property typed `T` is actually a legitimate output-
    /// only position - without that information, checking properties
    /// here would misflag every real one); and doesn't attempt real
    /// variance's positional *flipping* through a nested generic
    /// argument (a `T` found inside another class's own `Collection<T>`
    /// type argument is treated the same as a direct occurrence, not
    /// re-derived from that other class's own variance) - a full,
    /// correct algorithm needs real per-class variance composition this
    /// project doesn't attempt.
    pub(crate) fn check_variance(
        &mut self,
        sig: &FunctionSig,
        class_name: &str,
        label: &str,
        span: Span,
    ) {
        let Some(info) = self.registry.classes.get(class_name) else {
            return;
        };
        if info.covariant_templates.is_empty() && info.contravariant_templates.is_empty() {
            return;
        }
        let mut input_names = HashSet::new();
        for param in &sig.params {
            collect_template_params(&param.ty, &mut input_names);
        }
        let mut output_names = HashSet::new();
        collect_template_params(&sig.return_type, &mut output_names);

        // Sorted before iterating - both are `HashSet`s, and this project
        // has already been bitten once this phase by a `HashMap`/`HashSet`
        // iteration order silently varying a diagnostic's position
        // between runs (see `check_class_generic_bounds`'s own fix).
        let mut violations: Vec<String> = Vec::new();
        for name in &input_names {
            if info.covariant_templates.contains(name) {
                violations.push(format!(
                    "{class_name}'s @template-covariant {name} cannot be used in an input (parameter) position"
                ));
            }
        }
        for name in &output_names {
            if info.contravariant_templates.contains(name) {
                violations.push(format!(
                    "{class_name}'s @template-contravariant {name} cannot be used in an output (return) position"
                ));
            }
        }
        violations.sort();
        for message in violations {
            self.report(label.to_string(), message, span, 6);
        }
    }

    /// Binds a generic-typed receiver's own carried type arguments
    /// (`Collection<User>` -> `{"T": Object("User", [])}`) against
    /// `target`'s own declared `@template` parameter names, walking up
    /// `class`'s `extends`/`implements` chain to reach it if `target`
    /// isn't `class` itself (Phase 10.5) - so a concrete subclass that
    /// fixes its parent's template param (`class IntCollection extends
    /// Collection<int> {}`) resolves `Collection`'s own `T` to `int` even
    /// though `IntCollection` isn't generic itself, and a generic
    /// subclass that passes its own template through (`class Box<T>
    /// extends Collection<T> {}`) resolves it to whatever `Box`'s own `T`
    /// is bound to at each use, not a fixed value.
    ///
    /// `target` matters because template names are scoped to the class
    /// that declares them, not globally unique - two unrelated classes in
    /// the same chain commonly both use `T` (confirmed the hard way on a
    /// real corpus: `symfony/type-info`'s `NullableType<T of Type>
    /// extends UnionType<T|BuiltinType<...>>`, where `UnionType` has its
    /// *own*, unrelated `T`). Returning bindings scoped to exactly
    /// `target`'s own level - not a single flat map merged across the
    /// whole chain - is what keeps those two `T`s from colliding into
    /// whichever one happened to be computed last.
    ///
    /// A non-generic class (empty `templates`), a bare unspecified-generic
    /// reference (empty `type_args`), or a chain with no matching
    /// `@extends`/`@implements` tag along the way to `target` all just
    /// leave that level's names unbound - `substitute_template` already
    /// treats an unbound `TemplateParam` as `Mixed`, the right
    /// conservative fallback for an "erased" generic.
    pub(crate) fn class_template_bindings(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
    ) -> TemplateBindings {
        let mut seen = HashSet::new();
        self.class_template_bindings_rec(class, type_args, target, &mut seen)
            .unwrap_or_default()
    }

    /// `None` means `target` isn't reachable along this path at all (so
    /// the caller should treat every one of its templates as unbound,
    /// same as `Some` with an empty map) - the distinction only matters
    /// internally, to know when to keep searching sibling branches below.
    /// `seen` guards against a cyclic `extends`/`implements` graph
    /// (invalid PHP, but a malformed/partially-loaded real corpus could
    /// still produce one) - cloned per branch, not threaded through by
    /// mutable reference, so two independent ancestors (e.g. two
    /// interfaces that both eventually reach a common shared parent, a
    /// real "diamond" shape) can each still be searched.
    pub(crate) fn class_template_bindings_rec(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
        seen: &mut HashSet<String>,
    ) -> Option<TemplateBindings> {
        if !seen.insert(class.to_string()) {
            return None;
        }
        let info = self.registry.classes.get(class)?;
        let own_bindings: TemplateBindings = info
            .templates
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();

        if class == target {
            return Some(own_bindings);
        }

        if let Some(parent) = info.extends.first() {
            let parent_args: Vec<Type> = info
                .extends_args
                .iter()
                .map(|t| types::substitute_template(t, &own_bindings))
                .collect();
            if let Some(found) =
                self.class_template_bindings_rec(parent, &parent_args, target, &mut seen.clone())
            {
                return Some(found);
            }
        }
        for iface in &info.implements {
            let iface_args: Vec<Type> = info
                .implements_args
                .get(iface)
                .map(|args| {
                    args.iter()
                        .map(|t| types::substitute_template(t, &own_bindings))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(found) =
                self.class_template_bindings_rec(iface, &iface_args, target, &mut seen.clone())
            {
                return Some(found);
            }
        }
        None
    }

    /// Applies `class_template_bindings`' substitution to every param and
    /// the return type of a method signature - the class-level counterpart
    /// to `infer_call_return_type`'s existing per-call-argument template
    /// binding (which only ever sees a method's *own* `@template`, not one
    /// declared on the enclosing class). `owner` is whichever ancestor of
    /// `class` actually declares `sig` (from `lookup_method_with_owner`/
    /// `lookup_property_with_owner`) - `class` itself when it's not
    /// inherited.
    pub(crate) fn substitute_class_templates(
        &self,
        sig: &FunctionSig,
        class: &str,
        type_args: &[Type],
        owner: &str,
    ) -> FunctionSig {
        let bindings = self.class_template_bindings(class, type_args, owner);
        FunctionSig {
            params: sig
                .params
                .iter()
                .map(|p| ParamSig {
                    ty: types::substitute_template(&p.ty, &bindings),
                    ..p.clone()
                })
                .collect(),
            return_type: types::substitute_template(&sig.return_type, &bindings),
            return_type_native: types::substitute_template(&sig.return_type_native, &bindings),
        }
    }
}

/// Every `TemplateParam` name appearing anywhere in `ty`, recursing into
/// `Array`/`Union`/`Intersection`/`ArrayShape` (structural wrappers that
/// don't change what "position" a template occupies) but *not* into
/// another `Object`'s own type arguments - see `Checker::check_variance`'s
/// doc comment for why that second part is a deliberate scope cut, not an
/// oversight.
pub(crate) fn collect_template_params(ty: &Type, found: &mut HashSet<String>) {
    match ty {
        Type::TemplateParam(name) => {
            found.insert(name.clone());
        }
        Type::Array(inner) => collect_template_params(inner, found),
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                collect_template_params(m, found);
            }
        }
        Type::ArrayShape(fields) => {
            for (_, t, _) in fields {
                collect_template_params(t, found);
            }
        }
        _ => {}
    }
}
