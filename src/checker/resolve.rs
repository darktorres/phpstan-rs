use crate::checker::Checker;
use crate::registry::{self};
use crate::token::Span;
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
            Type::Array(key, inner) => Type::Array(
                Box::new(self.resolve_pseudo(key, defining_class)),
                Box::new(self.resolve_pseudo(inner, defining_class)),
            ),
            Type::ClassString(inner) => {
                Type::ClassString(Box::new(self.resolve_pseudo(inner, defining_class)))
            }
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

    /// Phase 48's "does this name actually resolve" primitive: `name` is a
    /// real class/interface/trait/enum this checker's own `Registry`
    /// indexed, or one of the small, fixed set of native type-hint
    /// keywords this project's own `types::parse_type_keyword_or_class`
    /// deliberately encodes as `Type::Object` despite not being a real
    /// class reference at all - mirrors `is_type_fully_known`'s (
    /// `checker/generics.rs`) identical `"object"` exemption, plus
    /// `resolve_names_in_type`'s (`registry/name_context.rs`) own
    /// `"Traversable"`/`"resource"` exemptions, so a plain `object`/
    /// `resource`-typed parameter or a namespaced file's `iterable`
    /// (desugared to `array|Traversable`) never gets flagged as an
    /// unresolvable class the way a genuine typo would. `"class@anonymous"`
    /// is this project's own fixed, literal synthetic name for EVERY
    /// anonymous class (`parser/classes.rs`'s own `ClassDecl::name` for
    /// one, confirmed never unique per-instance) - deliberately never
    /// indexed into the registry at all (see `ExprKind::NewAnonymousClass`'s
    /// own comment), so a `self`/`static`-typed return inside an anonymous
    /// class's own body (`resolve_pseudo` resolving `static` to exactly
    /// this name) would otherwise always read as an unresolvable class,
    /// on the most ordinary possible body.
    pub(crate) fn class_name_exists(&self, name: &str) -> bool {
        matches!(
            name,
            "object" | "Traversable" | "resource" | "class@anonymous"
        ) || self.registry.classes.contains_key(name)
    }

    /// Walks an already name-resolved `Type` (every `Object` inside it
    /// already a fully-qualified name, the same shape `check_generic_
    /// bounds`/`is_type_fully_known` already expect), collecting every
    /// distinct top-level class name that doesn't actually resolve
    /// (`class_name_exists`) - recurses through `Union`/`Intersection`/
    /// `Array`/`ArrayShape`/`ClassString` the same way `resolve_pseudo`
    /// does, so a typo buried inside e.g. `list<Foo\Bar>` or `Foo|Bar` is
    /// still caught, not just a bare top-level reference. A real
    /// `@template T` never reaches the `Object` arm at all - `registry::
    /// mark_template_params` already rewrites every one of a function's/
    /// class's own declared template names to `Type::TemplateParam`
    /// before this checker ever sees the signature, so this never
    /// mistakes a generic placeholder for a missing class.
    ///
    /// Deliberately does NOT recurse into a class's own `@template`
    /// argument list (`Object(name, args)`'s own `args`) - mirrors
    /// `check_generic_bounds`/`is_type_fully_known`'s own identical,
    /// deliberate leniency there (`checker/generics.rs`), for the exact
    /// same real reason: a real corpus (Piwigo, checking against a
    /// PHAR-bundled `phpstan/phpstan` with no loose source files at all)
    /// found a generic argument referencing a real class this project's
    /// own registry simply has no data for is common enough that
    /// existence-checking it too would trade a missed rare typo for a
    /// false positive on the ordinary case - the same tradeoff this
    /// project already made once for bound-checking.
    pub(crate) fn collect_unknown_class_names(&self, ty: &Type, out: &mut Vec<String>) {
        match ty {
            Type::Object(name, _args) => {
                if !self.class_name_exists(name) && !out.iter().any(|n| n == name) {
                    out.push(name.clone());
                }
            }
            Type::Array(key, inner) => {
                self.collect_unknown_class_names(key, out);
                self.collect_unknown_class_names(inner, out);
            }
            Type::ClassString(inner) => self.collect_unknown_class_names(inner, out),
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.collect_unknown_class_names(m, out);
                }
            }
            Type::ArrayShape(fields) => {
                for (_, t, _) in fields {
                    self.collect_unknown_class_names(t, out);
                }
            }
            _ => {}
        }
    }

    /// The shared reporting half every existence-checking call site
    /// (parameter/return/property type hints, closure/arrow-fn
    /// typehints, ...) reduces to once it has its own `subject` phrase
    /// ready (`"parameter $foo of Foo::bar()"`, `"return type of
    /// Foo::bar()"`, `"property Foo::$bar"`, ...) - one `class.notFound`
    /// diagnostic per distinct unresolvable name `ty` references,
    /// mirroring the message this project's own pre-existing `new Foo()`
    /// existence check (`checker/expr.rs`'s `ExprKind::New` arm) already
    /// uses the same identifier for.
    pub(crate) fn check_type_class_names_exist(
        &mut self,
        ty: &Type,
        subject: &str,
        span: Span,
        label: &str,
    ) {
        let mut unknown = Vec::new();
        self.collect_unknown_class_names(ty, &mut unknown);
        for name in unknown {
            self.report(
                label.to_string(),
                format!("{subject} has invalid type {name}"),
                span,
                0,
                "class.notFound",
            );
        }
    }
}
