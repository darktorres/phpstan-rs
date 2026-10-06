use crate::checker::{Checker, TemplateBindings};
use crate::registry::FunctionReflection;
use types::Type;

impl<'a> Checker<'a> {
    /// Thin delegation to `Registry::class_template_bindings` - kept
    /// here too since real call sites (`checker/assignment.rs`,
    /// `checker/expr.rs`) already call it as `self.class_template_
    /// bindings(...)`. See `Registry::class_template_bindings`'s own doc
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
    /// genuinely `owner` itself - not merely same-named as one of
    /// `owner`'s declared templates. `Type::substitute_templates`'s own
    /// fallback for a name absent from `bindings` is `mixed`,
    /// correct for a genuinely class-level template referenced with no or
    /// fewer type arguments than it declares (e.g. a bare `Collection`
    /// with no `<T>` at all), but that would wipe out every method's own
    /// separate template too the moment it's called on a non-generic
    /// class (a method's own generic identity-style function, declared
    /// entirely on its own docblock, on a class with zero templates of
    /// its own): that class has zero class-level templates, so
    /// `bindings` is always empty, and this substitution runs first (per
    /// this function's own doc comment above), so scoping by real
    /// declaration-site identity (`TemplateParam::scope`, not a
    /// same-named `HashSet`) is needed to avoid replacing the method's
    /// own bare template name with `Mixed` before
    /// `infer_call_return_type`'s later, correct per-call-argument pass
    /// ever gets a chance to bind it for real - e.g. `ReflectionClass`'s
    /// class-level `@template T of object` and its `getAttributes()`
    /// method's own, entirely unrelated `@template T` must not collide
    /// just because they share a name.
    pub(crate) fn substitute_class_templates(
        &self,
        sig: &'a FunctionReflection,
        class: &str,
        type_args: &[Type],
        owner: &str,
    ) -> std::borrow::Cow<'a, FunctionReflection> {
        // `(scope, bindings)` pairs to try, in order - almost always
        // exactly one (`owner` itself, the overwhelmingly common case of
        // a class directly declaring its own `@template`), occasionally
        // more than one (see the `else` branch below), and empty for the
        // overwhelmingly common non-generic case, the real fast path this
        // whole structure exists to preserve - `Registry::
        // class_template_bindings_rec`'s own `zip(owner.templates,
        // type_args)` is always empty when the left side is, so every
        // `substitute_templates_of` call below would be a no-op walk that still
        // clones the whole signature (every `ParameterReflection`, the return/
        // throws/assert types); bailing straight to `sig.clone()` instead
        // skips that for real.
        let mut scoped_bindings: Vec<(String, TemplateBindings)> = Vec::new();

        if self
            .registry
            .classes()
            .get(owner)
            .is_some_and(|c| !c.templates().is_empty())
        {
            let mut bindings = self.class_template_bindings(class, type_args, owner);
            // An erased receiver's template is its declared default
            // (`@template T = 'array'`, so a conditional on `T` resolves),
            // or else its bound: what a value of it is surely known to be.
            if let Some(info) = self.registry.classes().get(owner) {
                let bounds = self.registry.effective_template_bounds_by_name(owner);
                for name in info.templates() {
                    if !bindings.contains_key(name.as_ref())
                        && let Some(erased) = info
                            .template_defaults()
                            .get(name)
                            .or_else(|| bounds.get(name))
                    {
                        let erased =
                            erased.substitute_templates_of(&bindings, owner, &self.relation());
                        bindings.insert(name.to_string(), erased);
                    }
                }
            }
            scoped_bindings.push((owner.to_string(), bindings));
        } else if signature_references_any_template(sig) {
            // `owner` (the class `lookup_method_with_owner` says
            // lexically declares this signature) can itself declare no
            // `@template` at all, yet the signature it declares can still
            // reference a template scoped to one of `owner`'s own
            // ancestors - not through inheriting the method unedited (the
            // case the fast path above already covers via `owner` being
            // that ancestor directly), but through `owner` itself
            // directly re-declaring/implementing a generic ancestor's own
            // templated method (required, since it's implementing an
            // interface method) while that method's own effective type -
            // via ordinary docblock-inheritance backfill from the
            // ancestor's own tag - still carries a `TemplateParam` scoped
            // to that ancestor, not to `owner`. E.g. a
            // `DoctrineTransportFactory` implementing
            // `TransportFactoryInterface<DoctrineTransport>` declares no
            // `@template` of its own, but its own `@implements` tag
            // correctly binds `TransportFactoryInterface`'s own
            // `@template-covariant TTransport`/`@return TTransport` to
            // `DoctrineTransport` in real PHPStan - this would be lost
            // if `owner` itself having no templates made the fast path
            // bail on exactly that check alone.
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
                    .classes()
                    .get(&ancestor)
                    .is_some_and(|c| !c.templates().is_empty())
                {
                    continue;
                }
                let bindings = self.class_template_bindings(owner, &[], &ancestor);
                if !bindings.is_empty() {
                    scoped_bindings.push((ancestor.to_string(), bindings));
                }
            }
        }

        if scoped_bindings.is_empty() {
            return std::borrow::Cow::Borrowed(sig);
        }
        let subst = |ty: &Type| {
            scoped_bindings
                .iter()
                .fold(ty.clone(), |acc, (scope, bindings)| {
                    acc.substitute_templates_of(bindings, scope, &self.relation())
                })
        };
        std::borrow::Cow::Owned(sig.map_types(subst))
    }

    /// Same substitution as `substitute_class_templates`, for a single
    /// bare property type rather than a whole method signature - Phase
    /// 77's reflected-member-binding feature (`checker::expr::member_
    /// resolution`'s `generic_object_property_type`) is its one consumer
    /// so far. Only covers this function's own "fast path" (`owner`
    /// itself declares a real `@template`) - the rarer "`owner` declares
    /// none of its own, but its own inherited type still carries an
    /// ancestor's template" case `substitute_class_templates`
    /// additionally handles for a method signature is not replicated
    /// here; a disclosed, narrower scope than that sibling, not a silent
    /// one.
    pub(crate) fn substitute_class_template_property_type(
        &self,
        ty: &Type,
        class: &str,
        type_args: &[Type],
        owner: &str,
    ) -> Type {
        if !self
            .registry
            .classes()
            .get(owner)
            .is_some_and(|c| !c.templates().is_empty())
        {
            return ty.clone();
        }
        let bindings = self.class_template_bindings(class, type_args, owner);
        ty.substitute_templates_of(&bindings, owner, &self.relation())
    }
}

/// Whether any param (its merged or native type, or `@param-out` type) or
/// the return type (merged or native) of `sig` references a `TemplateParam`
/// at all - `throws`/asserts aren't checked, matching the existing
/// perf-sensitive fast path's own scope (a `@throws`/`@phpstan-assert`
/// tag referencing an ancestor's own template with no matching param/
/// return reference anywhere is not a real shape this project's own
/// corpus work has found).
fn signature_references_any_template(sig: &FunctionReflection) -> bool {
    sig.params().iter().any(|p| {
        p.ty().references_template()
            || p.ty_native().references_template()
            || p.out_ty().as_ref().is_some_and(Type::references_template)
    }) || sig.return_type().references_template()
        || sig.return_type_native().references_template()
}
