use crate::checker::expr::builtin_calls::{
    date_format_return_type, date_interval_format_return_type,
};
use crate::checker::expr::doctrine_query_result_list_type;
use crate::checker::{Checker, Locals, resolve_call_args};
use crate::registry::{FunctionReflection, PropertyReflection};
use syntax::ast::*;
use types::Name;

use types::{self, Shape, ShapeField, ShapeKey, Type};

impl<'a> Checker<'a> {
    /// The declaring class/`PropertyReflection` pair for `prop` on `base_ty`,
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
    ) -> Option<(Name, &'a PropertyReflection)> {
        syntax::stack::guard(|| {
            let members = base_ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .find_map(|m| self.find_property_owner(m, prop));
            }
            // `Relation::narrow_to_instance` puts the narrower, just-`instanceof`-
            // checked type last in an intersection: reversed, so its own
            // declaration is preferred over the broader original type's.
            let parts = base_ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .rev()
                    .find_map(|m| self.find_property_owner(m, prop));
            }
            let (class, _) = base_ty.as_object()?;
            self.registry.lookup_property_with_owner(class, prop)
        })
    }

    /// `find_property_owner`'s own method counterpart, used by
    /// `checker::calls::check_no_discard_statement`: a `#[\NoDiscard]`-marked
    /// method called through a `Union`/`Intersection` receiver (a value
    /// narrowed via `instanceof` from an unrelated interface) must not
    /// be silently skipped just because the receiver's own type isn't a
    /// plain object. Same registry-tied lifetime as
    /// `find_property_owner` above, for the same reason.
    pub(crate) fn find_method_owner(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> Option<(Name, &'a FunctionReflection)> {
        syntax::stack::guard(|| {
            let members = base_ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .find_map(|m| self.find_method_owner(m, method));
            }
            // Reversed as in `find_property_owner`.
            let parts = base_ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .rev()
                    .find_map(|m| self.find_method_owner(m, method));
            }
            let (class, _) = base_ty.as_object()?;
            self.registry.lookup_method_with_owner(class, method)
        })
    }

    /// `find_method_owner`'s own sibling, used by `ExprKind::
    /// FirstClassCallable`'s own `MethodCall` arm: returns the concrete
    /// receiver class actually queried alongside the signature, not the
    /// (possibly different, inherited-from) declaring owner - late static
    /// binding needs the real runtime receiver a `self`/`static` pseudo-
    /// return type resolves against, via a plain `lookup_method`, never
    /// `_with_owner`. Must not require the receiver's own type to be a
    /// plain object - a `Union`/`Intersection` receiver
    /// needs the same treatment. Same registry-tied lifetime as
    /// `find_property_owner` above, for the same reason.
    pub(crate) fn find_method_receiver_class(
        &self,
        base_ty: &Type,
        method: &str,
    ) -> (Option<&'a FunctionReflection>, Option<String>) {
        syntax::stack::guard(|| {
            let members = base_ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .map(|m| self.find_method_receiver_class(m, method))
                    .find(|(sig, _)| sig.is_some())
                    .unwrap_or((None, None));
            }
            // An instance of every member has the most visible of their
            // declarations: a subclass implementing a public interface
            // method is public, whatever a parent keeps private.
            let parts = base_ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .rev()
                    .map(|m| self.find_method_receiver_class(m, method))
                    .filter(|(sig, _)| sig.is_some())
                    .min_by_key(|(sig, _)| sig.map(|s| visibility_rank(&s.visibility())))
                    .unwrap_or((None, None));
            }
            match base_ty.as_object() {
                Some((class, _)) => (
                    self.registry.lookup_method(class, method),
                    Some(class.to_string()),
                ),
                None => (None, None),
            }
        })
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
    /// object: a value narrowed via `instanceof` from an
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
        syntax::stack::guard(|| {
            let members = ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .find_map(|m| self.thrown_value_not_throwable(m));
            }
            let parts = ty.intersection_members();
            if parts.len() > 1 {
                let failures: Vec<String> = parts
                    .iter()
                    .filter_map(|m| self.thrown_value_not_throwable(m))
                    .collect();
                return (failures.len() == parts.len())
                    .then(|| failures.into_iter().next().unwrap());
            }
            let (class, _) = ty.as_object()?;
            (self.class_name_exists(class) && !self.registry.is_subclass_of(class, "Throwable"))
                .then(|| class.to_string())
        })
    }

    /// `find_property_owner`'s own richer sibling, used by `checker::
    /// assignment::check_property_assignment`, the identical shape as the
    /// read-side one just above: the write side must see through a
    /// union or intersection too, not only a plain object, or it would
    /// skip every one of its checks
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
    ) -> Option<(String, Vec<Type>, Name, PropertyReflection)> {
        syntax::stack::guard(|| {
            let members = base_ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .find_map(|m| self.find_property_receiver(m, prop));
            }
            // Reversed as in `find_property_owner`.
            let parts = base_ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .rev()
                    .find_map(|m| self.find_property_receiver(m, prop));
            }
            let (class, type_args) = base_ty.as_object()?;
            self.registry
                .lookup_property_with_owner(class, prop)
                .map(|(owner, sig)| (class.to_string(), type_args.to_vec(), owner, sig.clone()))
        })
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
    /// `FunctionReflection` so the caller can freely call further `&mut self`
    /// methods afterward - `Cow`, not a plain owned `FunctionReflection`, since
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
        Name,
        std::borrow::Cow<'a, FunctionReflection>,
    )> {
        syntax::stack::guard(|| {
            let members = base_ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .find_map(|m| self.find_method_receiver(m, method));
            }
            // See `find_method_receiver_class`: the most visible declaration
            // among the members.
            let parts = base_ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .rev()
                    .filter_map(|m| self.find_method_receiver(m, method))
                    .min_by_key(|(_, _, _, sig)| visibility_rank(&sig.visibility()));
            }
            let (class, type_args) = base_ty.as_object()?;
            self.registry
                .lookup_method_with_owner(class, method)
                .map(|(owner, sig)| {
                    let sig = self.substitute_class_templates(sig, class, type_args, &owner);
                    (class.to_string(), type_args.to_vec(), owner, sig)
                })
        })
    }

    /// Both `check_expr`'s and `infer_expr`'s own `PropertyAccess` arms
    /// must not match `base_ty` against `Object`/`EnumCase`/`ObjectShape`
    /// directly with no `Union`/`Intersection` arm - falling to the
    /// `mixed` fallback would discard every bit of type
    /// information whenever the base's own type happens to be a
    /// `Union`/`Intersection` (a common shape: `Relation::narrow_to_instance`'s
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
        // A property the class surely lacks is reported where it's read,
        // so it's the error type from there on.
        if self.member_confirmed_missing(base_ty, |c| {
            c == "stdClass"
                || self.registry.lookup_property(c, prop).is_some()
                || self.registry.lookup_method(c, "__get").is_some()
                || self.pest_hook_properties.contains(prop)
        }) {
            return Type::error();
        }
        self.declared_property_type(base_ty, prop)
    }

    /// `property_access_type` once the property may exist.
    fn declared_property_type(&self, base_ty: &Type, prop: &str) -> Type {
        syntax::stack::guard(|| {
            // An intersection value is every member at once, so the first
            // member whose declaration resolves the property names its type -
            // searched in reverse, since `Relation::narrow_to_instance` puts the
            // narrower, just-`instanceof`-checked type last. A union value is
            // one member at a time: the union of whichever members resolve.
            let parts = base_ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .rev()
                    .find_map(|m| {
                        let ty = self.declared_property_type(m, prop);
                        (!ty.is_mixed()).then_some(ty)
                    })
                    .unwrap_or(Type::mixed());
            }
            let members = base_ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .map(|m| self.declared_property_type(m, prop))
                    .filter(|ty| !ty.is_mixed())
                    .reduce(Type::union)
                    .unwrap_or(Type::mixed());
            }
            if let Some((fqn, case)) = base_ty.as_enum_case() {
                return match prop {
                    "name" => Type::string_literal(case.to_string().into()),
                    "value" => self
                        .registry
                        .enum_case_value(fqn, case)
                        .cloned()
                        .or_else(|| {
                            self.registry
                                .lookup_property(fqn, "value")
                                .map(|p| p.ty().clone())
                        })
                        .unwrap_or(Type::mixed()),
                    _ => self
                        .registry
                        .lookup_property_with_owner(fqn, prop)
                        .map(|(owner, p)| self.resolve_pseudo(p.ty(), &owner))
                        .unwrap_or(Type::mixed()),
                };
            }
            if let Some((class, type_args)) = base_ty.as_object() {
                return self
                    .registry
                    .lookup_property_with_owner(class, prop)
                    .map(|(owner, p)| {
                        let bindings = self.class_template_bindings(class, type_args, &owner);
                        let resolved = p.ty().substitute_templates(&bindings, &self.relation());
                        self.resolve_pseudo_at(&resolved, &owner, class)
                    })
                    .unwrap_or_else(|| {
                        // Every undeclared property of a `SimpleXMLElement` is
                        // readable through `__get`, as the queried class or
                        // `null`.
                        if self.registry.is_subclass_of(class, "SimpleXMLElement") {
                            Type::union(Type::object(class.clone(), Vec::new()), Type::null())
                        } else {
                            Type::mixed()
                        }
                    });
            }
            base_ty
                .as_object_shape()
                .and_then(|shape| shape.field(&ShapeKey::from_name(prop)))
                .map(|f| f.ty.clone())
                .unwrap_or(Type::mixed())
        })
    }

    /// `infer_expr`'s own `ExprKind::MethodCall` return-type resolution -
    /// extracted so it can recurse through `Union`/`Intersection`, rather
    /// than silently collapsing any method call's own return type to
    /// `mixed` whenever the receiver is one. `infer_expr` is the
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
    /// The instance a `[[class_argument_returns]]` method returns: the
    /// class its configured argument names (`Foo::class` or a string
    /// naming a known class), intersected with the entry's extra types.
    pub(crate) fn class_argument_return(
        &self,
        class: &str,
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<Type> {
        let entry = self.registry.class_argument_returns().find(|entry| {
            entry.method.eq_ignore_ascii_case(method)
                && (class.eq_ignore_ascii_case(&entry.class)
                    || self.registry.is_subclass_of(class, &entry.class))
        })?;
        let arg = args.get(entry.argument.checked_sub(1)?)?;
        let arg_ty = self.infer_expr(&arg.value, locals);
        let named = match arg_ty.class_string_class() {
            Some(name) => name.to_string(),
            None => arg_ty
                .string_literal_value()?
                .as_str()?
                .trim_start_matches('\\')
                .to_string(),
        };
        let (canonical, _) = self.registry.lookup_class(&named)?;
        let object = Type::object(canonical.into(), Vec::new());
        if entry.intersect.is_empty() {
            return Some(object);
        }
        let mut members = vec![object];
        members.extend(
            entry
                .intersect
                .iter()
                .map(|t| Type::object((t.trim_start_matches('\\').to_string()).into(), Vec::new())),
        );
        Some(Type::intersection_of(members))
    }

    pub(crate) fn method_call_return_type(
        &self,
        recv: &Expr,
        recv_ty: &Type,
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        // A method a known class surely lacks is reported where it's
        // called, so the call is the error type from there on.
        if recv_ty
            .as_object()
            .is_some_and(|(class, _)| self.registry.classes.contains_key(class.as_str()))
            && self.member_confirmed_missing(recv_ty, |c| {
                self.registry.lookup_method(c, method).is_some()
                    || self.registry.lookup_method(c, "__call").is_some()
            })
        {
            return Type::error();
        }
        self.declared_method_return_type(recv, recv_ty, method, args, locals)
    }

    /// `method_call_return_type` once the method may exist.
    fn declared_method_return_type(
        &self,
        recv: &Expr,
        recv_ty: &Type,
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        syntax::stack::guard(|| {
            if let Some((class, type_args)) = recv_ty.as_object() {
                return match self.class_argument_return(class, method, args, locals) {
                    Some(ty) => ty,
                    None => self.object_method_call_return_type(
                        recv, class, type_args, method, args, locals,
                    ),
                };
            }
            // The consuming half of the feature just above -
            // `ReflectionProperty::getValue($obj)`/`ReflectionMethod::
            // invoke($obj, ...$args)` resolve the real member's own
            // declared type on the owner this reflector was bound to,
            // substituting the owner's own `@template` bindings the same
            // way an ordinary generic method call already does.
            // `invoke()`'s own extra arguments (after the leading
            // `$object`) map onto the target method's own parameters
            // positionally, so they're forwarded from index 1, matching
            // real PHP's own `invoke($object, ...$args)` signature. An
            // owner that isn't itself a plain resolved object
            // (an unresolved template, a union, ...) can't be looked up
            // at all - stays `Mixed`, not a guess.
            if let Some((owner_ty, name)) = recv_ty.as_reflected_member() {
                return match (method, owner_ty.as_object()) {
                    ("getValue", Some((class, type_args))) => {
                        self.generic_object_property_type(class, type_args, name)
                    }
                    ("invoke", Some((class, type_args))) => {
                        let forwarded = args.get(1..).unwrap_or(&[]);
                        self.generic_object_method_return_type(
                            class, type_args, name, forwarded, locals,
                        )
                    }
                    _ => Type::mixed(),
                };
            }
            // An enum case is an instance of its enum for method calls, with
            // no template arguments.
            if let Some((fqn, _)) = recv_ty.as_enum_case() {
                return self
                    .registry
                    .lookup_method_with_owner(fqn, method)
                    .map(|(owner, sig)| {
                        let resolved_args = resolve_call_args(args, sig.params());
                        let resolved = self.infer_call_return_type(sig, &resolved_args, locals);
                        self.resolve_pseudo(&resolved, &owner)
                    })
                    .unwrap_or(Type::mixed());
            }
            // The narrower, just-`instanceof`-checked type is the last member
            // `Relation::narrow_to_instance` puts into an intersection: searched
            // first, so its own (possibly overriding) signature wins.
            let parts = recv_ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .rev()
                    .find_map(|m| {
                        let ty = self.declared_method_return_type(recv, m, method, args, locals);
                        (!ty.is_mixed()).then_some(ty)
                    })
                    .unwrap_or(Type::mixed());
            }
            let members = recv_ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .map(|m| self.declared_method_return_type(recv, m, method, args, locals))
                    .filter(|ty| !ty.is_mixed())
                    .reduce(Type::union)
                    .unwrap_or(Type::mixed());
            }
            Type::mixed()
        })
    }

    /// `method_call_return_type` for an instance of `class<type_args>`:
    /// the library-aware returns first, then the declared one.
    fn object_method_call_return_type(
        &self,
        recv: &Expr,
        class: &types::Name,
        type_args: &[Type],
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        if method == "createQuery"
            && self.settings.doctrine
            && self
                .registry
                .is_subclass_of(class, "Doctrine\\ORM\\EntityManagerInterface")
        {
            // `EntityManager::createQuery($dql)`: the same DQL row-shape
            // inference as `QueryBuilder::getQuery()` below, fed a literal
            // DQL string instead of a reconstructed builder chain.
            let source = args
                .first()
                .and_then(|arg| self.doctrine_dql_source(&arg.value, locals));
            self.doctrine_query_type(source)
        } else if method == "getQuery"
            && self.settings.doctrine
            && self
                .registry
                .is_subclass_of(class, "Doctrine\\ORM\\QueryBuilder")
        {
            // `QueryBuilder::getQuery()`: the fluent chain is resolved,
            // its select items parsed and the row shape built, carried as
            // `Doctrine\ORM\Query`'s generic argument (see
            // `doctrine_query_row_type`). An undecidable chain or select
            // gives a plain, argument-less `Query`.
            let source = self.doctrine_query_builder_source(recv, locals);
            self.doctrine_query_type(source)
        } else if matches!(
            method,
            "getResult"
                | "execute"
                | "toIterable"
                | "getOneOrNullResult"
                | "getSingleResult"
                | "getArrayResult"
                | "getSingleColumnResult"
                | "getSingleScalarResult"
                | "getScalarResult"
        ) && self.settings.doctrine
            && self
                .registry
                .is_subclass_of(class, "Doctrine\\ORM\\AbstractQuery")
        {
            // `AbstractQuery`'s result methods, per hydration mode - see
            // `doctrine_query_result`.
            self.doctrine_query_result(recv, method, args, type_args, locals)
        } else if method == "format" && self.registry.is_subclass_of(class, "DateTimeInterface") {
            // `DateTimeInterface::format()`'s own already-
            // shipped `date()` format-character table applies verbatim -
            // real, substantial corpus usage (dozens of literal
            // `->format('...')` sites). Falls back to the stub's own
            // plain `string` (unchanged) whenever the format isn't a
            // literal, or contains a character the table doesn't
            // confidently classify.
            args.first()
                .and_then(|a| literal_string_value(&a.value))
                .and_then(|fmt| date_format_return_type(&fmt))
                .unwrap_or(Type::string())
        } else if method == "format" && self.registry.is_subclass_of(class, "DateInterval") {
            // `DateInterval::format()`'s own hand-rolled
            // specifier table - see `date_interval_format_return_type`'s
            // own doc comment for why the zero-valued representative
            // substitution is genuinely sound (not just an
            // approximation) for the fixed-width specifiers.
            args.first()
                .and_then(|a| literal_string_value(&a.value))
                .and_then(|fmt| date_interval_format_return_type(&fmt))
                .unwrap_or(Type::string())
        } else if method == "diff" && self.registry.is_subclass_of(class, "DateTimeInterface") {
            // `DateTimeInterface::diff()` narrows the stub's own
            // plain `DateInterval` return type to also carry a real,
            // always-int `days` property - mirrors real PHPStan's own
            // tiny stub-file override (this project deliberately never
            // vendors PHPStan's own stub files, so reproduced directly
            // here instead). Real, direct corpus usage with no clamping
            // user-declared return type.
            Type::intersection_of(vec![
                Type::object("DateInterval".into(), Vec::new()),
                Type::object_shape(Shape::from_named([("days", Type::int(), false)])),
            ])
        } else if method == "asXML" && self.registry.is_subclass_of(class, "SimpleXMLElement") {
            // `SimpleXMLElement::asXML()`'s 0-arg/1-arg overload split
            // (mirrors PHPStan's own
            // `SimpleXMLElementAsXMLMethodReturnTypeExtension`): exactly
            // one argument - a filename to write to - returns `bool`
            // (write success/failure); zero arguments (or any other
            // count) returns the XML itself, `string|false`.
            // Deliberately doesn't extend to the
            // separate `saveXML()` alias method, matching real PHPStan's
            // own narrower scope.
            if args.len() == 1 {
                Type::bool()
            } else {
                Type::union_of(vec![Type::string(), Type::bool_literal(false)])
            }
        } else if matches!(method, "getMethod" | "getProperty")
            && self.registry.is_subclass_of(class, "ReflectionClass")
        {
            // `ReflectionClass<T>::getMethod('name')`/`getProperty('name')`
            // with a literal name (or a value typed as one) attaches a
            // reflected-member type (`T`, `name`) to the stub's
            // `ReflectionMethod<T>`/`ReflectionProperty<T>` return, so a
            // later `->invoke()`/`->getValue()` resolves the member's own
            // type. Any other name keeps the stub's return type.
            let base =
                self.generic_object_method_return_type(class, type_args, method, args, locals);
            // An erased receiver `T` (`substitute_class_templates`'s
            // own unbound-template fallback, `mixed`) can never
            // resolve a real member type either - skip the wrap
            // entirely rather than attach a fact that can only ever
            // stay unresolved, the same "don't guess" leniency a
            // reflected member keeps for a not-genuinely-concrete
            // owner.
            let owner_ty = type_args.first().cloned().filter(|t| !t.is_mixed());
            let name = args
                .first()
                .and_then(|a| match literal_string_value(&a.value) {
                    Some(s) => Some(Name::from(s)),
                    None => self
                        .infer_expr(&a.value, locals)
                        .string_literal_value()
                        .map(|s| Name::from(s.to_string())),
                });
            match (owner_ty, name) {
                (Some(owner_ty), Some(name)) => {
                    Type::intersection_of(vec![base, Type::reflected_member(owner_ty, name)])
                }
                _ => base,
            }
        } else if matches!(
            method,
            "fetchAssociative"
                | "fetchAllAssociative"
                | "fetchOne"
                | "fetchFirstColumn"
                | "fetchNumeric"
                | "fetchAllNumeric"
                | "fetchAllKeyValue"
                | "fetchAllAssociativeIndexed"
                | "iterateAssociative"
                | "iterateNumeric"
                | "iterateKeyValue"
                | "iterateAssociativeIndexed"
                | "iterateColumn"
        ) && self
            .registry
            .is_subclass_of(class, "Doctrine\\DBAL\\Result")
        {
            // Sub-phase D of the live SQL query reflection phase (see
            // delegated-tumbling-dongarra.md): `Doctrine\DBAL\Result`'s
            // own no-argument terminal methods - the second hop of the
            // `executeQuery()` -> `Result` propagation just below reads
            // the row type back from `type_args[0]`, the same
            // synthesized-generic-slot convention 168e already
            // established for `Doctrine\ORM\Query` (`Result` isn't a
            // real templated class either - this project smuggles the
            // row type through the same way).
            dbal_row_method_return_type(method, type_args.first().cloned()).unwrap_or_else(|| {
                self.generic_object_method_return_type(class, type_args, method, args, locals)
            })
        } else if matches!(
            method,
            "fetchAssociative"
                | "fetchAllAssociative"
                | "fetchOne"
                | "fetchFirstColumn"
                | "fetchNumeric"
                | "fetchAllNumeric"
                | "fetchAllKeyValue"
                | "fetchAllAssociativeIndexed"
                | "iterateAssociative"
                | "iterateNumeric"
                | "iterateKeyValue"
                | "iterateAssociativeIndexed"
                | "iterateColumn"
        ) && (self
            .registry
            .is_subclass_of(class, "Doctrine\\DBAL\\Connection")
            || self
                .registry
                .is_subclass_of(class, "Doctrine\\DBAL\\Query\\QueryBuilder"))
        {
            // `Doctrine\DBAL\Connection`'s own directly-SQL-taking
            // fetch*/iterate* methods, and `Doctrine\DBAL\Query\
            // QueryBuilder`'s own no-argument convenience aliases for
            // the same ones (real DBAL: `QueryBuilder::
            // fetchAssociative()` etc. are pure sugar for
            // `$this->executeQuery()->fetchAssociative()` -
            // `QueryBuilder` itself has no `iterate*`/`fetchNumeric`-
            // family aliases at all, confirmed against its real source,
            // so those methods only ever reach this arm via
            // `Connection`) - `resolve_dbal_sql_from_call` picks the
            // right SQL source per receiver kind.
            let row_type = self
                .resolve_dbal_sql_from_call(recv, class, args, locals)
                .and_then(|sql| {
                    crate::checker::sql_reflection::resolve::resolve_sql_row_type(
                        self.doctrine_entities(),
                        self.sql,
                        &sql,
                    )
                });
            dbal_row_method_return_type(method, row_type).unwrap_or_else(|| {
                self.generic_object_method_return_type(class, type_args, method, args, locals)
            })
        } else if (method == "executeQuery"
            && (self
                .registry
                .is_subclass_of(class, "Doctrine\\DBAL\\Connection")
                || self
                    .registry
                    .is_subclass_of(class, "Doctrine\\DBAL\\Query\\QueryBuilder")))
            || (method == "executeCacheQuery"
                && self
                    .registry
                    .is_subclass_of(class, "Doctrine\\DBAL\\Connection"))
        {
            // `executeQuery()`/`executeCacheQuery()` - the first hop of
            // the two-hop propagation above: carries the resolved row
            // type forward as `Doctrine\DBAL\Result`'s own synthesized
            // generic slot, read back by the `Result`-method dispatch
            // above rather than re-resolving the SQL from scratch at
            // that call site. `executeCacheQuery()` is `Connection`-only
            // (confirmed against its real source - `QueryBuilder` has
            // no such method). `executeStatement()` needs no such
            // transform at all (a plain `int|string` affected-row
            // count, already correct via `generic_object_method_return_
            // type`'s own ordinary resolution) - not handled here.
            let row_type = self
                .resolve_dbal_sql_from_call(recv, class, args, locals)
                .and_then(|sql| {
                    crate::checker::sql_reflection::resolve::resolve_sql_row_type(
                        self.doctrine_entities(),
                        self.sql,
                        &sql,
                    )
                });
            match row_type {
                Some(row_type) => Type::object(
                    ("Doctrine\\DBAL\\Result".to_string()).into(),
                    vec![row_type],
                ),
                None => Type::object(("Doctrine\\DBAL\\Result".to_string()).into(), Vec::new()),
            }
        } else if method == "query" && self.registry.is_subclass_of(class, "mysqli") {
            self.mysqli_query_type(args.first().map(|a| &a.value), locals)
        } else if class.eq_ignore_ascii_case("mysqli_result")
            && let Some(ty) =
                self.mysqli_fetch_type(method, type_args.first(), args.first().map(|a| &a.value))
        {
            ty
        } else {
            self.generic_object_method_return_type(class, type_args, method, args, locals)
        }
    }

    /// The ordinary, no-special-casing method-call resolution: look up
    /// the method's own real signature (from real vendored source or a
    /// hand-written stub, whichever indexed it) and infer its return
    /// type the normal way. Every special-cased arm above that can fail
    /// to resolve a more precise answer (an unfoldable SQL string, an
    /// undecidable chain, ...) falls back to calling this directly,
    /// rather than returning `mixed` and silently discarding a
    /// real declared return type the ordinary path would have found.
    fn generic_object_method_return_type(
        &self,
        class: &str,
        type_args: &[Type],
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        self.registry
            .lookup_method_with_owner(class, method)
            .map(|(owner, sig)| {
                let sig = self.substitute_class_templates(sig, class, type_args, &owner);
                let resolved_args = resolve_call_args(args, sig.params());
                let resolved = self.infer_call_return_type(&sig, &resolved_args, locals);
                self.resolve_pseudo_at(&resolved, &owner, class)
            })
            .unwrap_or(Type::mixed())
    }

    /// `generic_object_method_return_type`'s property-typed sibling,
    /// which a reflected property's `getValue()` resolves through.
    fn generic_object_property_type(&self, class: &str, type_args: &[Type], prop: &str) -> Type {
        self.registry
            .lookup_property_with_owner(class, prop)
            .map(|(owner, sig)| {
                let ty = self.substitute_class_template_property_type(
                    sig.ty(),
                    class,
                    type_args,
                    &owner,
                );
                self.resolve_pseudo_at(&ty, &owner, class)
            })
            .unwrap_or(Type::mixed())
    }

    /// The SQL text a DBAL fetch*/`executeQuery` call actually runs -
    /// `QueryBuilder`'s own no-argument convenience methods get their
    /// SQL from the fluent chain itself (sub-phase B); `Connection`'s
    /// own methods take it as a literal/constant-foldable first
    /// argument instead. `None` for anything undecidable either way -
    /// callers fall back to `generic_object_method_return_type`, never
    /// a guess.
    fn resolve_dbal_sql_from_call(
        &self,
        recv: &Expr,
        class: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<String> {
        if self
            .registry
            .is_subclass_of(class, "Doctrine\\DBAL\\Query\\QueryBuilder")
        {
            self.resolve_dbal_query_builder_sql(recv, locals)
        } else {
            args.first()
                .and_then(|arg| self.foldable_sql_string(&arg.value, locals))
        }
    }
}

/// The "one row type, N per-method transforms" table -
/// shared between `Doctrine\DBAL\Result`'s own no-argument terminal
/// methods and `Connection`/`QueryBuilder`'s own directly-resolved
/// forms, which otherwise duplicate this exact dispatch. `row_type` is
/// always an array shape when it's `Some` at all (every
/// `resolve_sql_row_type`-producing path already guarantees this) -
/// anything else degrades to `None`, the same "don't guess" leniency as
/// an unresolvable chain.
///
/// Real per-method shapes read directly from `vendor/doctrine/dbal/src/
/// Result.php`'s own real implementations/docblocks, not guessed:
/// `fetchNumeric`/`fetchAllNumeric`/`iterateNumeric` re-key the row by
/// position (`0`, `1`, ...) instead of column name; `fetchAllKeyValue`/
/// `iterateKeyValue` use the *first* column's own value as the array
/// key and the *second* column's value as the value (real DBAL asserts
/// at least 2 columns); `fetchAllAssociativeIndexed`/
/// `iterateAssociativeIndexed` use the first column's value as the key
/// and the *rest* of the row (first column removed) as the value.
/// `iterateNumeric`/`iterateAssociative`/`iterateColumn`'s own
/// `Traversable` key is always plain `int` (the generator's own
/// sequential index - confirmed via `Result.php`'s own `@return
/// Traversable<int, ...>` docblocks), never derived from a column;
/// `iterateKeyValue`/`iterateAssociativeIndexed`'s own key *is* a real
/// column value (`Traversable<mixed, ...>` in the same docblocks),
/// coerced through `dynamic_write_key_type` the same way an ordinary
/// dynamic array-key write already is elsewhere in this project - the
/// identical "what does this uncertain-typed value become as a real PHP
/// array key" question.
fn dbal_row_method_return_type(method: &str, row_type: Option<Type>) -> Option<Type> {
    let row_type = row_type?;
    let shape = row_type.as_array_shape()?;
    let fields = &shape.fields;
    let numeric_shape = || {
        Type::array_shape(Shape::sealed(
            fields
                .iter()
                .enumerate()
                .map(|(i, f)| ShapeField::new(ShapeKey::Int(i as i64), f.ty.clone(), f.optional))
                .collect(),
        ))
    };
    let traversable_of =
        |key: Type, value: Type| Type::object("Traversable".into(), vec![key, value]);
    match method {
        "fetchAssociative" => Some(Type::union_of(vec![
            Type::array_shape(shape.clone()),
            Type::bool_literal(false),
        ])),
        "fetchAllAssociative" => Some(Type::list_of(Type::array_shape(shape.clone()))),
        "iterateAssociative" => Some(traversable_of(
            Type::int(),
            Type::array_shape(shape.clone()),
        )),
        "fetchNumeric" => Some(Type::union_of(vec![
            numeric_shape(),
            Type::bool_literal(false),
        ])),
        "fetchAllNumeric" => Some(Type::list_of(numeric_shape())),
        "iterateNumeric" => Some(traversable_of(Type::int(), numeric_shape())),
        "fetchOne" if !fields.is_empty() => Some(Type::union_of(vec![
            fields[0].ty.clone(),
            Type::bool_literal(false),
        ])),
        "fetchFirstColumn" if !fields.is_empty() => Some(Type::list_of(fields[0].ty.clone())),
        "iterateColumn" if !fields.is_empty() => {
            Some(traversable_of(Type::int(), fields[0].ty.clone()))
        }
        "fetchAllKeyValue" if fields.len() >= 2 => {
            let key_ty = fields[0].ty.write_key_type();
            Some(Type::array(key_ty, fields[1].ty.clone()))
        }
        "iterateKeyValue" if fields.len() >= 2 => {
            let key_ty = fields[0].ty.write_key_type();
            Some(traversable_of(key_ty, fields[1].ty.clone()))
        }
        "fetchAllAssociativeIndexed" if !fields.is_empty() => {
            let key_ty = fields[0].ty.write_key_type();
            let rest = Type::array_shape(Shape::sealed(fields[1..].to_vec()));
            Some(Type::array(key_ty, rest))
        }
        "iterateAssociativeIndexed" if !fields.is_empty() => {
            let key_ty = fields[0].ty.write_key_type();
            let rest = Type::array_shape(Shape::sealed(fields[1..].to_vec()));
            Some(traversable_of(key_ty, rest))
        }
        _ => None,
    }
}

/// An `AbstractQuery::HYDRATE_*` mode, or `Unknown` when the argument
/// isn't statically resolvable - every caller treats that as
/// undecidable, never guessed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HydrationMode {
    Object,
    Array,
    Scalar,
    SingleScalar,
    SimpleObject,
    ScalarColumn,
    Unknown,
}

/// The hydration mode argument at `index` - omitted or `null` is real
/// Doctrine's default, `HYDRATE_OBJECT`. The constants' own values
/// (`AbstractQuery`) are accepted as integer literals too.
fn resolve_hydration_mode_arg(args: &[Arg], index: usize) -> HydrationMode {
    let Some(arg) = args.get(index) else {
        return HydrationMode::Object;
    };
    let by_name = |name: &str| match name {
        "HYDRATE_OBJECT" => HydrationMode::Object,
        "HYDRATE_ARRAY" => HydrationMode::Array,
        "HYDRATE_SCALAR" => HydrationMode::Scalar,
        "HYDRATE_SINGLE_SCALAR" => HydrationMode::SingleScalar,
        "HYDRATE_SIMPLEOBJECT" => HydrationMode::SimpleObject,
        "HYDRATE_SCALAR_COLUMN" => HydrationMode::ScalarColumn,
        _ => HydrationMode::Unknown,
    };
    match &arg.value.kind {
        ExprKind::Literal(Literal::Null) => HydrationMode::Object,
        ExprKind::ClassConstFetch(_, member) => by_name(member),
        ExprKind::Literal(Literal::Int(n)) => match n {
            1 => HydrationMode::Object,
            2 => HydrationMode::Array,
            3 => HydrationMode::Scalar,
            4 => HydrationMode::SingleScalar,
            5 => HydrationMode::SimpleObject,
            6 => HydrationMode::ScalarColumn,
            _ => HydrationMode::Unknown,
        },
        _ => HydrationMode::Unknown,
    }
}

/// A resolved query: its chain (joins, `GROUP BY`, `INDEX BY`) and
/// parsed select items.
type DoctrineQuerySource = (
    crate::checker::doctrine::query_builder::QueryBuilderChain,
    Vec<crate::checker::doctrine::dql::SelectItem>,
);

impl<'a> Checker<'a> {
    /// A `QueryBuilder` chain's query, when resolvable. A
    /// `->delete()`/`->update()` chain has no select items to parse.
    fn doctrine_query_builder_source(
        &self,
        qb: &Expr,
        locals: &Locals,
    ) -> Option<DoctrineQuerySource> {
        let chain = self.resolve_query_builder_chain(qb, locals)?;
        if chain.dml {
            return Some((chain, Vec::new()));
        }
        let items = crate::checker::doctrine::dql::parse_select_items(&chain.select_args)?;
        Some((chain, items))
    }

    /// A literal `createQuery($dql)` string's query, when resolvable.
    fn doctrine_dql_source(&self, dql: &Expr, locals: &Locals) -> Option<DoctrineQuerySource> {
        let dql = match literal_string_value(dql) {
            Some(s) => s.into_owned(),
            None => self
                .infer_expr(dql, locals)
                .string_literal_value()?
                .to_string(),
        };
        crate::checker::doctrine::dql::parse_select_statement(&dql)
    }

    /// The query a result method's receiver was built from, when it's
    /// a direct `->getQuery()`/`createQuery(...)` call.
    fn doctrine_query_source_of(
        &self,
        recv: &Expr,
        locals: &Locals,
    ) -> Option<DoctrineQuerySource> {
        match &recv.peeled().kind {
            ExprKind::MethodCall(qb, method, call_args, _)
                if **method == *"getQuery" && call_args.is_empty() =>
            {
                self.doctrine_query_builder_source(qb, locals)
            }
            ExprKind::MethodCall(_, method, call_args, _) if **method == *"createQuery" => {
                self.doctrine_dql_source(&call_args.first()?.value, locals)
            }
            _ => None,
        }
    }

    /// `Doctrine\ORM\Query<TKey, TResult>` - `TKey` is the `INDEX BY`
    /// column's key type (`null` without one), `TResult` the
    /// `HYDRATE_OBJECT` row (`never` for a `DELETE`/`UPDATE`, which
    /// returns an affected-row count instead). Bare `Query` when the
    /// query isn't resolvable.
    fn doctrine_query_type(&self, source: Option<DoctrineQuerySource>) -> Type {
        let query = "Doctrine\\ORM\\Query".to_string();
        let Some((chain, items)) = source else {
            return Type::object(query.into(), Vec::new());
        };
        let key = crate::checker::doctrine::result_shape::index_key_type(self, &chain)
            .unwrap_or(Type::null());
        let row = if chain.dml {
            Some(Type::never())
        } else {
            crate::checker::doctrine::result_shape::build_result_row_type(self, &chain, &items)
        };
        match row {
            Some(row) => Type::object(query.into(), vec![key, row]),
            None => Type::object(query.into(), Vec::new()),
        }
    }

    /// One `AbstractQuery` result method's return type. The mode comes
    /// from the method itself (`getArrayResult()` is `HYDRATE_ARRAY`) or
    /// its hydration argument; `HYDRATE_OBJECT` reads the row the
    /// `Query<TKey, TResult>` type carries, every other mode rebuilds its
    /// own row shape from the receiver's query. Collections are lists,
    /// or keyed by the `INDEX BY` column. A `DELETE`/`UPDATE` returns its
    /// affected-row count from every method but `toIterable()`.
    fn doctrine_query_result(
        &self,
        recv: &Expr,
        method: &str,
        args: &[Arg],
        type_args: &[Type],
        locals: &Locals,
    ) -> Type {
        use crate::checker::doctrine::result_shape as shape;
        let mode = match method {
            "getResult" | "getOneOrNullResult" | "getSingleResult" => {
                resolve_hydration_mode_arg(args, 0)
            }
            "execute" | "toIterable" => resolve_hydration_mode_arg(args, 1),
            "getArrayResult" => HydrationMode::Array,
            "getSingleColumnResult" => HydrationMode::ScalarColumn,
            "getSingleScalarResult" => HydrationMode::SingleScalar,
            "getScalarResult" => HydrationMode::Scalar,
            _ => HydrationMode::Unknown,
        };
        let (key, object_row) = match type_args {
            [key, row] => (Some(key.clone()), Some(row.clone())),
            _ => (None, None),
        };
        let source = if mode == HydrationMode::Object && object_row.is_some() {
            None
        } else {
            self.doctrine_query_source_of(recv, locals)
        };
        let dml = object_row.as_ref().is_some_and(Type::is_never)
            || source.as_ref().is_some_and(|(chain, _)| chain.dml);
        if dml && method != "toIterable" {
            return Type::int_range(Some(0), None);
        }
        let key = key.or_else(|| {
            source
                .as_ref()
                .map(|(chain, _)| shape::index_key_type(self, chain).unwrap_or(Type::null()))
        });
        let element = match mode {
            HydrationMode::Object | HydrationMode::SimpleObject => object_row.or_else(|| {
                source
                    .as_ref()
                    .and_then(|(chain, items)| shape::build_result_row_type(self, chain, items))
            }),
            HydrationMode::Array => source
                .as_ref()
                .and_then(|(chain, items)| shape::build_array_result_row_type(self, chain, items)),
            HydrationMode::Scalar => source
                .as_ref()
                .and_then(|(chain, items)| shape::build_scalar_result_row_type(self, chain, items)),
            HydrationMode::ScalarColumn => source
                .as_ref()
                .and_then(|(chain, items)| shape::raw_scalar_column_type(self, chain, items)),
            HydrationMode::SingleScalar => source
                .as_ref()
                .and_then(|(chain, items)| shape::single_scalar_column_type(self, chain, items)),
            HydrationMode::Unknown => return Type::mixed(),
        };
        let element_or_mixed = element.clone().unwrap_or(Type::mixed());
        let collection = || {
            // `fetchFirstColumn()` always yields a list.
            let keyed = mode != HydrationMode::ScalarColumn;
            // An unresolved query may carry an unseen `INDEX BY`, so only
            // the always-list column mode (and `getResult()`'s long-
            // standing shape-only fallback) still claims a list.
            if element.is_none()
                && key.is_none()
                && !matches!(
                    mode,
                    HydrationMode::ScalarColumn
                        | HydrationMode::Object
                        | HydrationMode::SimpleObject
                )
            {
                return Type::mixed();
            }
            match (&element, &key) {
                (Some(el), Some(key)) if keyed && !key.is_null().yes() => {
                    Type::array(key.clone(), el.clone())
                }
                (Some(el), _) => Type::list_of(el.clone()),
                (None, _) => doctrine_query_result_list_type(),
            }
        };
        let single_scalar = mode == HydrationMode::SingleScalar;
        match method {
            "getSingleScalarResult" | "getSingleResult" => element_or_mixed,
            "getOneOrNullResult" => Type::union(element_or_mixed, Type::null()),
            "toIterable" => Type::union_of(vec![
                Type::array(
                    key.filter(|k| !k.is_null().yes())
                        .unwrap_or_else(Type::unknown_array_key),
                    element_or_mixed,
                ),
                Type::object("Traversable".into(), Vec::new()),
            ]),
            // `getResult(HYDRATE_SINGLE_SCALAR)` hands back the scalar
            // itself, not a collection.
            _ if single_scalar => element_or_mixed,
            _ => collection(),
        }
    }
}

/// Public first: the order in which a declaration's visibility lets more
/// callers in.
fn visibility_rank(visibility: &syntax::ast::Visibility) -> u8 {
    match visibility {
        syntax::ast::Visibility::Public => 0,
        syntax::ast::Visibility::Protected => 1,
        syntax::ast::Visibility::Private => 2,
    }
}
