use crate::checker::{Checker, TemplateBindings};
use crate::registry::{AssertInfo, FunctionSig, ParamSig};
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Thin delegation to `Registry::class_template_bindings` (Phase 54
    /// hoisted the real logic there, alongside `is_assignable`, since
    /// neither ever needed anything but `self.registry`/`self.classes` -
    /// kept here too since real call sites (`checker/assignment.rs`,
    /// `checker/expr.rs`) already call it as `self.class_template_
    /// bindings(...)`). See `Registry::class_template_bindings`'s own doc
    /// comment for the full binding/ancestor-walk semantics.
    pub(crate) fn class_template_bindings(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
    ) -> TemplateBindings {
        self.registry
            .class_template_bindings(class, type_args, target)
    }

    /// Applies `class_template_bindings`' substitution to every param and
    /// the return type of a method signature - the class-level counterpart
    /// to `infer_call_return_type`'s existing per-call-argument template
    /// binding (which only ever sees a method's *own* `@template`, not one
    /// declared on the enclosing class). `owner` is whichever ancestor of
    /// `class` actually declares `sig` (from `lookup_method_with_owner`/
    /// `lookup_property_with_owner`) - `class` itself when it's not
    /// inherited.
    ///
    /// Scoped to only `TemplateParam`s whose own `scope` field is
    /// genuinely `owner` itself - not merely same-*named* as one of
    /// `owner`'s declared templates. `types::substitute_template`'s own
    /// fallback for a name absent from `bindings` is `Type::Mixed`,
    /// correct for a genuinely class-level template referenced with no or
    /// fewer type arguments than it declares (e.g. a bare `Collection`
    /// with no `<T>` at all), but a real Piwigo-corpus round found this
    /// wiping out every method's own separate template too the moment it
    /// was called on a non-generic class (a method's own generic
    /// identity-style function, declared entirely on its own docblock, on
    /// a class with zero templates of its own). That class has zero
    /// class-level templates, so `bindings` was always empty, and this
    /// substitution runs first (per this function's own doc comment
    /// above), replacing the method's own bare template name with `Mixed`
    /// before `infer_call_return_type`'s later, correct per-call-argument
    /// pass ever got a chance to bind it for real.
    ///
    /// Real bug found and fixed this session: the original fix here
    /// scoped by bare *name* (a `HashSet<String>` built from `owner`'s own
    /// `ClassInfo::templates` name list), not by real declaration-site
    /// identity - so a method whose own `@template` happens to share its
    /// enclosing class's own template *name* (real, common: PHPStorm's
    /// own `ReflectionClass`'s class-level `@template T of object` and its
    /// `getAttributes()` method's own, entirely unrelated `@template T`)
    /// still got eagerly captured and replaced with `Mixed` here, before
    /// the method's own call-site binding ever ran - confirmed via a real
    /// `(new ReflectionClass($x))->getAttributes(Foo::class)` isolation.
    /// `TemplateParam::scope` already carries exactly the right identity
    /// to distinguish the two (`build_template_params`'s callers pass the
    /// declaring class's own name for a class-level template, the
    /// declaring function/method's own name for one of its own - see that
    /// field's own doc comment) - comparing against it directly, instead
    /// of a same-named `HashSet`, is strictly more correct and requires no
    /// new data.
    pub(crate) fn substitute_class_templates(
        &self,
        sig: &FunctionSig,
        class: &str,
        type_args: &[Type],
        owner: &str,
    ) -> FunctionSig {
        // `(scope, bindings)` pairs to try, in order - almost always
        // exactly one (`owner` itself, the overwhelmingly common case of
        // a class directly declaring its own `@template`), occasionally
        // more than one (see the `else` branch below), and empty for the
        // overwhelmingly common non-generic case, the real fast path this
        // whole structure exists to preserve (found via a perf-
        // optimization investigation workflow, verified against this
        // file's own real source before implementing) - `Registry::
        // class_template_bindings_rec`'s own `zip(owner.templates,
        // type_args)` is always empty when the left side is, so every
        // `substitute_scoped` call below would be a no-op walk that still
        // clones the whole signature (every `ParamSig`, the return/
        // throws/assert types); bailing straight to `sig.clone()` instead
        // skips that for real.
        let mut scoped_bindings: Vec<(String, TemplateBindings)> = Vec::new();

        if self
            .registry
            .classes
            .get(owner)
            .is_some_and(|c| !c.templates.is_empty())
        {
            scoped_bindings.push((
                owner.to_string(),
                self.class_template_bindings(class, type_args, owner),
            ));
        } else if signature_references_any_template(sig) {
            // A real bug found via a Piwigo/real-PHPStan corpus diff:
            // `owner` (the class `lookup_method_with_owner` says
            // *lexically* declares this signature) can itself declare no
            // `@template` at all, yet the signature it declares can still
            // reference a template scoped to one of `owner`'s *own*
            // ancestors - not through inheriting the method unedited (the
            // case the fast path above already covers via `owner` being
            // that ancestor directly), but through `owner` itself
            // directly re-declaring/implementing a generic ancestor's own
            // templated method (required, since it's implementing an
            // interface method) while that method's own effective type -
            // via ordinary docblock-inheritance backfill from the
            // ancestor's own tag - still carries a `TemplateParam` scoped
            // to that ancestor, not to `owner`. Confirmed live:
            // `MessengerFactory::transport`'s own `(new
            // DoctrineTransportFactory($registry))->createTransport(...)`
            // - `DoctrineTransportFactory` declares no `@template` of its
            // own, but its own `@implements TransportFactoryInterface
            // <DoctrineTransport>` tag correctly binds
            // `TransportFactoryInterface`'s own `@template-covariant
            // TTransport`/`@return TTransport` to `DoctrineTransport` in
            // real PHPStan (confirmed via `dumpType()`), lost here before
            // this fix since `owner` ("DoctrineTransportFactory") itself
            // has no templates, and the old fast path bailed on exactly
            // that check alone.
            //
            // Each of `owner`'s own ancestors (not `class`'s - `owner`
            // itself declares no template, so its OWN `@implements`/
            // `@extends` type-argument list is what resolves any
            // ancestor's template, independent of whatever the original
            // call site's own `class`/`type_args` happen to be) that
            // itself declares a real `@template` contributes its own
            // `(scope, bindings)` pair - `class_template_bindings` already
            // returns an empty map when `owner` doesn't actually reach
            // that ancestor with a real type argument (an erased/bare
            // reference), which the `is_empty()` filter below turns into
            // "contributes nothing," not a wrongly-populated one.
            for ancestor in self.registry.all_ancestor_names(owner) {
                if !self
                    .registry
                    .classes
                    .get(&ancestor)
                    .is_some_and(|c| !c.templates.is_empty())
                {
                    continue;
                }
                let bindings = self.class_template_bindings(owner, &[], &ancestor);
                if !bindings.is_empty() {
                    scoped_bindings.push((ancestor, bindings));
                }
            }
        }

        if scoped_bindings.is_empty() {
            return sig.clone();
        }
        let subst = |ty: &Type| {
            scoped_bindings
                .iter()
                .fold(ty.clone(), |acc, (scope, bindings)| {
                    substitute_scoped(&acc, bindings, scope)
                })
        };
        FunctionSig {
            params: sig
                .params
                .iter()
                .map(|p| ParamSig {
                    ty: subst(&p.ty),
                    ty_native: subst(&p.ty_native),
                    out_ty: p.out_ty.as_ref().map(&subst),
                    ..p.clone()
                })
                .collect(),
            return_type: subst(&sig.return_type),
            return_type_native: subst(&sig.return_type_native),
            return_has_own_doc: sig.return_has_own_doc,
            return_case_refs: sig.return_case_refs.clone(),
            return_explicit_mixed_hint: sig.return_explicit_mixed_hint,
            no_discard: sig.no_discard,
            visibility: sig.visibility.clone(),
            is_internal: sig.is_internal,
            is_api: sig.is_api,
            has_override_attr: sig.has_override_attr,
            deprecated: sig.deprecated.clone(),
            throws: sig.throws.as_ref().map(subst),
            assert_if_true: substitute_assert_infos(&sig.assert_if_true, &subst),
            assert_if_false: substitute_assert_infos(&sig.assert_if_false, &subst),
            assert: substitute_assert_infos(&sig.assert, &subst),
            body: sig.body.clone(),
            purity_dependencies: sig.purity_dependencies.clone(),
            is_transitively_pure: sig.is_transitively_pure,
            is_stub: sig.is_stub,
            is_magic: sig.is_magic,
        }
    }
}

/// Applies the same class-template substitution `substitute_class_
/// templates` computes for every other declared type on this same
/// signature to a `@phpstan-assert`-family list's own asserted type -
/// only `ty` needs it; `target`/`negated` name a real parameter/`$this`
/// chain, never a type, so they're carried through unchanged.
fn substitute_assert_infos(
    infos: &[AssertInfo],
    subst: &impl Fn(&Type) -> Type,
) -> Vec<AssertInfo> {
    infos
        .iter()
        .map(|info| AssertInfo {
            negated: info.negated,
            ty: subst(&info.ty),
            target: info.target.clone(),
            case_refs: info.case_refs.clone(),
        })
        .collect()
}

/// Same structural walk as `types::substitute_template`, except a
/// `TemplateParam` whose own `scope` field isn't `scope` (`owner`'s own
/// name) is left completely untouched instead of defaulting to `Mixed` -
/// see `substitute_class_templates`'s own doc comment for why that
/// distinction (and comparing by declaration-site identity, not bare
/// name) matters.
fn substitute_scoped(ty: &Type, bindings: &TemplateBindings, scope: &str) -> Type {
    match ty {
        Type::TemplateParam(tp) => {
            if tp.scope.as_deref() == Some(scope) {
                // Phase 54: mirrors `types::substitute_template`'s own
                // `@template T = Default` fallback - an "erased" class
                // reference (fewer type arguments than declared,
                // including a bare one with none at all) leaves `bindings`
                // with no entry for this name at all, same as before; the
                // only change is falling back to this template's own
                // declared default (when it has one) instead of
                // unconditionally `Mixed`.
                bindings.get(&tp.name).cloned().unwrap_or_else(|| {
                    tp.default
                        .as_ref()
                        .map(|d| substitute_scoped(d, bindings, scope))
                        .unwrap_or(Type::Mixed)
                })
            } else {
                ty.clone()
            }
        }
        // Phase 50: substitute through the key type too, not reset
        // to "unknown" every time (matches types::substitute_template's
        // own Array-arm fix).
        Type::Array(key, inner) => Type::Array(
            Box::new(substitute_scoped(key, bindings, scope)),
            Box::new(substitute_scoped(inner, bindings, scope)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(substitute_scoped(inner, bindings, scope)))
        }
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| substitute_scoped(m, bindings, scope))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| substitute_scoped(m, bindings, scope))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_scoped(t, bindings, scope), *opt))
                .collect(),
        ),
        // A real, previously-missing case, found via this session's own
        // corpus diff (`php-standard-library`'s own `ContainerType<Tk as
        // array-key, Tv>`, whose own `Tk`/`Tv` reach a caller's return
        // type nested inside `object{...}` by way of an *interface's*
        // own template): `object{...}`'s own field types were never
        // substituted here either, mirroring `types::substitute_
        // template`'s own identical fix.
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_scoped(t, bindings, scope), *opt))
                .collect(),
        ),
        // Phase 54: mirrors `types::substitute_template`'s own identical,
        // previously-missing fix - `Object`'s own generic argument list
        // wasn't recursed into at all, so a class-level template used
        // inside e.g. `Box<T>` never actually got substituted.
        Type::Object(name, args) => Type::Object(
            name.clone(),
            args.iter()
                .map(|a| substitute_scoped(a, bindings, scope))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether `ty` contains a `TemplateParam` *anywhere*, regardless of its
/// own `scope` - the cheap gate `substitute_class_templates`'s own
/// inherited-ancestor-template fallback uses before paying for an
/// `all_ancestor_names` graph walk, so an ordinary non-generic signature
/// (the overwhelming majority) never reaches it. Mirrors `substitute_
/// scoped`'s own exact recursive shape (same `Type` variants), since a
/// template this doesn't look inside couldn't be substituted by that
/// function either.
fn type_references_template(ty: &Type) -> bool {
    match ty {
        Type::TemplateParam(_) => true,
        Type::Array(key, inner) => type_references_template(key) || type_references_template(inner),
        Type::ClassString(inner) => type_references_template(inner),
        Type::Union(members) | Type::Intersection(members) => {
            members.iter().any(type_references_template)
        }
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => {
            fields.iter().any(|(_, t, _)| type_references_template(t))
        }
        Type::Object(_, args) => args.iter().any(type_references_template),
        _ => false,
    }
}

/// Whether any param (its merged or native type, or `@param-out` type) or
/// the return type (merged or native) of `sig` references a `TemplateParam`
/// at all - `throws`/asserts aren't checked, matching the existing
/// perf-sensitive fast path's own scope (a `@throws`/`@phpstan-assert`
/// tag referencing an ancestor's own template with no matching param/
/// return reference anywhere is not a real shape this project's own
/// corpus work has found).
fn signature_references_any_template(sig: &FunctionSig) -> bool {
    sig.params.iter().any(|p| {
        type_references_template(&p.ty)
            || type_references_template(&p.ty_native)
            || p.out_ty.as_ref().is_some_and(type_references_template)
    }) || type_references_template(&sig.return_type)
        || type_references_template(&sig.return_type_native)
}
