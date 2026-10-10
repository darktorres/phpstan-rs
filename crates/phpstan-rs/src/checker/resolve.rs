use crate::checker::{Checker, Locals, class_type_word};
use crate::registry::{self, FunctionReflection};
use diagnostics::DiagnosticBuilder;
use syntax::ast::{QualifiedName, StaticTarget, TypeHint};
use syntax::span::Span;
use types::Name;
use types::{self, Type};

impl<'a> Checker<'a> {
    /// The class type a class name written in code (`new Foo()`, `$x
    /// instanceof Foo`, ...) refers to: `self`, `parent` and `static`
    /// relative to the class being checked, any other name as resolved.
    pub(crate) fn resolve_name_expr(&self, name: &QualifiedName) -> Type {
        let keyword = |k: &str| name.written.eq_ignore_ascii_case(k);
        let raw = if keyword("self") {
            Type::self_ref(Vec::new())
        } else if keyword("parent") {
            Type::parent_ref()
        } else if keyword("static") {
            Type::static_ref(Vec::new())
        } else {
            return Type::object(self.resolve_class_name(name).into(), Vec::new());
        };
        match &self.current_class {
            Some(c) => self.resolve_pseudo(&raw, c),
            None => raw,
        }
    }

    /// The class type a class name held in a string names. PHP reads such
    /// a name as fully qualified, whatever the file's namespace.
    pub(crate) fn class_type_named(&self, name: &str) -> Type {
        Type::object(self.canonical_class_name(name).into(), Vec::new())
    }

    /// The fully-qualified class a class name written in code names, in
    /// its declared spelling when the class is known.
    pub(crate) fn resolve_class_name(&self, name: &QualifiedName) -> String {
        match name.resolved_class() {
            Some(resolved) => self.canonical_class_name(resolved),
            None => name.written.to_string(),
        }
    }

    /// `name`, a fully-qualified class name, in its declared spelling
    /// when the class is known - PHP class names are case-insensitive.
    pub(crate) fn canonical_class_name(&self, name: &str) -> String {
        let name = name.trim_start_matches('\\');
        match self.registry.lookup_class(name) {
            Some((canonical, _)) => canonical.to_string(),
            None => name.to_string(),
        }
    }

    /// The type a native type declaration declares.
    pub(crate) fn resolve_type_hint(&self, hint: Option<&TypeHint>) -> Type {
        let Some(hint) = hint else {
            return Type::mixed();
        };
        let Some(owner) = self.syntax_tree_owner() else {
            return Type::mixed();
        };
        types::native_type(hint, owner.arenas())
    }

    /// `resolve_type_hint`, with the `(written, resolved)` pairs of the
    /// class names in it that `class.nameCase` needs.
    #[allow(clippy::type_complexity)]
    pub(crate) fn resolve_type_hint_with_case_refs(
        &self,
        hint: Option<&TypeHint>,
    ) -> (Type, Vec<(Name, Name)>) {
        let Some(hint) = hint else {
            return (Type::mixed(), Vec::new());
        };
        let Some(owner) = self.syntax_tree_owner() else {
            return (Type::mixed(), Vec::new());
        };
        let arenas = owner.arenas();
        let case_refs = hint
            .class_names(arenas)
            .into_iter()
            .filter_map(|name| {
                let resolved = name.resolved_class()?;
                Some((Name::from(&name.written), Name::from(resolved)))
            })
            .collect();
        (types::native_type(hint, arenas), case_refs)
    }

    /// `resolve_type_hint` for a parsed PHPDoc type, which `site` reads.
    pub(crate) fn resolve_type_node<T: phpdoc::WrittenType + ?Sized>(
        &self,
        node: &T,
        site: &str,
    ) -> Type {
        let written = registry::parse_type_node(node);
        let resolved = self.resolve_written_type(written.clone()).0;
        self.record_doc_resolution(site, &written, &resolved);
        resolved
    }

    /// Resolves a declaration's PHPDoc type and returns its written class
    /// references for the checks that run at that same declaration.
    #[allow(clippy::type_complexity)]
    pub(crate) fn resolve_type_node_with_case_refs<T: phpdoc::WrittenType + ?Sized>(
        &self,
        node: &T,
    ) -> (Type, Vec<(Name, Name)>) {
        self.resolve_written_type(registry::parse_type_node(node))
    }

    /// Resolves a type as written in the current scope: its templates
    /// marked, its class names resolved, with the case refs of those names.
    #[allow(clippy::type_complexity)]
    fn resolve_written_type(&self, ty: Type) -> (Type, Vec<(Name, Name)>) {
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

    /// Rebuilds the same `HashMap<String, TemplateParam>` `registry::
    /// build_template_params` would produce for `self.current_class`, by
    /// reading it back from the already-indexed `ClassReflection` instead of
    /// re-parsing a docblock - `resolve_type_node` needs this to
    /// recognize a class-level template by name before ever resolving
    /// names in a type string built independently of the registry's own
    /// pipeline. Empty outside any class, or for a non-generic one.
    pub(crate) fn current_class_template_params(
        &self,
    ) -> rustc_hash::FxHashMap<String, types::TemplateParam> {
        match &self.current_class {
            Some(class) => self.class_template_params_for(class),
            None => rustc_hash::FxHashMap::default(),
        }
    }

    /// `current_class_template_params` for any class.
    pub(crate) fn class_template_params_for(
        &self,
        class: &str,
    ) -> rustc_hash::FxHashMap<String, types::TemplateParam> {
        let class = &class.to_string();
        let Some(info) = self.registry.class_by_name(class) else {
            return rustc_hash::FxHashMap::default();
        };
        let bounds = self.registry.effective_template_bounds_by_name(class);
        info.templates()
            .iter()
            .map(|name| {
                let variance = if info.covariant_templates().contains(name) {
                    types::Variance::Covariant
                } else if info.contravariant_templates().contains(name) {
                    types::Variance::Contravariant
                } else {
                    types::Variance::Invariant
                };
                (
                    name.to_string(),
                    types::TemplateParam {
                        name: name.to_string(),
                        bound: Box::new(
                            bounds
                                .get(name)
                                .cloned()
                                // No `of`/`as` clause at all - explicit
                                // mixed, matching `TemplateParam::
                                // unbounded`'s own identical reasoning.
                                .unwrap_or(Type::explicit_mixed()),
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
    /// function/method's own `@template`s. `FunctionReflection` doesn't persist
    /// its own raw template *names* the way `ClassReflection` does, so unlike
    /// the class-level lookup above, this reads them back off
    /// template nodes already present in the signature's
    /// params/return type (each still carrying its full
    /// bound/variance/default metadata from `mark_template_params` at
    /// registry-build time) - filtered to
    /// `tp.scope == Some(current_method)` so an *inherited* class-level
    /// template referenced in the same signature isn't double-counted.
    /// Only ever finds a template that appears somewhere in the
    /// signature itself; one declared but used only inside a body-local
    /// `@var`, never in the signature, isn't reachable this way.
    pub(crate) fn current_function_template_params(
        &self,
    ) -> rustc_hash::FxHashMap<String, types::TemplateParam> {
        let Some(method_name) = &self.current_method else {
            return rustc_hash::FxHashMap::default();
        };
        self.function_template_params(self.current_class.as_deref(), method_name)
    }

    /// `current_function_template_params` for the function `method_name`
    /// of `class` (none for a plain function), whatever the checker is
    /// inside of: a declaration check reads its own before it enters it.
    pub(crate) fn function_template_params(
        &self,
        class: Option<&str>,
        method_name: &str,
    ) -> rustc_hash::FxHashMap<String, types::TemplateParam> {
        let sig = match class {
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
            return self.current_method_templates.clone();
        };
        let mut out = rustc_hash::FxHashMap::default();
        for p in sig.params() {
            collect_scoped_template_params(p.ty(), method_name, &mut out);
        }
        collect_scoped_template_params(sig.return_type(), method_name, &mut out);
        out
    }

    /// `FunctionDefinitionCheck`'s own
    /// `method.templateTypeNotInParameter` half - applies uniformly to a
    /// function/method's own `@template` (real PHPStan runs the
    /// identical check for a closure/arrow-fn too, but neither is
    /// indexed into this project's registry, so there's no persisted
    /// `FunctionReflection`/template-name list for either to run against). Real
    /// PHPStan's `TypeTraverser`-based full-tree walk over every
    /// parameter's type is what the params-only pass below re-derives; a
    /// template that only shows up in the return type (the common,
    /// high-value shape - a generic factory method's `@return T` with no
    /// `T`-typed parameter) is what the difference against the
    /// params+return pass catches. Deliberately narrower in one way: a
    /// template declared but referenced nowhere at all in its signature
    /// is invisible to this check too, since `FunctionReflection` has no
    /// separate record of a declared template's bare name independent of
    /// where it's used (see `current_function_template_params`).
    pub(crate) fn check_template_type_not_in_parameter(
        &mut self,
        sig: &FunctionReflection,
        bare_name: &str,
        class_name: Option<&str>,
        label: &str,
        span: Span,
    ) {
        let mut in_params = rustc_hash::FxHashMap::default();
        for p in sig.params() {
            collect_scoped_template_params(p.ty(), bare_name, &mut in_params);
        }
        // A non-negated conditional return type on a parameter
        // (`($params is array{wrapperClass: class-string<T>} ? T : X)`)
        // references its templates through that parameter.
        if let Some((types::ConditionalSubject::Parameter(_), _, false)) =
            sig.return_type().as_conditional()
        {
            collect_scoped_template_params(sig.return_type(), bare_name, &mut in_params);
        }
        let mut in_signature = in_params.clone();
        collect_scoped_template_params(sig.return_type(), bare_name, &mut in_signature);
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
            report!(self, format!("Template type {name} of {kind_word} {label}() is not referenced in a parameter."),
                span,
                0,
                "method.templateTypeNotInParameter",
            );
        }
    }

    /// The class a `ClassConstFetch`/`StaticPropertyFetch`/`StaticCall`
    /// target's name (`self`/`parent`/`static`, or a class name) names;
    /// `None` for `self`/`parent`/`static` outside any class.
    pub(crate) fn resolve_static_class_name(&self, class_name: &QualifiedName) -> Option<String> {
        self.resolve_name_expr(class_name)
            .as_object()
            .map(|(class, _)| class.to_string())
    }

    /// `StaticCall`'s own full `StaticTarget` resolution, for a
    /// `check_expr`-side call site (full diagnostics): `Name` resolves
    /// exactly like `resolve_static_class_name` always has; a
    /// genuinely dynamic `ExprNode` receiver (`$obj::method()`) is checked
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
            StaticTarget::Name(name) => self.resolve_static_class_name(name),
            StaticTarget::Expr(receiver) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("static target belongs to its retained file owner");
                let receiver = syntax_owner
                    .arenas()
                    .expr(*receiver)
                    .expect("static target receiver belongs to its retained file owner");
                receiver_class(self.check_expr(receiver, locals, label))
            }
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
            StaticTarget::Name(name) => self.resolve_static_class_name(name),
            StaticTarget::Expr(receiver) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("static target belongs to its retained file owner");
                let receiver = syntax_owner
                    .arenas()
                    .expr(*receiver)
                    .expect("static target receiver belongs to its retained file owner");
                receiver_class(self.infer_expr(receiver, locals))
            }
        }
    }

    /// Resolves `self`/`static`/`parent` to a concrete `Object` relative
    /// to `defining_class` - the class whose signature this type came
    /// from. `self`/`static` both collapse to the same class here, since
    /// this project doesn't track late static binding through actual
    /// runtime dispatch; `parent` resolves via the registry's `extends`
    /// chain, falling back to leaving it unresolved if the class has no
    /// declared parent (so it still hits the permissive `Object`
    /// fallback in `is_super_type_of`/`accepts` rather than being
    /// wrongly flagged).
    ///
    /// Recurses into `Union`/`Intersection`/`Array`/`ArrayShape` the same
    /// way `Type::substitute_templates` does, not just the bare
    /// top-level case - needed for the common nullable-self return type
    /// (`?self`/`self|null`, e.g. `tryFromArray`-style factory methods):
    /// `accepts`/`is_super_type_of` have no arm for a concrete class
    /// instance satisfying a still-unresolved `SelfRef` (only the
    /// reverse direction is handled), so a bare `self` return type
    /// wrapped in any union must be resolved here first.
    pub(crate) fn resolve_pseudo(&self, ty: &Type, defining_class: &str) -> Type {
        self.resolve_pseudo_at(ty, defining_class, defining_class)
    }

    /// `resolve_pseudo` for a member read through an instance of
    /// `called_class`: `self`/`parent` stay relative to `declaring_class`,
    /// the class that declares the member, while `static`/`$this` are
    /// the called class (late static binding).
    pub(crate) fn resolve_pseudo_at(
        &self,
        ty: &Type,
        declaring_class: &str,
        called_class: &str,
    ) -> Type {
        let bound = ty.bind_self(declaring_class).bind_static(called_class);
        if !ty.has_parent_ref() {
            return bound;
        }
        match self
            .registry
            .class_references_by_name(declaring_class)
            .and_then(|references| references.extends.first())
        {
            Some(parent) => bound.bind_parent(parent),
            None => bound,
        }
    }

    /// Whether a `class_exists()`-style guard in `locals` proved `name`
    /// declared.
    pub(crate) fn class_guarded(name: &str, locals: &Locals) -> bool {
        !locals.guarded_classes().is_empty()
            && locals
                .guarded_classes()
                .contains(&name.trim_start_matches('\\').to_ascii_lowercase())
    }

    /// Whether `name` is a class/interface/trait/enum the registry
    /// indexed.
    pub(crate) fn class_name_exists(&self, name: &str) -> bool {
        self.registry.lookup_class(name).is_some()
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
    /// source (needed fresh here, not `ClassReflection::extends`/etc., which
    /// `Registry::canonicalize_class_name_case` already rewrites to
    /// canonical case at registry-build time).
    /// A fix rewriting the name `raw`, written at `span`, to `canonical`'s
    /// casing - keeping `raw`'s own qualification (only its segments'
    /// letter case changes). Offered only when `span` is exactly `raw`'s
    /// width, so the edit can't touch anything else.
    pub(crate) fn name_case_fix(
        raw: &str,
        canonical: &str,
        span: Span,
    ) -> Option<(Span, String, String)> {
        let leading = raw.starts_with('\\');
        let written = raw.trim_start_matches('\\');
        let segments = written.split('\\').count();
        let canonical_segments: Vec<&str> =
            canonical.trim_start_matches('\\').split('\\').collect();
        let tail = canonical_segments
            .get(canonical_segments.len().checked_sub(segments)?..)?
            .join("\\");
        if !tail.eq_ignore_ascii_case(written) || tail == written {
            return None;
        }
        if span.len() as usize != raw.len() {
            return None;
        }
        let replacement = if leading { format!("\\{tail}") } else { tail };
        let description = format!("Use {replacement}");
        Some((span, replacement, description))
    }

    /// `class.nameCase` for a class name written in code.
    pub(crate) fn check_class_name_case(&mut self, name: &QualifiedName) {
        if let Some(resolved) = name.resolved_class() {
            self.check_resolved_class_name_case(&name.written, resolved, name.span);
        }
    }

    /// `class.nameCase` for a class name written as `raw` that resolves
    /// to `resolved`.
    fn check_resolved_class_name_case(&mut self, raw: &str, resolved: &str, span: Span) {
        let mut segments = raw.trim_start_matches('\\').splitn(2, '\\');
        let first_segment = segments.next().unwrap_or(raw);
        if self.name_ctx.is_use_imported(first_segment) {
            return;
        }
        let Some((canonical, info)) = self.registry.lookup_class(resolved) else {
            return;
        };
        if self.registry.built_in_stub_classes.contains_key(canonical) {
            return;
        }
        if canonical != resolved {
            let type_word = class_type_word(info.kind());
            let mut builder = DiagnosticBuilder::new(
                format!("{type_word} referenced with incorrect case: {resolved}."),
                span,
                0,
                format!("{}.nameCase", type_word.to_lowercase()),
            );
            if let Some((span, replacement, description)) =
                Self::name_case_fix(raw, canonical, span)
            {
                builder = builder.fix(span, replacement, description);
            }
            self.report_builder(builder);
        }
    }

    /// `class.nameCase` for a PHPDoc-typed position (a
    /// param/return/property type hint, `@method`/`@mixin`/
    /// `@phpstan-type` tag target). Pure fan-out over `case_refs`
    /// (`(raw, resolved)` pairs from `collect_class_name_case_refs`,
    /// populated once at registry-build time) - re-runs the *unchanged*
    /// `check_resolved_class_name_case` per pair rather than duplicating its
    /// `is_use_imported`/built-in-stub-skip/`lookup_class` logic. Every
    /// reference nested inside one param/return/property's type (e.g.
    /// both class names in `array<Foo, Bar>`) is reported at the *same*
    /// `span`, since this project's PHPDoc parsing keeps no
    /// per-reference span within a docblock's type text, only the
    /// position of the type token as a whole.
    pub(crate) fn check_type_class_name_case(&mut self, case_refs: &[(Name, Name)], span: Span) {
        for (raw, resolved) in case_refs {
            self.check_resolved_class_name_case(raw, resolved, span);
        }
    }

    /// Method-call/static-call sibling of `ExprKind::Call`'s own inline
    /// `function.nameCase` check, gated on the same
    /// `RulesSettings::check_function_name_case` flag as `use function`'s
    /// own check.
    /// `Registry::lookup_method_canonical_name` mirrors `lookup_class`'s
    /// case-insensitive-fallback shape.
    /// `name_region` is the part of the call after its receiver, where
    /// the written method name is.
    pub(crate) fn check_call_site_method_name_case(
        &mut self,
        class: &str,
        method: &str,
        span: Span,
        name_region: Span,
    ) {
        if !self.settings.rules.check_function_name_case {
            return;
        }
        let Some(canonical) = self.registry.lookup_method_canonical_name(class, method) else {
            return;
        };
        if canonical != method {
            let mut builder = DiagnosticBuilder::new(
                format!("Call to method {class}::{canonical}() with incorrect case: {method}."),
                span,
                0,
                "method.nameCase",
            );
            builder = builder.fix_within(
                name_region,
                method,
                canonical.to_string(),
                format!("Use {canonical}"),
            );
            self.report_builder(builder);
        }
    }

    /// Each distinct class `ty` refers to, at any depth
    /// (`Type::referenced_class_names`), that doesn't exist - a typo in
    /// `list<Foo\Bar>`, `Box<Foo>` or `callable(Foo): void` as much as
    /// a bare `Foo`. A template never reaches here as a class: the
    /// registry marks every declared template name first.
    pub(crate) fn collect_unknown_class_names(&self, ty: &Type, out: &mut Vec<String>) {
        for name in ty.referenced_class_names() {
            if !self.class_name_exists(name) && !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
    }

    /// `FunctionDefinitionCheck`'s own "resolved but a trait" half -
    /// `parameter.trait`/`return.trait`/`property.trait`: a native
    /// type-hint resolving to a real, indexed class that turns out to be
    /// a `trait` rather than a class/interface/enum. Real PHP itself
    /// rejects this at the engine level, so this is always a real bug,
    /// never a corpus-noise risk. Walks what `collect_unknown_class_names`
    /// walks; the two are mutually exclusive per name, so no name is
    /// reported under both identifiers.
    pub(crate) fn collect_trait_class_names(&self, ty: &Type, out: &mut Vec<String>) {
        for name in ty.referenced_class_names() {
            if self
                .registry
                .lookup_class(name)
                .is_some_and(|(_, info)| info.kind() == syntax::ast::ClassKind::Trait)
                && !out.iter().any(|n| n == name)
            {
                out.push(name.to_string());
            }
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
        subject: impl std::fmt::Display,
        span: Span,
        identifier: &'static str,
    ) {
        let mut traits = Vec::new();
        self.collect_trait_class_names(ty, &mut traits);
        for name in traits {
            report!(self, 
                format!("{subject} has invalid type {name}"),
                span,
                0,
                identifier,
            );
        }
    }

    /// The reporting half for `Type::has_parent_ref` -
    /// `identifier` is `parameter.noParent` or `return.noParent`
    /// depending on the call site, `subject` the same phrase the sibling
    /// `class.notFound`/`.trait` checks at that exact site already use.
    pub(crate) fn check_no_parent_ref(
        &mut self,
        ty: &Type,
        subject: impl std::fmt::Display,
        span: Span,
        identifier: &'static str,
    ) {
        if ty.has_parent_ref() {
            report!(self, 
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
        subject: impl std::fmt::Display,
        span: Span,
    ) {
        let mut unknown = Vec::new();
        self.collect_unknown_class_names(ty, &mut unknown);
        let mut seen = rustc_hash::FxHashSet::default();
        unknown.retain(|n| seen.insert(n.clone()));
        for name in unknown {
            report!(self, 
                format!("{subject} has invalid type {name}"),
                span,
                0,
                "class.notFound",
            );
        }
        self.check_type_prefixed_class_names(ty, span);
    }

    /// `class.prefixed` for every class `ty` names.
    pub(crate) fn check_type_prefixed_class_names(&mut self, ty: &Type, span: Span) {
        // `class.prefixed`: checked on the *successfully-resolved* name
        // too, unlike the `class.notFound` loop above (which only sees a
        // name `collect_unknown_class_names` couldn't find) - a leaked
        // Rector/PHP-Scoper/PHPUnit-PHAR/Box-prefixed vendor class is
        // almost always still a real, indexed class, so it would never
        // appear in `unknown` above.
        let mut prefixed = Vec::new();
        collect_prefixed_class_names(ty, &mut prefixed);
        let mut seen = rustc_hash::FxHashSet::default();
        prefixed.retain(|(n, _)| seen.insert(*n));
        for (name, project) in prefixed {
            report_builder!(self, 
                DiagnosticBuilder::new(
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
/// (a prefixed vendor class is almost always a real, indexed one).
fn collect_prefixed_class_names<'b>(ty: &'b Type, out: &mut Vec<(&'b str, &'static str)>) {
    for name in ty.referenced_class_names() {
        if let Some((project, _)) = crate::checker::INTERNAL_CLASS_PREFIXES
            .iter()
            .find(|(_, prefix)| name.starts_with(prefix))
        {
            out.push((name.as_str(), project));
        }
    }
}

/// Walks `ty` collecting every distinct template node
/// whose `.scope` matches `scope` exactly - `Checker::
/// current_function_template_params`'s recursive helper. Mirrors
/// `resolve_pseudo`'s/`collect_unknown_class_names`'s recursive shape,
/// but includes `Object`'s own generic argument list (a template can
/// appear nested inside another class's type argument, e.g.
/// `array<int, Box<T>>`).
fn collect_scoped_template_params(
    ty: &Type,
    scope: &str,
    out: &mut rustc_hash::FxHashMap<String, types::TemplateParam>,
) {
    for tp in ty.nested_templates() {
        if tp.scope.as_deref() == Some(scope) {
            out.entry(tp.name.clone()).or_insert_with(|| tp.clone());
        }
    }
}

/// The class a receiver's value is an instance of, when its type names
/// one.
pub(crate) fn receiver_class(ty: Type) -> Option<String> {
    ty.as_object().map(|(class, _)| class.to_string())
}
