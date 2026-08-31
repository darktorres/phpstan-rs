use crate::checker::Checker;
use crate::registry::{self};
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Resolves `self`/`static`/`parent` to a concrete `Object` relative to
    /// `defining_class` - the class whose signature this type came from.
    /// `self`/`static` both collapse to the same class here since we don't
    /// track late static binding through actual runtime dispatch; `parent`
    /// resolves via the registry's extends chain, falling back to leaving
    /// it unresolved if the class has no declared parent (so it still hits
    /// the permissive Object fallback in `is_subtype`/`is_assignable`
    /// rather than being wrongly flagged).
    /// Resolves a bare name exactly as it appears at an expression site
    /// (`new Foo()`, `$x instanceof Foo`, ...) to the concrete `Type` it
    /// refers to - `self`/`parent`/`static` need `resolve_pseudo` against
    /// the class currently being checked, everything else goes through
    /// `name_ctx.resolve` (namespace/`use`-import resolution) as an
    /// ordinary class reference. A real bug this fixed: `apply_narrowing`'s
    /// `instanceof` handling used to treat `self`/`parent`/`static` as
    /// literal class names to look up via `name_ctx.resolve` directly
    /// (`$future instanceof self` in real code became a nonsensical
    /// `Amp\self` "class" once namespace-qualification existed to make
    /// the mistake visible) - `infer_expr`'s `ExprKind::New` arm already got
    /// this right; `apply_narrowing` never shared that logic.
    pub(crate) fn resolve_name_expr(&self, name: &str) -> Type {
        match name {
            "self" | "parent" | "static" => {
                let raw = match name {
                    "self" => Type::SelfRef,
                    "parent" => Type::ParentRef,
                    _ => Type::StaticRef,
                };
                match &self.current_class {
                    Some(c) => self.resolve_pseudo(&raw, c),
                    None => raw,
                }
            }
            _ => Type::Object(self.name_ctx.resolve(name), Vec::new()),
        }
    }

    /// Parses a raw native type-hint string (a closure/arrow-fn param or
    /// return type - the one place in this checker that builds a `Type`
    /// straight from source without going through the registry's own
    /// `FunctionSig::from_decl`, which already resolves names as part of
    /// its pipeline) and resolves any class name in it against this
    /// file's `NameContext`, exactly like a real declaration would. Found
    /// necessary the hard way: an unresolved bare name here compared as a
    /// real mismatch against the correctly-resolved FQN the closure body
    /// itself infers, on real code (a `static fn(array $row): ActionCount
    /// => new ActionCount(...)` where `ActionCount` is `use`-imported).
    pub(crate) fn resolve_type_hint(&self, hint: Option<&str>) -> Type {
        match hint {
            Some(h) => registry::resolve_names_in_type(types::parse_type_string(h), &self.name_ctx),
            None => Type::Mixed,
        }
    }

    /// Resolves a `StaticAccess`/`StaticCall` target (`self`/`parent`/
    /// `static`, or a plain/namespaced class name) to a concrete FQN.
    /// `"?"` is the parser's own placeholder for a genuinely dynamic
    /// class target (`$obj::method()`, `(new Foo())::CONST`) - there's no
    /// name to resolve at all there, so it's `None`, same as any other
    /// case `resolve_name_expr` can't turn into a concrete `Object`
    /// (`self`/`parent`/`static` used outside any class context).
    pub(crate) fn resolve_static_class_name(&self, class_name: &str) -> Option<String> {
        if class_name == "?" {
            return None;
        }
        match self.resolve_name_expr(class_name) {
            Type::Object(class, _) => Some(class),
            _ => None,
        }
    }

    /// Resolves `self`/`static`/`parent` to a concrete class - recurses
    /// into `Union`/`Intersection`/`Array`/`ArrayShape` the same way
    /// `types::substitute_template` does, not just the bare top-level
    /// case. A real, previously-live gap this found: the extremely
    /// common nullable-self return type (`?self`/`self|null`, e.g.
    /// `tryFromArray`-style factory methods) never resolved at all
    /// before this - only a *bare* `self` return type ever reached the
    /// `SelfRef` arm directly, so `self` wrapped in *any* union stayed
    /// unresolved, and `is_assignable`/`is_subtype` have no arm at all
    /// for a concrete class instance satisfying a still-unresolved
    /// `SelfRef` (only the reverse direction, `self` satisfying a
    /// concrete class, is handled) - a real return-type mismatch was
    /// reported on entirely correct code returning `new self(...)` or
    /// `null` from a `?self`-typed method.
    pub(crate) fn resolve_pseudo(&self, ty: &Type, defining_class: &str) -> Type {
        match ty {
            Type::SelfRef | Type::StaticRef => Type::Object(defining_class.to_string(), Vec::new()),
            Type::ParentRef => self
                .registry
                .classes
                .get(defining_class)
                .and_then(|info| info.extends.first())
                .map(|p| Type::Object(p.clone(), Vec::new()))
                .unwrap_or_else(|| ty.clone()),
            Type::Array(inner) => Type::Array(Box::new(self.resolve_pseudo(inner, defining_class))),
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|m| self.resolve_pseudo(m, defining_class))
                    .collect(),
            ),
            Type::Intersection(members) => Type::Intersection(
                members
                    .iter()
                    .map(|m| self.resolve_pseudo(m, defining_class))
                    .collect(),
            ),
            Type::ArrayShape(fields) => Type::ArrayShape(
                fields
                    .iter()
                    .map(|(k, t, opt)| (k.clone(), self.resolve_pseudo(t, defining_class), *opt))
                    .collect(),
            ),
            _ => ty.clone(),
        }
    }
}
