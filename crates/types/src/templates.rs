use rustc_hash::FxHashMap;

use crate::{CallableSig, TemplateParam, Type};

/// Unifies a template-bearing param type against an actual argument type,
/// recording the binding it implies. Only the two shapes that come up in
/// practice are handled: a bare `T` param, and `T[]`/`array<T>` unifying
/// against an array argument's element type. Anything else (a template
/// buried in a union, an array shape, etc) just isn't bound, which is safe
/// since `substitute_template` falls back to `Mixed` for anything left
/// unbound. First binding for a given name wins; conflicting uses of the
/// same `T` across multiple params aren't reconciled (no union-widening).
pub fn bind_template_param(param_ty: &Type, arg_ty: &Type, bindings: &mut FxHashMap<String, Type>) {
    match param_ty {
        Type::TemplateParam(tp) if tp.scope.as_deref() == Some(crate::CALLABLE_TEMPLATE_SCOPE) => {}
        Type::TemplateParam(tp) => {
            // Reject a bare `T` binding to a *definite scalar* when `T`'s
            // own declared bound requires an object (`@template T of
            // object`) - e.g. `new ReflectionClass($className)` with a
            // plain `string`-typed variable, not a `Foo::class`/
            // object-instance `T`-carrying argument: without this, the
            // bare-`T` union member of `class-string<T>|T` would happily
            // bind `T` to the raw `string` itself once the `ClassString`
            // member's own, stricter arm declined to match - producing a
            // bound-violating `ReflectionClass<string>` that then breaks
            // real downstream `ReflectionClass<object>` call sites.
            // Deliberately narrower than a full `subtype`/bound check:
            // this module has no registry access, so it can't tell a
            // real subclass (`T of Base` satisfied by a `Derived`
            // argument, the common case) from an unrelated class - only
            // a scalar-vs-object mismatch is structurally unambiguous
            // without one.
            let bound_requires_object = matches!(&*tp.bound, Type::ObjectWithoutClass);
            let arg_is_definite_scalar = matches!(
                arg_ty,
                Type::String
                    | Type::StringLiteral(_)
                    | Type::Int
                    | Type::IntLiteral(_)
                    | Type::IntRange(_, _)
                    | Type::Float
                    | Type::FloatLiteral(_)
                    | Type::Bool
                    | Type::True
                    | Type::False
                    | Type::Null
            );
            if *arg_ty != Type::Mixed(false) && !(bound_requires_object && arg_is_definite_scalar) {
                bindings
                    .entry(tp.name.clone())
                    .or_insert_with(|| arg_ty.clone());
            }
        }
        Type::Array(_, inner) => match arg_ty {
            Type::Array(_, arg_inner) => bind_template_param(inner, arg_inner, bindings),
            // `list<V>`/`non-empty-array<V>`/`non-empty-list<V>` - an
            // `Array` member alongside accessory markers.
            Type::Intersection(members) => {
                if let Some(Type::Array(_, arg_inner)) =
                    members.iter().find(|m| matches!(m, Type::Array(..)))
                {
                    bind_template_param(inner, arg_inner, bindings);
                }
            }
            // An array shape's element type is the union of its fields'
            // value types (an unsealed tail's excluded).
            Type::ArrayShape(shape) => {
                let values: Vec<Type> = shape.fields.iter().map(|f| f.ty.clone()).collect();
                let element = match values.len() {
                    0 => return,
                    1 => values.into_iter().next().expect("len checked"),
                    _ => Type::Union(values),
                };
                bind_template_param(inner, &element, bindings);
            }
            _ => {}
        },
        Type::ClassString(inner) => {
            if let Type::ClassString(arg_inner) = arg_ty {
                bind_template_param(inner, arg_inner, bindings);
            }
        }
        // Handles phpstorm-stubs' pervasive `@param class-string<T>|null
        // $name` idiom (and any other nullable/union-wrapped templated
        // parameter). Each member is tried against the same, whole
        // `arg_ty` (not a decomposed one) - a member that isn't itself
        // `TemplateParam`/`Array`/`ClassString`/`Union` (e.g. the `Null`
        // half of `class-string<T>|null`) is simply a no-op here, exactly
        // as it already is at the top level for any other type that
        // isn't one of those shapes.
        Type::Union(members) => {
            for m in members {
                bind_template_param(m, arg_ty, bindings);
            }
        }
        _ => {}
    }
}

/// Rewrites `arg_ty` so a string-literal argument occupying a
/// `class-string<T>`-shaped position in `param_ty` is treated the way real
/// PHPStan's own `ConstantStringType::isClassString()`/
/// `getClassStringObjectType()` treats it: a literal naming a real,
/// existing class binds a template exactly as well as an explicit
/// `Foo::class` would. Mirrors `bind_template_
/// param`'s own structural recursion (`Array`/`ClassString`/`Union`) so a
/// literal nested inside e.g. `class-string<T>[]` or `class-string<T>|null`
/// is reached too. Kept as a separate preprocessing pass rather than
/// folded into `bind_template_param` itself since only a caller with
/// registry access can answer "is this string a real class name" - this
/// module deliberately has none (see the module doc above). Deliberately
/// does NOT touch a bare `TemplateParam` position (`T $x` with no
/// `class-string<>` wrapper) - real PHPStan's own `isClassString()` check
/// only ever fires where the declared type is actually class-string-
/// shaped, so a bare-`T` param taking a plain string argument must keep
/// binding `T` to the string itself, not silently reinterpret it as an
/// object.
pub fn promote_class_string_literals(
    param_ty: &Type,
    arg_ty: &Type,
    resolve_class: &impl Fn(&str) -> Option<String>,
) -> Type {
    match (param_ty, arg_ty) {
        (Type::ClassString(_), Type::StringLiteral(s)) => match resolve_class(s) {
            Some(canonical) => {
                Type::ClassString(Box::new(Type::Object(canonical.into(), Vec::new())))
            }
            None => arg_ty.clone(),
        },
        (Type::Array(_, inner_p), Type::Array(key, inner_a)) => Type::Array(
            key.clone(),
            Box::new(promote_class_string_literals(
                inner_p,
                inner_a,
                resolve_class,
            )),
        ),
        (Type::Union(members), _) => members
            .iter()
            .find_map(|m| {
                let promoted = promote_class_string_literals(m, arg_ty, resolve_class);
                (promoted != *arg_ty).then_some(promoted)
            })
            .unwrap_or_else(|| arg_ty.clone()),
        _ => arg_ty.clone(),
    }
}

/// Applies a set of template bindings through a type tree, replacing every
/// `TemplateParam` with its bound type. A name left unbound falls back
/// to that template's own declared `@template T = Default` when it has
/// one, rather than unconditionally `Mixed` - e.g. a function-own
/// `@template T = int` whose `T` never actually gets bound from any real
/// call-site argument (used only in the return type, say) resolves to
/// `int` instead of `Mixed`. `checker::generics::
/// substitute_scoped` mirrors this same fallback for the separate
/// class-level substitution path (an "erased" - too-few-arguments, or
/// bare - generic class reference).
impl CallableSig {
    /// The return type of calling a value of this callable type with
    /// arguments of `arg_types`: its own templates bound from them, an
    /// unbound one falling back to its bound.
    pub fn instantiated_return(&self, arg_types: &[Type]) -> Type {
        if self.templates.is_empty() {
            return (*self.return_type).clone();
        }
        fn open(ty: &Type) -> Type {
            match ty {
                Type::TemplateParam(tp)
                    if tp.scope.as_deref() == Some(crate::CALLABLE_TEMPLATE_SCOPE) =>
                {
                    Type::TemplateParam(TemplateParam {
                        scope: None,
                        ..tp.clone()
                    })
                }
                other => other.map_children(&mut open),
            }
        }
        let mut bindings = FxHashMap::default();
        for (i, arg) in arg_types.iter().enumerate() {
            let param = self
                .params
                .get(i)
                .or_else(|| self.params.last().filter(|p| p.variadic));
            if let Some(param) = param {
                bind_template_param(&open(&param.ty), arg, &mut bindings);
            }
        }
        substitute(&open(&self.return_type), &bindings, &|_| true)
    }
}

pub fn substitute_template(ty: &Type, bindings: &FxHashMap<String, Type>) -> Type {
    substitute(ty, bindings, &|_| false)
}

/// `substitute_template`, except that an unbound template `to_bound`
/// selects falls back to its declared bound when it has no default
/// (`@template T of Connection` gives `Connection`) - real PHPStan's
/// `resolveToBounds` for a call's own templates that no argument
/// inferred, e.g. Doctrine DBAL's `DriverManager::getConnection`
/// conditional return `T` when no `wrapperClass` is passed. Class-level
/// templates of an erased receiver stay `Mixed`, the wildcard argument
/// an erased class's arguments compare as.
pub fn substitute_template_resolving_to_bounds(
    ty: &Type,
    bindings: &FxHashMap<String, Type>,
    to_bound: &dyn Fn(&TemplateParam) -> bool,
) -> Type {
    substitute(ty, bindings, to_bound)
}

fn substitute(
    ty: &Type,
    bindings: &FxHashMap<String, Type>,
    to_bound: &dyn Fn(&TemplateParam) -> bool,
) -> Type {
    match ty {
        // A callable type's own template stays until that callable is
        // itself called.
        Type::TemplateParam(tp) if tp.scope.as_deref() == Some(crate::CALLABLE_TEMPLATE_SCOPE) => {
            ty.clone()
        }
        Type::TemplateParam(tp) => bindings.get(&tp.name).cloned().unwrap_or_else(|| {
            let fallback = match tp.default.as_deref() {
                Some(default) => default,
                None if to_bound(tp) && !matches!(*tp.bound, Type::Mixed(_)) => &tp.bound,
                None => return Type::Mixed(false),
            };
            // The template's own name is bound to `Mixed` while its
            // fallback is substituted, so a self-referential bound
            // (`@template T of Foo<T>`), or two templates bounded by each
            // other, can't recurse forever.
            let mut guarded = bindings.clone();
            guarded.insert(tp.name.clone(), Type::Mixed(false));
            substitute(fallback, &guarded, to_bound)
        }),
        // A template-subject conditional (`(T is int ? A : B)`) resolves
        // once `T` is bound, the way real PHPStan's `ConditionalType`
        // does; otherwise both branches stay.
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
            template_subject: true,
        } if bindings
            .get(subject)
            .is_some_and(|b| !matches!(b, Type::Mixed(_))) =>
        {
            let target = substitute(target, bindings, to_bound);
            match super::conditional::conditional_branch(&bindings[subject], &target, *negated) {
                Some(true) => substitute(if_true, bindings, to_bound),
                Some(false) => substitute(if_false, bindings, to_bound),
                None => super::conditional::union_of_branches(
                    substitute(if_true, bindings, to_bound),
                    substitute(if_false, bindings, to_bound),
                ),
            }
        }
        // Every nested position - a generic argument (`Box<T>` becomes
        // `Box<int>`), a shape field, a `Closure(T): U` shape, a
        // conditional return type's branches (`subject` is a raw
        // parameter name, not a `Type`, so there's nothing to substitute
        // there).
        _ => super::late::settle(ty.map_children(&mut |t| substitute(t, bindings, to_bound))),
    }
}
