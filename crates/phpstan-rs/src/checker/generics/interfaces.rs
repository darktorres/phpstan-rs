//! `Rules/Generics/CrossCheckInterfacesHelper.php`,
//! ported directly - walks every path by which a class/interface/enum reaches the same
//! generic interface (its own `@extends`/`@implements` plus every
//! ancestor's own, recursively) and flags it if two different paths pin
//! that interface's same `@template` parameter to two different concrete
//! types (a real "diamond generic" conflict - `class Box<T> implements
//! A<T>, B<int> {}` where `A<X> extends Base<X>` and `B<Y> extends
//! Base<Y>` pins `Base`'s own template to `T` via one path and to `int`
//! via the other).
//!
//! A deliberately simplified traversal relative to real PHPStan's own
//! `ClassReflection::getInterfaces()`/`getActiveTemplateTypeMap()`
//! machinery (which rebuilds and caches the full transitive interface
//! closure at every recursion level, with its own `$first`-gated
//! parent-chain short-circuit) - this walks the `extends`/`implements`
//! graph directly via `ClassReflection`, substituting one level's own concrete
//! arguments into the next via the same `Type::substitute_templates`
//! primitive `Registry::class_template_bindings_rec` already uses for a
//! single-target search, just without stopping at the first match: every
//! reachable interface is recorded, and a second, differently-bound
//! reach of the same one is what triggers `generics.interfaceConflict`.
//! Every judgment call below is made in the "favor false negatives
//! over false positives" direction this project has held throughout.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::checker::Checker;
use crate::registry::ClassReflection;
use syntax::span::Span;
use types::{self, Type};

impl<'a> Checker<'a> {
    /// Entry point - called once per class/interface/enum declaration
    /// alongside `check_ancestor_generic_bounds`'s own `extends`/
    /// `implements`/`@use` checking (sharing one call site across all
    /// three declaration kinds).
    pub(crate) fn check_cross_path_interface_conflicts(&mut self, fqn: &str, span: Span) {
        let Some(root_info) = self.registry.class_by_name(fqn) else {
            return;
        };
        // Real PHPStan's own message literally only ever says "Class" or
        // "Interface" here (`$classReflection->isInterface() ? "Interface"
        // : "Class"`) - even for an enum, `EnumAncestorsRule` calls this
        // exact same helper unchanged, so an enum genuinely gets called a
        // "Class" in this one specific message. Ported as-is, not
        // "corrected" to a third word real PHPStan itself doesn't use here.
        let kind_word = if root_info.kind() == syntax::ast::ClassKind::Interface {
            "Interface"
        } else {
            "Class"
        };

        // Seeds the walk with an *identity* binding for `fqn`'s own
        // templates (each maps to itself, as a real `TemplateParam` node)
        // rather than an empty map - `Type::substitute_templates` treats
        // any name *absent* from the bindings map as immediately unbound
        // (collapsed to `Mixed`, or its own default), which would erase
        // `fqn`'s own template identity at the very first substitution
        // step and lose the ability to tell two *different* root
        // templates apart later (a real precision gap, not just cosmetic
        // - `class Box<T, U> implements A<T>, B<U>` with `A<X> extends
        // Base<X>`/`B<Y> extends Base<Y>` would otherwise have both `T`
        // and `U` collapse to the same `Mixed` before ever reaching
        // `Base`, silently agreeing when they're actually two unrelated,
        // independently-instantiable templates). An identity entry makes
        // `Type::substitute_templates` leave it alone instead.
        let own_bindings = identity_template_bindings(self.registry, root_info, fqn);

        let mut found: FxHashMap<String, Vec<Type>> = FxHashMap::default();
        let mut diagnostics: Vec<(String, usize, Type, Type)> = Vec::new();
        self.walk_for_interface_conflicts(
            fqn,
            &own_bindings,
            &mut found,
            &mut diagnostics,
            &FxHashSet::default(),
        );

        for (iface, idx, new_ty, old_ty) in diagnostics {
            let Some(info) = self.registry.class_by_name(iface.as_str()) else {
                continue;
            };
            let Some(template_name) = info.templates().get(idx) else {
                continue;
            };
            self.report(format!(
                    "{kind_word} {fqn} specifies template type {template_name} of interface {iface} as {new_ty} but it's already specified as {old_ty}."
                ),
                span,
                6,
                "generics.interfaceConflict",
            );
        }
    }

    /// `own_bindings` substitutes `class`'s own template names as
    /// reached via the path taken to get here. `visited` is a snapshot of
    /// every class already on the path from the root down to (and
    /// including) `class`'s own parent - cloned and extended once here,
    /// then shared *unmutated* across every one of `class`'s own direct
    /// children (`visit_ancestor`'s calls below) rather than threaded
    /// through by mutable reference: two sibling branches (e.g. two
    /// unrelated interfaces `class Box implements A, B {}`) must each see
    /// only the path *to* `class`, never each other's own descendants -
    /// legitimately reaching the same interface twice via two different
    /// branches is the entire point of this walk, not a cycle to
    /// suppress. Only a genuinely cyclic graph *within one single
    /// branch* (invalid PHP, but a malformed/partially-loaded real corpus
    /// could still produce one) is guarded against, via the early return
    /// below - the same shape `class_template_bindings_rec` already
    /// established, just with a shared, not per-call, clone point.
    fn walk_for_interface_conflicts(
        &self,
        class: &str,
        own_bindings: &FxHashMap<String, Type>,
        found: &mut FxHashMap<String, Vec<Type>>,
        diagnostics: &mut Vec<(String, usize, Type, Type)>,
        visited: &FxHashSet<String>,
    ) {
        if visited.contains(class) {
            return;
        }
        let Some(references) = self.registry.class_references_by_name(class) else {
            return;
        };
        let mut visited = visited.clone();
        visited.insert(class.to_string());

        let implements_args = self.registry.effective_implements_args_by_name(class);
        for iface in references.implements {
            let raw_args = implements_args.get(iface).cloned().unwrap_or_default();
            let concrete_args: Vec<Type> = raw_args
                .iter()
                .map(|t| t.substitute_templates(own_bindings, &self.relation()))
                .collect();
            self.visit_ancestor(iface, &concrete_args, found, diagnostics, &visited);
        }

        // A class extending a genuinely non-generic parent has no `@extends`
        // tag to write at all (nothing generic to pass through), so
        // `extends_args` is empty not because this project failed to
        // parse one but because there is none - walking into that parent
        // anyway would compare *its own*, entirely independent
        // declaration of some shared interface against this class's own,
        // as if the two were bound through a real generic relationship
        // when they aren't related that way at all. Real PHPStan's own
        // `CrossCheckInterfacesHelper::check` has the identical gate
        // (`$checkParents`, `getExtendsTags()`) for exactly this reason -
        // only cross-check into a parent when there's a real `@extends`
        // tag linking the two. `extends.len() == 1` is required in
        // addition, for the same multi-extends-conflation reason
        // `check_ancestor_generic_bounds`'s own arity checking already
        // established (`ClassReflection::extends_args`'s own doc comment) - an
        // interface `extends`-ing more than one parent at once conflates
        // their argument lists into one flat, un-keyed list, unsafe to
        // attribute to either.
        let extends_args = self.registry.effective_extends_args_by_name(class);
        if references.extends.len() == 1 && !extends_args.is_empty() {
            let parent = &references.extends[0];
            let concrete_args: Vec<Type> = extends_args
                .iter()
                .map(|t| t.substitute_templates(own_bindings, &self.relation()))
                .collect();
            self.visit_ancestor(parent, &concrete_args, found, diagnostics, &visited);
        }
    }

    /// Records `name`'s own binding (if it's a generic interface - a
    /// visited class/trait is walked through for its own further
    /// ancestors but never itself cross-checked, matching real PHPStan's
    /// identical scope: a conflict is only ever about a shared
    /// *interface*, never a shared parent *class*, since single
    /// inheritance makes reaching the same concrete parent class via two
    /// differently-bound paths structurally impossible in the first
    /// place) against whatever was already found for it, then recurses
    /// into its own ancestors with its own templates bound to
    /// `concrete_args`.
    fn visit_ancestor(
        &self,
        name: &str,
        concrete_args: &[Type],
        found: &mut FxHashMap<String, Vec<Type>>,
        diagnostics: &mut Vec<(String, usize, Type, Type)>,
        visited: &FxHashSet<String>,
    ) {
        let Some(info) = self.class_info(name) else {
            return;
        };
        if info.kind() == syntax::ast::ClassKind::Interface && !info.templates().is_empty() {
            match found.get(name) {
                Some(existing) => {
                    for (idx, (new_ty, old_ty)) in
                        concrete_args.iter().zip(existing.iter()).enumerate()
                    {
                        if new_ty != old_ty {
                            diagnostics.push((
                                name.to_string(),
                                idx,
                                new_ty.clone(),
                                old_ty.clone(),
                            ));
                        }
                    }
                }
                None => {
                    found.insert(name.to_string(), concrete_args.to_vec());
                }
            }
        }
        let next_bindings: FxHashMap<String, Type> = info
            .templates()
            .iter()
            .map(|s| s.to_string())
            .zip(concrete_args.iter().cloned())
            .collect();
        self.walk_for_interface_conflicts(name, &next_bindings, found, diagnostics, visited);
    }
}

fn identity_template_bindings(
    registry: &crate::registry::Registry,
    info: &ClassReflection,
    class: &str,
) -> FxHashMap<String, Type> {
    info.templates()
        .iter()
        .map(|name| name.to_string())
        .zip(registry.own_template_args(class))
        .collect()
}
