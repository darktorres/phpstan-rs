use crate::ast::StaticTarget;
use crate::checker::{Checker, Locals, class_type_word};
use crate::registry::{self, FunctionSig};
use crate::token::Span;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Resolves a bare name exactly as it appears at an expression site
    /// (`new Foo()`, `$x instanceof Foo`, ...) to the concrete `Type` it
    /// refers to - `self`/`parent`/`static` need `resolve_pseudo` against
    /// the class currently being checked; everything else goes through
    /// `name_ctx.resolve` (namespace/`use`-import resolution) as an
    /// ordinary class reference. `apply_narrowing`'s `instanceof` handling
    /// needs this too, not just `name_ctx.resolve` directly - otherwise
    /// `self`/`parent`/`static` get treated as literal class names
    /// (`$future instanceof self` in real code would become a
    /// nonsensical `Amp\self` "class").
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

    /// Parses a raw type-hint string (a closure/arrow-fn param/return, or
    /// An inline `@var` annotation via `inline_var_
    /// annotation` below - one of a few places in this checker that build
    /// a `Type` straight from source without going through the registry's
    /// own `FunctionSig::from_decl`, which already resolves names as part
    /// of its pipeline; `checker.rs`'s anonymous-class param/return
    /// docblock fallback is another, patched the same way at its own two
    /// call sites) and resolves any class name in it against this file's
    /// `NameContext`, exactly like a real declaration would - otherwise
    /// an unresolved bare name here compares as a real mismatch against
    /// the correctly-resolved FQN the closure body itself infers, on
    /// real code (a `static fn(array $row): ActionCount =>
    /// new ActionCount(...)` where `ActionCount` is `use`-imported).
    ///
    /// Also resolves `types::parse_type_keyword_or_class`'s
    /// `value-of<T>`-on-an-enum sentinel (`Registry::resolve_deferred_value_of_now`) -
    /// unlike `Registry::resolve_deferred_value_of`'s own one-time,
    /// registry-build-time pass, every class/enum is already fully
    /// indexed by the time *this* function ever runs, so resolution
    /// happens immediately, inline, with no separate pass needed. An
    /// inline `@var` annotation is checked through here, and without
    /// this, the raw, unresolved sentinel would leak straight into a
    /// real diagnostic message (real shape:
    /// `shipmonk/dead-code-detector`'s own `CollectedUsage::deserialize()`,
    /// `/** @var array{..., t: value-of<MemberType>, ...} $result */`).
    ///
    /// Also marks the *current class's own*
    /// `@template` names as `Type::TemplateParam` (`registry::
    /// mark_template_params`) *before* resolving names - order matters,
    /// the same as the registry-build pipeline's own `build_template_
    /// params` always running before `resolve_names_in_type` - since
    /// resolving names first would already have namespace-mangled the
    /// template's own short name beyond recognition. Without this, a
    /// class-level template referenced inside a raw type string parsed
    /// through here (an inline `@var`, most commonly) would otherwise be
    /// indistinguishable from a genuine, unqualified class reference and
    /// get resolved (and then, downstream, flagged) as one (real shape:
    /// `php-standard-library`'s own `SetType`/`MapType`/`MutableSetType`,
    /// each with an inline `/** @var Type\Type<T> $type */` referencing
    /// its own class-level `@template`).
    ///
    /// Also merges in the *current function/method's own* `@template`s
    /// (`current_function_template_params` below) - real corpus evidence
    /// this is needed too, not just a hypothetical: `php-standard-
    /// library`'s own top-level `group_by()` function, `@template Tk of
    /// array-key`, `/** @var Tk $key */` referencing it directly (no
    /// enclosing class at all). Function-level wins on a name collision
    /// with a class-level template of the same name, mirroring
    /// `FunctionSig::from_decl`'s own identical "this function's own
    /// same-named template wins over the class's" precedent.
    pub(crate) fn resolve_type_hint(&self, hint: Option<&str>) -> Type {
        self.resolve_type_hint_with_case_refs(hint).0
    }

    /// `resolve_type_hint`'s own case_refs-
    /// collecting sibling - a closure/arrow-fn literal has no `FunctionSig`
    /// of its own for `case_refs` to live on at registry-build time the
    /// way an ordinary function/method parameter's does, so this
    /// registry-independent, checker-side pipeline collects them fresh at
    /// check time instead, identically to how every other `resolve_type_
    /// hint` caller already resolves the type itself.
    #[allow(clippy::type_complexity)]
    pub(crate) fn resolve_type_hint_with_case_refs(
        &self,
        hint: Option<&str>,
    ) -> (Type, Vec<(std::sync::Arc<str>, std::sync::Arc<str>)>) {
        match hint {
            Some(h) => {
                let ty = types::parse_type_string(h);
                let mut params = self.current_class_template_params();
                params.extend(self.current_function_template_params());
                let ty = if params.is_empty() {
                    ty
                } else {
                    registry::mark_template_params(ty, &params)
                };
                let mut case_refs = Vec::new();
                registry::collect_class_name_case_refs(&ty, &self.name_ctx, &mut case_refs);
                let ty = registry::resolve_names_in_type(ty, &self.name_ctx);
                (self.registry.resolve_deferred_value_of_now(ty), case_refs)
            }
            None => (Type::Mixed(false), Vec::new()),
        }
    }

    /// Rebuilds the same `HashMap<String, TemplateParam>` `registry::
    /// build_template_params` would produce for `self.current_class`, by
    /// reading it back from the already-indexed `ClassInfo` instead of
    /// re-parsing a docblock - `resolve_type_hint` needs this to
    /// recognize a class-level template by name before ever resolving
    /// names in a type string built independently of the registry's own
    /// pipeline. Empty outside any class, or for a non-generic one.
    fn current_class_template_params(
        &self,
    ) -> std::collections::HashMap<String, types::TemplateParam> {
        let Some(class) = &self.current_class else {
            return std::collections::HashMap::new();
        };
        let Some(info) = self.registry.classes.get(class.as_str()) else {
            return std::collections::HashMap::new();
        };
        info.templates
            .iter()
            .map(|name| {
                let variance = if info.covariant_templates.contains(name) {
                    types::Variance::Covariant
                } else if info.contravariant_templates.contains(name) {
                    types::Variance::Contravariant
                } else {
                    types::Variance::Invariant
                };
                (
                    name.to_string(),
                    types::TemplateParam {
                        name: name.to_string(),
                        bound: Box::new(
                            info.template_bounds
                                .get(name)
                                .cloned()
                                // No `of`/`as` clause at all - explicit
                                // mixed, matching `TemplateParam::
                                // unbounded`'s own identical reasoning.
                                .unwrap_or(Type::Mixed(true)),
                        ),
                        variance,
                        scope: Some(class.clone()),
                        default: None,
                    },
                )
            })
            .collect()
    }

    /// The `current_class_template_params` equivalent for the current
    /// function/method's own `@template`s. `FunctionSig` doesn't persist
    /// its own raw template *names* the way `ClassInfo` does, so unlike
    /// the class-level lookup above, this reads them back off
    /// `Type::TemplateParam` nodes already present in the signature's
    /// params/return type (each still carrying its full
    /// bound/variance/default metadata from `mark_template_params` at
    /// registry-build time) - filtered to
    /// `tp.scope == Some(current_method)` so an *inherited* class-level
    /// template referenced in the same signature isn't double-counted.
    /// Only ever finds a template that appears somewhere in the
    /// signature itself; one declared but used only inside a body-local
    /// `@var`, never in the signature, isn't reachable this way.
    fn current_function_template_params(
        &self,
    ) -> std::collections::HashMap<String, types::TemplateParam> {
        let Some(method_name) = &self.current_method else {
            return std::collections::HashMap::new();
        };
        let sig = match &self.current_class {
            Some(class) => self.registry.lookup_method(class, method_name),
            // A plain function's own `label`/`current_method` is
            // deliberately the bare name (matching real PHPStan's
            // uncluttered diagnostic messages), but `Registry::functions`
            // is keyed by each declaration's real FQN - a namespaced
            // function's bare name never matches there directly (real
            // shape: `php-standard-library`'s own `Psl\Dict\group_by()`
            // - `lookup_function("group_by")` alone finds nothing, since
            // there's no *global* function by that name).
            None => self
                .registry
                .lookup_function(&self.name_ctx.declared_name(method_name)),
        };
        let Some(sig) = sig else {
            return std::collections::HashMap::new();
        };
        let mut out = std::collections::HashMap::new();
        for p in &sig.params {
            collect_scoped_template_params(&p.ty, method_name, &mut out);
        }
        collect_scoped_template_params(&sig.return_type, method_name, &mut out);
        out
    }

    /// `FunctionDefinitionCheck`'s own
    /// `method.templateTypeNotInParameter` half - applies uniformly to a
    /// function/method's own `@template` (real PHPStan runs the
    /// identical check for a closure/arrow-fn too, but neither is
    /// indexed into this project's registry, so there's no persisted
    /// `FunctionSig`/template-name list for either to run against). Real
    /// PHPStan's `TypeTraverser`-based full-tree walk over every
    /// parameter's type is what the params-only pass below re-derives; a
    /// template that only shows up in the return type (the common,
    /// high-value shape - a generic factory method's `@return T` with no
    /// `T`-typed parameter) is what the difference against the
    /// params+return pass catches. Deliberately narrower in one way: a
    /// template declared but referenced nowhere at all in its signature
    /// is invisible to this check too, since `FunctionSig` has no
    /// separate record of a declared template's bare name independent of
    /// where it's used (see `current_function_template_params`).
    pub(crate) fn check_template_type_not_in_parameter(
        &mut self,
        sig: &FunctionSig,
        bare_name: &str,
        class_name: Option<&str>,
        label: &str,
        span: Span,
    ) {
        let mut in_params = std::collections::HashMap::new();
        for p in &sig.params {
            collect_scoped_template_params(&p.ty, bare_name, &mut in_params);
        }
        let mut in_signature = in_params.clone();
        collect_scoped_template_params(&sig.return_type, bare_name, &mut in_signature);
        if in_signature.len() == in_params.len() {
            return;
        }
        let kind_word = if class_name.is_some() {
            "method"
        } else {
            "function"
        };
        let mut missing: Vec<&String> = in_signature
            .keys()
            .filter(|name| !in_params.contains_key(*name))
            .collect();
        missing.sort();
        for name in missing {
            self.report(
                label.to_string(),
                format!("Template type {name} of {kind_word} {label}() is not referenced in a parameter."),
                span,
                0,
                "method.templateTypeNotInParameter",
            );
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

    /// `StaticCall`'s own full `StaticTarget` resolution, for a
    /// `check_expr`-side call site (full diagnostics): `Name` resolves
    /// exactly like `resolve_static_class_name` always has; a
    /// genuinely dynamic `Expr` receiver (`$obj::method()`) is checked
    /// in full first - so an undefined variable, an unresolvable
    /// property access, etc. inside it is still caught, the same as any
    /// other checked sub-expression - then resolved via its own real
    /// inferred type, exactly like a literal class name already is.
    /// `None` whenever that type isn't a known `Object` (unresolvable,
    /// same "don't know, don't guess" leniency `resolve_static_class_name`
    /// already has for `self`/`parent`/`static` outside any class
    /// context).
    pub(crate) fn resolve_static_target_class(
        &mut self,
        target: &StaticTarget,
        locals: &mut Locals,
        label: &str,
    ) -> Option<String> {
        match target {
            StaticTarget::Name(name, _) => self.resolve_static_class_name(name),
            StaticTarget::Expr(receiver) => match self.check_expr(receiver, locals, label) {
                Type::Object(class, _) => Some(class),
                _ => None,
            },
        }
    }

    /// `resolve_static_target_class`'s own `infer_expr`-side counterpart
    /// (no diagnostics - the receiver is only *inferred*, via the exact
    /// same silent path every other speculative/probe inference already
    /// uses, never re-checked).
    pub(crate) fn infer_static_target_class(
        &self,
        target: &StaticTarget,
        locals: &Locals,
    ) -> Option<String> {
        match target {
            StaticTarget::Name(name, _) => self.resolve_static_class_name(name),
            StaticTarget::Expr(receiver) => match self.infer_expr(receiver, locals) {
                Type::Object(class, _) => Some(class),
                _ => None,
            },
        }
    }

    /// Resolves `self`/`static`/`parent` to a concrete `Object` relative
    /// to `defining_class` - the class whose signature this type came
    /// from. `self`/`static` both collapse to the same class here, since
    /// this project doesn't track late static binding through actual
    /// runtime dispatch; `parent` resolves via the registry's `extends`
    /// chain, falling back to leaving it unresolved if the class has no
    /// declared parent (so it still hits the permissive `Object`
    /// fallback in `is_subtype`/`is_assignable` rather than being
    /// wrongly flagged).
    ///
    /// Recurses into `Union`/`Intersection`/`Array`/`ArrayShape` the same
    /// way `types::substitute_template` does, not just the bare
    /// top-level case - needed for the common nullable-self return type
    /// (`?self`/`self|null`, e.g. `tryFromArray`-style factory methods):
    /// `is_assignable`/`is_subtype` have no arm for a concrete class
    /// instance satisfying a still-unresolved `SelfRef` (only the
    /// reverse direction is handled), so a bare `self` return type
    /// wrapped in any union must be resolved here first.
    pub(crate) fn resolve_pseudo(&self, ty: &Type, defining_class: &str) -> Type {
        match ty {
            Type::SelfRef | Type::StaticRef => Type::Object(defining_class.to_string(), Vec::new()),
            Type::Object(name, args) if name == "$$self" || name == "$$static" => Type::Object(
                defining_class.to_string(),
                args.iter()
                    .map(|a| self.resolve_pseudo(a, defining_class))
                    .collect(),
            ),
            Type::ParentRef => self
                .registry
                .classes
                .get(defining_class)
                .and_then(|info| info.extends.first())
                .map(|p| Type::Object(p.to_string(), Vec::new()))
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
            // `object{...}`'s own field types also need
            // `self`/`static`/`parent` resolved.
            Type::ObjectShape(fields) => Type::ObjectShape(
                fields
                    .iter()
                    .map(|(k, t, opt)| (k.clone(), self.resolve_pseudo(t, defining_class), *opt))
                    .collect(),
            ),
            // A generic class's own template arguments
            // (`SortEntry<self, string>` - `Object("SortEntry",
            // [SelfRef, String])`) need `self`/`static`/`parent`
            // resolved inside the argument list too, not just at the top
            // level - otherwise a `list<SortEntry<self, string>>`
            // declared return type's `self` would never match the
            // concrete class an actual returned
            // `SortEntry<UserSortField, string>` carries.
            Type::Object(name, args) => Type::Object(
                name.clone(),
                args.iter()
                    .map(|a| self.resolve_pseudo(a, defining_class))
                    .collect(),
            ),
            // PHPDoc conditional return types (`@return ($x is Foo ? A :
            // B)`) can carry `self`/`static`/`parent` inside any of their
            // three type positions too.
            Type::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
                template_subject,
            } => Type::Conditional {
                subject: subject.clone(),
                target: Box::new(self.resolve_pseudo(target, defining_class)),
                if_true: Box::new(self.resolve_pseudo(if_true, defining_class)),
                if_false: Box::new(self.resolve_pseudo(if_false, defining_class)),
                negated: *negated,
                template_subject: *template_subject,
            },
            // `Closure(self): static` - a callable shape's own parameter
            // and return types.
            Type::Callable(Some(sig)) => {
                Type::Callable(Some(std::sync::Arc::new(types::CallableSig {
                    params: sig
                        .params
                        .iter()
                        .map(|p| types::CallableParam {
                            ty: self.resolve_pseudo(&p.ty, defining_class),
                            ..p.clone()
                        })
                        .collect(),
                    return_type: Box::new(self.resolve_pseudo(&sig.return_type, defining_class)),
                })))
            }
            _ => ty.clone(),
        }
    }

    /// The "does this name actually resolve" primitive: `name` is a
    /// real class/interface/trait/enum this checker's `Registry`
    /// indexed, or one of the small, fixed set of native type-hint
    /// keywords `types::parse_type_keyword_or_class` deliberately
    /// encodes as `Type::Object` despite not being a real class
    /// reference (mirrors `is_type_fully_known`'s identical `"object"`
    /// exemption, plus `resolve_names_in_type`'s
    /// `"Traversable"`/`"resource"` exemptions), so a plain
    /// `object`/`resource`-typed parameter or `iterable` (desugared to
    /// `array|Traversable`) never gets flagged as an unresolvable class.
    /// `"class@anonymous"` is this project's fixed, literal synthetic
    /// name for every anonymous class, deliberately never indexed into
    /// the registry - without this exemption, a `self`/`static`-typed
    /// return inside an anonymous class's own body would always read as
    /// an unresolvable class.
    pub(crate) fn class_name_exists(&self, name: &str) -> bool {
        matches!(
            name,
            "object"
                | "Traversable"
                | "resource"
                | "closed-resource"
                | "class@anonymous"
                | "$$self"
                | "$$static"
        ) || self.registry.lookup_class(name).is_some()
    }

    /// `ClassCaseSensitivityCheck`'s equivalent, covering every
    /// *native-syntax* class-name reference: a class/enum declaration's
    /// own `extends`/`implements`/trait-`use` references, an attribute
    /// reference, `new`, `instanceof`, a static method
    /// call/property/constant access (`Foo::...`), and a `catch` clause,
    /// where the raw, as-written text is already sitting in the AST as a
    /// bare `String`/`ExprKind::Name` field, unlike a
    /// *PHPDoc-typed* position (a param/return/property type hint,
    /// `@method`/`@mixin`/`@phpstan-type` tag target), where this
    /// project's type-parsing pipeline collapses every reference
    /// straight into a resolved `Type` with no per-reference raw
    /// spelling retained - that needs a parallel traversal run earlier
    /// in the parse pipeline instead
    /// (`registry::collect_class_name_case_refs`); see
    /// `check_type_class_name_case` below, this function's fan-out
    /// consumer for that PHPDoc-typed case. Skips a `use`-imported
    /// reference and a built-in stub class, matching real PHPStan's own
    /// non-bleeding-edge default config
    /// (`checkImportedClassNameCase`/`checkInternalClassCaseSensitivity`
    /// both `false`). `raw` is the reference exactly as written in
    /// source (needed fresh here, not `ClassInfo::extends`/etc., which
    /// `Registry::canonicalize_class_name_case` already rewrites to
    /// canonical case at registry-build time).
    pub(crate) fn check_class_name_case(&mut self, fqn: &str, raw: &str, span: Span) {
        let mut segments = raw.trim_start_matches('\\').splitn(2, '\\');
        let first_segment = segments.next().unwrap_or(raw);
        if self.name_ctx.is_use_imported(first_segment) {
            return;
        }
        let resolved = self.name_ctx.resolve(raw);
        let Some((canonical, info)) = self.registry.lookup_class(&resolved) else {
            return;
        };
        if self.registry.built_in_stub_classes.contains(canonical) {
            return;
        }
        if canonical != resolved {
            let type_word = class_type_word(info.kind);
            self.report(
                fqn.to_string(),
                format!("{type_word} referenced with incorrect case: {resolved}."),
                span,
                0,
                &format!("{}.nameCase", type_word.to_lowercase()),
            );
        }
    }

    /// `class.nameCase` for a PHPDoc-typed position (a
    /// param/return/property type hint, `@method`/`@mixin`/
    /// `@phpstan-type` tag target). Pure fan-out over `case_refs`
    /// (`(raw, resolved)` pairs from `collect_class_name_case_refs`,
    /// populated once at registry-build time) - re-runs the *unchanged*
    /// `check_class_name_case` per pair rather than duplicating its
    /// `is_use_imported`/built-in-stub-skip/`lookup_class` logic. Every
    /// reference nested inside one param/return/property's type (e.g.
    /// both class names in `array<Foo, Bar>`) is reported at the *same*
    /// `span`, since this project's PHPDoc parsing keeps no
    /// per-reference span within a docblock's type text, only the
    /// position of the type token as a whole.
    pub(crate) fn check_type_class_name_case(
        &mut self,
        case_refs: &[(std::sync::Arc<str>, std::sync::Arc<str>)],
        label: &str,
        span: Span,
    ) {
        for (raw, _resolved) in case_refs {
            self.check_class_name_case(label, raw, span);
        }
    }

    /// Method-call/static-call sibling of `ExprKind::Call`'s own inline
    /// `function.nameCase` check, gated on the same `Registry::
    /// check_function_name_case` flag as `use function`'s own check.
    /// `Registry::lookup_method_canonical_name` mirrors `lookup_class`'s
    /// case-insensitive-fallback shape.
    pub(crate) fn check_call_site_method_name_case(
        &mut self,
        class: &str,
        method: &str,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_function_name_case {
            return;
        }
        let Some(canonical) = self.registry.lookup_method_canonical_name(class, method) else {
            return;
        };
        if canonical != method {
            self.report(
                label.to_string(),
                format!("Call to method {class}::{canonical}() with incorrect case: {method}."),
                span,
                0,
                "method.nameCase",
            );
        }
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
    /// argument list (`Object(name, args)`'s own `args`), mirroring
    /// `check_generic_bounds`/`is_type_fully_known`'s identical
    /// leniency: a generic argument referencing a real class this
    /// project's registry has no data for (e.g. a PHAR-bundled
    /// `phpstan/phpstan`) is common enough that existence-checking it
    /// too would trade a missed rare typo for a false positive on the
    /// ordinary case.
    pub(crate) fn collect_unknown_class_names(&self, ty: &Type, out: &mut Vec<String>) {
        match ty {
            // A call-site variance sentinel's own name is never a real
            // class reference to flag, matching this function's leniency
            // of not recursing into a generic argument list at all - the
            // wrapped inner type isn't recursed into here either.
            // `$$interface`/`$$trait` (`interface-string`/`trait-string`'s
            // internal bound) need the same exclusion, checked by
            // `ClassKind` directly at `registry::is_assignable`'s
            // `ClassString` arm instead.
            Type::Object(name, _)
                if matches!(
                    name.as_str(),
                    "$$covariant" | "$$contravariant" | "$$bivariant" | "$$interface" | "$$trait"
                ) => {}
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
            // `object{...}`'s own field types need checking for
            // existence too.
            Type::ObjectShape(fields) => {
                for (_, t, _) in fields {
                    self.collect_unknown_class_names(t, out);
                }
            }
            _ => {}
        }
    }

    /// `FunctionDefinitionCheck`'s own "resolved but a trait" half -
    /// `parameter.trait`/`return.trait`/`property.trait`: a native
    /// type-hint resolving to a real, indexed class that turns out to be
    /// a `trait` rather than a class/interface/enum. Real PHP itself
    /// rejects this at the engine level, so this is always a real bug,
    /// never a corpus-noise risk. Mirrors `collect_unknown_class_names`'s
    /// exact recursive shape (including its non-recursion into a class's
    /// `@template` argument list) - the two are mutually exclusive per
    /// name, so no name is ever double-reported under both identifiers.
    pub(crate) fn collect_trait_class_names(&self, ty: &Type, out: &mut Vec<String>) {
        match ty {
            Type::Object(name, _)
                if matches!(
                    name.as_str(),
                    "$$covariant" | "$$contravariant" | "$$bivariant"
                ) => {}
            Type::Object(name, _args) => {
                if self
                    .registry
                    .lookup_class(name)
                    .is_some_and(|(_, info)| info.kind == crate::ast::ClassKind::Trait)
                    && !out.iter().any(|n| n == name)
                {
                    out.push(name.clone());
                }
            }
            Type::Array(key, inner) => {
                self.collect_trait_class_names(key, out);
                self.collect_trait_class_names(inner, out);
            }
            Type::ClassString(inner) => self.collect_trait_class_names(inner, out),
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.collect_trait_class_names(m, out);
                }
            }
            Type::ArrayShape(fields) => {
                for (_, t, _) in fields {
                    self.collect_trait_class_names(t, out);
                }
            }
            Type::ObjectShape(fields) => {
                for (_, t, _) in fields {
                    self.collect_trait_class_names(t, out);
                }
            }
            _ => {}
        }
    }

    /// The reporting half for `collect_trait_class_names` above -
    /// `identifier` is one of `parameter.trait`/`return.trait`/
    /// `property.trait` depending on the call site's kind, matching real
    /// PHPStan's uniform "same message template as `class.notFound`,
    /// different identifier" shape.
    pub(crate) fn check_type_trait_refs(
        &mut self,
        ty: &Type,
        subject: &str,
        span: Span,
        label: &str,
        identifier: &'static str,
    ) {
        let mut traits = Vec::new();
        self.collect_trait_class_names(ty, &mut traits);
        for name in traits {
            self.report(
                label.to_string(),
                format!("{subject} has invalid type {name}"),
                span,
                0,
                identifier,
            );
        }
    }

    /// `parameter.noParent`/`return.noParent` - real PHPStan's
    /// `NonexistentParentClassType` (`describe()` returns the literal
    /// string `"parent"`), produced when a native `parent` type-hint is
    /// used on a class with no `extends` ancestor. This project's
    /// equivalent is `resolve_pseudo` deliberately leaving a bare
    /// `Type::ParentRef` unresolved in exactly that situation - `ty`
    /// here must already be the `resolve_pseudo`'d result, not the raw
    /// pre-resolution type, or this can never find anything. Walks
    /// the full type tree (unlike real PHPStan's own top-level-only
    /// `instanceof` check) since this project's own `Type` can nest a
    /// `ParentRef` inside a `Union` for the common `?parent` shape real
    /// PHP itself allows - a real, more-thorough case real PHPStan's own
    /// narrower check structurally can't hit the same way.
    pub(crate) fn collect_bare_parent_refs(&self, ty: &Type, found: &mut bool) {
        match ty {
            Type::ParentRef => *found = true,
            Type::Array(key, inner) => {
                self.collect_bare_parent_refs(key, found);
                self.collect_bare_parent_refs(inner, found);
            }
            Type::ClassString(inner) => self.collect_bare_parent_refs(inner, found),
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.collect_bare_parent_refs(m, found);
                }
            }
            Type::ArrayShape(fields) | Type::ObjectShape(fields) => {
                for (_, t, _) in fields {
                    self.collect_bare_parent_refs(t, found);
                }
            }
            _ => {}
        }
    }

    /// The reporting half for `collect_bare_parent_refs` above -
    /// `identifier` is `parameter.noParent` or `return.noParent`
    /// depending on the call site, `subject` the same phrase the sibling
    /// `class.notFound`/`.trait` checks at that exact site already use.
    pub(crate) fn check_no_parent_ref(
        &mut self,
        ty: &Type,
        subject: &str,
        span: Span,
        label: &str,
        identifier: &'static str,
    ) {
        let mut found = false;
        self.collect_bare_parent_refs(ty, &mut found);
        if found {
            self.report(
                label.to_string(),
                format!("{subject} has invalid type parent"),
                span,
                0,
                identifier,
            );
        }
    }

    /// The shared reporting half every existence-checking call site
    /// (parameter/return/property type hints, closure/arrow-fn
    /// typehints, ...) reduces to once it has its `subject` phrase ready
    /// (`"parameter $foo of Foo::bar()"`, `"return type of Foo::bar()"`,
    /// `"property Foo::$bar"`, ...) - one `class.notFound` diagnostic per
    /// distinct unresolvable name `ty` references, mirroring the message
    /// `checker/expr.rs`'s `ExprKind::New` arm uses the same identifier
    /// for.
    pub(crate) fn check_type_class_names_exist(
        &mut self,
        ty: &Type,
        subject: &str,
        span: Span,
        label: &str,
    ) {
        let mut unknown = Vec::new();
        self.collect_unknown_class_names(ty, &mut unknown);
        let mut seen = std::collections::HashSet::new();
        unknown.retain(|n| seen.insert(n.clone()));
        for name in unknown {
            self.report(
                label.to_string(),
                format!("{subject} has invalid type {name}"),
                span,
                0,
                "class.notFound",
            );
        }
        // `class.prefixed`: checked on the *successfully-resolved* name
        // too, unlike the `class.notFound` loop above (which only sees a
        // name `collect_unknown_class_names` couldn't find) - a leaked
        // Rector/PHP-Scoper/PHPUnit-PHAR/Box-prefixed vendor class is
        // almost always still a real, indexed class, so it would never
        // appear in `unknown` above.
        let mut prefixed = Vec::new();
        collect_prefixed_class_names(ty, &mut prefixed);
        let mut seen = std::collections::HashSet::new();
        prefixed.retain(|(n, _)| seen.insert(*n));
        for (name, project) in prefixed {
            self.report_builder(
                crate::checker::DiagnosticBuilder::new(
                    label.to_string(),
                    format!("Referencing prefixed {project} class: {name}."),
                    span,
                    0,
                    "class.prefixed",
                )
                .non_ignorable(),
            );
        }
    }
}

/// `collect_unknown_class_names`'s sibling, minus the existence filter
/// (a prefixed vendor class is almost always a real, indexed one) -
/// otherwise the identical recursive shape.
fn collect_prefixed_class_names<'b>(ty: &'b Type, out: &mut Vec<(&'b str, &'static str)>) {
    match ty {
        Type::Object(name, _args) => {
            for entry in crate::checker::INTERNAL_CLASS_PREFIXES {
                let (project, prefix): (&str, &str) = *entry;
                if name.starts_with(prefix) {
                    out.push((name.as_str(), project));
                    break;
                }
            }
        }
        Type::Array(key, inner) => {
            collect_prefixed_class_names(key, out);
            collect_prefixed_class_names(inner, out);
        }
        Type::ClassString(inner) => collect_prefixed_class_names(inner, out),
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                collect_prefixed_class_names(m, out);
            }
        }
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => {
            for (_, t, _) in fields {
                collect_prefixed_class_names(t, out);
            }
        }
        _ => {}
    }
}

/// Walks `ty` collecting every distinct `Type::TemplateParam` node
/// whose `.scope` matches `scope` exactly - `Checker::
/// current_function_template_params`'s recursive helper. Mirrors
/// `resolve_pseudo`'s/`collect_unknown_class_names`'s recursive shape,
/// but includes `Object`'s own generic argument list (a template can
/// appear nested inside another class's type argument, e.g.
/// `array<int, Box<T>>`).
fn collect_scoped_template_params(
    ty: &Type,
    scope: &str,
    out: &mut std::collections::HashMap<String, types::TemplateParam>,
) {
    match ty {
        Type::TemplateParam(tp) if tp.scope.as_deref() == Some(scope) => {
            out.entry(tp.name.clone()).or_insert_with(|| tp.clone());
        }
        Type::Object(_, args) => {
            for a in args {
                collect_scoped_template_params(a, scope, out);
            }
        }
        Type::Array(key, inner) => {
            collect_scoped_template_params(key, scope, out);
            collect_scoped_template_params(inner, scope, out);
        }
        Type::ClassString(inner) => collect_scoped_template_params(inner, scope, out),
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                collect_scoped_template_params(m, scope, out);
            }
        }
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => {
            for (_, t, _) in fields {
                collect_scoped_template_params(t, scope, out);
            }
        }
        // A template only reachable inside a `Closure(...)`-shaped
        // parameter's return-type slot (real shape:
        // `php-standard-library`'s own `Psl\Dict\group_by()`,
        // `@template Tk of array-key`/`@param (Closure(Tv): ?Tk) $keyFunc`
        // - `Tk` never appears as that parameter's own bare top-level
        // type) is exactly as real an inference source as one reachable
        // through an ordinary class's generic argument list above.
        Type::Callable(Some(sig)) => {
            for p in &sig.params {
                collect_scoped_template_params(&p.ty, scope, out);
            }
            collect_scoped_template_params(&sig.return_type, scope, out);
        }
        _ => {}
    }
}
