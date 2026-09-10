use crate::ast::*;
use crate::checker::expr::builtin_calls::{
    date_format_return_type, date_interval_format_return_type,
};
use crate::checker::expr::doctrine_query_result_list_type;
use crate::checker::expr::is_doctrine_query_get_result;
use crate::checker::type_union::union_types;
use crate::checker::{Checker, Locals, literal_string_value, resolve_call_args};
use crate::registry::{FunctionSig, PropertySig};
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// The declaring class/`PropertySig` pair for `prop` on `base_ty`,
    /// used by `ExprKind::PropertyAccess`'s own visibility/`@internal`/
    /// `@deprecated` checks - see that call site's own doc comment for
    /// why this recurses through `Union`/`Intersection` instead of using
    /// `MethodCall`'s own simpler "first Object/EnumCase member,
    /// regardless of whether it actually declares the member" shortcut.
    pub(crate) fn find_property_owner(
        &self,
        base_ty: &Type,
        prop: &str,
    ) -> Option<(String, &PropertySig)> {
        match base_ty {
            Type::Object(class, _) => self.registry.lookup_property_with_owner(class, prop),
            Type::EnumCase(fqn, _) => self.registry.lookup_property_with_owner(fqn, prop),
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.find_property_owner(m, prop)),
            // Real bug found and fixed this session: `narrow_instanceof_
            // true` always puts the narrower, just-`instanceof`-checked
            // type *last* in an `Intersection` - see `property_access_
            // type`'s own doc comment for the real corpus trace that
            // found this. Reversed so that narrower type's own
            // declaration (if any) is preferred over the broader original
            // type's.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| self.find_property_owner(m, prop)),
            _ => None,
        }
    }

    /// `find_property_owner`'s own method counterpart, used by
    /// `checker::calls::check_no_discard_statement` - real bug found and
    /// fixed this session, the identical gap: a `#[\NoDiscard]`-marked
    /// method called through a `Union`/`Intersection` receiver (a value
    /// narrowed via `instanceof` from an unrelated interface) used to be
    /// silently skipped, since the receiver's own type had to be
    /// literally `Type::Object`.
    pub(crate) fn find_method_owner(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> Option<(String, &FunctionSig)> {
        match base_ty {
            Type::Object(class, _) => self.registry.lookup_method_with_owner(class, method),
            Type::EnumCase(fqn, _) => self.registry.lookup_method_with_owner(fqn, method),
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.find_method_owner(m, method)),
            // Same real bug and fix as `find_property_owner` just above.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| self.find_method_owner(m, method)),
            _ => None,
        }
    }

    /// `find_method_owner`'s own sibling, used by `ExprKind::
    /// FirstClassCallable`'s own `MethodCall` arm: returns the *concrete
    /// receiver class actually queried* alongside the signature, not the
    /// (possibly different, inherited-from) declaring owner - late static
    /// binding needs the real runtime receiver a `self`/`static` pseudo-
    /// return type resolves against, the same reasoning the pre-existing
    /// single-`Object` code already relied on (it used the receiver's own
    /// class for exactly this, via a plain `lookup_method`, never
    /// `_with_owner`). Real bug found and fixed this session: this call
    /// site required the receiver's own type to be literally `Type::
    /// Object` too, silently skipping the check for a `Union`/
    /// `Intersection` receiver, the same anti-pattern found and fixed
    /// project-wide.
    pub(crate) fn find_method_receiver_class(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> (Option<&FunctionSig>, Option<String>) {
        match base_ty {
            Type::Object(class, _) => (
                self.registry.lookup_method(class, method),
                Some(class.clone()),
            ),
            Type::EnumCase(fqn, _) => (self.registry.lookup_method(fqn, method), Some(fqn.clone())),
            Type::Union(members) => members
                .iter()
                .map(|m| self.find_method_receiver_class(m, method))
                .find(|(sig, _)| sig.is_some())
                .unwrap_or((None, None)),
            // Same real bug and fix as `find_property_owner`'s own doc
            // comment above.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .map(|m| self.find_method_receiver_class(m, method))
                .find(|(sig, _)| sig.is_some())
                .unwrap_or((None, None)),
            _ => (None, None),
        }
    }

    /// Whether `ty` (a thrown value's own type) fails real PHP's
    /// unconditional "must be Throwable" rule - `Some(name)` names a
    /// concrete non-Throwable class to report, `None` when nothing's
    /// wrong or `ty` isn't concrete enough to judge at all (matching this
    /// project's usual "don't guess" leniency - an unmodeled member
    /// anywhere in a `Union`/`Intersection` makes the whole thing
    /// unconfirmed, not a guess either way). Used by both `StmtKind::
    /// Throw` (`checker/stmt.rs`) and `ExprKind::Throw` just below - real
    /// bug found and fixed this session, the same shape as `PropertyAccess`'s
    /// own: both call sites used to require the thrown value's own type
    /// to be literally `Type::Object`, silently skipping the check for a
    /// `Union`/`Intersection` - a value narrowed via `instanceof` from an
    /// unrelated interface, then thrown, is exactly as real a shape here
    /// as it was for property access. A `Union` member fails if *any one*
    /// of them isn't Throwable (the runtime value could be that one); an
    /// `Intersection` fails only if *every* member is confirmed non-
    /// Throwable (any single Throwable member is enough to satisfy
    /// `throw`) - mirrors `member_confirmed_missing`'s own identical
    /// Union/Intersection asymmetry exactly.
    ///
    /// `class_name_exists` is checked *first*, same as `StmtKind::Try`'s
    /// own multi-catch handling a few hundred lines up (`Finding 57`'s own
    /// comment there) - `is_subclass_of` silently treats a name the
    /// registry has no entry for as "not a subclass of Throwable",
    /// indistinguishable from a real, known, non-Throwable class. Real
    /// bug this exact guard fixes, found via the Union case above newly
    /// making an existing multi-catch `throw $e;` reachable at all: real
    /// PHP multi-catch binds a real union type, and a caught interface
    /// that happens to live in an unresolvable/unscanned vendored
    /// package (`RectorPrefix202608\Symfony\Component\Messenger\Exception\
    /// UnrecoverableExceptionInterface` - a PHP-Scoper-prefixed name this
    /// project's vendor scan never actually indexes a declaration for,
    /// confirmed directly - not merely assumed) was wrongly reported as
    /// not implementing `Throwable`, even though the real, unprefixed
    /// interface genuinely does.
    pub(crate) fn thrown_value_not_throwable(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Object(class, _) => (self.class_name_exists(class)
                && !self.registry.is_subclass_of(class, "Throwable"))
            .then(|| class.clone()),
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.thrown_value_not_throwable(m)),
            Type::Intersection(members) => {
                let failures: Vec<String> = members
                    .iter()
                    .filter_map(|m| self.thrown_value_not_throwable(m))
                    .collect();
                (failures.len() == members.len()).then(|| failures.into_iter().next().unwrap())
            }
            _ => None,
        }
    }

    /// `find_property_owner`'s own richer sibling, used by `checker::
    /// assignment::check_property_assignment` - real bug found and fixed
    /// this session, the identical shape as the read-side one just above:
    /// the write side matched `Type::Object` directly too, silently
    /// skipping *every* one of its checks (readonly, visibility,
    /// `@internal`/`@deprecated`, and the assigned value's own type) for
    /// a `Union`/`Intersection`-typed target. Also returns the *matching
    /// member's own* class/type-args (not just the ultimate declaring
    /// owner) - `class_template_bindings` needs the receiver's own real
    /// generic parameterization to substitute correctly up to whichever
    /// ancestor actually declares the property, the same relationship
    /// the original single-`Object` code already relied on.
    pub(crate) fn find_property_receiver(
        &self,
        base_ty: &Type,
        prop: &str,
    ) -> Option<(String, Vec<Type>, String, PropertySig)> {
        match base_ty {
            Type::Object(class, type_args) => self
                .registry
                .lookup_property_with_owner(class, prop)
                .map(|(owner, sig)| (class.clone(), type_args.clone(), owner, sig.clone())),
            Type::EnumCase(fqn, _) => self
                .registry
                .lookup_property_with_owner(fqn, prop)
                .map(|(owner, sig)| (fqn.clone(), Vec::new(), owner, sig.clone())),
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.find_property_receiver(m, prop)),
            // Same real bug and fix as `find_property_owner`'s own doc
            // comment above.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| self.find_property_receiver(m, prop)),
            _ => None,
        }
    }

    /// `find_property_receiver`'s own method counterpart, used by
    /// `check_expr`'s own `MethodCall` arm - real bug found and fixed
    /// this session: that arm's own `recv_class` extraction (a plain
    /// "first Object/EnumCase member, regardless of whether it actually
    /// declares the method" pick, kept as-is purely to gate whether the
    /// existence-check block below even runs at all) was *also* being
    /// used to resolve the method's own real signature, owner, argument-
    /// type checking, visibility/`@internal`/`@deprecated`, the Doctrine
    /// query special case, and the final return type - all of which need
    /// a member that actually declares the method, in the correct
    /// (`Intersection` reversed) order, exactly like `find_method_owner`/
    /// `method_call_return_type` already get right. Returns an owned
    /// `FunctionSig` (unlike `find_method_owner`'s borrowed one) so the
    /// caller can freely call further `&mut self` methods afterward.
    pub(crate) fn find_method_receiver(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> Option<(String, Vec<Type>, String, FunctionSig)> {
        match base_ty {
            Type::Object(class, type_args) => self
                .registry
                .lookup_method_with_owner(class, method)
                .map(|(owner, sig)| (class.clone(), type_args.clone(), owner, sig.clone())),
            Type::EnumCase(fqn, _) => self
                .registry
                .lookup_method_with_owner(fqn, method)
                .map(|(owner, sig)| (fqn.clone(), Vec::new(), owner, sig.clone())),
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.find_method_receiver(m, method)),
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| self.find_method_receiver(m, method)),
            _ => None,
        }
    }

    /// Real bug found and fixed this session: both `check_expr`'s and
    /// `infer_expr`'s own `PropertyAccess` arms matched `base_ty` against
    /// `Object`/`EnumCase`/`ObjectShape` directly, with no `Union`/
    /// `Intersection` arm at all - falling to the `_ => Type::Mixed`
    /// catch-all discarded every bit of type information whenever the
    /// base's own type happened to be a `Union`/`Intersection` (a real,
    /// common shape: `narrow_instanceof_true`'s own deliberate design
    /// intersects an unrelated-looking `instanceof` target with the
    /// original type - see its own doc comment - and a plain `A|B`
    /// property/parameter type is exactly as common). Recurses through
    /// both, trying each member in turn and keeping the first real
    /// (non-`Mixed`) answer - correct for an `Intersection` (the property
    /// only needs to exist on *one* member) and a reasonable, consistent
    /// best-effort for a `Union` (matches this project's own general
    /// "fewer false positives over more precision" bias elsewhere, e.g.
    /// `MethodCall`'s own identical `Union`/`Intersection` receiver-class
    /// extraction just above). Real corpus false positive this fixes:
    /// `PhpParser\Node\Stmt\ClassMethod::$stmts` (a real, docblock-typed
    /// `Node\Stmt[]|null` property) resolving to bare `Mixed` the moment
    /// its receiver was narrowed via `instanceof` from the `PhpParser\
    /// Node` interface - `PhpParser`'s own AST hierarchy routes every
    /// concrete node type through an interface this way, not a common
    /// base *class*, so this shape was never rare there. See the `Union`/
    /// `Intersection` arms below for the real distinction between them
    /// (added in a later refinement) - initially both used the same
    /// "first real answer wins" shortcut, correct for `Intersection` but
    /// an avoidable imprecision for `Union`.
    pub(crate) fn property_access_type(&self, base_ty: &Type, prop: &str) -> Type {
        match base_ty {
            Type::Object(class, type_args) => self
                .registry
                .lookup_property_with_owner(class, prop)
                .map(|(owner, p)| {
                    let bindings = self.class_template_bindings(class, type_args, &owner);
                    let resolved = types::substitute_template(&p.ty, &bindings);
                    self.resolve_pseudo(&resolved, class)
                })
                .unwrap_or_else(|| {
                    // Phase 77-A: `SimpleXMLElementClassPropertyReflectionExtension`'s
                    // real-PHPStan equivalent - every undeclared property
                    // on `SimpleXMLElement` (or a subclass) is magically
                    // readable via `__get`, resolving to the *actual*
                    // concrete class being queried (so a subclass gets
                    // `MyXml|null`, not a hardcoded `SimpleXMLElement|null`)
                    // rather than degrading all the way to `Mixed`.
                    if self.registry.is_subclass_of(class, "SimpleXMLElement") {
                        union_types(Type::Object(class.clone(), Vec::new()), Type::Null)
                    } else {
                        Type::Mixed
                    }
                }),
            Type::EnumCase(_, case) if prop == "name" => Type::StringLiteral(case.clone()),
            Type::EnumCase(fqn, case) if prop == "value" => self
                .registry
                .enum_case_value(fqn, case)
                .cloned()
                .or_else(|| {
                    self.registry
                        .lookup_property(fqn, "value")
                        .map(|p| p.ty.clone())
                })
                .unwrap_or(Type::Mixed),
            Type::EnumCase(fqn, _) => self
                .registry
                .lookup_property_with_owner(fqn, prop)
                .map(|(owner, p)| self.resolve_pseudo(&p.ty, &owner))
                .unwrap_or(Type::Mixed),
            Type::ObjectShape(fields) => fields
                .iter()
                .find(|(name, _, _)| name == prop)
                .map(|(_, ty, _)| ty.clone())
                .unwrap_or(Type::Mixed),
            // Real refinement made this session, while adding the
            // identical `Union`/`Intersection` handling to `infer_expr`'s
            // own `MethodCall` arm below (`method_call_return_type`): an
            // `Intersection` value genuinely *is* every member type
            // simultaneously, so the first member whose declaration
            // actually resolves the property names its one real type
            // (no other member could disagree without the source itself
            // being contradictory). A `Union` value is only *one* of its
            // members at a time, so the honest answer is the union of
            // whichever members' own declarations resolve at all, not
            // just the first - `A|B` where `A::$prop: int` and `B::
            // $prop: string` is really `int|string`, not just `int`.
            // Skips (doesn't guess) a member that doesn't resolve at all,
            // matching this project's usual leniency for a real, if
            // incomplete, per-member answer rather than collapsing back
            // to `Mixed`.
            //
            // A second real bug found and fixed this session, in the
            // `Intersection` arm specifically: `narrow_instanceof_true`
            // (`checker/narrowing.rs`) always builds `Intersection(vec![
            // original_type, target_type])` - the pre-existing, broader
            // type first, the just-checked, narrower `instanceof` target
            // last. Searching members in that same (forward) order means
            // a same-named property declared on *both* the broader
            // original type and the narrower target - a real, if rare,
            // shape confirmed via a real corpus false positive
            // (`Rector\Php54\...\RemoveZeroBreakContinueRector`'s own
            // `$staticType instanceof ConstantIntegerType`, where a
            // shared `PHPStan\Type\Type`-family property/method the
            // narrower type itself overrides was found on the *broader*
            // interface first) - would resolve to the broader type's own
            // less-specific declaration instead of the narrower,
            // presumably-intended one. Reversed: the narrower type is
            // always checked first, which is never wrong (a class's own
            // `lookup_property_with_owner` already walks its full
            // ancestor chain, so this can only ever find a *more*
            // specific answer, or fall through to the broader type
            // exactly as before when the narrower one doesn't have it).
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| {
                    let ty = self.property_access_type(m, prop);
                    (ty != Type::Mixed).then_some(ty)
                })
                .unwrap_or(Type::Mixed),
            Type::Union(members) => members
                .iter()
                .map(|m| self.property_access_type(m, prop))
                .filter(|ty| *ty != Type::Mixed)
                .reduce(union_types)
                .unwrap_or(Type::Mixed),
            _ => Type::Mixed,
        }
    }

    /// `infer_expr`'s own `ExprKind::MethodCall` return-type resolution -
    /// extracted so it can recurse through `Union`/`Intersection`. Real
    /// bug found and fixed this session, the highest-impact instance of
    /// this project's own `Union`/`Intersection` anti-pattern found so
    /// far: this used to be inlined directly into the `MethodCall` arm
    /// with no `Union`/`Intersection` case at all, silently collapsing
    /// *any* method call's own return type to `Type::Mixed` whenever the
    /// receiver was one - unlike `check_expr`'s own separate `MethodCall`
    /// arm, which already extracts a representative class from `Union`/
    /// `Intersection` for its own diagnostic purposes. `infer_expr` is
    /// the core type-*inference* path (used whenever a method call's
    /// result type needs computing without also running full
    /// diagnostics, e.g. as a sub-expression of a larger one), so this
    /// was reachable far more often than any of this session's other
    /// fixes in this same family. `Intersection` keeps "first member
    /// whose declaration actually resolves wins" (the value genuinely is
    /// every member simultaneously, so any one real declaration names its
    /// one true type); `Union` unions every resolving member's own return
    /// type together instead, matching `property_access_type`'s own
    /// identical, more-correct distinction (a `Union` value is only ever
    /// *one* of its members at a time, so calling the same method name on
    /// two different possible runtime types can genuinely return two
    /// different types).
    pub(crate) fn method_call_return_type(
        &self,
        recv_ty: &Type,
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        match recv_ty {
            Type::Object(class, _) if is_doctrine_query_get_result(self, class, method) => {
                doctrine_query_result_list_type()
            }
            // Phase 78: `DateTimeInterface::format()`'s own already-
            // shipped `date()` format-character table applies verbatim -
            // real, substantial corpus usage (dozens of literal
            // `->format('...')` sites). Falls back to the stub's own
            // plain `string` (unchanged) whenever the format isn't a
            // literal, or contains a character the table doesn't
            // confidently classify.
            Type::Object(class, _)
                if method == "format"
                    && self.registry.is_subclass_of(class, "DateTimeInterface") =>
            {
                args.first()
                    .and_then(|a| literal_string_value(&a.value))
                    .and_then(|fmt| date_format_return_type(&fmt))
                    .unwrap_or(Type::String)
            }
            // Phase 78: `DateInterval::format()`'s own hand-rolled
            // specifier table - see `date_interval_format_return_type`'s
            // own doc comment for why the zero-valued representative
            // substitution is genuinely sound (not just an
            // approximation) for the fixed-width specifiers.
            Type::Object(class, _)
                if method == "format" && self.registry.is_subclass_of(class, "DateInterval") =>
            {
                args.first()
                    .and_then(|a| literal_string_value(&a.value))
                    .and_then(|fmt| date_interval_format_return_type(&fmt))
                    .unwrap_or(Type::String)
            }
            // Phase 78: `DateTimeInterface::diff()` narrows the stub's own
            // plain `DateInterval` return type to also carry a real,
            // always-int `days` property - mirrors real PHPStan's own
            // tiny stub-file override (this project deliberately never
            // vendors PHPStan's own stub files, so reproduced directly
            // here instead). Real, direct corpus usage with no clamping
            // user-declared return type.
            Type::Object(class, _)
                if method == "diff" && self.registry.is_subclass_of(class, "DateTimeInterface") =>
            {
                Type::Intersection(vec![
                    Type::Object("DateInterval".to_string(), Vec::new()),
                    Type::ObjectShape(vec![("days".to_string(), Type::Int, false)]),
                ])
            }
            // Phase 78: `SimpleXMLElement::asXML()`'s real 0-arg/1-arg
            // overload split (verified against real PHPStan's own
            // `SimpleXMLElementAsXMLMethodReturnTypeExtension`, whose
            // arity check is the *opposite* direction the design doc's
            // own paraphrase had it: exactly one argument - a filename to
            // write to - returns `bool` (write success/failure); zero
            // arguments (or any other count) returns the XML itself,
            // `string|false`). Deliberately doesn't extend to the
            // separate `saveXML()` alias method, matching real PHPStan's
            // own narrower scope.
            Type::Object(class, _)
                if method == "asXML" && self.registry.is_subclass_of(class, "SimpleXMLElement") =>
            {
                if args.len() == 1 {
                    Type::Bool
                } else {
                    Type::Union(vec![Type::String, Type::False])
                }
            }
            Type::Object(class, type_args) => self
                .registry
                .lookup_method_with_owner(class, method)
                .map(|(owner, sig)| {
                    // Class-level substitution (Collection<User>'s own
                    // `T`, or an inherited method's owner-level `T` via
                    // Phase 10.5) runs first, then infer_call_return_
                    // type's existing per-call-argument binding handles
                    // any template the *method itself* separately
                    // declares - the two are independent parameter scopes
                    // that can both appear on the same signature.
                    let sig = self.substitute_class_templates(sig, class, type_args, &owner);
                    let resolved_args = resolve_call_args(args, &sig.params);
                    let resolved = self.infer_call_return_type(&sig, &resolved_args, locals);
                    self.resolve_pseudo(&resolved, class)
                })
                .unwrap_or(Type::Mixed),
            // Phase 79: an enum case is a real instance of its enum for
            // method-call purposes (see `member_confirmed_missing`'s
            // identical `EnumCase` arm) - no real template-args list to
            // carry through, same as every other synthesized enum-
            // instance type.
            Type::EnumCase(fqn, _) => self
                .registry
                .lookup_method_with_owner(fqn, method)
                .map(|(owner, sig)| {
                    let resolved_args = resolve_call_args(args, &sig.params);
                    let resolved = self.infer_call_return_type(sig, &resolved_args, locals);
                    self.resolve_pseudo(&resolved, &owner)
                })
                .unwrap_or(Type::Mixed),
            // Real bug found and fixed this session, the exact same
            // shape as `property_access_type`'s own identical fix (see
            // its own doc comment for the full real corpus trace): the
            // narrower, just-`instanceof`-checked type is always the
            // *last* member `narrow_instanceof_true` puts into an
            // `Intersection`, so members must be searched in reverse to
            // prefer its own (possibly overriding) signature over the
            // broader original type's.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| {
                    let ty = self.method_call_return_type(m, method, args, locals);
                    (ty != Type::Mixed).then_some(ty)
                })
                .unwrap_or(Type::Mixed),
            Type::Union(members) => members
                .iter()
                .map(|m| self.method_call_return_type(m, method, args, locals))
                .filter(|ty| *ty != Type::Mixed)
                .reduce(union_types)
                .unwrap_or(Type::Mixed),
            _ => Type::Mixed,
        }
    }
}
