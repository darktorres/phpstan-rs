use crate::checker::GotoState;
use crate::checker::{
    ATTRIBUTE_TARGET_METHOD, Checker, body_contains_explicit_throw, expr_contains_explicit_throw,
};
use crate::registry::{self, PropertyReflection, resolve_names_in_type};
use diagnostics::DiagnosticBuilder;
use phpdoc::docblock;
use syntax::ast::*;
use syntax::span::Span;

use types::{self, Type};

impl<'a> Checker<'a> {
    /// PHP 8.4 property hooks: `GetNonVirtualPropertyHookReadRule`/
    /// `SetNonVirtualPropertyHookAssignRule`, `SetPropertyHookParameterRule`'s
    /// native-type-contravariance half (see below), and
    /// `ExistingClassesInPropertyHookTypehintsRule`'s existence check for
    /// the `set` hook's parameter type, combined into one per-property
    /// pass. Each hook's body is checked as its own function-like scope
    /// (mirroring `checker/expr.rs`'s `Closure`/`ArrowFn` handling), not
    /// through `check_function`'s registry lookup - a hook is never
    /// itself callable, so it doesn't need that path's method-specific
    /// machinery (`#[Override]`/`#[NoDiscard]`, first-class-callable
    /// eligibility).
    ///
    /// Also covers a promoted constructor property's inline hooks
    /// (`Param::hooks`), reusing `check_get_hook`/`check_set_hook` via
    /// scalar `name`/`span`/`type_hint` arguments.
    pub(crate) fn check_property_hooks(&mut self, fqn: &str, c: &ClassDef) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(hooks) = &p.hooks else {
                continue;
            };
            let Some(prop_sig) = info.properties().get(p.name.as_ref()).cloned() else {
                continue;
            };
            if let Some(hook) = &hooks.get {
                self.check_hook_phpdoc_types(
                    fqn,
                    p.type_hint.as_ref(),
                    None,
                    hook.doc.as_deref(),
                    p.span,
                );
                self.check_get_hook(fqn, &p.name, p.span, hook, &prop_sig);
            }
            if let Some(hook) = &hooks.set {
                self.check_set_hook(fqn, &p.name, p.span, p.type_hint.as_ref(), hook, &prop_sig);
            }
        }
        if let Some(ctor) = c.constructor() {
            for param in &ctor.function.params {
                if param.promotion.is_none() {
                    continue;
                }
                let Some(hooks) = param.hooks.as_deref() else {
                    continue;
                };
                let Some(prop_sig) = info.properties().get(param.name.as_ref()).cloned() else {
                    continue;
                };
                if let Some(hook) = &hooks.get {
                    self.check_hook_phpdoc_types(
                        fqn,
                        param.type_hint.as_ref(),
                        None,
                        hook.doc.as_deref(),
                        param.span,
                    );
                    self.check_get_hook(fqn, &param.name, param.span, hook, &prop_sig);
                }
                if let Some(hook) = &hooks.set {
                    self.check_set_hook(
                        fqn,
                        &param.name,
                        param.span,
                        param.type_hint.as_ref(),
                        hook,
                        &prop_sig,
                    );
                }
            }
        }
    }

    /// `PropertyHookAttributesRule` - a hook's own `#[Attr]` groups are
    /// checked like a method's (`Attribute::TARGET_METHOD`), plus
    /// `#[\NoDiscard]`, which real PHPStan bans on any hook (never itself
    /// directly callable).
    fn check_hook_attributes(&mut self, attrs: &[Attribute], label: &str) {
        self.check_attributes(attrs, label);
        self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_METHOD, "method");
        if let Some(nodiscard) = attrs
            .iter()
            .find(|a| registry::is_no_discard_attribute_name(&a.name))
        {
            let fqn = self.resolve_class_name(&nodiscard.name);
            self.report_builder(
                DiagnosticBuilder::new(
                    format!("Attribute class {fqn} cannot be used on property hooks."),
                    nodiscard.span,
                    0,
                    "attribute.target",
                )
                .non_ignorable(),
            );
        }
    }

    /// Resolves a hook's own `@throws` tag into a real `Type`, computed
    /// at check time since a hook has no `FunctionReflection` of its own to
    /// cache it on.
    fn resolve_hook_throws(&self, doc: Option<&str>) -> Option<Type> {
        let block = docblock::parse_shared(doc?);
        block.throws.iter().fold(None::<Type>, |acc, node| {
            let ty = crate::registry::parse_type_node(node);
            let ty = resolve_names_in_type(ty, &self.name_ctx);
            Some(match acc {
                Some(prev) => self.relation().union(prev, ty),
                None => ty,
            })
        })
    }

    /// Mirrors `TooWideMethodThrowTypeRule`'s property-hook sibling,
    /// `TooWidePropertyHookThrowTypeRule` - skip unless the property or
    /// hook is private/final, the declaring class is final, or the
    /// shared `check_too_wide_throw_type_in_protected_and_public_methods`
    /// opt-in flag is on.
    fn should_check_too_wide_property_hook_throw_type(
        &self,
        class: &str,
        property: &str,
        visibility: Visibility,
        hook_final: bool,
    ) -> bool {
        if visibility == Visibility::Private || hook_final {
            return true;
        }
        let Some(info) = self.class_info(class) else {
            return true;
        };
        if info.is_final() || info.final_properties().contains(property) {
            return true;
        }
        self.settings
            .rules
            .check_too_wide_throw_type_in_protected_and_public_methods
    }

    /// Scalar `name`/`span`/`type_hint` (rather than a whole
    /// `&PropertyDecl`) let this and `check_set_hook` handle both a plain
    /// property and a promoted constructor parameter uniformly.
    fn check_get_hook(
        &mut self,
        fqn: &str,
        name: &str,
        span: Span,
        hook: &PropertyHook,
        prop_sig: &PropertyReflection,
    ) {
        let (body, attrs, doc, hook_final) =
            (&hook.body, &hook.attrs, hook.doc.as_deref(), hook.is_final);
        let label = format!("{fqn}::${name} (get hook)");
        self.check_hook_attributes(attrs, &label);
        // `InvalidThrowsPhpDocValueRule`'s @throws-validity check runs
        // even for a bodyless hook; the too-wide/missing checks below
        // need a real body to compare against.
        let throws = self.resolve_hook_throws(doc);
        if let Some(throws) = &throws {
            self.check_throws_tag_validity(throws, span);
            // `ThrowsVoidPropertyHookWithExplicitThrowPointRule`, same
            // simplified wording as `check_function_with_ancestor_hint`.
            if throws.is_void() {
                let throw_span = match body {
                    PropertyHookBody::Arrow(e) => expr_contains_explicit_throw(e),
                    PropertyHookBody::Block(stmts) => body_contains_explicit_throw(stmts),
                    PropertyHookBody::Abstract => None,
                };
                if let Some(throw_span) = throw_span {
                    self.report(format!(
                            "Get hook for property {fqn}::${name} throws exception but the PHPDoc contains @throws void."
                        ),
                        throw_span,
                        3,
                        "throws.void",
                    );
                }
            }
        }
        // An abstract hook has no body - read/write checks don't apply;
        // see `check_property_hook_override` for the ones that do.
        if matches!(body, PropertyHookBody::Abstract) {
            return;
        }
        // `GetNonVirtualPropertyHookReadRule`: a non-virtual get hook
        // that never reads its own backing storage almost certainly
        // forgot to.
        if !prop_sig.is_virtual()
            && !registry::hook_body_references_property(body, name)
            && !registry::hook_body_calls_parent_hook(body, name)
        {
            self.report(
                format!(
                    "Get hook for non-virtual property {fqn}::${name} does not read its value."
                ),
                span,
                3,
                "propertyGetHook.noRead",
            );
        }

        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        let mut locals = self.new_locals();
        locals.insert_var("this", Type::object(fqn.into(), Vec::new()));
        let declared_return = Some(prop_sig.ty().clone());
        let previous_generator_flag = self.current_function_is_generator;
        let previous_declared_return_type = std::mem::replace(
            &mut self.current_declared_return_type,
            declared_return.clone(),
        );
        // Same save/restore as any function-shaped body, so a `throw`
        // here doesn't leak into whatever's checked next.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_try_throw_states = std::mem::take(&mut self.try_throw_states);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
        let previous_assigned_variables = match body {
            PropertyHookBody::Block(stmts) => self.enter_assigned_variables(stmts),
            _ => self.enter_assigned_variables(&[]),
        };
        let previous_goto_targets = match body {
            PropertyHookBody::Arrow(_) => {
                self.current_function_is_generator = hook.is_generator;
                None
            }
            PropertyHookBody::Block(_) => {
                self.current_function_is_generator = hook.is_generator;
                Some(std::mem::replace(
                    &mut self.current_function_goto_targets,
                    GotoState::new(hook.goto_labels.clone()),
                ))
            }
            // No real body at all - nothing to scan for yield/goto.
            PropertyHookBody::Abstract => {
                self.current_function_is_generator = false;
                None
            }
        };
        self.check_generator_return_type(declared_return.as_ref(), span);
        match body {
            PropertyHookBody::Arrow(e) => {
                let body_ty = self.check_expr(e, &mut locals, &label);
                if !body_ty.is_mixed() && !self.accepts(prop_sig.ty(), &body_ty).yes() {
                    self.report(
                        format!(
                            "return type mismatch: expected {}, got {body_ty}",
                            prop_sig.ty()
                        ),
                        e.span,
                        3,
                        "return.type",
                    );
                }
            }
            PropertyHookBody::Block(stmts) => {
                let body_terminates = self
                    .check_block(stmts, &mut locals, declared_return.as_ref(), &label)
                    .completion
                    .terminates;
                self.compare_body_completion(stmts, &locals, span, body_terminates);
                // Same fall-off-the-end completeness check `check_function`
                // performs - real PHPStan runs `MissingReturnRule` against
                // a hook the same as any other function-like node.
                if !body_terminates
                    && !self.current_function_is_generator
                    && let Some(level) = crate::checker::function_decl::missing_return_level(
                        prop_sig.ty(),
                        prop_sig.has_native_type(),
                        false,
                    )
                {
                    let mut builder = DiagnosticBuilder::new(
                        format!(
                            "should return {} but return statement is missing",
                            prop_sig.ty()
                        ),
                        span,
                        level,
                        "return.missing",
                    );
                    if prop_sig.has_native_type() {
                        builder = builder.non_ignorable();
                    }
                    self.report_builder(builder);
                }
            }
            // No real statement/expression to type-check at all.
            PropertyHookBody::Abstract => {}
        }
        // `TooWideMethodThrowTypeRule`'s property-hook sibling plus the
        // "missing checked exception" check - both need the full
        // throw-point set, so they run here before state resets.
        let subject = format!("Get hook for property {fqn}::${name}");
        if let Some(throws) = &throws
            && self.should_check_too_wide_property_hook_throw_type(
                fqn,
                name,
                prop_sig.visibility(),
                hook_final,
            )
        {
            self.check_too_wide_throw_type(throws, &subject, span);
        }
        self.check_missing_checked_exceptions(throws.as_ref(), &subject);
        self.current_function_is_generator = previous_generator_flag;
        self.current_declared_return_type = previous_declared_return_type;
        self.current_function_throw_points = previous_throw_points;
        self.try_throw_states = previous_try_throw_states;
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_function_assigned_variables = previous_assigned_variables;
        self.current_class = previous_class;
    }

    fn check_set_hook(
        &mut self,
        fqn: &str,
        name: &str,
        span: Span,
        type_hint: Option<&TypeHint>,
        hook: &PropertyHook,
        prop_sig: &PropertyReflection,
    ) {
        let (param, body, attrs, doc, hook_final) = (
            hook.param.as_ref(),
            &hook.body,
            &hook.attrs,
            hook.doc.as_deref(),
            hook.is_final,
        );
        let label = format!("{fqn}::${name} (set hook)");
        self.check_hook_attributes(attrs, &label);
        self.check_hook_phpdoc_types(fqn, type_hint, Some(param), doc, span);
        // See `check_get_hook` - runs regardless of body.
        let throws = self.resolve_hook_throws(doc);
        if let Some(throws) = &throws {
            self.check_throws_tag_validity(throws, span);
            if throws.is_void() {
                let throw_span = match body {
                    PropertyHookBody::Arrow(e) => expr_contains_explicit_throw(e),
                    PropertyHookBody::Block(stmts) => body_contains_explicit_throw(stmts),
                    PropertyHookBody::Abstract => None,
                };
                if let Some(throw_span) = throw_span {
                    self.report(format!(
                            "Set hook for property {fqn}::${name} throws exception but the PHPDoc contains @throws void."
                        ),
                        throw_span,
                        3,
                        "throws.void",
                    );
                }
            }
        }
        // An abstract hook has no body - only the assign check is
        // skipped; native-type/class-existence checks on the parameter
        // still apply.
        if !matches!(body, PropertyHookBody::Abstract)
            && !prop_sig.is_virtual()
            && !registry::hook_body_references_property(body, name)
            && !registry::hook_body_calls_parent_hook(body, name)
        {
            self.report(
                format!(
                    "Set hook for non-virtual property {fqn}::${name} does not assign value to it."
                ),
                span,
                3,
                "propertySetHook.noAssign",
            );
        }

        // `SetPropertyHookParameterRule`'s native-type-contravariance
        // half only; its docblock-merged sibling is skipped, matching
        // real PHPStan's own default-off override-compatibility flag
        // (see `FunctionReflection::return_type_native`). A hook without a
        // parameter list has an implicit `$value` of the property's type.
        let param_has_native = param.is_some_and(|p| p.type_hint.is_some());
        let prop_has_native = type_hint.is_some();
        match (prop_has_native, param_has_native) {
            _ if param.is_none() => {}
            (false, true) => {
                self.report_builder(
                    DiagnosticBuilder::new(format!(
                            "Parameter ${} of set hook has a native type but the property {fqn}::${name} does not.",
                            param.map(|p| p.name.as_ref()).unwrap_or("value"),
                        ),
                        span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, false) => {
                self.report_builder(
                    DiagnosticBuilder::new(format!(
                            "Parameter ${} of set hook does not have a native type but the property {fqn}::${name} does.",
                            param.map(|p| p.name.as_ref()).unwrap_or("value"),
                        ),
                        span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, true) => {
                let param_ty = self.resolve_type_hint(param.and_then(|p| p.type_hint.as_ref()));
                let prop_native_ty = self.resolve_type_hint(type_hint);
                if !self.accepts(&param_ty, &prop_native_ty).yes() {
                    self.report_builder(
                        DiagnosticBuilder::new(format!(
                                "Native type {param_ty} of set hook parameter ${} is not contravariant with native type {prop_native_ty} of property {fqn}::${name}.",
                                param.map(|p| p.name.as_ref()).unwrap_or("value"),
                            ),
                            span,
                            0,
                            "propertySetHook.nativeParameterType",
                        )
                        .non_ignorable(),
                    );
                }
            }
            (false, false) => {}
        }

        // `ExistingClassesInPropertyHookTypehintsRule`'s existence half;
        // `prop_sig.set_ty` is already resolved.
        if let Some(set_ty) = prop_sig.set_ty() {
            self.check_type_class_names_exist(
                set_ty,
                format!(
                    "Parameter ${} of set hook for property {fqn}::${name}",
                    param.map(|p| p.name.as_ref()).unwrap_or("value"),
                ),
                span,
            );
            // Real PHPStan also checks `parameter.trait` here, not
            // `property.trait`.
            self.check_type_trait_refs(
                set_ty,
                format!(
                    "Parameter ${} of set hook for property {fqn}::${name}",
                    param.map(|p| p.name.as_ref()).unwrap_or("value"),
                ),
                span,
                "parameter.trait",
            );
            // `class.nameCase` for the set hook's parameter type.
            self.check_type_class_name_case(prop_sig.set_case_refs(), span);
        }

        // `ExistingClassesInPropertyHookTypehintsRule`'s native-union-
        // pre-8.0 half, reusing `check_function_impl`'s own raw-text
        // core - a hook has no `FunctionDecl` to hand it. Only the `set`
        // hook's own parameter can ever carry a native union type; a
        // hook has no return-type position at all (`nikic/php-parser`'s
        // own `PropertyHook::getReturnType()` is hardcoded `null` -
        // confirmed real PHPStan's own equivalent call passes an
        // impossible-to-trigger return-type message for this exact
        // reason), so this only ever checks the parameter half.
        if let Some(p) = param {
            self.check_native_union_types_supported_raw(
                std::iter::once(p.type_hint.as_ref()),
                None,
                format!(
                    "Set hook for property {fqn}::${name} uses native union types but they're supported only on PHP 8.0 and later."
                ),
                p.name.span,
            );
            // `ExistingClassesInPropertyHookTypehintsRule`'s
            // `parameter.unresolvableNativeType` half - real PHP hooks
            // have no return-type position (see above), so only the
            // parameter half is reachable here.
            self.check_unresolvable_native_type(
                p.type_hint.as_ref(),
                format!(
                    "Parameter ${} of set hook for property {fqn}::${name} has unresolvable native type.",
                    p.name,
                ),
                "parameter.unresolvableNativeType",
                p.name.span,
            );
        }

        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        let mut locals = self.new_locals();
        locals.insert_var("this", Type::object(fqn.into(), Vec::new()));
        let param_name = param
            .map(|p| p.name.name.clone())
            .unwrap_or_else(|| "value".into());
        let param_span = param.map(|p| p.name.span).unwrap_or(span);
        let value_ty = prop_sig
            .set_ty()
            .clone()
            .unwrap_or_else(|| prop_sig.ty().clone());
        locals.declare_var(&param_name, value_ty, param_span);
        let declared_return = Some(Type::void());
        let previous_generator_flag = self.current_function_is_generator;
        let previous_declared_return_type = std::mem::replace(
            &mut self.current_declared_return_type,
            declared_return.clone(),
        );
        // See `check_get_hook`.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_try_throw_states = std::mem::take(&mut self.try_throw_states);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
        let previous_assigned_variables = match body {
            PropertyHookBody::Block(stmts) => self.enter_assigned_variables(stmts),
            _ => self.enter_assigned_variables(&[]),
        };
        let previous_goto_targets = match body {
            PropertyHookBody::Arrow(_) => {
                self.current_function_is_generator = hook.is_generator;
                None
            }
            PropertyHookBody::Block(_) => {
                self.current_function_is_generator = hook.is_generator;
                Some(std::mem::replace(
                    &mut self.current_function_goto_targets,
                    GotoState::new(hook.goto_labels.clone()),
                ))
            }
            PropertyHookBody::Abstract => {
                self.current_function_is_generator = false;
                None
            }
        };
        self.check_generator_return_type(declared_return.as_ref(), span);
        match body {
            PropertyHookBody::Arrow(e) => {
                self.check_expr(e, &mut locals, &label);
            }
            PropertyHookBody::Block(stmts) => {
                self.check_block(stmts, &mut locals, declared_return.as_ref(), &label);
            }
            PropertyHookBody::Abstract => {}
        }
        // See `check_get_hook` - `check_set_hook` never returns early for
        // an abstract body, so this guard is explicit instead.
        let subject = format!("Set hook for property {fqn}::${name}");
        if let Some(throws) = &throws
            && !matches!(body, PropertyHookBody::Abstract)
            && self.should_check_too_wide_property_hook_throw_type(
                fqn,
                name,
                prop_sig.visibility(),
                hook_final,
            )
        {
            self.check_too_wide_throw_type(throws, &subject, span);
        }
        if !matches!(body, PropertyHookBody::Abstract) {
            self.check_missing_checked_exceptions(throws.as_ref(), &subject);
        }
        self.current_function_is_generator = previous_generator_flag;
        self.current_declared_return_type = previous_declared_return_type;
        self.current_function_throw_points = previous_throw_points;
        self.try_throw_states = previous_try_throw_states;
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_function_assigned_variables = previous_assigned_variables;
        self.current_class = previous_class;
    }
}
