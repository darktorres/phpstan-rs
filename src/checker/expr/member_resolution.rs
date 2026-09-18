use crate::ast::*;
use crate::checker::expr::builtin_calls::{
    date_format_return_type, date_interval_format_return_type,
};
use crate::checker::expr::doctrine_query_result_list_type;
use crate::checker::expr::doctrine_query_result_list_type_of;
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
    /// Returns a reference tied to the registry's lifetime, not `&self`'s,
    /// so holding it doesn't conflict with the `&mut self` calls that
    /// follow (see `lookup_own_signature`'s identical reasoning).
    pub(crate) fn find_property_owner(
        &self,
        base_ty: &Type,
        prop: &str,
    ) -> Option<(std::sync::Arc<str>, &'a PropertySig)> {
        match base_ty {
            Type::Object(class, _) => self.registry.lookup_property_with_owner(class, prop),
            Type::EnumCase(fqn, _) => self.registry.lookup_property_with_owner(fqn, prop),
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.find_property_owner(m, prop)),
            // `narrow_instanceof_true` always puts the narrower,
            // just-`instanceof`-checked type last in an `Intersection` -
            // see `property_access_type`'s own doc comment. Reversed so
            // that narrower type's own declaration (if any) is
            // preferred over the broader original type's.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| self.find_property_owner(m, prop)),
            _ => None,
        }
    }

    /// `find_property_owner`'s own method counterpart, used by
    /// `checker::calls::check_no_discard_statement`: a `#[\NoDiscard]`-marked
    /// method called through a `Union`/`Intersection` receiver (a value
    /// narrowed via `instanceof` from an unrelated interface) must not
    /// be silently skipped just because the receiver's own type isn't
    /// literally `Type::Object`. Same registry-tied lifetime as
    /// `find_property_owner` above, for the same reason.
    pub(crate) fn find_method_owner(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> Option<(std::sync::Arc<str>, &'a FunctionSig)> {
        match base_ty {
            Type::Object(class, _) => self.registry.lookup_method_with_owner(class, method),
            Type::EnumCase(fqn, _) => self.registry.lookup_method_with_owner(fqn, method),
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.find_method_owner(m, method)),
            // Same reasoning as `find_property_owner` just above.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| self.find_method_owner(m, method)),
            _ => None,
        }
    }

    /// `find_method_owner`'s own sibling, used by `ExprKind::
    /// FirstClassCallable`'s own `MethodCall` arm: returns the concrete
    /// receiver class actually queried alongside the signature, not the
    /// (possibly different, inherited-from) declaring owner - late static
    /// binding needs the real runtime receiver a `self`/`static` pseudo-
    /// return type resolves against, via a plain `lookup_method`, never
    /// `_with_owner`. Must not require the receiver's own type to be
    /// literally `Type::Object` - a `Union`/`Intersection` receiver
    /// needs the same treatment. Same registry-tied lifetime as
    /// `find_property_owner` above, for the same reason.
    pub(crate) fn find_method_receiver_class(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> (Option<&'a FunctionSig>, Option<String>) {
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
            // Same reasoning as `find_property_owner`'s own doc
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
    /// Throw` (`checker/stmt.rs`) and `ExprKind::Throw` just below -
    /// must not require the thrown value's own type to be literally
    /// `Type::Object`: a value narrowed via `instanceof` from an
    /// unrelated interface, then thrown, is exactly as real a shape here
    /// as it was for property access. A `Union` member fails if any one
    /// of them isn't Throwable (the runtime value could be that one); an
    /// `Intersection` fails only if every member is confirmed non-
    /// Throwable (any single Throwable member is enough to satisfy
    /// `throw`) - mirrors `member_confirmed_missing`'s own identical
    /// Union/Intersection asymmetry exactly.
    ///
    /// `class_name_exists` is checked first, same as `StmtKind::Try`'s
    /// own multi-catch handling a few hundred lines up - `is_subclass_of`
    /// silently treats a name the registry has no entry for as "not a
    /// subclass of Throwable", indistinguishable from a real, known,
    /// non-Throwable class. This guard matters for real PHP multi-catch,
    /// which binds a real union type: a caught interface that happens to
    /// live in an unresolvable/unscanned vendored package (e.g. a
    /// PHP-Scoper-prefixed name this project's vendor scan never indexes
    /// a declaration for) must not be reported as not implementing
    /// `Throwable`, even though the real, unprefixed interface genuinely
    /// does.
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
    /// assignment::check_property_assignment`, the identical shape as the
    /// read-side one just above: the write side must not match
    /// `Type::Object` directly, silently skipping every one of its checks
    /// (readonly, visibility,
    /// `@internal`/`@deprecated`, and the assigned value's own type) for
    /// a `Union`/`Intersection`-typed target. Also returns the matching
    /// member's own class/type-args (not just the ultimate declaring
    /// owner) - `class_template_bindings` needs the receiver's own real
    /// generic parameterization to substitute correctly up to whichever
    /// ancestor actually declares the property, the same relationship
    /// the original single-`Object` code already relied on.
    pub(crate) fn find_property_receiver(
        &self,
        base_ty: &Type,
        prop: &str,
    ) -> Option<(String, Vec<Type>, std::sync::Arc<str>, PropertySig)> {
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
            // Same reasoning as `find_property_owner`'s own doc
            // comment above.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| self.find_property_receiver(m, prop)),
            _ => None,
        }
    }

    /// `find_property_receiver`'s own method counterpart, used by
    /// `check_expr`'s own `MethodCall` arm: `recv_class` extraction (a
    /// plain "first Object/EnumCase member, regardless of whether it
    /// actually declares the method" pick, kept as-is purely to gate
    /// whether the existence-check block below even runs at all) must
    /// not also be used to resolve the method's own real signature,
    /// owner, argument-type checking, visibility/`@internal`/
    /// `@deprecated`, the Doctrine query special case, and the final
    /// return type - all of which need a member that actually declares
    /// the method, in the correct (`Intersection` reversed) order,
    /// exactly like `find_method_owner`/`method_call_return_type`
    /// already get right. Returns an already-class-template-substituted
    /// `FunctionSig` so the caller can freely call further `&mut self`
    /// methods afterward - `Cow`, not a plain owned `FunctionSig`, since
    /// `substitute_class_templates` itself only actually needs to
    /// allocate a new one when real substitution happened; the
    /// overwhelmingly common non-generic case borrows straight from the
    /// registry (tied to `'a`, so it's just as safe to hold across those
    /// later `&mut self` calls as an owned value would be).
    ///
    /// Substitution happens right here, inline in the leaf arms, rather
    /// than as a separate step the caller applies afterward - a separate
    /// step would clone `sig` here and then have
    /// `substitute_class_templates`'s own non-generic fast path clone it
    /// again, fully discarding the first clone. Mirrors
    /// `method_call_return_type`'s own already-correct "substitute once,
    /// right after the lookup" shape.
    #[allow(clippy::type_complexity)]
    pub(crate) fn find_method_receiver(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> Option<(
        String,
        Vec<Type>,
        std::sync::Arc<str>,
        std::borrow::Cow<'a, FunctionSig>,
    )> {
        match base_ty {
            Type::Object(class, type_args) => self
                .registry
                .lookup_method_with_owner(class, method)
                .map(|(owner, sig)| {
                    let sig = self.substitute_class_templates(sig, class, type_args, &owner);
                    (class.clone(), type_args.clone(), owner, sig)
                }),
            Type::EnumCase(fqn, _) => {
                self.registry
                    .lookup_method_with_owner(fqn, method)
                    .map(|(owner, sig)| {
                        let sig = self.substitute_class_templates(sig, fqn, &[], &owner);
                        (fqn.clone(), Vec::new(), owner, sig)
                    })
            }
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

    /// Both `check_expr`'s and `infer_expr`'s own `PropertyAccess` arms
    /// must not match `base_ty` against `Object`/`EnumCase`/`ObjectShape`
    /// directly with no `Union`/`Intersection` arm - falling to the
    /// `_ => Type::Mixed` catch-all would discard every bit of type
    /// information whenever the base's own type happens to be a
    /// `Union`/`Intersection` (a common shape: `narrow_instanceof_true`'s
    /// own deliberate design intersects an unrelated-looking `instanceof`
    /// target with the original type - see its own doc comment - and a
    /// plain `A|B` property/parameter type is exactly as common).
    /// Recurses through both, trying each member in turn - correct for
    /// an `Intersection` (the property only needs to exist on one
    /// member) and a reasonable, consistent best-effort for a `Union`
    /// (matches this project's own general "fewer false positives over
    /// more precision" bias elsewhere). See the `Union`/`Intersection`
    /// arms below for the real distinction between them.
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
                    // `SimpleXMLElementClassPropertyReflectionExtension`'s
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
            // An `Intersection` value genuinely is every member type
            // simultaneously, so the first member whose declaration
            // actually resolves the property names its one real type
            // (no other member could disagree without the source itself
            // being contradictory). A `Union` value is only one of its
            // members at a time, so the honest answer is the union of
            // whichever members' own declarations resolve at all, not
            // just the first - `A|B` where `A::$prop: int` and `B::
            // $prop: string` is really `int|string`, not just `int`.
            // Skips (doesn't guess) a member that doesn't resolve at all,
            // matching this project's usual leniency for a real, if
            // incomplete, per-member answer rather than collapsing back
            // to `Mixed`.
            //
            // In the `Intersection` arm specifically: `narrow_instanceof_
            // true` (`checker/narrowing.rs`) always builds
            // `Intersection(vec![original_type, target_type])` - the
            // pre-existing, broader type first, the just-checked,
            // narrower `instanceof` target last. Searching members in
            // that same (forward) order would mean a same-named property
            // declared on both the broader original type and the
            // narrower target resolves to the broader type's own
            // less-specific declaration instead of the narrower,
            // presumably-intended one. Reversed instead: the narrower
            // type is always checked first, which is never wrong (a
            // class's own `lookup_property_with_owner` already walks its
            // full ancestor chain, so this can only ever find a more
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
    /// extracted so it can recurse through `Union`/`Intersection`, rather
    /// than silently collapsing any method call's own return type to
    /// `Type::Mixed` whenever the receiver is one. `infer_expr` is the
    /// core type-inference path (used whenever a method call's result
    /// type needs computing without also running full diagnostics, e.g.
    /// as a sub-expression of a larger one). `Intersection` keeps "first
    /// member whose declaration actually resolves wins" (the value
    /// genuinely is every member simultaneously, so any one real
    /// declaration names its one true type); `Union` unions every
    /// resolving member's own return type together instead, matching
    /// `property_access_type`'s own identical, more-correct distinction
    /// (a `Union` value is only ever one of its members at a time, so
    /// calling the same method name on two different possible runtime
    /// types can genuinely return two different types).
    pub(crate) fn method_call_return_type(
        &self,
        recv: &Expr,
        recv_ty: &Type,
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        match recv_ty {
            // `EntityManager::createQuery($dql)` (PHASE_BACKLOG Phase 168
            // 168g) - the same DQL-aware row-shape inference as
            // `QueryBuilder::getQuery()` just below, fed a literal DQL
            // string directly instead of a `QueryBuilder`-reconstructed
            // one. Not corpus-confirmed necessary (the real corpus's only
            // literal-DQL-string usages are `DELETE`/`UPDATE` DML, never
            // `SELECT`), but the same select-item grammar doing strictly
            // less work, so building it keeps the feature honestly
            // general rather than `QueryBuilder`-only.
            Type::Object(class, _)
                if method == "createQuery"
                    && self
                        .registry
                        .is_subclass_of(class, "Doctrine\\ORM\\EntityManagerInterface") =>
            {
                let row_type = args.first().and_then(|arg| {
                    let dql = match literal_string_value(&arg.value) {
                        Some(s) => Some(s.into_owned()),
                        None => match self.infer_expr(&arg.value, locals) {
                            Type::StringLiteral(s) => Some(s),
                            _ => None,
                        },
                    }?;
                    let (chain, items) =
                        crate::checker::doctrine::dql::parse_select_statement(&dql)?;
                    crate::checker::doctrine::result_shape::build_result_row_type(
                        self, &chain, &items,
                    )
                });
                match row_type {
                    Some(row_type) => {
                        Type::Object("Doctrine\\ORM\\Query".to_string(), vec![row_type])
                    }
                    None => Type::Object("Doctrine\\ORM\\Query".to_string(), Vec::new()),
                }
            }
            // `QueryBuilder::getQuery()` (PHASE_BACKLOG Phase 168 168e) -
            // resolves the fluent chain (168b), parses its own select
            // items (168c) and builds the row shape (168d), carrying it
            // as `Doctrine\ORM\Query`'s own generic-argument slot (see
            // `doctrine_query_row_type`'s own doc comment for why this
            // reuses `Type::Object`'s existing slot rather than a new
            // `Type` variant). An undecidable chain/select falls back to
            // today's plain, argument-less `Query` object - no worse
            // than before this phase existed.
            Type::Object(class, _)
                if method == "getQuery"
                    && self
                        .registry
                        .is_subclass_of(class, "Doctrine\\ORM\\QueryBuilder") =>
            {
                let row_type = self
                    .resolve_query_builder_chain(recv, locals)
                    .and_then(|chain| {
                        // A `->delete(...)`/`->update(...)` chain never
                        // produces row data at all (see `QueryBuilderChain::
                        // dml`'s own doc comment) - `Type::Never` marks
                        // that here, read back by `getResult`/`execute`'s
                        // own dispatch below.
                        if chain.dml {
                            return Some(Type::Never);
                        }
                        let items =
                            crate::checker::doctrine::dql::parse_select_items(&chain.select_args)?;
                        crate::checker::doctrine::result_shape::build_result_row_type(
                            self, &chain, &items,
                        )
                    });
                match row_type {
                    Some(row_type) => {
                        Type::Object("Doctrine\\ORM\\Query".to_string(), vec![row_type])
                    }
                    None => Type::Object("Doctrine\\ORM\\Query".to_string(), Vec::new()),
                }
            }
            // The five `AbstractQuery` terminal methods real
            // `phpstan-doctrine` itself resolves precisely via its own
            // `QueryResultDynamicReturnTypeExtension` (PHASE_BACKLOG
            // Phase 168 168e) - checkable against that reference
            // implementation's own `dumpType()` output.
            // `getArrayResult`/`getSingleColumnResult`/
            // `getSingleScalarResult`/`getScalarResult` (168f, beyond
            // real PHPStan - none of those four get a dynamic-return-
            // type extension there either) aren't handled here yet.
            Type::Object(class, type_args)
                if matches!(
                    method,
                    "getResult"
                        | "execute"
                        | "toIterable"
                        | "getOneOrNullResult"
                        | "getSingleResult"
                ) && self
                    .registry
                    .is_subclass_of(class, "Doctrine\\ORM\\AbstractQuery") =>
            {
                let row_type = type_args.first();
                match method {
                    // `Type::Never` marks a `->delete(...)`/`->update(...)`
                    // chain (`QueryBuilderChain::dml`'s own doc comment).
                    // `getResult`/`getOneOrNullResult`/`getSingleResult`
                    // all funnel through `execute()` internally in real
                    // Doctrine (confirmed against `vendor/doctrine-orm`),
                    // which returns a plain affected-row `int` for either
                    // DML statement, bypassing hydration (and any `NoResult
                    // Exception`/`NonUniqueResultException` wrapping)
                    // entirely regardless of hydration mode - `toIterable`
                    // is the one exception (calls `_doExecute()` directly,
                    // with no such bypass, so a DML chain there is a real
                    // runtime `TypeError` in real Doctrine, not a value
                    // this project needs to type sensibly).
                    "getResult" | "execute" | "getOneOrNullResult" | "getSingleResult"
                        if matches!(row_type, Some(Type::Never)) =>
                    {
                        Type::IntRange(Some(0), None)
                    }
                    "getResult" => match row_type {
                        Some(row_type) => doctrine_query_result_list_type_of(row_type.clone()),
                        // Today's only previously-covered method - keep
                        // its existing shape-only fallback rather than
                        // regressing to bare `Mixed` when the chain
                        // itself was undecidable.
                        None => doctrine_query_result_list_type(),
                    },
                    "execute" => match resolve_hydration_mode_arg(args, 1) {
                        HydrationMode::Object => row_type
                            .cloned()
                            .map(doctrine_query_result_list_type_of)
                            .unwrap_or_else(doctrine_query_result_list_type),
                        // The other hydration modes need 168f's own
                        // per-mode transforms, not yet built.
                        _ => Type::Mixed,
                    },
                    "toIterable" => match resolve_hydration_mode_arg(args, 0) {
                        HydrationMode::Object => Type::Union(vec![
                            Type::Array(
                                Box::new(Type::unknown_array_key()),
                                Box::new(row_type.cloned().unwrap_or(Type::Mixed)),
                            ),
                            Type::Object("Traversable".to_string(), Vec::new()),
                        ]),
                        _ => Type::Mixed,
                    },
                    "getOneOrNullResult" => {
                        union_types(row_type.cloned().unwrap_or(Type::Mixed), Type::Null)
                    }
                    "getSingleResult" => row_type.cloned().unwrap_or(Type::Mixed),
                    _ => unreachable!(),
                }
            }
            // `getArrayResult`/`getSingleColumnResult`/
            // `getSingleScalarResult`/`getScalarResult` (PHASE_BACKLOG
            // Phase 168 168f) - beyond real PHPStan (168e's own stub
            // check confirms none of these four get a dynamic-return-
            // type extension in real `phpstan-doctrine` either).
            // Re-resolves the `QueryBuilder` chain directly from
            // `->getQuery()`'s own receiver rather than reading back a
            // pre-stored row type the way the five methods above do -
            // every one of these four needs its own, differently-shaped
            // hydration transform (`entity_array_shape`'s nested
            // identifier-only associations, the flat `getScalarResult`
            // key format), not a single fixed `HYDRATE_OBJECT` shape to
            // reuse. Real corpus usage is overwhelmingly chain-shaped
            // like `getResult()`'s own call sites, but not exclusively -
            // `$qb = ...; ...; $qb->getQuery()->getSingleColumnResult()`
            // is a real, confirmed Piwigo shape (`UserRepository::
            // findAuthorizedFavoriteImageIds`) this design can't resolve
            // (`resolve_query_builder_chain`'s own backward walk stops at
            // a bare variable, with no way back to the assignment that
            // built it) - degrades to `Mixed` for that one call site
            // rather than a false claim, a real, disclosed coverage gap
            // (see PHASE_BACKLOG.md), not a design defect.
            Type::Object(class, _)
                if matches!(
                    method,
                    "getArrayResult"
                        | "getSingleColumnResult"
                        | "getSingleScalarResult"
                        | "getScalarResult"
                ) && self
                    .registry
                    .is_subclass_of(class, "Doctrine\\ORM\\AbstractQuery") =>
            {
                let chain = doctrine_get_query_receiver(recv)
                    .and_then(|qb_recv| self.resolve_query_builder_chain(qb_recv, locals));
                // Every one of these four also funnels through `execute()`
                // internally in real Doctrine (confirmed directly against
                // `vendor/doctrine-orm`), so a `->delete(...)`/`->update(...)`
                // chain returns a plain affected-row `int` here too,
                // bypassing hydration regardless of which of these four was
                // actually called - see `QueryBuilderChain::dml`'s own doc
                // comment.
                if matches!(&chain, Some(c) if c.dml) {
                    return Type::IntRange(Some(0), None);
                }
                let resolved = chain.and_then(|chain| {
                    let items =
                        crate::checker::doctrine::dql::parse_select_items(&chain.select_args)?;
                    Some((chain, items))
                });
                match (method, resolved) {
                    ("getArrayResult", Some((chain, items))) => {
                        crate::checker::doctrine::result_shape::build_array_result_row_type(
                            self, &chain, &items,
                        )
                        .map(doctrine_query_result_list_type_of)
                        .unwrap_or_else(doctrine_query_result_list_type)
                    }
                    ("getSingleColumnResult", Some((chain, items))) => {
                        crate::checker::doctrine::result_shape::raw_scalar_column_type(
                            self, &chain, &items,
                        )
                        .map(doctrine_query_result_list_type_of)
                        .unwrap_or_else(doctrine_query_result_list_type)
                    }
                    ("getSingleScalarResult", Some((chain, items))) => {
                        crate::checker::doctrine::result_shape::single_scalar_column_type(
                            self, &chain, &items,
                        )
                        .unwrap_or(Type::Mixed)
                    }
                    ("getScalarResult", Some((chain, items))) => {
                        crate::checker::doctrine::result_shape::build_scalar_result_row_type(
                            self, &chain, &items,
                        )
                        .map(doctrine_query_result_list_type_of)
                        .unwrap_or_else(doctrine_query_result_list_type)
                    }
                    _ => Type::Mixed,
                }
            }
            // `DateTimeInterface::format()`'s own already-
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
            // `DateInterval::format()`'s own hand-rolled
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
            // `DateTimeInterface::diff()` narrows the stub's own
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
            // `SimpleXMLElement::asXML()`'s 0-arg/1-arg overload split
            // (mirrors PHPStan's own
            // `SimpleXMLElementAsXMLMethodReturnTypeExtension`): exactly
            // one argument - a filename to write to - returns `bool`
            // (write success/failure); zero arguments (or any other
            // count) returns the XML itself, `string|false`.
            // Deliberately doesn't extend to the
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
                    // does) runs first, then infer_call_return_
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
            // An enum case is a real instance of its enum for
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
            // The same shape as `property_access_type`'s own
            // identical handling: the narrower, just-`instanceof`-checked
            // type is always the last member `narrow_instanceof_true`
            // puts into an `Intersection`, so members must be searched
            // in reverse to prefer its own (possibly overriding)
            // signature over the broader original type's.
            Type::Intersection(members) => members
                .iter()
                .rev()
                .find_map(|m| {
                    let ty = self.method_call_return_type(recv, m, method, args, locals);
                    (ty != Type::Mixed).then_some(ty)
                })
                .unwrap_or(Type::Mixed),
            Type::Union(members) => members
                .iter()
                .map(|m| self.method_call_return_type(recv, m, method, args, locals))
                .filter(|ty| *ty != Type::Mixed)
                .reduce(union_types)
                .unwrap_or(Type::Mixed),
            _ => Type::Mixed,
        }
    }
}

/// Which of `AbstractQuery::HYDRATE_*` mode an `execute()`/`toIterable()`
/// call's own hydration-mode argument names - `Object` both when it's
/// omitted (real Doctrine's own default) and when it's the literal
/// `HYDRATE_OBJECT` constant; `Unknown` for anything this project can't
/// statically resolve (a runtime variable), which every caller treats as
/// undecidable for that call, never guessed.
enum HydrationMode {
    Object,
    Unknown,
}

fn resolve_hydration_mode_arg(args: &[Arg], index: usize) -> HydrationMode {
    match args.get(index) {
        None => HydrationMode::Object,
        Some(arg) => match &arg.value.kind {
            ExprKind::Literal(Literal::Null) => HydrationMode::Object,
            ExprKind::StaticAccess(_, member) if member == "HYDRATE_OBJECT" => {
                HydrationMode::Object
            }
            _ => HydrationMode::Unknown,
        },
    }
}

/// `recv`'s own `QueryBuilder` receiver, when `recv` is exactly a bare
/// `->getQuery()` call (`MethodCall(qb, "getQuery", [], _)`) - used by
/// the `getArrayResult`/`getSingleColumnResult`/`getSingleScalarResult`/
/// `getScalarResult` dispatch (168f) to re-resolve the fluent chain
/// directly, rather than reading back a row type `getQuery()`'s own
/// dispatch already computed and stored for a different (`HYDRATE_
/// OBJECT`) hydration mode. `None` for anything else, including a
/// variable holding an already-built `Query` - a real, disclosed gap
/// (see this function's own caller's doc comment), not a claim that
/// shape never occurs.
fn doctrine_get_query_receiver(recv: &Expr) -> Option<&Expr> {
    match &recv.kind {
        ExprKind::MethodCall(qb, method, call_args, _)
            if **method == *"getQuery" && call_args.is_empty() =>
        {
            Some(qb)
        }
        _ => None,
    }
}
