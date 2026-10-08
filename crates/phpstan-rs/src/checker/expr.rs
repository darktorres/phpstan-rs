use crate::checker::ExitKind;
use crate::checker::GotoState;
use crate::checker::InferQuestion;
use crate::checker::call_effects::is_this;
use crate::checker::calls::parameters::Callee;
use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::resolve::receiver_class;
use crate::checker::suggest;
use crate::checker::{
    ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION, ATTRIBUTE_TARGET_PROPERTY,
    CalleeLabel, CheckArgsCall, Checker, Locals, LoopZeroIterations, MemberKind, MemberLabel,
    MemberVisibilityCheck, TemplateBindings, UsageTokenKind, binop_sigil, generator_template_param,
    literal_string_key, merge_scopes, propagate_container_field_facts,
    purge_stale_narrowing_for_var, resolve_call_args, unassigned_variable_type,
};
use crate::pest_config;
use crate::registry::FunctionReflection;
use crate::registry::summaries::{Access, Operation};
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::span::Span;
use types::Name;
use types::UnionAll as _;
use types::{self, Type};

mod array_ops;
mod builtin_calls;
mod member_resolution;
mod mysqli;
mod preg_shapes;
mod probe_checks;
pub(crate) mod ranges;

use crate::checker::operators::{arithmetic_op, ordering_op};
pub(crate) use ranges::{NullProbeVerdictFn, empty_probe_verdict, null_probe_verdict};

// `checker::throws::dynamic` reuses this bitwise-or-flag-chain scan for
// `JSON_THROW_ON_ERROR`/`FILTER_THROW_ON_FAILURE`, the same helper
// `builtin_calls.rs`'s `json_encode`/`filter_var` return-type narrowing
// uses.
pub(crate) use builtin_calls::contains_flag_name;
pub(crate) use preg_shapes::{PregCall, preg_call, preg_result_outcome, preg_result_type};

/// A bare `ExprKind::Name` with no registered global constant (an
/// unresolved name stays `Mixed`, e.g. `PHP_EOL`/`M_PI`) - except
/// `true`/`false`/`null`, real PHP language keywords rather than
/// ordinary constants. A fully-qualified reference (`\false`/`\true`/
/// `\null`) parses as a plain `Name`, not the `Keyword::False`/`True`/
/// `Null` token `parse_primary` matches directly for the unqualified
/// form, even though real PHP treats both forms identically (PHP doesn't
/// allow declaring a constant with one of these three names). Rector's
/// namespace-prefixing transform rewrites every unqualified global name
/// to its fully-qualified form, including these three, so without this,
/// such a name stayed `Mixed` and silently skipped narrowing through
/// `apply_equality_narrowing`'s `target_ty.is_mixed()` bail-out.
fn bare_name_literal_type(name: &str) -> Type {
    match name.trim_start_matches('\\').to_ascii_lowercase().as_str() {
        "true" => Type::bool_literal(true),
        "false" => Type::bool_literal(false),
        "null" => Type::null(),
        _ => Type::mixed(),
    }
}

/// Whether no value of `ty` is `null`, for a reachable `ty` (`never`, the
/// type of unreachable code, isn't judged).
pub(crate) fn type_definitely_excludes_null(ty: &Type) -> bool {
    !ty.is_never() && ty.is_null().no()
}

/// PHPStan's `TemplateTypeHelper::generalizeInferredTemplateType`: a
/// value inferred for a class template that isn't covariant widens past
/// its literal - `new Box(5)` is `Box<int>`, so it can later hold a `6` -
/// unless the template's bound is itself scalar (`T of int` keeps `5`),
/// an `array-key` bound widening any scalar.
fn generalize_inferred_class_template(
    info: &crate::registry::ClassReflection,
    bounds: &syntax::name::NameMap<Type>,
    template: &Name,
    ty: &Type,
) -> Type {
    if info.covariant_templates().contains(template) {
        return ty.clone();
    }
    let bound = bounds.get(template).cloned().unwrap_or_else(Type::mixed);
    let array_key_bound =
        bound == Type::unknown_array_key() || bound == Type::benevolent_array_key();
    let widens = if ty.is_scalar().yes() && array_key_bound {
        true
    } else {
        ty.is_constant_value().yes() && (!bound.is_scalar().yes() || array_key_bound)
    };
    if widens {
        ty.generalize_template_argument()
    } else {
        ty.clone()
    }
}

/// The function or method a first-class callable names.
pub(crate) struct FirstClassTarget<'a> {
    pub(crate) sig: &'a FunctionReflection,
    /// The class a method was looked up on; `None` for a function.
    pub(crate) class: Option<String>,
    /// The function's (unqualified) or method's name, the scope its own
    /// templates carry.
    pub(crate) name: String,
}

impl<'a> Checker<'a> {
    /// An `isset`/`empty` operand. PHP raises no notice for a missing
    /// variable, offset or property inside one, so the operand's root is
    /// read quietly (as `??`'s left side is); the probe is flagged when it
    /// can't fail or can't pass. A bare variable probed here is declared
    /// from this point on, so the branch the probe guards can read it.
    fn check_existence_probe(
        &mut self,
        operand: &ExprNode,
        locals: &mut Locals,
        label: &str,
        (identifier_prefix, operator_description, verdict_fn): (&str, &str, NullProbeVerdictFn),
    ) {
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("existence-probe operand retains its parsed file owner");
        let arenas = syntax_owner.arenas();
        self.check_expr_quiet_root(arenas, operand, locals, label);
        self.check_pointless_null_probe(
            operand,
            locals,
            identifier_prefix,
            operator_description,
            verdict_fn,
        );
        self.check_pointless_offset_or_property_probe(
            arenas,
            operand,
            locals,
            identifier_prefix,
            operator_description,
            verdict_fn,
        );
    }

    /// What a backtick command returns: `shell_exec()`'s return type.
    fn shell_exec_type(&self) -> Type {
        self.registry
            .lookup_function("shell_exec")
            .map_or(Type::mixed(), |f| f.return_type().clone())
    }

    /// Checks anonymous class `name`'s definition `def`, written at
    /// `span`: its members, under its registry name.
    fn check_anonymous_class(&mut self, name: &str, def: &ClassDef, span: Span) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        self.check_inheritance_target_kinds(name, true, span);
        self.check_magic_serialization_methods(name, "anonymous class", span);
        self.check_property_declarations(
            name,
            name,
            ClassKind::Class,
            false,
            def.properties(arenas),
            arenas.methods(def.methods.clone()),
        );
        self.check_readonly_class_version(def.is_readonly, true, span);
        for m in arenas.methods(def.methods.clone()) {
            self.check_function(&m.function, Some(name), m.is_static, true);
        }
        self.record_property_type_site(arenas, name, vec![Name::from(name)], def);
        for p in def.properties(arenas) {
            let attrs = arenas.attributes(p.attrs.clone());
            self.check_attributes(attrs, name);
            self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_PROPERTY, "property");
        }
        for cc in def.consts(arenas) {
            let attrs = arenas.attributes(cc.attrs.clone());
            self.check_attributes(attrs, name);
            self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_CLASS_CONSTANT, "class constant");
        }
    }

    /// What calling `method` on a value of `receiver`'s type may write: over
    /// every class the value may be an object of, by the hierarchy, or over
    /// every loaded declaration of the method when the class is not known.
    pub(crate) fn method_call_effect(
        &self,
        receiver: &Type,
        method: &str,
    ) -> crate::registry::Effect {
        let mut effect = crate::registry::Effect::nothing();
        for member in receiver.union_members() {
            let member_effect = match member.as_object() {
                Some((class, _)) => self
                    .registry
                    .virtual_method_effect(class, method)
                    .on_object(Some(&Name::from(class))),
                None if member.is_object().no() => continue,
                None => self.registry.method_effect_by_name(method).on_object(None),
            };
            effect.join(&member_effect);
            if effect.is_anything() {
                break;
            }
        }
        effect
    }

    /// Thin wrapper over `check_expr_impl` - the real work - so every
    /// external `rule::Rule` registered via `Checker::set_external_rules`
    /// still fires at every recursion depth without each of
    /// `check_expr_impl`'s own many internal `self.check_expr(...)`
    /// recursive calls needing to know about rule dispatch at all (see
    /// `rule`'s own module doc comment).
    pub(crate) fn check_expr(&mut self, e: &ExprNode, locals: &mut Locals, label: &str) -> Type {
        let outermost = self.open_infer_scope();
        let ty = self.check_expr_cached(e, locals, label);
        if outermost {
            self.close_infer_scope();
        }
        ty
    }

    fn check_expr_cached(&mut self, e: &ExprNode, locals: &mut Locals, label: &str) -> Type {
        syntax::stack::guard(|| {
            let native = self
                .records_expr_types()
                .then(|| self.native_expr_type(e, locals));
            let ty = crate::checker::profiling::measure_outermost!("checker::check_expr", {
                self.check_expr_impl(e, locals, label)
            });
            if let Some(native) = native {
                self.record_expr_type(self.expr_span(e.id), ty.clone(), native);
            }
            self.forget_what_a_call_may_change(e, locals);
            if !self.try_throw_states.is_empty()
                && let Some(thrown) = self.thrown_by(e, locals)
            {
                self.record_throw_state(thrown, locals);
            }
            if !self.external_rules.is_empty() {
                self.dispatch_expr_rules(e, locals, &ty);
            }
            ty
        })
    }

    /// Records `locals` as a scope the innermost enclosing `try` body can
    /// throw `thrown` from.
    pub(crate) fn record_throw_state(&mut self, thrown: Type, locals: &Locals) {
        if let Some(frame) = self.try_throw_states.last_mut() {
            frame.push((thrown, locals.clone()));
        }
    }

    /// What evaluating `e` itself, after its operands, can throw, or
    /// `None` when it cannot: a call what its signature says
    /// (`call_throw_types`), anything when unresolved; `throw` its value;
    /// division and modulo `DivisionByZeroError`; shifts
    /// `ArithmeticError`; a `clone`, a `yield` (the generator may be
    /// thrown into) or a pipe's call anything. A `match` records its own
    /// throw point, before its arms.
    fn thrown_by(&mut self, e: &ExprNode, locals: &Locals) -> Option<Type> {
        let anything = || Type::object("Throwable".into(), Vec::new());
        // An offset of an `ArrayAccess` object calls one of its methods
        // (`array_access`).
        let implicit = if matches!(
            e.kind,
            ExprKind::Call(..)
                | ExprKind::New(..)
                | ExprKind::NewAnonymousClass { .. }
                | ExprKind::MethodCall(..)
                | ExprKind::DynamicMethodCall(..)
                | ExprKind::StaticCall(..)
                | ExprKind::DynamicStaticCall(..)
        ) {
            None
        } else {
            self.call_throw_types
                .remove(&self.expr_span(e.id))
                .flatten()
        };
        let own = self.thrown_by_itself(e, locals, anything);
        match (implicit, own) {
            (Some(a), Some(b)) => Some(self.relation().union(a, b)),
            (a, b) => a.or(b),
        }
    }

    /// `thrown_by` for what `e` does itself, beside an implicit call.
    fn thrown_by_itself(
        &mut self,
        e: &ExprNode,
        locals: &Locals,
        anything: impl Fn() -> Type,
    ) -> Option<Type> {
        match &e.kind {
            ExprKind::Call(..)
            | ExprKind::New(..)
            | ExprKind::NewAnonymousClass { .. }
            | ExprKind::MethodCall(..)
            | ExprKind::DynamicMethodCall(..)
            | ExprKind::StaticCall(..)
            | ExprKind::DynamicStaticCall(..) => self
                .call_throw_types
                .remove(&self.expr_span(e.id))
                .unwrap_or_else(|| Some(anything())),
            ExprKind::Throw(inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("throw expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("thrown expression belongs to its retained file owner");
                Some(self.infer_expr(inner, locals))
            }
            ExprKind::Clone(..) | ExprKind::Yield(..) | ExprKind::YieldFrom(_) => Some(anything()),
            ExprKind::Binary(op, _, _) | ExprKind::CompoundAssign(op, _, _) => match op {
                BinOp::Div | BinOp::Mod => {
                    Some(Type::object("DivisionByZeroError".into(), Vec::new()))
                }
                BinOp::Shl | BinOp::Shr => Some(Type::object("ArithmeticError".into(), Vec::new())),
                BinOp::Pipe => Some(anything()),
                _ => None,
            },
            _ => None,
        }
    }

    fn dispatch_expr_rules(&mut self, e: &ExprNode, locals: &Locals, ty: &Type) {
        let rules = self.external_rules;
        let mut found = Vec::new();
        let arenas = self
            .syntax_arenas()
            .expect("external expression rules retain their parsed file owner");
        for rule in rules {
            if rule.wants_expr(arenas, &e.kind) {
                let scope = crate::rule::Scope::new(self, locals);
                found.extend(rule.check_expr(&scope, arenas, e, ty));
            }
        }
        self.diagnostics.extend(found);
    }

    fn check_expr_impl(&mut self, e: &ExprNode, locals: &mut Locals, label: &str) -> Type {
        // See `Checker::in_bare_statement_position`'s doc comment - taken,
        // not just read, so it reads `true` only for the exact top-level
        // expression `StmtKind::Expression` set it for, never for
        // anything this match's arms go on to recursively check via a
        // nested `check_expr` call.
        let is_bare_statement = std::mem::take(&mut self.in_bare_statement_position);
        // See `Checker::in_condition_position`'s doc comment - same
        // "take, not just read" reasoning as `is_bare_statement` above.
        let is_condition_position = std::mem::take(&mut self.in_condition_position);
        let is_exempt_last_condition = std::mem::take(&mut self.in_exempt_last_condition);
        if self.settings.shipmonk.check_null_coercion
            || self.settings.shipmonk.check_enum_exhaustiveness
            || self.settings.extras.check_identity_vs_equals
        {
            self.check_strict_expression_idioms(e, locals);
        }
        if self.settings.sidz.check_magic_numbers {
            self.check_magic_numbers_expression(e);
        }
        if self.settings.slam.check_current_time_dependence {
            self.check_current_time_expression(e, locals);
        }
        if (self.settings.psr3.check_psr3_logging
            || self.settings.psr3.check_psr3_exception_context)
            && matches!(e.kind, ExprKind::MethodCall(..) | ExprKind::StaticCall(..))
        {
            self.check_psr3_call(e, locals);
        }
        if self.settings.strict_rules.check_illegal_constructor_calls
            || self.settings.strict_rules.check_dynamic_static_calls
            || self.settings.strict_rules.check_arithmetic_operands
            || self.settings.shipmonk.check_comparable_types
            || self.settings.voku.check_mixed_type_operands
            || self.settings.strict_rules.check_implicit_array_creation
        {
            self.check_strict_rules_expression(e, locals);
        }
        if !is_bare_statement && let ExprKind::Match { arms, .. } = &e.kind {
            let syntax_owner = self
                .syntax_tree_owner()
                .expect("match expressions retain their parsed file owner");
            let arenas = syntax_owner.arenas();
            self.check_void_match_usage(
                arenas,
                arenas.match_arms(arms.clone()),
                locals,
                self.expr_span(e.id),
            );
        }
        match &e.kind {
            ExprKind::Call(callee, args) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("calls retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let callee = arenas
                    .expr(*callee)
                    .expect("call callee belongs to its retained file owner");
                let args = arenas.args(args.clone());
                let callee_name = match &callee.kind {
                    ExprKind::Name(range) => arenas.qualified_name(range),
                    _ => None,
                };
                // `PHPStan\Testing\assertType('expected type string', $expr)`
                // is PHPStan's own testing-DSL function, used to pin down
                // an exact expected inferred type at a point of interest.
                // Recognizing it here - comparing `infer_expr`'s result
                // (rendered via `Type`'s PHPStan-style `Display` impl)
                // against the literal expected string - lets fixture
                // files serve as directly-runnable ground truth, with no
                // separate test harness needed. PHPStan treats this the
                // same way (a no-op stub function its own internal
                // tooling specially recognizes), so it's additive, not a
                // diagnostic anyone would trip on outside a fixture file.
                if let Some(name) = callee_name
                    && (name == "assertType" || name.ends_with("\\assertType"))
                    && let [expected_arg, actual_arg] = args
                    && let Some(expected_expr) = arenas.expr(expected_arg.value)
                    && let ExprKind::Literal(Literal::String(expected)) = &expected_expr.kind
                {
                    let actual_expr = arenas
                        .expr(actual_arg.value)
                        .expect("assertType argument belongs to its retained file owner");
                    let actual = self.infer_expr(actual_expr, locals).to_string();
                    if let Some(expected) = arenas.string_bytes(expected) {
                        let expected = String::from_utf8_lossy(expected);
                        if actual != expected {
                            self.report(format!(
                                "assertType() mismatch: expected type \"{expected}\", actual type \"{actual}\""
                            ), self.expr_span(actual_expr.id), 0, "phpstanRs.assertType");
                        }
                    }
                    self.check_expr(actual_expr, locals, label);
                    return Type::mixed();
                }
                // `count($o)` on a `Countable` object calls its `count()`.
                if let Some(name) = callee_name
                    && matches!(
                        name.trim_start_matches('\\').to_ascii_lowercase().as_str(),
                        "count" | "sizeof"
                    )
                    && let Some(counted) = args.first()
                {
                    let counted = arenas
                        .expr(counted.value)
                        .expect("call argument belongs to its retained file owner");
                    let counted_ty = self.infer_expr(counted, locals);
                    self.implicit_method_call(
                        &counted_ty,
                        "Countable",
                        "count",
                        self.expr_span(e.id),
                        locals,
                    );
                }
                if let Some(name) = callee_name
                    && name == "extract"
                {
                    self.saw_dynamic_var_construct = true;
                }
                if let Some(name) = callee_name
                    && name
                        .trim_start_matches('\\')
                        .eq_ignore_ascii_case("extract")
                {
                    locals.set_any_variable_may_exist(true);
                }
                // Resolved once up front (namespace/`use`-import rules
                // applied, with the same global-namespace fallback real
                // PHP gives an unqualified function call) so both the
                // arg-checking path and the undefined-call diagnostic
                // below see the same name(s) a real call would actually
                // resolve to.
                let (resolved_name, sig) = match callee_name {
                    Some(name) => match self.registry.resolve_function(name) {
                        Some((resolved, sig)) => (Some(resolved), Some(sig)),
                        None => (Some(name.candidates().0), None),
                    },
                    None => (None, None),
                };
                // `resolved_name` is already namespace-qualified the
                // same way `check_class_name_case`'s own `resolved`
                // variable is, so the identical canonical-vs-written
                // comparison applies unchanged.
                if self.settings.rules.check_function_name_case
                    && let Some(resolved) = resolved_name
                    && let Some((canonical, _)) = self.registry.lookup_function_with_name(resolved)
                    && canonical != resolved
                {
                    let mut builder = DiagnosticBuilder::new(
                        format!("Function {canonical} used with incorrect case: {resolved}."),
                        self.expr_span(callee.id),
                        0,
                        "function.nameCase",
                    );
                    if let Some(written) = callee_name
                        && let Some((span, replacement, description)) =
                            Self::name_case_fix(written, canonical, self.expr_span(callee.id))
                    {
                        builder = builder.fix(span, replacement, description);
                    }
                    self.report_builder(builder);
                }
                // Resolves the call's arguments against the real
                // signature (named/reordered-named arguments no longer
                // get checked as if they were positional in written
                // order) before any consumer below sees them.
                let resolved = sig
                    .map(|s| resolve_call_args(arenas, args, s.params()))
                    .unwrap_or_default();
                self.maybe_record_param_name_hints(sig, args);
                // `array_pop`/`array_shift`'s by-ref first argument gets
                // widened right below (`predefine_byref_or_unknown_args`'s
                // dedicated arm for these two - correctly dropping any
                // known non-emptiness, since removing an element can
                // leave `[]`) before this same call's return type is
                // computed further down. Reading the already-widened
                // array there made a provably non-empty array (e.g.
                // `array_shift(explode(...))`, `explode()`'s return type
                // unconditionally non-empty) look like it could return
                // `null`, when the value actually removed and returned is
                // the pre-widening array's definitely-present first/last
                // element. Captured here, before the widening runs, and
                // substituted back in once the return-type computation
                // below has run.
                //
                // Disclosed residual: fixes the common `$x =
                // array_shift($arr); needsFoo($x);` shape but not
                // `needsFoo(array_shift($arr))` (the identical call passed
                // directly as another call's argument) - argument-type
                // checking independently re-derives each argument's type
                // via `infer_expr` from whatever `locals` holds then,
                // rather than reusing this already-corrected value.
                let array_shift_pop_pre_widen_ty: Option<Type> =
                    matches!(resolved_name, Some("array_shift") | Some("array_pop"))
                        .then(|| args.first())
                        .flatten()
                        .map(|a| {
                            let value = arenas
                                .expr(a.value)
                                .expect("call argument belongs to its retained file owner");
                            self.infer_expr(value, locals)
                        });
                self.predefine_byref_or_unknown_args(
                    arenas,
                    resolved_name,
                    sig,
                    args,
                    &resolved,
                    locals,
                );
                // `it`/`test`/`describe`/`beforeEach`/`afterEach`/
                // `beforeAll`/`afterAll` rebind `$this` inside their
                // closure/arrow-fn argument at runtime (Pest's own
                // `Closure::bindTo()`) - reuses the same
                // check_closure_literal_with_this helper the
                // Closure::bind/bindTo special case below uses for the
                // identical "externally-bound $this" shape. Only a
                // closure/arrow-fn literal argument is recognized, the
                // same restriction the bind/bindTo case has -
                // `it('...', $variableHoldingAClosure)` falls through to
                // plain check_expr, unrebound.
                let is_this_binding_call = self.settings.pest
                    && callee_name.is_some_and(|name| {
                        pest_config::is_this_binding_call(name.written.as_ref())
                    });
                let pest_call_name: Option<&str> = is_this_binding_call
                    .then(|| callee_name.map(|name| name.written.as_ref()))
                    .flatten();
                // `array_map($callback, $items)`'s contextual parameter-
                // type injection - computed once, up front, from
                // `$items`' element type (not per-argument inside the
                // loop below, since it's the other argument's type this
                // needs). Scoped to the common 2-arg single-array form,
                // matching `infer_builtin_call_return_type`'s identical
                // restriction for the same function.
                let array_map_elem_override = (resolved_name == Some("array_map")
                    && args.len() == 2)
                    .then(|| {
                        let value = arenas
                            .expr(args[1].value)
                            .expect("call argument belongs to its retained file owner");
                        self.infer_expr(value, locals)
                    })
                    .and_then(|ty| ty.array_value_type());
                // The same contextual parameter-type injection as
                // `array_map` above, extended to `array_filter`/`usort`/
                // `uasort`/`array_walk`/`preg_replace_callback` - each
                // computes its own `(callback argument index, param
                // overrides)` pair once up front, the identical shape
                // `array_map_elem_override` established, just
                // generalized to a callback that isn't always argument 0
                // and doesn't always take exactly one parameter.
                // `check_closure_literal_with_param_overrides`'s
                // exact-arity requirement already handles every mode/
                // shape this doesn't specifically recognize (e.g.
                // `array_filter`'s `ARRAY_FILTER_USE_*`
                // modes, `array_walk`'s optional third `$arg` callback
                // parameter) by simply falling back to a plain check,
                // with no separate gate needed here.
                let contextual_callback_override: Option<(usize, Vec<Type>)> = resolved_name
                    .and_then(|name| match name {
                        "array_filter" if args.len() == 2 => {
                            let value = arenas
                                .expr(args[0].value)
                                .expect("call argument belongs to its retained file owner");
                            let arr_ty = self.infer_expr(value, locals);
                            arr_ty.array_value_type().map(|t| (1, vec![t]))
                        }
                        "usort" | "uasort" if args.len() == 2 => {
                            let value = arenas
                                .expr(args[0].value)
                                .expect("call argument belongs to its retained file owner");
                            let arr_ty = self.infer_expr(value, locals);
                            arr_ty.array_value_type().map(|t| (1, vec![t.clone(), t]))
                        }
                        "array_walk" | "array_find" | "array_find_key" | "array_any"
                        | "array_all"
                            if args.len() == 2 =>
                        {
                            let value = arenas
                                .expr(args[0].value)
                                .expect("call argument belongs to its retained file owner");
                            let arr_ty = self.infer_expr(value, locals);
                            match (arr_ty.array_value_type(), arr_ty.array_key_type()) {
                                (Some(elem), Some(key)) => Some((1, vec![elem, key])),
                                _ => None,
                            }
                        }
                        // `$matches`'s own real shape depends on the
                        // pattern's own capture groups, which this
                        // project doesn't re-derive here from an
                        // arbitrary (possibly non-literal) pattern
                        // argument - a generic `array<int|string, string>`
                        // (real PHP's own actual runtime shape for a
                        // pattern with no *named* capture groups, the
                        // common case) is still strictly more useful
                        // than the `Mixed` the callback's own unhinted
                        // parameter would otherwise get.
                        "preg_replace_callback" if args.len() >= 3 => Some((
                            1,
                            vec![Type::array(Type::unknown_array_key(), Type::string())],
                        )),
                        _ => None,
                    });
                for (i, a) in args.iter().enumerate() {
                    let argument = arenas
                        .expr(a.value)
                        .expect("call argument belongs to its retained file owner");
                    let peeled_argument = arenas
                        .peeled(a.value)
                        .and_then(|id| arenas.expr(id))
                        .expect("call argument belongs to its retained file owner");
                    // `argument.unpackNonIterable` - `foo(...$x)` where
                    // `$x` is confirmed to never be iterable at all.
                    // Checked via `infer_expr` (read-only, no side
                    // effects) rather than threading a captured type
                    // through every branch below - this arm's own
                    // per-argument dispatch already has four different
                    // shapes, none of which uniformly produce one.
                    if a.spread {
                        let spread_ty = self.infer_expr(argument, locals);
                        if let Some(level) = self
                            .failing_level(&spread_ty, 3, |m| self.relation().is_iterable(m).no())
                        {
                            self.report(
                                format!("Only iterables can be unpacked, {spread_ty} given."),
                                self.expr_span(argument.id),
                                level,
                                "argument.unpackNonIterable",
                            );
                        }
                    }
                    if is_this_binding_call
                        && matches!(
                            &peeled_argument.kind,
                            ExprKind::Closure(_) | ExprKind::ArrowFn(_)
                        )
                    {
                        let this_ty = self.pest_this_type.clone();
                        // Pest's `$this`-binding (a real `TestCase`-
                        // subclass instance, almost always) grants the
                        // same private/protected access real PHP's
                        // `Closure::bind`/`->bindTo()` third argument does,
                        // since `current_class` has no other way to
                        // reflect Pest's file-scoped binding.
                        let scope_class = this_ty.single_object_class().map(|c| c.to_string());
                        self.check_closure_literal_with_this_and_scope(
                            peeled_argument,
                            locals,
                            label,
                            this_ty,
                            scope_class,
                        );
                        if self.settings.phpunit.check_risky_tests
                            && let Some(name) = pest_call_name
                        {
                            self.check_pest_risky_test(name, peeled_argument, self.expr_span(e.id));
                        }
                    } else if i == 0
                        && let Some(elem_ty) = &array_map_elem_override
                        && self.check_closure_literal_with_param_overrides(
                            peeled_argument,
                            std::slice::from_ref(elem_ty),
                            locals,
                            label,
                        )
                    {
                        // Body already checked with the injected element
                        // type above - no plain check_expr fallback.
                    } else if let Some((idx, overrides)) = &contextual_callback_override
                        && i == *idx
                        && self.check_closure_literal_with_param_overrides(
                            peeled_argument,
                            overrides,
                            locals,
                            label,
                        )
                    {
                        // Same as the `array_map` arm just above, for
                        // one of the other four contextually-checked
                        // functions.
                    } else if let Some(this_ty) = sig
                        .and_then(|s| s.params().get(i))
                        .and_then(|p| p.closure_this_type().clone())
                        && matches!(
                            &peeled_argument.kind,
                            ExprKind::Closure(_) | ExprKind::ArrowFn(_)
                        )
                    {
                        // `@param-closure-this` - a Closure/arrow-fn
                        // literal argument at this parameter's position
                        // gets `$this` seeded to the tag's declared type,
                        // the same mechanism `Closure::bind`/`->bindTo()`
                        // use. Only fires for a literal, matching real
                        // PHPStan's `resolveClosureThisType` restriction -
                        // a closure passed by variable falls through to
                        // plain `check_expr`, unchanged.
                        self.check_closure_literal_with_this_and_scope(
                            peeled_argument,
                            locals,
                            label,
                            this_ty,
                            None,
                        );
                    } else if let Some(callable_sig) = sig
                        .and_then(|s| s.params().get(i))
                        .and_then(|p| p.ty().callable_sig())
                        && matches!(
                            &peeled_argument.kind,
                            ExprKind::Closure(_) | ExprKind::ArrowFn(_)
                        )
                        && self.check_closure_literal_with_param_overrides(
                            peeled_argument,
                            &callable_sig
                                .params
                                .iter()
                                .map(|p| p.ty.clone())
                                .collect::<Vec<_>>(),
                            locals,
                            label,
                        )
                    {
                        // A `Closure(...)`-shaped PHPDoc parameter type
                        // gives an unhinted closure/arrow-fn literal
                        // argument's params their declared types, the
                        // same contextual-checking mechanism `array_map`/
                        // the four other special-cased functions above
                        // use.
                    } else {
                        self.check_expr(argument, locals, label);
                    }
                }
                // `function.impossibleType`/`function.alreadyNarrowedType`
                // - deliberately checked against every call regardless of
                // `resolved_name`/`sig` (the same raw-callee-name shape
                // `narrowing.rs`'s own `is_string`/`is_int`/... narrowing
                // dispatch already keys off of, not this arm's own
                // resolved/namespace-qualified name), since these are
                // always-global builtin predicate names with no
                // legitimate user override to resolve against.
                self.check_impossible_type_check_call(
                    arenas,
                    callee,
                    args,
                    locals,
                    is_exempt_last_condition,
                    self.expr_span(e.id),
                );
                if let Some(name) = &resolved_name {
                    if let Some(sig) = sig {
                        // This function's own throw contribution - tried
                        // before `sig.throws` itself.
                        let bare = name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .to_ascii_lowercase();
                        let dynamic_throw =
                            self.dynamic_function_throw_type(arenas, &bare, args, locals);
                        self.record_call_throw_point(
                            dynamic_throw,
                            Some(sig),
                            self.expr_span(e.id),
                        );
                        self.check_internal_function_call(name, sig, self.expr_span(e.id));
                        self.check_restricted_function_call(
                            name,
                            args,
                            locals,
                            self.expr_span(e.id),
                        );
                        self.check_deprecated_function_call(name, sig, self.expr_span(e.id));
                        if self.settings.slam.check_current_time_dependence {
                            self.check_current_time_call(
                                arenas,
                                &name.to_ascii_lowercase(),
                                args,
                                locals,
                                self.expr_span(e.id),
                            );
                        }
                        self.record_function_usage_token(name, sig, self.expr_span(callee.id));
                        match *name {
                            "array_filter" => {
                                self.check_array_filter_no_op(
                                    arenas,
                                    args,
                                    locals,
                                    self.expr_span(e.id),
                                );
                                if self.settings.strict_rules.check_strict_array_filter {
                                    self.check_strict_array_filter(
                                        arenas,
                                        sig,
                                        args,
                                        locals,
                                        self.expr_span(e.id),
                                    );
                                }
                            }
                            "array_values" => self.check_array_values_no_op(
                                arenas,
                                args,
                                locals,
                                self.expr_span(e.id),
                            ),
                            "ini_get" | "ini_set" | "ini_alter" | "ini_restore" | "get_cfg_var" => {
                                self.check_ini_directive_call(arenas, name, args, locals)
                            }
                            "in_array" | "array_search" | "array_keys" | "base64_decode"
                            | "iterator_to_array"
                                if self.settings.strict_rules.check_strict_function_calls =>
                            {
                                self.check_strict_function_call(
                                    arenas,
                                    name,
                                    sig,
                                    args,
                                    locals,
                                    self.expr_span(e.id),
                                )
                            }
                            "in_array" | "array_search" | "array_keys"
                                if self.settings.extras.check_loose_haystack_comparison =>
                            {
                                self.check_loose_haystack_comparison(
                                    arenas,
                                    name,
                                    sig,
                                    args,
                                    locals,
                                    self.expr_span(e.id),
                                )
                            }
                            _ => {}
                        }
                        // A spread argument (`f(...$args)`) means the
                        // real argument count/types past it aren't known
                        // statically - `check_args` is spread-aware (it
                        // stops trusting a count/"missing" check at or
                        // past a spread's position), so it's always worth
                        // calling.
                        let declared = self
                            .registry
                            .lookup_function_with_name(name)
                            .map_or_else(|| name.to_string(), |(n, _)| n.to_string());
                        self.check_args(CheckArgsCall {
                            callee_label: CalleeLabel::Text(name),
                            callee: Callee::Function(&declared),
                            sig,
                            args,
                            locals,
                            defining_class: None,
                            is_builtin: self.registry.is_builtin_function(name),
                            span: self.expr_span(e.id),
                        });
                        self.apply_param_out_types(sig, args, locals);
                        self.check_castable_array_arguments(
                            arenas, name, sig, args, &resolved, locals,
                        );
                        self.check_impossible_haystack_values(
                            arenas,
                            name,
                            args,
                            locals,
                            self.expr_span(e.id),
                        );
                        if name.eq_ignore_ascii_case("define") {
                            self.check_define_value(arenas, args, locals, self.expr_span(e.id));
                        }
                        self.check_format_string_call(
                            arenas,
                            &name.to_ascii_lowercase(),
                            args,
                            locals,
                            self.expr_span(e.id),
                        );
                        // Real PHP resolves `define` case-insensitively
                        // like any other function call - matched against
                        // just the last namespace segment, the same as
                        // every other by-name builtin special-case here.
                        if name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .eq_ignore_ascii_case("define")
                        {
                            self.check_define_third_argument(args, self.expr_span(e.id));
                        }
                        if name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .eq_ignore_ascii_case("random_int")
                        {
                            self.check_random_int_call(arenas, args, locals);
                        }
                        let bare = name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .to_ascii_lowercase();
                        if matches!(
                            bare.as_str(),
                            "array_combine" | "array_flip" | "array_count_values"
                        ) {
                            self.check_value_to_key_call(arenas, &bare, args, locals);
                        }
                        if name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .eq_ignore_ascii_case("filter_var")
                            && args.len() >= 3
                        {
                            self.check_filter_var_flag_conflict(arenas, args, self.expr_span(e.id));
                        }
                        if name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .eq_ignore_ascii_case("simplexml_load_string")
                        {
                            self.check_simplexml_load_string_call(
                                arenas,
                                args,
                                self.expr_span(e.id),
                            );
                        }
                        self.check_preg_pattern_validity(arenas, name, args);
                        {
                            let bare = name.rsplit('\\').next().unwrap_or(name);
                            if bare.eq_ignore_ascii_case("sscanf")
                                || bare.eq_ignore_ascii_case("fscanf")
                            {
                                self.check_scanf_argument_count(
                                    arenas,
                                    bare,
                                    args,
                                    self.expr_span(e.id),
                                );
                            }
                            if bare.eq_ignore_ascii_case("call_user_func")
                                || bare.eq_ignore_ascii_case("call_user_func_array")
                            {
                                self.check_call_user_func(
                                    arenas,
                                    bare,
                                    args,
                                    locals,
                                    self.expr_span(e.id),
                                );
                            }
                            self.check_preg_quote_delimiter(arenas, bare, args);
                        }
                        if !is_bare_statement {
                            self.check_useless_return_value_call(
                                arenas,
                                name,
                                args,
                                locals,
                                self.expr_span(e.id),
                            );
                        }
                    } else if locals.guarded_functions().is_empty()
                        || !locals.guarded_functions().contains(
                            &name
                                .rsplit('\\')
                                .next()
                                .unwrap_or(name)
                                .to_ascii_lowercase(),
                        )
                    {
                        // Since a function name always resolves to a
                        // definite name (unlike `ExprKind::New`'s
                        // class resolution, which still has a "can't
                        // resolve" case for a multi-segment name with no
                        // matching import), every miss here is a real
                        // undefined-function finding, unless it's runtime-
                        // guarded by a `function_exists()` check this
                        // registry doesn't otherwise know how to resolve -
                        // purely presentational, so argument checking
                        // against a "found" signature still correctly
                        // never happens for a guarded, still-genuinely-
                        // unresolvable call.
                        self.report(
                            format!("call to undefined function {name}()"),
                            self.expr_span(e.id),
                            0,
                            "function.notFound",
                        );
                    }
                } else if !matches!(&callee.kind, ExprKind::Name(_)) {
                    // `resolved_name` is `None` here specifically because
                    // `callee` isn't a plain function-name reference at
                    // all (`$fn(...)`, `$arr['key'](...)`,
                    // `($this->makeCallback())(...)`) - every language-
                    // construct name (`isset`, `print`, ...) that also
                    // produces `None` above is excluded by the
                    // `ExprKind::Name` guard, so this branch never fires
                    // for those.
                    let callee_ty = self.check_expr(callee, locals, label);
                    // `callable.nonCallable` - checked (and, by
                    // reference, before the `if let` below consumes
                    // `callee_ty` by value) for every dynamic-callee
                    // shape this branch covers.
                    if let Some(level) =
                        self.failing_level(&callee_ty, 2, |m| self.relation().is_callable(m).no())
                    {
                        let verdict = if self.relation().is_callable(&callee_ty).no() {
                            "it's not a callable"
                        } else {
                            "it might not be a callable"
                        };
                        self.report(
                            format!("Trying to invoke {callee_ty} but {verdict}."),
                            self.expr_span(callee.id),
                            level,
                            "callable.nonCallable",
                        );
                    }
                    if let Some(callable_sig) = callee_ty.callable_sig().cloned() {
                        self.check_dynamic_callable_args(
                            arenas,
                            &callable_sig,
                            args,
                            locals,
                            self.expr_span(e.id),
                        );
                    } else {
                        self.check_named_callable_invocation(
                            &callee_ty,
                            args,
                            locals,
                            self.expr_span(e.id),
                        );
                    }
                    // A `Closure(...)`-shaped PHPDoc callable carries no
                    // `@throws` info of its own, and any other dynamic-
                    // callee shape has even less to go on - genuinely
                    // unbounded.
                    self.current_function_has_unannotated_call = true;
                }
                // `preg_match`'s and `preg_match_all`'s by-ref `$matches`,
                // for every result the call can give. Placed after
                // `check_args`, which checks `$matches`'s pre-call type
                // against the `?array<string>` parameter.
                if matches!(resolved_name, Some("preg_match" | "preg_match_all"))
                    && let Some(call) = preg_call(arenas, e.id)
                    && call.subject.is_some()
                    && let Some(matches_arg) = call.matches
                    && let Some(outcome) = preg_result_outcome(&preg_result_type(call.all))
                    && let Some(shape) = self.preg_matches_type(&call, outcome, locals)
                {
                    if let ExprKind::Variable(matches_name) = &matches_arg.kind {
                        purge_stale_narrowing_for_var(locals, matches_name);
                        // `$matches` is often first bound by this call.
                        if locals.contains_var(matches_name) {
                            locals.insert_var(matches_name, shape);
                        } else {
                            locals.declare_var(matches_name, shape, self.expr_span(matches_arg.id));
                        }
                    } else if self.key_of_expr(locals, matches_arg).is_some() {
                        self.bind_assignment_target(arenas, matches_arg, shape, locals);
                    }
                }
                // `proc_open()`'s by-ref `$pipes` out-parameter (its 3rd
                // argument) would otherwise fall to `predefine_byref_
                // or_unknown_args`'s generic `Mixed` leniency, since its
                // real stub (`@param-out array<P, resource> $pipes`,
                // `@template P of int`) lives in a separate stub bundle
                // this project doesn't ingest. PHPStan correlates
                // `$descriptor_spec`'s literal keys into `P`; this project
                // doesn't have a template-binding-through-`@param-out`
                // mechanism to key off a different parameter's literal
                // keys, but a plain `array<int, resource>` is still
                // enough to be useful.
                if resolved_name == Some("proc_open")
                    && let Some(pipes_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(pipes_name) = &pipes_arg.kind
                {
                    let shape = Type::array(Type::int(), Type::resource(false));
                    purge_stale_narrowing_for_var(locals, pipes_name);
                    if locals.contains_var(pipes_name) {
                        locals.insert_var(pipes_name, shape);
                    } else {
                        locals.declare_var(pipes_name, shape, self.expr_span(pipes_arg.id));
                    }
                }
                // Reuses the `resolved_name`/`sig` already resolved above
                // - the same lookup `infer_expr`'s own `Call` arm would
                // otherwise redundantly redo from scratch.
                let call_ty = sig
                    .map(|s| {
                        self.infer_builtin_call_return_type(
                            resolved_name.unwrap_or_default(),
                            s,
                            args,
                            locals,
                        )
                    })
                    .unwrap_or_else(|| {
                        self.call_fallback_return_type(arenas, callee, args, locals)
                    });
                match array_shift_pop_pre_widen_ty.and_then(|pre_widen_ty| {
                    pre_widen_ty.array_value_type().map(|elem_ty| {
                        Self::array_pointer_result(&pre_widen_ty, elem_ty, Type::null(), false)
                    })
                }) {
                    Some(corrected) => corrected,
                    None => call_ty,
                }
            }
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("method calls retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let recv = arenas
                    .expr(*recv)
                    .expect("method receiver belongs to its retained file owner");
                let recv_peeled = arenas
                    .peeled(recv.id)
                    .and_then(|id| arenas.expr(id))
                    .expect("method receiver belongs to its retained file owner");
                let args = arenas.args(args.clone());
                // `(function () {...})->bindTo($newThis, $scope)` - same
                // idea as `Closure::bind`, just on the receiver instead
                // of the first argument.
                //
                // The `bindTo` branch can't reuse `check_expr`'s own
                // return value the way the ordinary branch below does -
                // `check_closure_literal_with_this` checks the closure's
                // *body* under a rebound `$this`, it doesn't compute
                // `recv`'s own type - so `recv` still needs its own
                // `infer_expr` call there specifically.
                let recv_ty = if **method == *"bindTo"
                    && matches!(
                        &recv_peeled.kind,
                        ExprKind::Closure(_) | ExprKind::ArrowFn(_)
                    ) {
                    let this_ty = args
                        .first()
                        .map(|a| {
                            let value = arenas
                                .expr(a.value)
                                .expect("bindTo argument belongs to its retained file owner");
                            self.infer_expr(value, locals)
                        })
                        .unwrap_or(Type::mixed());
                    let scope_class = args
                        .get(1)
                        .and_then(|a| arenas.expr(a.value))
                        .and_then(|value| self.literal_class_name_from_expr(value));
                    self.check_closure_literal_with_this_and_scope(
                        recv_peeled,
                        locals,
                        label,
                        this_ty,
                        scope_class,
                    );
                    self.infer_expr(recv, locals)
                } else {
                    self.check_expr(recv, locals, label)
                };
                // A nullsafe access earlier in the chain that met `null`
                // skipped this call.
                let recv_ty = if self
                    .syntax_arenas()
                    .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, recv))
                {
                    self.chain_link_type(recv, locals)
                } else {
                    recv_ty
                };
                // See `probe_span`'s doc comment.
                self.probe_hit_at(self.expr_span(recv.id), &recv_ty);
                // `$x?->method()` is pointless once `$x`'s type already
                // proves it's never null - mirrors real PHPStan's
                // `NullsafeMethodCallRule` exactly. Guarded by
                // `loop_depth`/`static_var_names` for the same reasons
                // `check_pointless_null_probe` is - a receiver only ever
                // widened to nullable on a later loop iteration or an
                // earlier call still looks non-nullable on this single
                // pass.
                if nullsafe.is_nullsafe()
                    && self.loop_depth == 0
                    && !matches!(&recv.kind, ExprKind::Variable(n) if self.static_var_names.contains(n.as_ref()))
                    && type_definitely_excludes_null(&recv_ty)
                {
                    self.report(format!(
                            "Using nullsafe method call on non-nullable type {recv_ty}. Use -> instead."
                        ),
                        self.expr_span(e.id),
                        0,
                        "nullsafe.neverNull",
                    );
                }
                // A method lookup trusts the specific `ClassReflection` stored
                // under a fully-qualified name, not a bare one two
                // unrelated packages could collide on. `member_missing`
                // below picks up the "should this be flagged" question
                // for a `Union`/`Intersection` receiver (e.g. a multi-type
                // `catch` variable), using a representative member (the
                // first `Object` found) for argument-checking.
                let recv_class = recv_ty.first_object_class().cloned();
                // `method.nonObject` - a receiver whose type is confirmed
                // to never be an object at all (a scalar, an array,
                // `stdClass`) gets flagged here and short-circuits the
                // rest of this arm's method-resolution logic entirely,
                // rather than falling through to `method.notFound` below.
                // `recv_class` above and `Relation::can_call_methods` are
                // *not* disjoint sets once a `Union` mixes a real class
                // with a closed member (e.g. `Foo|string`) - this early
                // return, not non-overlapping domains, is what keeps the
                // two checks from double-firing on the same call.
                // Nullsafe-aware the same way real PHPStan's own
                // `NullsafeOperatorHelper::getNullsafeShortcircuitedExprRespectingScope`
                // is: `$x?->method()` on a `null`-only `$x` short-circuits
                // to `null` rather than erroring, while plain
                // `$x->method()` on the same `$x` is a real bug.
                let nonobject_ty = if nullsafe.is_nullsafe() {
                    recv_ty.without(&Type::null())
                } else {
                    recv_ty.clone()
                };
                if let Some(level) = self.failing_level(&nonobject_ty, 0, |m| {
                    self.relation().can_call_methods(m).no()
                }) {
                    self.report(
                        format!(
                            "call to method {method}() on {} {recv_ty}",
                            non_object_kind(level)
                        ),
                        self.expr_span(e.id),
                        level,
                        "method.nonObject",
                    );
                    // Only a receiver no part of which has methods ends
                    // the call here; a partly failing one is still read.
                    if level == 0 {
                        for a in args {
                            let value = arenas
                                .expr(a.value)
                                .expect("method argument belongs to its retained file owner");
                            self.check_expr(value, locals, label);
                        }
                        return Type::mixed();
                    }
                }
                if let Some(from) = types::strict_mixed_receiver_from(&nonobject_ty) {
                    self.report_found_from(
                        format!("Cannot call method {method}() on mixed."),
                        self.expr_span(e.id),
                        0,
                        from,
                        "method.nonObject",
                    );
                }
                // `sig` (and every class-name-dependent decision below
                // it - argument-type checking, visibility/`@internal`/
                // `@deprecated`, the Doctrine special case, and the final
                // return-type resolution) must NOT derive `class`/`owner`
                // from `recv_class` above, which is picked purely to
                // answer "is there any concrete class at all," regardless
                // of whether it actually has the method. `find_method_
                // receiver` searches every member (`Intersection` in
                // reverse, preferring the narrower, just-`instanceof`-
                // checked type - see `method_call_return_type`'s doc
                // comment) for one that genuinely resolves the method,
                // carrying its type-args and declaring owner through in
                // one pass. Substituted through the receiver's carried
                // type arguments, walking up to whichever ancestor
                // actually declares the method if it's inherited - a
                // method declared `add(T $item)` on `Collection<User>`,
                // or inherited from `Collection` by a concrete
                // `IntCollection extends Collection<int>`, gets checked
                // against `User`/`int`, not an unbound `T`/`Mixed`.
                // `find_method_receiver` already applies class-template
                // substitution inline - `sig` just borrows the already-
                // owned, already-substituted signature straight out of
                // `receiver`'s tuple, no second clone/substitution here.
                let receiver = self.find_method_receiver(&recv_ty, method);
                if is_bare_statement && let Some((_, _, owner, _)) = &receiver {
                    self.statement_method_call = Some(crate::checker::StatementMethodCall {
                        id: e.id,
                        owner: owner.clone(),
                        receiver: recv_ty.clone(),
                    });
                }
                let sig = receiver
                    .as_ref()
                    .map(|(_, _, _, sig)| std::borrow::Borrow::borrow(sig));
                if let Some((_, _, owner, _)) = &receiver
                    && let Some(s) = sig
                {
                    self.check_phpstan_api_method_call(owner, method, s, self.expr_span(e.id));
                }
                if let Some(class) = &recv_class {
                    self.check_node_connecting_attribute(
                        arenas,
                        class,
                        method,
                        args,
                        self.expr_span(e.id),
                    );
                }
                // This method call's own throw contribution.
                if let Some((class, _, _, _)) = &receiver {
                    let dynamic_throw = self.dynamic_method_throw_type(class, method, args);
                    self.record_call_throw_point(dynamic_throw, sig, self.expr_span(e.id));
                } else {
                    // No resolved receiver class at all - genuinely
                    // unbounded, same reasoning as a dynamic callee.
                    self.current_function_has_unannotated_call = true;
                }
                let resolved = sig
                    .map(|s| resolve_call_args(arenas, args, s.params()))
                    .unwrap_or_default();
                self.maybe_record_param_name_hints(sig, args);
                self.predefine_byref_or_unknown_args(arenas, None, sig, args, &resolved, locals);
                for (i, a) in args.iter().enumerate() {
                    let argument = arenas
                        .expr(a.value)
                        .expect("method argument belongs to its retained file owner");
                    let peeled_argument = arenas
                        .peeled(a.value)
                        .and_then(|id| arenas.expr(id))
                        .expect("method argument belongs to its retained file owner");
                    // `argument.unpackNonIterable` - see `ExprKind::Call`'s
                    // identical check above.
                    if a.spread {
                        let spread_ty = self.infer_expr(argument, locals);
                        if let Some(level) = self
                            .failing_level(&spread_ty, 3, |m| self.relation().is_iterable(m).no())
                        {
                            self.report(
                                format!("Only iterables can be unpacked, {spread_ty} given."),
                                self.expr_span(argument.id),
                                level,
                                "argument.unpackNonIterable",
                            );
                        }
                    }
                    let closure_this = sig
                        .and_then(|s| s.params().get(i))
                        .and_then(|p| p.closure_this_type().clone());
                    if let Some(this_ty) = closure_this
                        && matches!(
                            &peeled_argument.kind,
                            ExprKind::Closure(_) | ExprKind::ArrowFn(_)
                        )
                    {
                        // `@param-closure-this` - see `ExprKind::Call`'s
                        // identical branch.
                        self.check_closure_literal_with_this_and_scope(
                            peeled_argument,
                            locals,
                            label,
                            this_ty,
                            None,
                        );
                    } else {
                        self.check_expr(argument, locals, label);
                    }
                }
                if let Some(class) = &recv_class {
                    // Checked before falling back to whether the
                    // representative member's `sig` was found: for a
                    // `Union`, the representative is just the first
                    // `Object` member encountered, which could easily be
                    // one that does have the method even though another
                    // member doesn't - the real question is never "does
                    // the first member have it," it's "is it missing from
                    // any member (Union) / every member (Intersection)."
                    // A class not known to the registry at all is left
                    // unchecked rather than risking a false positive -
                    // only fires once the class itself is confirmed
                    // known. `__call` anywhere in the chain means any
                    // method name is legitimately callable at runtime, so
                    // its presence suppresses this entirely, matching
                    // real PHPStan.
                    // Mirrors real PHPStan's
                    // `MethodExistsTypeSpecifyingExtension` - a
                    // `method_exists($recv, 'name')` true branch proves
                    // this exact call safe for the rest of that branch,
                    // regardless of `$recv`'s static type
                    // (`Locals::guarded_methods`, set in
                    // `apply_narrowing`). Only fires when the receiver is
                    // the exact bare-variable shape the guard was
                    // recorded against.
                    let method_exists_guarded = !locals.guarded_methods().is_empty()
                        && matches!(
                            &recv_peeled.kind,
                            ExprKind::Variable(name)
                                if locals.find_var(name).is_some_and(|receiver| locals
                                    .guarded_methods()
                                    .contains(&(receiver, method.to_string())))
                        );
                    if !method_exists_guarded
                        && self.registry.has_class(class.as_str())
                        && self.member_confirmed_missing(&recv_ty, |c| {
                            self.registry.lookup_method(c, method).is_some()
                                || self.registry.lookup_method(c, "__call").is_some()
                        })
                    {
                        let mut builder = DiagnosticBuilder::new(
                            format!("call to undefined method {recv_ty}::{method}()"),
                            self.expr_span(e.id),
                            0,
                            "method.notFound",
                        );
                        if let Some(suggestion) = suggest::suggest_similar_name(
                            method,
                            self.registry.all_method_names(class),
                        ) {
                            builder = builder.tip(format!("Did you mean {suggestion}()?"));
                        }
                        self.report_builder(builder);
                    } else if let Some((class, _, owner, _)) = &receiver
                        && let Some(sig) = sig
                    {
                        // `owner`/`sig` both come from the same
                        // `find_method_receiver` lookup, guaranteed
                        // consistent with each other - no separate
                        // re-lookup needed.
                        self.check_member_visibility(MemberVisibilityCheck {
                            member: MemberLabel {
                                kind: MemberKind::Method,
                                owner,
                                name: method,
                            },
                            visibility: &sig.visibility(),
                            span: self.expr_span(e.id),
                        });
                        self.check_internal_method_call(
                            owner,
                            method,
                            sig,
                            false,
                            self.expr_span(e.id),
                        );
                        self.check_restricted_method_call(
                            owner,
                            method,
                            false,
                            args,
                            locals,
                            self.expr_span(e.id),
                        );
                        self.check_deprecated_method_call(
                            owner,
                            method,
                            sig,
                            false,
                            self.expr_span(e.id),
                        );
                        self.record_member_usage_token(
                            UsageTokenKind::Method,
                            owner,
                            sig.deprecated().is_some(),
                            sig.is_internal(),
                            self.expr_span(e.id),
                        );
                        self.record_method_ref(self.expr_span(e.id), owner, method);
                        self.check_call_site_method_name_case(
                            class,
                            method,
                            self.expr_span(e.id),
                            Span {
                                start: self.expr_span(recv.id).end,
                                end: self.expr_span(e.id).end,
                            },
                        );
                        // `DOMDocument::createElement()`'s tag-name
                        // validation - independent of the internal/
                        // deprecated checks above.
                        self.check_dom_create_element(
                            arenas,
                            class,
                            method,
                            args,
                            self.expr_span(e.id),
                        );
                        // SQL parameter binding validation - independent of
                        // the internal/deprecated checks above too.
                        self.check_dbal_parameter_binding(
                            arenas,
                            class,
                            method,
                            recv,
                            args,
                            locals,
                            self.expr_span(e.id),
                        );
                        // Renamed named arguments - see `checker::named_argument_renames`'s own
                        // module doc comment.
                        self.record_named_argument_call(
                            owner,
                            method,
                            &sig.visibility(),
                            self.expr_span(e.id),
                            args,
                        );
                        // See the identical comment on the plain-function
                        // Call arm above - `check_args` is spread-aware,
                        // so it's always worth calling.
                        let declared = self.registry.declared_method_name(owner, method);
                        self.check_args(CheckArgsCall {
                            callee_label: CalleeLabel::Method { class, method },
                            callee: Callee::Method {
                                class: owner,
                                name: &declared,
                            },
                            sig,
                            args,
                            locals,
                            defining_class: Some(class),
                            is_builtin: self.registry.is_builtin_class(owner),
                            span: self.expr_span(e.id),
                        });
                        self.apply_param_out_types(sig, args, locals);
                    }
                }
                // Delegates to `method_call_return_type`, the same
                // function `infer_expr`'s identical arm uses, rather than
                // re-deriving the return type independently - otherwise
                // every special case in `method_call_return_type` (e.g.
                // `DateTimeInterface::format()`/`diff()`,
                // `DateInterval::format()`) would silently never apply
                // here: `$diff = $a->diff($b); $diff->days;` would infer
                // plain `DateInterval` for `$diff`, discarding the
                // `days: int` refinement the identical call written as
                // one direct chain (`$a->diff($b)->days`) gets right.
                let result =
                    self.method_call_return_type(arenas, recv, &recv_ty, method, args, locals);
                if let Some((class, _, _, s)) = &receiver {
                    let s: &FunctionReflection = std::borrow::Borrow::borrow(s);
                    self.apply_self_out(recv, s, &resolved, locals, class);
                }
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if (nullsafe.is_nullsafe()
                    || self
                        .syntax_arenas()
                        .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, recv)))
                    && !result.is_mixed()
                {
                    self.relation().union(result, Type::null())
                } else {
                    result
                }
            }
            // `&&`/`||` short-circuit, so the right operand only ever runs
            // once the left one's already settled the outcome - it needs
            // to see whatever that implies (real code leans on this
            // constantly: `$x instanceof Foo && $x->bar()`,
            // `$x !== null && $x->prop`). Without this, checking the
            // right side against the un-narrowed left-hand type produced
            // real false positives once undefined-property detection
            // started exercising exactly this idiom on real code.
            // `And`/`LogicalOr` are `&&`/`and` - runtime-identical,
            // spelling-only difference (`BinOp::And`'s own doc comment) -
            // handled in one arm, with the exact matched `op` threaded
            // through to `check_non_boolean_and_or_operand`/
            // `check_boolean_and_or`/`infer_binary` so each renders the
            // spelling the source actually used.
            ExprKind::Binary(op @ (BinOp::And | BinOp::LogicalAnd), l, r) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("binary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let l = arenas
                    .expr(*l)
                    .expect("binary left operand belongs to its retained file owner");
                let r = arenas
                    .expr(*r)
                    .expect("binary right operand belongs to its retained file owner");
                let sigil = binop_sigil(op);
                let l_ty = self.check_expr(l, locals, label);
                self.check_non_boolean_and_or_operand(
                    l,
                    &l_ty,
                    sigil,
                    "left",
                    "booleanAnd.leftNotBoolean",
                );
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, true);
                let r_ty = self.check_expr(r, &mut narrowed, label);
                self.check_non_boolean_and_or_operand(
                    r,
                    &r_ty,
                    sigil,
                    "right",
                    "booleanAnd.rightNotBoolean",
                );
                // Mirrors real PHPStan's `BooleanAndConstantConditionRule` -
                // see `check_boolean_and_or`'s doc comment. Passes the
                // original, pre-narrowing `locals`, not `narrowed`:
                // `check_boolean_and_or` narrows its own right-side/
                // whole-result checks internally. `is_first_level` is
                // `is_condition_position || is_bare_statement`, matching
                // real PHPStan's `isInFirstLevelStatement()`: a bare
                // `$a && $b;` used as its own statement is checked with
                // `ExpressionContext::createTopLevel()` the same way an
                // `if`/`while`'s direct condition is.
                self.check_boolean_and_or(
                    op,
                    l,
                    r,
                    e,
                    locals,
                    is_condition_position || is_bare_statement,
                );
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(op, l, r, locals)
            }
            ExprKind::Binary(op @ (BinOp::Or | BinOp::LogicalOr), l, r) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("binary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let l = arenas
                    .expr(*l)
                    .expect("binary left operand belongs to its retained file owner");
                let r = arenas
                    .expr(*r)
                    .expect("binary right operand belongs to its retained file owner");
                let sigil = binop_sigil(op);
                let l_ty = self.check_expr(l, locals, label);
                self.check_non_boolean_and_or_operand(
                    l,
                    &l_ty,
                    sigil,
                    "left",
                    "booleanOr.leftNotBoolean",
                );
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, false);
                let r_ty = self.check_expr(r, &mut narrowed, label);
                self.check_non_boolean_and_or_operand(
                    r,
                    &r_ty,
                    sigil,
                    "right",
                    "booleanOr.rightNotBoolean",
                );
                // Mirrors real PHPStan's `BooleanOrConstantConditionRule` -
                // same reasoning as `&&` above.
                self.check_boolean_and_or(
                    op,
                    l,
                    r,
                    e,
                    locals,
                    is_condition_position || is_bare_statement,
                );
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(op, l, r, locals)
            }
            // `??` is built on isset()-style quiet semantics - its left
            // side is evaluated without ever raising an undefined-
            // variable/undefined-property notice, even when the root
            // variable was never assigned anywhere in scope. Real PHP
            // silences this for the whole dereference chain on the left
            // (`$config['key'] ?? 'default'` is just as warning-free as
            // a bare `$x ?? 'default'`), which is exactly what
            // `check_expr_quiet_root` walks. `??`'s result type is
            // usually just the right side's type - reusing
            // `check_expr(r, ...)`'s return value directly avoids a
            // double-walk of `r` - except when `widen_coalesce_result`
            // says the left side's type carries real information the
            // right side's bare fallback would otherwise discard.
            ExprKind::Binary(BinOp::Coalesce, l, r) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("binary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let l = arenas
                    .expr(*l)
                    .expect("binary left operand belongs to its retained file owner");
                let r = arenas
                    .expr(*r)
                    .expect("binary right operand belongs to its retained file owner");
                self.check_expr_quiet_root(arenas, l, locals, label);
                self.check_pointless_null_probe(
                    l,
                    locals,
                    "nullCoalesce",
                    "on left side of ??",
                    null_probe_verdict,
                );
                self.check_pointless_offset_or_property_probe(
                    arenas,
                    l,
                    locals,
                    "nullCoalesce",
                    "on left side of ??",
                    null_probe_verdict,
                );
                self.implicit_array_access_probe(
                    l,
                    &[Access::Isset, Access::Read],
                    self.expr_span(e.id),
                    locals,
                );
                let r_ty = self.check_expr(r, locals, label);
                self.widen_coalesce_result(l, r_ty, locals)
            }
            ExprKind::Binary(op, l, r) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("binary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let l = arenas
                    .expr(*l)
                    .expect("binary left operand belongs to its retained file owner");
                let r = arenas
                    .expr(*r)
                    .expect("binary right operand belongs to its retained file owner");
                let l_ty = self.check_expr(l, locals, label);
                let r_ty = self.check_expr(r, locals, label);
                self.binary_string_conversions(op, (l, &l_ty), (r, &r_ty), locals);
                let invalid = self.check_binary_operand_validity(
                    op,
                    &l_ty,
                    &r_ty,
                    false,
                    self.expr_span(l.id),
                );
                self.check_comparison_validity(op, &l_ty, &r_ty, self.expr_span(l.id));
                self.check_duplicate_operands(op, l, r);
                self.check_division_by_zero(op, &r_ty, self.expr_span(r.id));
                self.check_constant_loose_comparison(
                    arenas,
                    op,
                    l,
                    r,
                    &l_ty,
                    &r_ty,
                    self.expr_span(e.id),
                );
                self.check_constant_numeric_comparison(
                    arenas,
                    op,
                    l,
                    r,
                    &l_ty,
                    &r_ty,
                    self.expr_span(e.id),
                );
                self.check_strict_comparison_of_different_types(
                    arenas,
                    op,
                    l,
                    r,
                    &l_ty,
                    &r_ty,
                    self.expr_span(e.id),
                    is_exempt_last_condition,
                );
                // Mirrors real PHPStan's `LogicalXorConstantConditionRule` -
                // see `check_logical_xor`'s doc comment. `BinOp::And`/`Or`
                // are deliberately not handled here: they each have their
                // own, earlier-matching `ExprKind::Binary` arm above
                // (short-circuit-narrowing-aware), which this generic arm
                // never reaches for them.
                if *op == BinOp::LogicalXor {
                    self.check_logical_xor(l, r, locals);
                }
                if *op == BinOp::Pipe {
                    self.check_pipe_byref(r, locals);
                }
                // A reported invalid operation is the error type from here.
                if invalid {
                    Type::error()
                } else {
                    self.infer_binary(op, l, r, locals)
                }
            }
            ExprKind::Variable(name) => {
                if !locals.contains_var(name) && !is_superglobal(name) {
                    self.report(
                        format!("undefined variable ${name}"),
                        self.expr_span(e.id),
                        0,
                        "variable.undefined",
                    );
                }
                // See `Checker::probe_var_provenance`'s doc comment - the
                // same "stash it when the span matches" pattern
                // `probe_hit` uses.
                if self.probe_span.get() == Some(self.expr_span(e.id)) {
                    *self.probe_var_provenance.borrow_mut() = locals
                        .get_provenance_var(name)
                        .map(|p| p.iter().copied().collect());
                }
                let ty = locals
                    .get_var(name)
                    .cloned()
                    .unwrap_or_else(|| unassigned_variable_type(name));
                // A read's inferred type, for hover - see
                // `Checker::probe_hit_at`'s doc comment.
                self.probe_hit_at(self.expr_span(e.id), &ty);
                ty
            }
            ExprKind::Unary(
                op @ (UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec),
                inner,
            ) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_incdec_validity(op, inner, &inner_ty, self.expr_span(e.id));
                // A literal or range steps by one, so a counter widened to
                // `null|int` across a loop's passes still advances.
                //
                // Computed against `inner_ty` (whatever `inner` itself
                // just inferred as, any shape) rather than only a bare
                // `locals.get(name)` lookup, and rebound via the same
                // `bind_assignment_target` the `Assign`/`CompoundAssign`
                // arms use: `++$arr[$k]` incrementing an array element
                // must widen the array's tracked shape too, not just a
                // bare variable's. `bind_assignment_target` is a strict
                // superset of a plain `locals.declare` call for the
                // `Variable` case (identical rebind, plus the general
                // `key_of` purge every other assignment gets) and
                // a safe no-op for any `inner` shape it doesn't specially
                // handle (including `PropertyAccess` - a property's read
                // type is always its declared type, never a locally-
                // tracked literal default, so `$obj->prop++` has no
                // equivalent gap to fix here).
                let is_increment = matches!(op, UnaryOp::PreInc | UnaryOp::PostInc);
                let updated = if is_increment {
                    inner_ty.increment_type()
                } else {
                    inner_ty.decrement_type()
                };
                // `$x++`/`$x--` is real, visible rebinding syntax, the
                // same category as `$x += 1`.
                self.implicit_array_access_write(inner, self.expr_span(e.id), locals);
                self.bind_assignment_target(arenas, inner, updated.clone(), locals);
                // `++$x` is the value after the step, `$x++` the one before.
                if matches!(op, UnaryOp::PreInc | UnaryOp::PreDec) {
                    updated
                } else {
                    inner_ty
                }
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_non_boolean_condition(
                    inner,
                    &inner_ty,
                    "a negated boolean",
                    "booleanNot.exprNotBoolean",
                );
                // Mirrors real PHPStan's `BooleanNotConstantConditionRule` -
                // fires anywhere `!expr` appears, not just as a condition
                // (`should_skip_constant_condition` excludes `!` from
                // `if`/`while`/`ternary`'s own generic check specifically
                // so this standalone rule is the one place it's caught).
                // Negating an always-true inner expression makes the
                // whole `!expr` always false, and vice versa.
                // `is_exempt_last_condition` (only ever `true` when this
                // `!expr` is itself a genuine last-elseif-without-else
                // condition, set by `check_if`) mirrors real PHPStan's
                // `isLast` exemption for this rule too, unlike
                // `Ternary`'s.
                if let Some(inner_value) = self.inferred_constant_condition(inner, locals) {
                    let result_value = !inner_value;
                    if !(result_value && is_exempt_last_condition) {
                        self.report(
                            format!(
                                "Negated boolean expression is always {}.",
                                if result_value { "true" } else { "false" }
                            ),
                            self.expr_span(inner.id),
                            4,
                            if result_value {
                                "booleanNot.alwaysTrue"
                            } else {
                                "booleanNot.alwaysFalse"
                            },
                        );
                    }
                }
                Type::bool()
            }
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) - the
            // same implicit-numeric-coercion rule arithmetic operators
            // apply generally.
            ExprKind::Unary(op @ (UnaryOp::Neg | UnaryOp::Plus), inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_unary_operand_validity(op, &inner_ty, self.expr_span(inner.id));
                if inner_ty.is_string().yes() {
                    Type::int()
                } else if *op == UnaryOp::Neg {
                    inner_ty.negate()
                } else {
                    inner_ty
                }
            }
            ExprKind::Unary(op @ UnaryOp::BitNot, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_unary_operand_validity(op, &inner_ty, self.expr_span(inner.id));
                self.bitwise_not_type(&inner_ty)
            }
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("grouped expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("grouped expression belongs to its retained file owner");
                self.check_expr(inner, locals, label)
            }
            ExprKind::Cast(cast, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("cast expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("cast operand belongs to its retained file owner");
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_cast_validity(cast.kind(), &inner_ty, self.expr_span(inner.id));
                if cast.kind() == CastKind::Unset {
                    self.check_unset_cast_supported(self.expr_span(e.id));
                }
                if *cast == CastSpelling::Real {
                    self.check_real_cast_supported(self.expr_span(e.id));
                }
                if cast.kind() == CastKind::Void {
                    self.check_void_cast(self.expr_span(e.id), is_bare_statement);
                }
                self.check_deprecated_cast_spelling(*cast, self.expr_span(e.id));
                // An implicit `(string)` cast of an object triggers a
                // real `__toString()` call - see
                // `check_implicit_tostring_usage`'s doc comment.
                if cast.kind() == CastKind::String {
                    self.check_implicit_tostring_usage(&inner_ty, self.expr_span(e.id));
                    self.forget_what_an_operation_may_change(
                        &inner_ty,
                        is_this(self, inner),
                        Operation::String,
                        locals,
                    );
                }
                if self.settings.strict_rules.check_useless_casts && cast.kind() != CastKind::Void {
                    self.check_useless_cast(
                        &cast_result_type(cast.kind()),
                        &inner_ty,
                        self.expr_span(e.id),
                    );
                }
                self.cast_type(cast.kind(), &inner_ty)
            }
            // `null_probe_verdict` (nullability) for `isset`,
            // `empty_probe_verdict` (falsiness) for `empty`.
            ExprKind::Isset(operands) => {
                let Some(syntax_owner) = self.syntax_tree_owner() else {
                    return Type::bool();
                };
                let arenas = syntax_owner.arenas();
                for operand_id in arenas.expr_ids(operands).iter().copied() {
                    let Some(operand) = arenas.expr(operand_id) else {
                        continue;
                    };
                    self.check_existence_probe(
                        operand,
                        locals,
                        label,
                        ("isset", "in isset()", null_probe_verdict),
                    );
                    self.implicit_array_access_probe(
                        operand,
                        &[Access::Isset],
                        self.expr_span(e.id),
                        locals,
                    );
                }
                Type::bool()
            }
            ExprKind::Empty(operand) => {
                let Some(syntax_owner) = self.syntax_tree_owner() else {
                    return Type::bool();
                };
                let Some(operand) = syntax_owner.arenas().expr(*operand) else {
                    return Type::bool();
                };
                self.check_existence_probe(
                    operand,
                    locals,
                    label,
                    ("empty", "in empty()", empty_probe_verdict),
                );
                self.implicit_array_access_probe(
                    operand,
                    &[Access::Isset, Access::Read],
                    self.expr_span(e.id),
                    locals,
                );
                Type::bool()
            }
            // The operand must be string-coercible (`print.nonString`) and
            // triggers the implicit `__toString()` call `echo` does.
            ExprKind::Print(inner) => {
                let owner = self
                    .syntax_tree_owner()
                    .expect("print operand retains its file owner");
                let inner = owner
                    .arenas()
                    .expr(*inner)
                    .expect("print operand belongs to its retained file owner");
                let ty = self.check_expr(inner, locals, label);
                self.check_print_validity(&ty, self.expr_span(inner.id));
                self.check_implicit_tostring_usage(&ty, self.expr_span(inner.id));
                self.forget_what_an_operation_may_change(
                    &ty,
                    is_this(self, inner),
                    Operation::String,
                    locals,
                );
                Type::int_literal(1)
            }
            // Checked as the call to the `exit`/`die` function it is since
            // PHP 8.4 (`string|int $status = 0`), where the stubs have it.
            ExprKind::Exit(kind, args) => {
                if let Some(args) = args {
                    let syntax_owner = self
                        .syntax_tree_owner()
                        .expect("exit arguments retain their parsed file owner");
                    let arenas = syntax_owner.arenas();
                    let args = arenas.args(args.clone());
                    for (i, arg) in args.iter().enumerate() {
                        let Some(value) = arenas.expr(arg.value) else {
                            continue;
                        };
                        let ty = self.check_expr(value, locals, label);
                        // A status that is not an int is printed.
                        if i == 0 {
                            self.forget_what_an_operation_may_change(
                                &ty,
                                is_this(self, value),
                                Operation::String,
                                locals,
                            );
                        }
                    }
                    let name = kind.as_str();
                    if let Some(sig) = self.registry.lookup_function(name) {
                        self.check_args(CheckArgsCall {
                            callee_label: CalleeLabel::Text(name),
                            callee: Callee::Function(name),
                            sig,
                            args,
                            locals,
                            defining_class: None,
                            is_builtin: true,
                            span: self.expr_span(e.id),
                        });
                    }
                }
                Type::never()
            }
            // The evaluated code can define any variable.
            ExprKind::Eval(code) => {
                let owner = self
                    .syntax_tree_owner()
                    .expect("eval operand retains its file owner");
                let code = owner
                    .arenas()
                    .expr(*code)
                    .expect("eval operand belongs to its retained file owner");
                self.check_expr(code, locals, label);
                self.saw_dynamic_var_construct = true;
                locals.set_any_variable_may_exist(true);
                Type::mixed()
            }
            // The included file can define any variable.
            ExprKind::Include(kind, path) => {
                let owner = self
                    .syntax_tree_owner()
                    .expect("include path retains its file owner");
                let path = owner
                    .arenas()
                    .expr(*path)
                    .expect("include path belongs to its retained file owner");
                self.check_expr(path, locals, label);
                self.check_include_path(
                    crate::checker::includes::include_construct(*kind),
                    path.id,
                    locals,
                    self.expr_span(e.id),
                );
                locals.set_any_variable_may_exist(true);
                Type::mixed()
            }
            ExprKind::ShellExec(cmd) => {
                self.check_backtick_deprecation(self.expr_span(e.id));
                let owner = self
                    .syntax_tree_owner()
                    .expect("shell command retains its file owner");
                let cmd = owner
                    .arenas()
                    .expr(*cmd)
                    .expect("shell command belongs to its retained file owner");
                self.check_expr(cmd, locals, label);
                self.shell_exec_type()
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used
            // for a function that always throws or exits.
            ExprKind::Throw(inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("throw expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("thrown expression belongs to its retained file owner");
                // A `throw` expression (`$x ?? throw new Foo();`,
                // `default => throw ...`, an arrow function body, ...) is
                // the same language construct as a bare `throw $x;`
                // statement at runtime and accepts only a real Throwable
                // just as unconditionally (a real PHP fatal error
                // otherwise).
                let inner_ty = self.check_expr(inner, locals, label);
                if let Some(class) = self.thrown_value_not_throwable(&inner_ty) {
                    self.report(
                        format!("cannot throw {class}: it does not implement Throwable"),
                        self.expr_span(inner.id),
                        0,
                        "throw.notThrowable",
                    );
                }
                // Mirrors `check_throw`'s identical recording for the
                // statement form.
                self.current_function_explicit_throw_points
                    .push((inner_ty.clone(), self.expr_span(inner.id)));
                self.current_function_throw_points.push(inner_ty);
                self.push_exit_point(ExitKind::ThrowExpr, self.expr_span(e.id));
                Type::never()
            }
            ExprKind::YieldFrom(inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("yield-from expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("yield-from operand belongs to its retained file owner");
                let inner_ty = self.check_expr(inner, locals, label);
                self.forget_what_an_operation_may_change(
                    &inner_ty,
                    is_this(self, inner),
                    Operation::Iterate { keys: true },
                    locals,
                );
                let result_ty = generator_template_param(&inner_ty, self.registry, "TReturn");
                self.check_yield_from_compatibility(
                    &inner_ty,
                    &result_ty,
                    self.expr_span(e.id),
                    is_bare_statement,
                );
                result_ty
            }
            // Never given a stable nominal type by infer_expr either,
            // since multiple clone-with-overrides expressions could
            // produce runtime objects that don't share a common shape
            // this checker tracks - the clone target's own type is the
            // honest answer.
            ExprKind::Clone(inner, overrides) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("clone expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("clone target belongs to its retained file owner");
                let overrides = overrides.map(|id| {
                    arenas
                        .expr(id)
                        .expect("clone overrides belong to their retained file owner")
                });
                let inner_ty = self.check_expr(inner, locals, label);
                // `clone.nonObject` - shares `property.nonObject`'s own
                // `stdClass`-unaware classifier, not `method.nonObject`'s:
                // real PHPStan's `ObjectType::isCloneable()` (unlike
                // `canCallMethods()`) has no `stdClass` exception.
                if let Some(level) = self.failing_level(&inner_ty, 3, |m| {
                    self.relation().can_access_properties(m).no()
                }) {
                    self.report(
                        format!("Cannot clone non-object variable of type {inner_ty}."),
                        self.expr_span(inner.id),
                        level,
                        "clone.nonObject",
                    );
                }
                if let Some(o) = overrides {
                    self.check_expr(o, locals, label);
                }
                self.forget_what_an_operation_may_change(
                    &inner_ty,
                    is_this(self, inner),
                    Operation::Clone {
                        with: overrides.is_some(),
                    },
                    locals,
                );
                inner_ty
            }
            ExprKind::Yield(key, value) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("yield expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                if let Some(k) = key {
                    let k = arenas
                        .expr(*k)
                        .expect("yield key belongs to its retained file owner");
                    self.check_expr(k, locals, label);
                }
                if let Some(v) = value {
                    let v = arenas
                        .expr(*v)
                        .expect("yield value belongs to its retained file owner");
                    self.check_expr(v, locals, label);
                }
                self.current_declared_return_type
                    .as_ref()
                    .map(|t| generator_template_param(t, self.registry, "TSend"))
                    .unwrap_or(Type::mixed())
            }
            ExprKind::Instanceof(inner, target) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("instanceof expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("instanceof operand belongs to its retained file owner");
                self.check_expr(inner, locals, label);
                match target {
                    InstanceofTarget::Expr(inner_target) => {
                        let inner_target = arenas
                            .expr(*inner_target)
                            .expect("instanceof target belongs to its retained file owner");
                        self.check_expr(inner_target, locals, label);
                    }
                    // Mirrors real PHPStan's `ExistingClassInInstanceOfRule` -
                    // a dynamic target (`InstanceofTarget::Expr`, handled
                    // above) never names a concrete class to check
                    // existence for, only a literal one does.
                    // `resolve_name_expr` already handles `self`/`parent`/
                    // `static` (never flagged - see its doc comment), so
                    // anything that comes back as an `Object` here is a
                    // genuine bare class-name reference.
                    InstanceofTarget::Name(name_range) => {
                        let name = arenas
                            .qualified_name(name_range)
                            .expect("instanceof name belongs to its retained file owner");
                        let resolved = self.resolve_name_expr(name);
                        if let Some(class) = resolved.single_object_class()
                            && !resolved.is_enum_case()
                        {
                            if !self.class_name_exists(class) && !Self::class_guarded(class, locals)
                            {
                                self.report(
                                    format!("instanceof check target class {class} not found"),
                                    self.expr_span(e.id),
                                    0,
                                    "class.notFound",
                                );
                            } else {
                                // `class.nameCase` - `name` is the raw,
                                // as-written text, still a bare string
                                // right here in the AST.
                                self.check_class_name_case(name);
                                self.check_internal_instanceof(class, self.expr_span(e.id));
                                self.check_restricted_class_name_usage(
                                    class,
                                    "Instanceof references",
                                    self.expr_span(e.id),
                                    crate::checker::restricted_symbol_usage::ClassUsageLocation::Other,
                                );
                            }
                        }
                    }
                }
                self.check_impossible_instanceof(
                    arenas,
                    inner,
                    target,
                    self.expr_span(e.id),
                    locals,
                    is_exempt_last_condition,
                );
                self.infer_expr(e, locals)
            }
            ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("assignment expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let lhs = arenas
                    .expr(*lhs)
                    .expect("assignment target belongs to its retained file owner");
                let rhs = arenas
                    .expr(*rhs)
                    .expect("assigned value belongs to its retained file owner");
                self.check_nullsafe_assignment_target(lhs);
                self.check_assignable_target(lhs);
                if matches!(&e.kind, ExprKind::AssignRef(..)) {
                    self.check_nullsafe_byref_target(rhs);
                    self.check_property_assign_by_ref(rhs, locals);
                }
                // `$r = &$a[];` legally creates the new element it refers to.
                let rhs_ty = match &rhs.kind {
                    ExprKind::Index(base, None) if matches!(&e.kind, ExprKind::AssignRef(..)) => {
                        let base = arenas
                            .expr(*base)
                            .expect("empty index base belongs to its retained file owner");
                        let base_ty = self.check_expr(base, locals, label);
                        self.implicit_array_access_call(
                            &base_ty,
                            is_this(self, base),
                            Access::Read,
                            self.expr_span(e.id),
                            locals,
                        );
                        Type::mixed()
                    }
                    _ => self.check_expr(rhs, locals, label),
                };
                // `offsetAccess.nonArray` - `[$a, $b] = $x;`/
                // `list($a, $b) = $x;` destructuring a value confirmed to
                // never be array-like at all.
                if let ExprKind::Destructure(items, _) = &lhs.kind
                    && !self.relation().is_destructurable(&rhs_ty).no()
                {
                    self.check_destructuring_offsets(arenas, items, &rhs_ty, locals);
                }
                self.check_offset_assignment(lhs, &rhs_ty, false, locals);
                if matches!(&lhs.kind, ExprKind::Destructure(..))
                    && let Some(level) = self
                        .failing_level(&rhs_ty, 3, |m| self.relation().is_destructurable(m).no())
                {
                    self.report(
                        format!("Cannot use array destructuring on {rhs_ty}."),
                        self.expr_span(rhs.id),
                        level,
                        "offsetAccess.nonArray",
                    );
                }
                // `$this` is real PHP's reserved, implicitly-bound
                // reference to the current object - reassigning it is an
                // unconditional fatal error ("Cannot re-assign $this.").
                if matches!(&lhs.kind, ExprKind::Variable(v) if **v == *"this") {
                    self.report(
                        "cannot re-assign $this".to_string(),
                        self.expr_span(lhs.id),
                        0,
                        "assign.this",
                    );
                }
                // A bare variable (or a destructuring target inside one)
                // on the left is a definition site, not a read - checking
                // it here, before it's bound below, would make undefined-
                // variable detection flag every variable on the very
                // statement that defines it. PropertyAccess/Index targets
                // still need their *base* checked (`$obj->prop = ...`
                // reads $obj).
                self.check_assignment_target(arenas, lhs, locals, label);
                match &lhs.kind {
                    _ if matches!(&e.kind, ExprKind::AssignRef(..)) => {
                        self.implicit_reference(lhs, self.expr_span(e.id), locals);
                    }
                    // Destructuring an `ArrayAccess` object reads each
                    // key through `offsetGet`.
                    ExprKind::Destructure(items, _) => {
                        self.implicit_destructure(
                            arenas,
                            items,
                            &rhs_ty,
                            self.expr_span(e.id),
                            locals,
                        );
                    }
                    _ => self.implicit_array_access_write(lhs, self.expr_span(e.id), locals),
                }
                self.check_self_assignment(lhs, rhs);
                self.check_property_assignment(lhs, rhs, locals);
                // A bare by-ref-parameter variable directly assigned a
                // value its declared type can't hold. Also covers a
                // destructured by-ref target (`[$byRef] = $arr;`) - see
                // `check_by_ref_destructuring`'s own doc comment.
                self.check_param_out_assignment(lhs, &rhs_ty);
                self.check_by_ref_destructuring(arenas, lhs, &rhs_ty);
                let native = match (&e.kind, &lhs.kind) {
                    (ExprKind::Assign(..), ExprKind::Variable(name)) => Some((
                        name.clone(),
                        self.native_expr_type_id(arenas, rhs.id, locals),
                    )),
                    _ => None,
                };
                self.assigned_values.insert(
                    self.expr_span(e.id),
                    (rhs_ty.clone(), native.as_ref().map(|(_, n)| n.clone())),
                );
                self.bind_assignment_target(arenas, lhs, rhs_ty.clone(), locals);
                if let Some((name, native)) = native {
                    locals.set_native_var(&name, native);
                }
                self.record_condition_alias(lhs, rhs, locals);
                self.record_query_builder_alias(lhs, rhs, locals);
                // See `ContainerFieldFactsMap`'s own doc comment
                // (`checker.rs`) - `$x = $container[$dynamicKey];` is the
                // other real-world shape (alongside a `foreach` value
                // binding, handled in `checker/stmt/loops.rs`) that
                // transparently rebinds a fresh variable from a
                // container's own element without a literal key of its
                // own to narrow through - propagate any already-recorded
                // per-field facts onto `lhs`'s own synthetic keys the
                // same way. A *literal*-key read (`literal_string_key(idx)`
                // is `Some`) already gets its own, more precise answer
                // from the ordinary `key_of` fast path, so this is
                // deliberately gated to the dynamic-key case only.
                if let ExprKind::Variable(target_name) = &lhs.kind
                    && let Some(rhs_id) = arenas.peeled(rhs.id)
                    && let Some(rhs_node) = arenas.expr(rhs_id)
                    && let ExprKind::Index(base, Some(idx)) = &rhs_node.kind
                    && let Some(base_id) = arenas.peeled(*base)
                    && let Some(base_node) = arenas.expr(base_id)
                    && let ExprKind::Variable(container_name) = &base_node.kind
                    && let Some(idx) = arenas.expr(*idx)
                    && literal_string_key(arenas, idx).is_none()
                {
                    let target = locals.var_key(target_name);
                    let container = locals.var_key(container_name);
                    propagate_container_field_facts(locals, target, container);
                }
                rhs_ty
            }
            ExprKind::CompoundAssign(op, lhs, rhs) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("compound assignments retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let lhs = arenas
                    .expr(*lhs)
                    .expect("compound assignment target belongs to its retained file owner");
                let rhs = arenas
                    .expr(*rhs)
                    .expect("compound assigned value belongs to its retained file owner");
                self.check_nullsafe_assignment_target(lhs);
                self.check_assignable_target(lhs);
                // `$x ??= $default;` is defined as exactly
                // `$x = $x ?? $default;` - the same quiet-left-side
                // exemption as plain `??` applies to its own left side.
                let lhs_ty = if *op == BinOp::Coalesce {
                    if matches!(lhs.kind, ExprKind::Index(_, None)) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                "Cannot use [] for reading.".to_string(),
                                self.expr_span(lhs.id),
                                0,
                                "offsetAccess.noDim",
                            )
                            .non_ignorable(),
                        );
                    }
                    let ty = self.check_expr_quiet_root(arenas, lhs, locals, label);
                    self.check_pointless_null_probe(
                        lhs,
                        locals,
                        "nullCoalesce",
                        "on left side of ??=",
                        null_probe_verdict,
                    );
                    self.check_pointless_offset_or_property_probe(
                        arenas,
                        lhs,
                        locals,
                        "nullCoalesce",
                        "on left side of ??=",
                        null_probe_verdict,
                    );
                    self.implicit_array_access_probe(
                        lhs,
                        &[Access::Isset, Access::Read],
                        self.expr_span(e.id),
                        locals,
                    );
                    ty
                } else {
                    self.check_expr(lhs, locals, label)
                };
                let rhs_ty = self.check_expr(rhs, locals, label);
                self.binary_string_conversions(op, (lhs, &lhs_ty), (rhs, &rhs_ty), locals);
                self.implicit_array_access_write(lhs, self.expr_span(e.id), locals);
                self.check_offset_assignment(lhs, &rhs_ty, true, locals);
                if *op != BinOp::Coalesce {
                    let _ = self.check_binary_operand_validity(
                        op,
                        &lhs_ty,
                        &rhs_ty,
                        true,
                        self.expr_span(lhs.id),
                    );
                    self.check_division_by_zero(op, &rhs_ty, self.expr_span(rhs.id));
                }
                // `$x += 1`/`$x ??= ...` is a real rebind of `$x` -
                // routes through the same `bind_assignment_target` every
                // other assignment shape uses, so a property/index target
                // rebinds too, not just a bare local (a bare-`Variable`-
                // only check would leave e.g. `$this->watermark ??= ...`
                // never actually updating the property's tracked type).
                let ty = self.infer_binary(op, lhs, rhs, locals);
                // Same by-ref-parameter check as plain `=`, against the
                // compound operation's own real result.
                self.check_param_out_assignment(lhs, &ty);
                let native = match &lhs.kind {
                    ExprKind::Variable(name) => {
                        Some((name.clone(), self.native_expr_type_id(arenas, e.id, locals)))
                    }
                    _ => None,
                };
                self.bind_assignment_target(arenas, lhs, ty, locals);
                if let Some((name, native)) = native {
                    locals.set_native_var(&name, native);
                }
                Type::mixed()
            }
            ExprKind::Ternary(cond, then_, else_) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("ternary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let cond = arenas
                    .expr(*cond)
                    .expect("ternary condition belongs to its retained file owner");
                let then_ = then_.as_ref().map(|id| {
                    arenas
                        .expr(*id)
                        .expect("ternary branch belongs to its retained file owner")
                });
                let else_ = arenas
                    .expr(*else_)
                    .expect("ternary else branch belongs to its retained file owner");
                self.in_condition_position = true;
                let cond_ty = self.check_expr(cond, locals, label);
                // The short/"elvis" `$cond ?: $else` form is exempted,
                // matching real `BooleanInTernaryOperatorRule` exactly -
                // it's a coalescing idiom (`$x ?: 'default'`), not a
                // real boolean condition.
                if then_.is_some() {
                    self.check_non_boolean_condition(
                        cond,
                        &cond_ty,
                        "a ternary operator condition",
                        "ternary.condNotBoolean",
                    );
                }
                // Mirrors real PHPStan's `TernaryOperatorConstantConditionRule`,
                // same design as `If`/`While`/`DoWhile` (`check_if`) - no
                // exclusions beyond `inferred_constant_condition`'s shared
                // ones; this rule has none of `BooleanAnd`/`BooleanOr`'s
                // `isLast`/first-level-statement complexity.
                if let Some(value) = self.inferred_constant_condition(cond, locals) {
                    self.report(
                        format!(
                            "Ternary operator condition is always {}.",
                            if value { "true" } else { "false" }
                        ),
                        self.expr_span(cond.id),
                        4,
                        if value {
                            "ternary.alwaysTrue"
                        } else {
                            "ternary.alwaysFalse"
                        },
                    );
                }
                // Same clone-and-narrow shape as `StmtKind::If` - a
                // ternary is just as branch-based, and without this,
                // `$x === null ? "default" : $x` never benefits from the
                // narrowing an equivalent `if` already gets.
                //
                // Deliberately does NOT write `merge_scopes(&then_locals,
                // &else_locals)` back into the real `locals` afterward,
                // unlike `StmtKind::If`: a ternary is an expression, not a
                // control-flow statement. Once it finishes evaluating,
                // only its value matters to whatever contains it, never
                // which hypothetical branch's narrowing applied - the
                // outer `locals` should come out exactly as they went in.
                // Writing the merge back corrupted a variable's tracked
                // type across a second, separate evaluation of the same
                // ternary tree (`check_expr` and `infer_expr` diverging on
                // the identical expression, since `infer_expr` only ever
                // receives `&Locals`, never `&mut`).
                let mut then_locals = locals.clone();
                self.apply_narrowing(cond, &mut then_locals, true);
                let then_ty = then_
                    .as_ref()
                    .map(|t| self.check_expr(t, &mut then_locals, label));
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.check_expr(else_, &mut else_locals, label);
                // `Type::union`, not a raw union -
                // either branch's type is commonly already a `Union`
                // itself (e.g. a stub-declared `resource|false`), and a
                // bare `vec![then_ty, else_ty]` nests it unflattened
                // instead of merging its members in: a later `!== false`
                // narrowing filters only the outer union's members, so it
                // can strip a bare `false` sitting alongside the nested
                // union but can't reach inside it to strip its own
                // `false` member.
                match then_ty {
                    Some(then_ty) => self.relation().union(then_ty, else_ty),
                    None => Type::mixed(),
                }
            }
            // Only ever a target, checked as one where it's assigned to.
            ExprKind::Destructure(..) => Type::mixed(),
            ExprKind::ArrayLiteral(items) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("array literals retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let items = arenas.array_items(items.clone());
                for item in items {
                    if let Some(k) = item.key.map(|id| {
                        arenas
                            .expr(id)
                            .expect("array item key belongs to its retained file owner")
                    }) {
                        self.check_expr(k, locals, label);
                        self.check_array_item_key_type(k, locals);
                    }
                    let value = arenas
                        .expr(item.value)
                        .expect("array item value belongs to its retained file owner");
                    // `[&$x]` makes `$x` a reference, creating it as `null`
                    // when it doesn't exist.
                    if item.by_ref
                        && let ExprKind::Variable(name) = &value.kind
                        && !locals.contains_var(name)
                    {
                        locals.declare_var(name, Type::null(), self.expr_span(value.id));
                    }
                    let value_ty = self.check_expr(value, locals, label);
                    // `arrayUnpacking.nonIterable` - `[...$x]` where
                    // `$x` is confirmed to never be iterable at all.
                    // Shares `foreach.nonIterable`'s own classifier.
                    if item.spread
                        && let Some(level) = self
                            .failing_level(&value_ty, 3, |m| self.relation().is_iterable(m).no())
                    {
                        self.report(
                            format!("Only iterables can be unpacked, {value_ty} given."),
                            self.expr_span(value.id),
                            level,
                            "arrayUnpacking.nonIterable",
                        );
                    }
                    if item.spread {
                        self.check_unpacked_string_keys(&value_ty, self.expr_span(value.id));
                        self.forget_what_an_operation_may_change(
                            &value_ty,
                            is_this(self, value),
                            Operation::Iterate { keys: true },
                            locals,
                        );
                    }
                }
                self.check_array_literal_duplicate_keys(arenas, items, locals);
                self.infer_array_literal_type(arenas, items, locals)
            }
            ExprKind::Index(base, idx) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("index expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let base = arenas
                    .expr(*base)
                    .expect("index base belongs to its retained file owner");
                let idx = idx.as_ref().map(|id| {
                    arenas
                        .expr(*id)
                        .expect("index expression belongs to its retained file owner")
                });
                let base_ty = self.check_expr(base, locals, label);
                // A nullsafe access earlier in the chain that met `null`
                // skipped this one.
                let base_ty = if self
                    .syntax_arenas()
                    .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, base))
                {
                    self.chain_link_type(base, locals)
                } else {
                    base_ty
                };
                match idx {
                    Some(i) => {
                        self.check_expr(i, locals, label);
                        self.check_offset_accessibility(&base_ty, i);
                        self.check_offset_existence(e, base, i, locals);
                        self.check_dim_fetch_key_type(base, i, locals);
                        let key_ty = match crate::checker::literal_shape_key(arenas, i) {
                            Some(key) => key.to_type(),
                            None => self.infer_expr(i, locals),
                        };
                        self.check_array_access_offset(
                            &base_ty,
                            "offsetGet",
                            &key_ty,
                            self.expr_span(i.id),
                        );
                        self.implicit_array_access_call(
                            &base_ty,
                            is_this(self, base),
                            Access::Read,
                            self.expr_span(e.id),
                            locals,
                        );
                        // The `Index` counterpart of `PropertyAccess`'s
                        // "check the narrowed key first" handling above -
                        // without this, `check_expr` (unlike `infer_expr`,
                        // which already does this for `Index`) would
                        // re-derive the type from the base's declared/
                        // inferred shape, discarding whatever
                        // `apply_narrowing` had written under this exact
                        // `key_of`. A bare `return $x['k'];` right
                        // after narrowing `$x['k']` would otherwise see
                        // the un-narrowed type.
                        if locals.may_have_member_narrowing()
                            && let Some(key) = self.find_expr_key(locals, e)
                            && let Some(narrowed) = locals.get(key)
                        {
                            return narrowed.clone();
                        }
                        // The same read `chain_link_type` gives.
                        self.offset_read_type(&base_ty, &key_ty)
                    }
                    // `OffsetAccessWithoutDimForReadingRule` - every
                    // write target routes around this arm, and a
                    // by-reference argument slot is recorded ahead of it.
                    None => {
                        if self
                            .by_ref_empty_dim_args
                            .borrow()
                            .contains(&self.expr_span(e.id))
                        {
                            return Type::mixed();
                        }
                        self.report_builder(
                            DiagnosticBuilder::new(
                                "Cannot use [] for reading.".to_string(),
                                self.expr_span(e.id),
                                0,
                                "offsetAccess.noDim",
                            )
                            .non_ignorable(),
                        );
                        Type::mixed()
                    }
                }
            }
            ExprKind::New(class_expr, args) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("instantiations retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let class_expr = arenas
                    .expr(*class_expr)
                    .expect("instantiation target belongs to its retained file owner");
                let args = arenas.args(args.clone());
                // `new Foo()` never carries known type arguments itself -
                // real PHP's constructor syntax has no way to write
                // `new Foo<Bar>()` at all - but `infer_expr`'s `New` arm
                // binds them from the constructor's own arguments instead,
                // the way real PHPStan does, when `Foo` is itself generic
                // (`@template` on the class).
                let new_ty = self.infer_expr(e, locals);
                let resolved_class = new_ty.single_object_class().cloned();
                // Rebind to the registry's canonical-case spelling up
                // front, once, rather than at each individual lookup
                // below - `lookup_method`'s ancestor-walk starts from an
                // exact-case `self.classes.get(&current)` with no
                // case-insensitive fallback, so a differently-cased
                // `new foo()` of a real `class Foo {}` would otherwise
                // resolve no constructor at all.
                let resolved_class = resolved_class.map(|class| {
                    self.registry
                        .lookup_class(&class)
                        .map(|(canonical, _)| canonical.to_string())
                        .unwrap_or(class.to_string())
                });
                // A dynamic target (`new $className()`) or an unresolved
                // pseudo-type (`new parent()` with no parent class) has no
                // concrete name to check - only a plain `new Name()` (or
                // `self`/`static` resolving to the current class) reaches
                // here with `Some`.
                let ctor_sig = resolved_class
                    .as_deref()
                    .and_then(|class| self.registry.lookup_method(class, "__construct"));
                let resolved = ctor_sig
                    .map(|s| resolve_call_args(arenas, args, s.params()))
                    .unwrap_or_default();
                self.maybe_record_param_name_hints(ctor_sig, args);
                self.predefine_byref_or_unknown_args(
                    arenas, None, ctor_sig, args, &resolved, locals,
                );
                for (i, a) in args.iter().enumerate() {
                    let argument = arenas
                        .expr(a.value)
                        .expect("constructor argument belongs to its retained file owner");
                    let peeled_argument = arenas
                        .peeled(a.value)
                        .and_then(|id| arenas.expr(id))
                        .expect("constructor argument belongs to its retained file owner");
                    // `argument.unpackNonIterable` - see `ExprKind::Call`'s
                    // identical check above.
                    if a.spread {
                        let spread_ty = self.infer_expr(argument, locals);
                        if let Some(level) = self
                            .failing_level(&spread_ty, 3, |m| self.relation().is_iterable(m).no())
                        {
                            self.report(
                                format!("Only iterables can be unpacked, {spread_ty} given."),
                                self.expr_span(argument.id),
                                level,
                                "argument.unpackNonIterable",
                            );
                        }
                    }
                    let closure_this = ctor_sig
                        .and_then(|s| s.params().get(i))
                        .and_then(|p| p.closure_this_type().clone());
                    if let Some(this_ty) = closure_this
                        && matches!(
                            &peeled_argument.kind,
                            ExprKind::Closure(_) | ExprKind::ArrowFn(_)
                        )
                    {
                        // `@param-closure-this` - see `ExprKind::Call`'s
                        // identical branch.
                        self.check_closure_literal_with_this_and_scope(
                            peeled_argument,
                            locals,
                            label,
                            this_ty,
                            None,
                        );
                    } else {
                        self.check_expr(argument, locals, label);
                    }
                }
                if let Some(class) = &resolved_class {
                    // `resolved_class` is already the fully-qualified
                    // name (`infer_expr`'s New arm resolves it via
                    // `name_ctx`), so every miss here is a real
                    // undefined-class finding - no "can't resolve a
                    // namespaced name" case left.
                    // `class.nameCase` - the raw, as-written name is
                    // still available directly on `class_expr`'s
                    // `ExprKind::Name` (a dynamic target, `new $var()`,
                    // never reaches this arm - `resolved_class` above is
                    // `None` for it). `new`'s class name is still a bare
                    // source string right here in the AST, exactly like
                    // `instanceof`/`catch` below.
                    if let ExprKind::Name(raw) = &class_expr.kind
                        && let Some(raw) = arenas.qualified_name(raw)
                    {
                        self.check_class_name_case(raw);
                    }
                    // Called before the borrow just below (this check
                    // does its own, separate registry lookup) since it
                    // needs `&mut self`.
                    self.check_internal_instantiation(class, self.expr_span(e.id));
                    self.check_restricted_class_name_usage(
                        class,
                        "Instantiation of",
                        self.expr_span(e.id),
                        crate::checker::restricted_symbol_usage::ClassUsageLocation::Other,
                    );
                    self.check_deprecated_instantiation(class, self.expr_span(e.id));
                    self.record_instantiation_usage_token(class, self.expr_span(class_expr.id));
                    // `DateInterval`'s duration-string constructor
                    // validation - independent of whether the class is
                    // otherwise instantiable, so this runs unconditionally
                    // before that dispatch.
                    self.check_date_interval_constructor(
                        arenas,
                        class,
                        args,
                        self.expr_span(e.id),
                        locals,
                    );
                    // `DateTime`/`DateTimeImmutable`'s own constructor-
                    // string validation - same shape.
                    self.check_date_time_constructor(
                        arenas,
                        class,
                        args,
                        self.expr_span(e.id),
                        locals,
                    );
                    // `SimpleXMLElement`'s construction well-formedness
                    // validation - same shape.
                    self.check_simplexml_element_constructor(
                        arenas,
                        class,
                        args,
                        self.expr_span(e.id),
                    );
                    // This constructor's own throw contribution.
                    let dynamic_throw = self.dynamic_new_throw_type(arenas, class, args);
                    self.record_call_throw_point(dynamic_throw, ctor_sig, self.expr_span(e.id));
                    let (is_name, is_static) = match &class_expr.kind {
                        ExprKind::Name(raw) => arenas
                            .qualified_name(raw)
                            .map(|raw| (true, raw.eq_ignore_ascii_case("static")))
                            .unwrap_or((false, false)),
                        _ => (false, false),
                    };
                    if self.registry.lookup_class(class).is_some() {
                        if self.check_instantiated_class(
                            class,
                            is_name,
                            is_static,
                            !args.is_empty(),
                            self.expr_span(e.id),
                        ) && let Some(sig) = ctor_sig
                        {
                            // See the identical comment on the plain-
                            // function Call arm above - `check_args` is
                            // spread-aware, so it's always worth calling.
                            self.check_args(CheckArgsCall {
                                callee_label: CalleeLabel::Constructor(class),
                                callee: Callee::Constructor(class),
                                sig,
                                args,
                                locals,
                                defining_class: Some(class),
                                is_builtin: self.registry.is_builtin_class(class),
                                span: self.expr_span(e.id),
                            });
                            self.apply_param_out_types(sig, args, locals);
                        }
                    } else if !Self::class_guarded(class, locals) {
                        self.report(
                            format!("instantiated class {class} not found"),
                            self.expr_span(e.id),
                            0,
                            "class.notFound",
                        );
                    }
                } else {
                    match &class_expr.kind {
                        ExprKind::Name(raw) => {
                            if let Some(raw) = arenas.qualified_name(raw) {
                                self.check_new_pseudo_target(raw, self.expr_span(class_expr.id));
                            }
                        }
                        _ => {
                            let target_ty = self.infer_expr(class_expr, locals);
                            self.check_new_on_non_class_name(
                                &target_ty,
                                self.expr_span(class_expr.id),
                            );
                        }
                    }
                }
                new_ty
            }
            ExprKind::NewAnonymousClass(class_id) => {
                let Some(syntax_owner) = self.syntax_tree_owner() else {
                    return Type::mixed();
                };
                let arenas = syntax_owner.arenas();
                let Some(class) = arenas.anonymous_class(*class_id) else {
                    return Type::mixed();
                };
                let Some(def) = arenas.class(class.def) else {
                    return Type::mixed();
                };
                self.check_enum_cases_outside_enum(def.enum_cases(arenas));
                let ctor_params: Vec<_> = def
                    .constructor(arenas)
                    .map(|m| arenas.params(m.function.params.clone()))
                    .into_iter()
                    .flatten()
                    .map(|param| (param.name.to_string(), param.by_ref, param.variadic))
                    .collect();
                for (i, a) in arenas.args(class.args.clone()).iter().enumerate() {
                    let Some(value) = arenas.expr(a.value) else {
                        continue;
                    };
                    let param = match &a.name {
                        Some(name) => ctor_params.iter().find(|p| p.0 == **name),
                        None => ctor_params
                            .get(i)
                            .or_else(|| ctor_params.last().filter(|p| p.2)),
                    };
                    if matches!(value.kind, ExprKind::Index(_, None)) && param.is_some_and(|p| p.1)
                    {
                        self.by_ref_empty_dim_args
                            .borrow_mut()
                            .insert(self.expr_span(value.id));
                    }
                    self.check_expr(value, locals, label);
                }
                let name = arenas.anonymous_class_name(*class_id);
                if let Some(name) = name {
                    self.check_anonymous_class(name, def, self.expr_span(e.id));
                }
                anonymous_class_type(name)
            }
            ExprKind::StaticCall(target, method, args) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("static-call arguments retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let args = arenas.args(args.clone());
                if let Some(property) = target.parent_hook_property(arenas) {
                    for a in args {
                        if let Some(value) = arenas.expr(a.value) {
                            self.check_expr(value, locals, label);
                        }
                    }
                    return self.parent_hook_call_type(property, method);
                }
                // Mirrors ExprKind::MethodCall's own structure exactly -
                // resolve first (self/parent/static-aware, or - for a
                // genuinely dynamic receiver - via its own checked,
                // inferred type), predefine byref/unknown args using the
                // real signature before checking them, then the
                // existence/arg-type check.
                let resolved_class = self.resolve_static_target_class(target, locals, label);
                // `_with_owner`, not the plain `lookup_method` - the
                // visibility check needs the *declaring* class, which can
                // differ from `resolved_class` for an inherited method.
                let sig_with_owner = resolved_class
                    .as_ref()
                    .and_then(|class| self.static_call_method(target, class, method));
                let sig = sig_with_owner.as_ref().map(|(_, s)| s.as_ref());
                // The `::cases()` escape hatch - see
                // `Checker::escaped_enums`'s own doc comment. `owner`
                // (not `resolved_class`) is the registry's own
                // canonically-declared name, which matters here: a
                // differently-cased call site (`SUIT::cases()`) would
                // silently fail to match `self.registry.classes()` if
                // keyed by `resolved_class` instead.
                if method.eq_ignore_ascii_case("cases")
                    && let Some((owner, _)) = &sig_with_owner
                    && self
                        .registry
                        .class_by_name(owner.as_ref())
                        .is_some_and(|c| c.kind() == ClassKind::Enum)
                {
                    self.record_enum_escape(owner);
                }
                // This static call's own throw contribution.
                if let Some(class) = &resolved_class {
                    let dynamic_throw = self.dynamic_method_throw_type(class, method, args);
                    self.record_call_throw_point(dynamic_throw, sig, self.expr_span(e.id));
                } else {
                    self.current_function_has_unannotated_call = true;
                }
                let resolved = sig
                    .map(|s| resolve_call_args(arenas, args, s.params()))
                    .unwrap_or_default();
                self.maybe_record_param_name_hints(sig, args);
                self.predefine_byref_or_unknown_args(arenas, None, sig, args, &resolved, locals);
                // `Closure::bind($closure, $newThis, $scope)` makes
                // `$this` (and, for private/protected access, the
                // calling scope) available inside a closure that
                // otherwise wouldn't have it - real, idiomatic PHP for
                // reaching into an object's state from outside its class.
                // Only special-cased when the first argument is literally
                // a closure expression right here (not some arbitrary
                // `Closure`-typed value) - the closure
                // body needs to still be reachable as an `ExprNode` to
                // re-check with the rebound `$this`.
                if resolved_class.as_deref() == Some("Closure")
                    && method == "bind"
                    && !args.is_empty()
                    && arenas
                        .peeled(args[0].value)
                        .and_then(|id| arenas.expr(id))
                        .is_some_and(|value| {
                            matches!(
                                value.kind,
                                ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                            )
                        })
                {
                    let this_ty = args
                        .get(1)
                        .and_then(|a| arenas.expr(a.value))
                        .map(|value| self.infer_expr(value, locals))
                        .unwrap_or(Type::mixed());
                    let scope_class = args
                        .get(2)
                        .and_then(|a| arenas.expr(a.value))
                        .and_then(|value| self.literal_class_name_from_expr(value));
                    let closure = arenas
                        .peeled(args[0].value)
                        .and_then(|id| arenas.expr(id))
                        .expect("Closure::bind literal belongs to its parsed file owner");
                    self.check_closure_literal_with_this_and_scope(
                        closure,
                        locals,
                        label,
                        this_ty,
                        scope_class,
                    );
                    for a in &args[1..] {
                        let Some(value) = arenas.expr(a.value) else {
                            continue;
                        };
                        self.check_expr(value, locals, label);
                        // `argument.unpackNonIterable` - see
                        // `ExprKind::Call`'s identical check above.
                        if a.spread {
                            let spread_ty = self.infer_expr(value, locals);
                            if let Some(level) = self.failing_level(&spread_ty, 3, |m| {
                                self.relation().is_iterable(m).no()
                            }) {
                                self.report(
                                    format!("Only iterables can be unpacked, {spread_ty} given."),
                                    self.expr_span(value.id),
                                    level,
                                    "argument.unpackNonIterable",
                                );
                            }
                        }
                    }
                } else {
                    for (i, a) in args.iter().enumerate() {
                        let Some(value) = arenas.expr(a.value) else {
                            continue;
                        };
                        // `argument.unpackNonIterable` - see
                        // `ExprKind::Call`'s identical check above.
                        if a.spread {
                            let spread_ty = self.infer_expr(value, locals);
                            if let Some(level) = self.failing_level(&spread_ty, 3, |m| {
                                self.relation().is_iterable(m).no()
                            }) {
                                self.report(
                                    format!("Only iterables can be unpacked, {spread_ty} given."),
                                    self.expr_span(value.id),
                                    level,
                                    "argument.unpackNonIterable",
                                );
                            }
                        }
                        let closure_this = sig
                            .and_then(|s| s.params().get(i))
                            .and_then(|p| p.closure_this_type().clone());
                        if let Some(this_ty) = closure_this
                            && let Some(closure_id) = arenas.peeled(a.value)
                            && let Some(closure) = arenas.expr(closure_id)
                            && matches!(
                                closure.kind,
                                ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                            )
                        {
                            // `@param-closure-this` - see
                            // `ExprKind::Call`'s identical branch.
                            self.check_closure_literal_with_this_and_scope(
                                closure, locals, label, this_ty, None,
                            );
                        } else {
                            self.check_expr(value, locals, label);
                        }
                    }
                }
                if let Some(class) = &resolved_class {
                    // `class.nameCase` - `class_name` is the raw,
                    // as-written text (`resolve_static_class_name` above
                    // already resolved `self`/`parent`/`static`/a
                    // variable target to `class`, but never touches
                    // `class_name` itself; `check_class_name_case`'s
                    // registry lookup harmlessly no-ops for a pseudo-
                    // keyword like `self` or a genuinely undefined class).
                    if let Some(raw) = target.as_name() {
                        self.check_class_name_case(raw);
                    }
                    if let Some(sig) = sig {
                        if let Some((owner, _)) = &sig_with_owner {
                            self.check_member_visibility(MemberVisibilityCheck {
                                member: MemberLabel {
                                    kind: MemberKind::StaticMethod,
                                    owner,
                                    name: method,
                                },
                                visibility: &sig.visibility(),
                                span: self.expr_span(e.id),
                            });
                            self.check_internal_method_call(
                                owner,
                                method,
                                sig,
                                true,
                                self.expr_span(e.id),
                            );
                            self.check_restricted_method_call(
                                owner,
                                method,
                                true,
                                args,
                                locals,
                                self.expr_span(e.id),
                            );
                            self.check_deprecated_method_call(
                                owner,
                                method,
                                sig,
                                true,
                                self.expr_span(e.id),
                            );
                            self.record_member_usage_token(
                                UsageTokenKind::Method,
                                owner,
                                sig.deprecated().is_some(),
                                sig.is_internal(),
                                self.expr_span(e.id),
                            );
                            self.record_method_ref(self.expr_span(e.id), owner, method);
                            self.check_static_call_context(
                                target,
                                class,
                                owner,
                                method,
                                self.expr_span(e.id),
                            );
                            // Mirrors real PHPStan's
                            // `CallPrivateMethodThroughStaticRule` - same
                            // late-static-binding unsafety as the
                            // identical class-constant/static-property
                            // rules.
                            if target.as_name().is_some_and(|n| n.is_keyword("static"))
                                && sig.visibility() == Visibility::Private
                            {
                                self.check_static_private_method_access(
                                    owner,
                                    method,
                                    self.expr_span(e.id),
                                );
                            }
                        }
                        self.check_call_site_method_name_case(
                            class,
                            method,
                            self.expr_span(e.id),
                            Span {
                                start: self.static_target_end(target),
                                end: self.expr_span(e.id).end,
                            },
                        );
                        // See the identical comment on the plain-
                        // function Call arm above - `check_args` is
                        // spread-aware, so it's always worth calling.
                        let owner = sig_with_owner
                            .as_ref()
                            .map_or_else(|| class.to_string(), |(o, _)| o.to_string());
                        let declared = self.registry.declared_method_name(&owner, method);
                        let is_static = self
                            .registry
                            .method_is_static(owner.as_str(), declared.as_str());
                        self.check_args(CheckArgsCall {
                            callee_label: CalleeLabel::Method { class, method },
                            callee: Callee::StaticMethod {
                                class: &owner,
                                name: &declared,
                                is_static,
                            },
                            sig,
                            args,
                            locals,
                            defining_class: Some(class),
                            is_builtin: self.registry.is_builtin_class(&owner),
                            span: self.expr_span(e.id),
                        });
                        self.apply_param_out_types(sig, args, locals);
                    } else if self.registry.has_class(class.as_str())
                        && self.registry.lookup_method(class, "__callStatic").is_none()
                    {
                        // Same leniency as MethodCall's own undefined-
                        // method check: only fires once the class itself
                        // is confirmed known, and __callStatic (the
                        // static equivalent of __call) suppresses it
                        // entirely, matching real PHPStan.
                        self.report(
                            format!("call to undefined static method {class}::{method}()"),
                            self.expr_span(e.id),
                            0,
                            "staticMethod.notFound",
                        );
                    } else if !self.registry.has_class(class.as_str())
                        && !Self::class_guarded(class, locals)
                    {
                        // Mirrors real PHPStan's `StaticMethodCallCheck`'s
                        // "class doesn't exist at all" half - neither
                        // branch above fires for a genuinely undefined
                        // class (no `sig` to check args against, and the
                        // `contains_key` guard on the undefined-method
                        // branch correctly excludes it too).
                        self.report(
                            format!("static method call target class {class} not found"),
                            self.expr_span(e.id),
                            0,
                            "class.notFound",
                        );
                    }
                } else {
                    self.check_unresolved_static_call_target(
                        target,
                        method,
                        locals,
                        self.expr_span(e.id),
                    );
                }
                match (&resolved_class, sig) {
                    (Some(class), Some(s)) => {
                        let resolved_ty = self.infer_call_return_type(s, &resolved, locals);
                        let resolved_ty = self.narrow_enum_from_call(
                            arenas,
                            class,
                            method,
                            args,
                            resolved_ty,
                            locals,
                        );
                        // The `::from()`/`::tryFrom()` literal-argument
                        // signal - see `project::whole_program::
                        // dead_enum_cases`'s own doc comment. Reads the
                        // type `narrow_enum_from_call` just computed
                        // rather than threading a new span parameter
                        // through it.
                        if let Some((case_class, case)) = resolved_ty.as_enum_case() {
                            self.record_member_ref(self.expr_span(e.id), case_class, case);
                        }
                        match self.class_argument_return(arenas, class, method, args, locals) {
                            Some(ty) => ty,
                            None => self.resolve_static_call_return(&resolved_ty, target, class),
                        }
                    }
                    _ => Type::mixed(),
                }
            }
            ExprKind::DynamicClassConstFetch(target, name) => {
                // `$obj::{$name}`: the receiver is checked once, whatever
                // names the fetch may resolve to.
                let receiver = match target {
                    StaticTarget::Name(..) => None,
                    StaticTarget::Expr(receiver) => {
                        let syntax_owner = self
                            .syntax_tree_owner()
                            .expect("static target belongs to its retained file owner");
                        let receiver = syntax_owner
                            .arenas()
                            .expr(*receiver)
                            .expect("static target receiver belongs to its retained file owner");
                        Some(receiver_class(self.check_expr(receiver, locals, label)))
                    }
                };
                let owner = self
                    .syntax_tree_owner()
                    .expect("dynamic constant name retains its file owner");
                let name = owner
                    .arenas()
                    .expr(*name)
                    .expect("dynamic constant name belongs to its retained file owner");
                let name_ty = self.check_expr(name, locals, label);
                self.check_dynamic_class_constant_fetch(&name_ty, self.expr_span(e.id));
                let literal_names: Option<Vec<String>> = {
                    let names = name_ty.constant_strings_text();
                    (!names.is_empty()).then(|| names.iter().map(|n| n.to_string()).collect())
                };
                match (literal_names, receiver) {
                    (Some(names), None) if !names.is_empty() => {
                        let fetched: Vec<Type> = names
                            .into_iter()
                            .map(|n| {
                                let arena = self.synthetic_expr_arena();
                                let fetch =
                                    self.constant_fetch(&arena, target, n, self.expr_span(e.id));
                                self.check_expr(fetch, locals, label)
                            })
                            .collect();
                        self.relation().union_of(fetched)
                    }
                    (Some(names), Some(Some(class))) if !names.is_empty() => names
                        .into_iter()
                        .map(|n| {
                            if n == "class" {
                                Type::class_string(Type::object(class.clone().into(), Vec::new()))
                            } else {
                                self.static_member_of_class(e, &class, None, &n, false)
                            }
                        })
                        .collect::<Vec<_>>()
                        .into_iter()
                        .union_all_with(&self.relation())
                        .unwrap_or(Type::mixed()),
                    (_, Some(Some(class))) => {
                        if self.collect_member_refs {
                            self.record_member_class_escape(&class);
                        }
                        Type::mixed()
                    }
                    _ => {
                        self.record_dynamic_static_member_escape(target);
                        Type::mixed()
                    }
                }
            }
            ExprKind::ClassNameFetch(target) => {
                // `Foo::class` / `$obj::class` is a string naming the
                // class, not a member.
                match self.resolve_static_target_class(target, locals, label) {
                    Some(class) => Type::class_string(Type::object(class.into(), Vec::new())),
                    None => Type::string(),
                }
            }
            ExprKind::DynamicStaticPropertyFetch(target, name) => {
                // A computed static property name (`Foo::${$e}`/
                // `Foo::$$e`) has no statically known name to check.
                let class = self.resolve_static_target_class(target, locals, label);
                let owner = self
                    .syntax_tree_owner()
                    .expect("dynamic property name retains its file owner");
                let name = owner
                    .arenas()
                    .expr(*name)
                    .expect("dynamic property name belongs to its retained file owner");
                let name_ty = self.check_expr(name, locals, label);
                self.forget_what_an_operation_may_change(
                    &name_ty,
                    is_this(self, name),
                    Operation::String,
                    locals,
                );
                self.record_dynamic_static_member_escape(target);
                if self.collect_member_refs
                    && let Some(class) = class
                {
                    self.record_member_class_escape(&class);
                }
                self.record_call_throw_point(None, None, self.expr_span(e.id));
                Type::mixed()
            }
            ExprKind::ClassConstFetch(target, name) => {
                self.check_static_member_fetch(e, target, name, false, locals, label)
            }
            ExprKind::StaticPropertyFetch(target, name) => {
                self.check_static_member_fetch(e, target, name, true, locals, label)
            }
            ExprKind::DynamicVariable(name_expr) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("dynamic variables retain their parsed file owner");
                let name_expr = syntax_owner
                    .arenas()
                    .expr(*name_expr)
                    .expect("dynamic variable name belongs to its retained file owner");
                let name_ty = self.check_expr(name_expr, locals, label);
                self.forget_what_an_operation_may_change(
                    &name_ty,
                    is_this(self, name_expr),
                    Operation::String,
                    locals,
                );
                Type::mixed()
            }
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("property accesses retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let base = arenas
                    .expr(*base)
                    .expect("property receiver belongs to its retained file owner");
                // Unlike a write (check_property_assignment, and the
                // Assign/destructuring arms below that
                // deliberately route a PropertyAccess target through only
                // its base, not here) - a genuinely undeclared property
                // creates nothing on read, it just returns null with a
                // warning in real PHP (unless __get exists). So there's
                // no "could be a legitimate dynamic property" case to
                // stay lenient for here, the way there is on write. Same
                // `Union`/`Intersection` handling as `MethodCall` above,
                // for the identical reason: a multi-type `catch`
                // variable, or an ordinarily union-typed parameter, must
                // not skip this check entirely.
                let base_ty = self.check_expr(base, locals, label);
                // See `probe_span`'s doc comment.
                self.probe_hit_at(self.expr_span(base.id), &base_ty);
                // A nullsafe access earlier in the chain that met `null`
                // skipped this one.
                let base_ty = if self
                    .syntax_arenas()
                    .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, base))
                {
                    self.chain_link_type(base, locals)
                } else {
                    base_ty
                };
                // `$x?->prop` is pointless once `$x`'s type already
                // proves it's never null - mirrors real PHPStan's
                // `NullsafePropertyFetchRule` exactly. Guarded by
                // `loop_depth`/`static_var_names` - see the identical
                // guard on the `MethodCall` arm above for why.
                if nullsafe.is_nullsafe()
                    && self.loop_depth == 0
                    && !matches!(&base.kind, ExprKind::Variable(n) if self.static_var_names.contains(n.as_ref()))
                    && type_definitely_excludes_null(&base_ty)
                {
                    self.report(format!(
                            "Using nullsafe property access on non-nullable type {base_ty}. Use -> instead."
                        ),
                        self.expr_span(e.id),
                        0,
                        "nullsafe.neverNull",
                    );
                }
                // A Pest `beforeEach(...)` closure's `$this->prop = ...`
                // legitimately defines a dynamic property every other
                // Pest closure in the file/directory scope can read, real
                // PHPUnit\Framework\TestCase declaration or not - see
                // `pest_hook_properties`'s doc comment. Keyed off the
                // literal `$this` expression, unlike the real
                // `pestphp/pest-plugin-phpstan` extension, which resolves
                // by the base's type.
                let is_pest_this = self.settings.pest
                    && matches!(&base.kind, ExprKind::Variable(name) if **name == *"this");
                // `stdClass` (a real, registered, zero-property class in
                // the vendored stubs) is PHP's canonical "arbitrary
                // property bag" - real PHP (and real PHPStan) never flag
                // a dynamic property read or write on it, by design.
                // Scoped to this property-access call site only, not
                // `member_confirmed_missing` itself, which two method-call
                // checks below also share - an undefined method call on
                // `stdClass` is still a real fatal error.
                // Mirrors real PHPStan's
                // `PropertyExistsTypeSpecifyingExtension` - the
                // `property_exists()` mirror of the `method_exists()`
                // guard on `MethodCall` above.
                let property_exists_guarded = !locals.guarded_properties().is_empty()
                    && matches!(
                        &base.kind,
                        ExprKind::Variable(name)
                            if locals.find_var(name).is_some_and(|receiver| locals
                                .guarded_properties()
                                .contains(&(receiver, prop.to_string())))
                    );
                // `property.nonObject` - same reasoning and nullsafe
                // handling as `method.nonObject` above, checked first so
                // it short-circuits before `property.notFound` ever gets
                // a chance to also fire on the same access.
                let nonobject_ty = if nullsafe.is_nullsafe() {
                    base_ty.without(&Type::null())
                } else {
                    base_ty.clone()
                };
                let non_object_level = self.failing_level(&nonobject_ty, 0, |m| {
                    self.relation().can_access_properties(m).no()
                });
                if let Some(level) = non_object_level {
                    self.report(
                        format!(
                            "access to property ${prop} on {} {base_ty}",
                            non_object_kind(level)
                        ),
                        self.expr_span(e.id),
                        level,
                        "property.nonObject",
                    );
                }
                if non_object_level != Some(0) {
                    if let Some(from) = types::strict_mixed_receiver_from(&nonobject_ty) {
                        self.report_found_from(
                            format!("Cannot access property ${prop} on mixed."),
                            self.expr_span(e.id),
                            0,
                            from,
                            "property.nonObject",
                        );
                    } else if !property_exists_guarded
                        && self.member_confirmed_missing(&base_ty, |c| {
                            c == "stdClass"
                                || self.registry.lookup_property(c, prop).is_some()
                                || self.registry.lookup_method(c, "__get").is_some()
                                || (is_pest_this && self.pest_hook_properties.contains(&*prop.name))
                        })
                    {
                        self.report(
                            format!("access to undefined property {base_ty}::${prop}"),
                            self.expr_span(e.id),
                            2,
                            "property.notFound",
                        );
                    }
                }
                // Visibility - only meaningful once the property is
                // confirmed to exist at all. Deliberately not the same
                // "just the first Object/EnumCase member, regardless of
                // whether it actually has the property" shortcut
                // `MethodCall`'s own `recv_class` extraction settles for:
                // for an `Intersection`, the property can easily live on
                // a different member than whichever happens to come
                // first (`View&Foo` where `View` is an unrelated
                // interface and `$secret` is `Foo`'s private property -
                // the first-found `View` would never have it).
                // `find_property_owner` recurses through both `Union` and
                // `Intersection`, keeping the first member that actually
                // declares it.
                let owner_and_sig = self.find_property_owner(&base_ty.without(&Type::null()), prop);
                if let Some((owner, p)) = owner_and_sig {
                    self.check_member_visibility(MemberVisibilityCheck {
                        member: MemberLabel {
                            kind: MemberKind::Property,
                            owner: &owner,
                            name: prop,
                        },
                        visibility: &p.visibility(),
                        span: self.expr_span(e.id),
                    });
                    if !p.is_readable()
                        && self.is_member_access_visible(&owner, &p.visibility(), prop, "property")
                    {
                        self.report(
                            format!("Property {owner}::${prop} is not readable."),
                            self.expr_span(e.id),
                            0,
                            "property.writeOnly",
                        );
                    }
                    self.check_internal_property_access(&owner, prop, p, self.expr_span(e.id));
                    self.check_restricted_property_access(
                        &owner,
                        prop,
                        false,
                        self.expr_span(e.id),
                    );
                    self.check_deprecated_property_access(&owner, prop, p, self.expr_span(e.id));
                    self.record_member_usage_token(
                        UsageTokenKind::Property,
                        &owner,
                        p.deprecated().is_some(),
                        p.is_internal(),
                        self.expr_span(e.id),
                    );
                    self.record_member_ref(self.expr_span(e.id), &owner, prop);
                }
                self.forget_what_an_operation_may_change(
                    &base_ty,
                    is_this(self, base),
                    Operation::Property(Some(prop.name.clone()), Access::Read),
                    locals,
                );
                if locals.may_have_member_narrowing()
                    && let Some(base_key) = self.find_expr_key(locals, base)
                    && let Some(narrowed) = locals
                        .find(&KeyData::Prop {
                            base: base_key,
                            prop: prop.name.clone(),
                        })
                        .and_then(|key| locals.get(key))
                {
                    return narrowed.clone();
                }
                // `EnumCase`/`ObjectShape` precision and the general
                // `Object` lookup all live in `property_access_type`,
                // shared with `infer_expr`'s identical arm - see its doc
                // comment.
                let result = self.property_access_type(&base_ty.without(&Type::null()), prop);
                if (nullsafe.is_nullsafe()
                    || self
                        .syntax_arenas()
                        .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, base)))
                    && !result.is_mixed()
                {
                    self.relation().union(result, Type::null())
                } else {
                    result
                }
            }
            // `$obj->{$expr}` - when `$expr` is a literal string, this is
            // PHP-semantically an ordinary, fully-known access
            // (`$obj->{'foo'}` === `$obj->foo`), so it's recorded as a
            // real member reference instead of escaping anything - any
            // Tier-2 consumer sees it exactly like a plain `PropertyAccess`
            // would. Only a genuinely computed name - the receiver's own
            // type is known or not, the *name* itself never is - falls
            // back to `escape_targets_for_receiver_type`'s conservative
            // whole-class escape hatch.
            ExprKind::DynamicPropertyAccess(base, name_expr, operator) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("dynamic property accesses retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let base = arenas
                    .expr(*base)
                    .expect("dynamic property receiver belongs to its retained file owner");
                let name_expr = arenas
                    .expr(*name_expr)
                    .expect("dynamic property name belongs to its retained file owner");
                let base_ty = self.check_expr(base, locals, label);
                // A nullsafe access earlier in the chain that met `null`
                // skipped this one.
                let base_ty = if self
                    .syntax_arenas()
                    .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, base))
                {
                    self.chain_link_type(base, locals)
                } else {
                    base_ty
                };
                let name_ty = self.check_expr(name_expr, locals, label);
                self.forget_what_an_operation_may_change(
                    &name_ty,
                    is_this(self, name_expr),
                    Operation::String,
                    locals,
                );
                self.forget_what_an_operation_may_change(
                    &base_ty,
                    is_this(self, base),
                    Operation::Property(None, Access::Read),
                    locals,
                );
                // Whatever the name, a value that can't have properties
                // can't have this one.
                self.check_dynamic_member_receiver(
                    &base_ty,
                    *operator,
                    false,
                    self.expr_span(e.id),
                );
                if let ExprKind::Literal(Literal::String(name)) = &name_expr.kind
                    && let Some(name) = self.syntax_string_text(name)
                {
                    if let Some((owner, _)) = self.find_property_owner(&base_ty, name) {
                        self.record_member_ref(self.expr_span(e.id), &owner, name);
                    }
                } else if self.collect_member_refs {
                    // `escape_targets_for_receiver_type` can do a real,
                    // whole-registry scan (an interface/abstract
                    // receiver) - `record_member_class_escape`'s own
                    // internal self-gate only skips *recording* its
                    // result, not computing it, so it must not be called
                    // at all unless Tier 2 is actually being collected.
                    for class in self.escape_targets_for_receiver_type(&base_ty) {
                        self.record_member_class_escape(&class);
                    }
                }
                Type::mixed()
            }
            // Same literal-name-is-real-usage/genuinely-dynamic-name-
            // escapes split as `DynamicPropertyAccess` just above.
            ExprKind::DynamicMethodCall(recv, name_expr, args, operator) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("dynamic method-call arguments retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let recv = arenas
                    .expr(*recv)
                    .expect("dynamic method receiver belongs to its retained file owner");
                let name_expr = arenas
                    .expr(*name_expr)
                    .expect("dynamic method name belongs to its retained file owner");
                let recv_ty = self.check_expr(recv, locals, label);
                // A nullsafe access earlier in the chain that met `null`
                // skipped this one.
                let recv_ty = if self
                    .syntax_arenas()
                    .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, recv))
                {
                    self.chain_link_type(recv, locals)
                } else {
                    recv_ty
                };
                self.check_expr(name_expr, locals, label);
                self.check_dynamic_member_receiver(&recv_ty, *operator, true, self.expr_span(e.id));
                for a in arenas.args(args.clone()) {
                    if let Some(value) = arenas.expr(a.value) {
                        self.check_expr(value, locals, label);
                    }
                }
                let mut callee = None;
                if let ExprKind::Literal(Literal::String(name)) = &name_expr.kind
                    && let Some(name) = self.syntax_string_text(name)
                {
                    if let Some((owner, _)) = self.find_method_owner(&recv_ty, name) {
                        self.record_method_ref(self.expr_span(e.id), &owner, name);
                        callee = self.registry.lookup_method(&owner, name);
                    }
                } else if self.collect_member_refs {
                    for class in self.escape_targets_for_receiver_type(&recv_ty) {
                        self.record_member_class_escape(&class);
                    }
                }
                self.record_call_throw_point(None, callee, self.expr_span(e.id));
                Type::mixed()
            }
            ExprKind::DynamicStaticCall(target, name, args) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("dynamic static-call arguments retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let name = arenas
                    .expr(*name)
                    .expect("dynamic static method name belongs to its retained file owner");
                let args_range = args.clone();
                let args = arenas.args(args_range.clone());
                let name_ty = self.check_expr(name, locals, label);
                // `Foo::{'bar'}()`, or a name variable holding one known
                // string: a call to that method.
                if let [method] = &name_ty.constant_strings_text()[..] {
                    let arena = self.synthetic_expr_arena();
                    let call = self.static_call(
                        &arena,
                        target,
                        method.to_string(),
                        args_range,
                        self.expr_span(e.id),
                    );
                    return self.check_expr(&call, locals, label);
                }
                let class = self.resolve_static_target_class(target, locals, label);
                for a in args {
                    if let Some(value) = arenas.expr(a.value) {
                        self.check_expr(value, locals, label);
                    }
                }
                self.record_dynamic_static_member_escape(target);
                if self.collect_member_refs
                    && let Some(class) = class
                {
                    self.record_member_class_escape(&class);
                }
                self.record_call_throw_point(None, None, self.expr_span(e.id));
                Type::mixed()
            }
            ExprKind::Interpolated(parts) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("interpolated expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                // Mirrors real PHPStan's `InvalidPartOfEncapsedStringRule`
                // (`encapsedStringPart.nonString`) - an interpolated
                // `"{$x}"` part implicitly casts `$x` to string exactly
                // like an explicit `(string)`/`echo`/`print` would (down
                // to triggering the same `__toString()` call), so an
                // array or an object with no usable `__toString()` there
                // is a real PHP fatal error, not just a style nit. A raw
                // literal text chunk between substitutions is modeled as
                // a synthetic `Literal(String(_))` part
                // (`build_interpolated`'s doc comment) - skipped here,
                // since it was never a real expression.
                //
                // Deliberate departure from real PHPStan's message text:
                // its wording embeds the part's original PHP source via
                // its own `ExprPrinter`; this project has no general
                // expression-to-source printer, so the message here names
                // only the type, not the source snippet.
                let parts = arenas.expr_ids(parts);
                let mut part_types = Vec::with_capacity(parts.len());
                for part in parts {
                    let p = arenas
                        .expr(*part)
                        .expect("interpolation part belongs to its retained file owner");
                    let part_ty = self.check_expr(p, locals, label);
                    part_types.push(
                        part_ty
                            .to_string_type(self.registry)
                            .unwrap_or(Type::string()),
                    );
                    if matches!(&p.kind, ExprKind::Literal(Literal::String(_))) {
                        continue;
                    }
                    self.check_implicit_tostring_usage(&part_ty, self.expr_span(p.id));
                    self.forget_what_an_operation_may_change(
                        &part_ty,
                        is_this(self, p),
                        Operation::String,
                        locals,
                    );
                    if part_ty.to_string_type(self.registry).is_none() {
                        self.report(
                            format!(
                                "Part of encapsed string ({part_ty}) cannot be cast to string."
                            ),
                            self.expr_span(p.id),
                            2,
                            "encapsedStringPart.nonString",
                        );
                    }
                }
                concat_string_parts(part_types, self.registry)
            }
            ExprKind::Match { subject, arms } => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("match expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let arms = arenas.match_arms(arms.clone());
                let subject = arenas
                    .expr(*subject)
                    .expect("match subject belongs to its retained file owner");
                let default = match_default(arms);
                let default_expr = default.map(|id| {
                    arenas
                        .expr(id)
                        .expect("match default body belongs to its retained file owner")
                });
                let before_match = (default.is_none() && !self.try_throw_states.is_empty())
                    .then(|| locals.clone());
                self.check_expr(subject, locals, label);
                // Real `match` has no fallthrough - each arm is mutually
                // exclusive, so (unlike `switch`) a later arm/default can
                // always safely assume every earlier arm's condition was
                // false. `match(true) { $cond => ... }` is a common
                // `switch`-replacement idiom, since each arm's condition
                // is checked with `===` - treat the arm's condition
                // itself as an `if`-style condition in that case;
                // otherwise narrow via an implicit `subject === cond`
                // equality, the same as an ordinary `switch` case.
                // A later arm sees every condition of each earlier arm
                // false: `match_true` applies each one's false branch in
                // turn (`!(a || b)` is `!a && !b`), and the equality side
                // excludes each earlier value. Feeds every later arm's
                // own body-narrowing precision (e.g. `default`'s body
                // seeing `subject` with every earlier arm's value already
                // excluded) - the `match.unhandled` exhaustiveness check
                // further below is a deliberately separate, independent
                // computation, not read from `accumulated` at all (see
                // its own doc comment for why).
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut merged: Option<Locals> = None;
                let mut result_types: Vec<Type> = Vec::new();
                // Snapshotted right before the *last* arm's own condition
                // gets excluded from `accumulated` below - `match.unhandled`'s
                // own `match(true)` handling further down needs exactly
                // this state (every *earlier* arm already proven false,
                // but not yet this one) to ask "is the last arm's own
                // condition already tautologically true from here."
                let mut accumulated_before_last_arm = accumulated.clone();
                let arms_count = arms.iter().filter(|arm| arm.conditions.is_some()).count();
                // `match.alwaysTrue`/`match.alwaysFalse` - real PHPStan's
                // own `MatchExpressionRule` dead-arm half, the sibling of
                // `match.unhandled`'s own missing-arm half just below.
                // Each arm condition is checked as `$matchCondition ===
                // $rawCondition` (`registry::identical_result`, the exact
                // same primitive `identical.alwaysFalse`/`.alwaysTrue`
                // uses for a real `===` expression) - `$matchCondition`
                // is the literal `true` for a `match(true)` subject,
                // `remaining_subject_ty` otherwise: a deliberately
                // independent running exclusion, NOT `accumulated`'s own
                // narrowed value the way arm *bodies* read it - confirmed
                // by direct testing that `accumulated`'s own subject
                // entry stays the bare, unexpanded enum type across
                // repeated exclusions (a bare `Object(EnumClass)` has no
                // case list of its own for `Type::without` to
                // filter against without `expand_enum_cases` first, the
                // exact same reason `match.unhandled`'s own "remaining"
                // computation just below is independent too - see its
                // own doc comment). Seeded once via `expand_enum_cases`
                // before the loop, then narrowed incrementally per arm
                // exactly the way `match.unhandled`'s own exclusion loop
                // does, just spread one arm at a time instead of all at
                // once at the end.
                //
                // `next_arm_is_dead` mirrors real PHPStan's own
                // `$nextArmIsDeadForType`: once any condition in some arm
                // proves always-true, every arm *after* it is dead code
                // real PHPStan stops reporting on entirely (not narrowing/
                // body-checking, which this project's existing loop below
                // is untouched and keeps doing regardless - only this
                // diagnostic's own emission stops) - within the *same*
                // arm, every remaining condition is still individually
                // checked and reported, matching real PHPStan's identical
                // "only the outer per-arm loop skips, never the inner
                // per-condition one" structure. The last arm is
                // unconditionally exempted from `alwaysTrue` specifically
                // (never `alwaysFalse`) - not the `elseif`-style
                // `reportAlwaysTrueInLastCondition` config gate `identical.
                // alwaysTrue` itself honors, a plain, unconditional
                // "redundant" concept doesn't even apply to whichever arm
                // there's nothing left after" (confirmed directly in this
                // exact rule's own source: `$i === $armsCount - 1` alone,
                // no config flag involved). Guarded on `loop_depth` for
                // the same reason every sibling `alwaysTrue`/`alwaysFalse`
                // check in this project already is - only the diagnostic
                // itself is skipped, not the surrounding arm processing.
                let mut next_arm_is_dead = false;
                let mut remaining_subject_ty = if match_true {
                    Type::bool_literal(true)
                } else {
                    self.expand_enum_cases(&self.infer_expr(subject, locals))
                };
                for (i, (conds, result)) in
                    arms.iter().filter_map(MatchArm::conditioned).enumerate()
                {
                    for c in arenas.expr_ids(&conds).iter().map(|id| {
                        arenas
                            .expr(*id)
                            .expect("match condition belongs to its retained file owner")
                    }) {
                        self.check_expr(c, locals, label);
                    }
                    if self.loop_depth == 0 && !next_arm_is_dead {
                        for c in arenas.expr_ids(&conds).iter().map(|id| {
                            arenas
                                .expr(*id)
                                .expect("match condition belongs to its retained file owner")
                        }) {
                            let cond_ty = self.infer_expr(c, &accumulated);
                            let Some(outcome) = self
                                .registry
                                .identical_result(&remaining_subject_ty, &cond_ty)
                            else {
                                continue;
                            };
                            if !outcome {
                                self.report(format!(
                                        "Match arm comparison between {remaining_subject_ty} and {cond_ty} is always false."
                                    ),
                                    self.expr_span(c.id),
                                    4,
                                    "match.alwaysFalse",
                                );
                                continue;
                            }
                            next_arm_is_dead = true;
                            // Real PHPStan's own `$armsCount` includes a
                            // trailing `default` arm as one more "arm"
                            // (an empty-condition one its own inner loop
                            // never iterates, but that still counts for
                            // *this* "is anything left after this one"
                            // check) - `default` isn't part of `arms`
                            // here at all, so the exemption additionally
                            // requires no `default` present, or a
                            // genuinely redundant last *real* arm
                            // followed only by an unreachable `default`
                            // would wrongly go unreported.
                            if i != arms_count - 1 || default.is_some() {
                                self.report(format!(
                                        "Match arm comparison between {remaining_subject_ty} and {cond_ty} is always true."
                                    ),
                                    self.expr_span(c.id),
                                    4,
                                    "match.alwaysTrue",
                                );
                            }
                        }
                    }
                    let mut arm_locals = self.narrow_match_arm_conditions(
                        arenas,
                        subject,
                        arenas.expr_ids(&conds),
                        &accumulated,
                        match_true,
                    );
                    let result = arenas
                        .expr(result)
                        .expect("match arm body belongs to its retained file owner");
                    result_types.push(self.check_expr(result, &mut arm_locals, label));
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&self.relation(), &m, &arm_locals),
                        None => arm_locals.clone(),
                    });
                    if i == arms_count - 1 {
                        accumulated_before_last_arm = accumulated.clone();
                    }
                    if match_true {
                        // A skipped arm had every condition false.
                        for c in arenas.expr_ids(&conds).iter().map(|id| {
                            arenas
                                .expr(*id)
                                .expect("match condition belongs to its retained file owner")
                        }) {
                            self.apply_narrowing(c, &mut accumulated, false);
                        }
                    } else {
                        for c in arenas.expr_ids(&conds).iter().map(|id| {
                            arenas
                                .expr(*id)
                                .expect("match condition belongs to its retained file owner")
                        }) {
                            self.apply_equality_narrowing(subject, c, &mut accumulated, false);
                            // `remaining_subject_ty`'s own incremental
                            // exclusion - see this loop's own leading
                            // comment for why this can't just reuse
                            // `accumulated`'s value above.
                            let cond_ty = self.infer_expr(c, locals);
                            if cond_ty.is_single_value() {
                                remaining_subject_ty = remaining_subject_ty.without(&cond_ty);
                            }
                        }
                    }
                }
                // `match.unhandled` - real PHPStan's own `MatchExpressionRule`
                // (`vendor/phpstan-src/src/Rules/Comparison/MatchExpressionRule.php:160-181`),
                // including both of its real exemptions (`isUnhandledMatchErrorCaught`/
                // `hasUnhandledMatchErrorThrowsTag`, `match_unhandled_error_is_exempted`
                // below) and its `match(true)` handling.
                // An unhandled value throws `UnhandledMatchError` before any
                // arm runs, whether or not the report is exempted.
                if default.is_none() {
                    let exempted = self.match_unhandled_error_is_exempted();
                    if match_true {
                        // Real PHPStan's own mechanism for this shape: a
                        // `match(true)` chain is exhaustive exactly when
                        // its *last* arm's own condition is already
                        // tautologically true given every earlier arm's
                        // condition having been false
                        // (`$nextArmIsDeadForType` staying true through
                        // the last arm, `MatchExpressionRule.php:90-92`)
                        // - ordinary narrowing already proves this the
                        // same way it already proves any other condition
                        // constant (`condition_is_provably_true`'s own
                        // doc comment), no boolean-SAT solving needed for
                        // it - only a single-condition last arm is
                        // attempted here (a multi-condition last arm's
                        // own OR-combined exhaustiveness isn't), matching
                        // this same arm's own narrowing-loop exemption
                        // just above.
                        let last_is_exhaustive = arms
                            .iter()
                            .filter_map(MatchArm::conditioned)
                            .next_back()
                        .is_some_and(|(conds, _)| {
                            matches!(arenas.expr_ids(&conds), [only_cond]
                                if self.condition_is_provably_true(
                                    arenas.expr(*only_cond).expect("match condition belongs to its retained file owner"),
                                    &accumulated_before_last_arm
                                ))
                        });
                        if !last_is_exhaustive && let Some(before) = &before_match {
                            self.record_throw_state(unhandled_match_error(), before);
                        }
                        if !last_is_exhaustive && !exempted {
                            // `$matchCondition` is the literal `true` -
                            // real PHPStan's own `$remainingType` for
                            // this shape is always just that same
                            // constant (narrowing doesn't apply to a
                            // literal), so its message is this same
                            // degenerate-but-accurate "true" regardless
                            // of which arm(s) are actually unhandled.
                            self.report(
                                "Match expression does not handle remaining value: true"
                                    .to_string(),
                                self.expr_span(e.id),
                                4,
                                "match.unhandled",
                            );
                        }
                    } else {
                        // A deliberately independent computation from
                        // `accumulated` above (which also carries every
                        // OTHER variable's own narrowing, threaded
                        // through for arm-BODY precision) - this
                        // re-derives `subject`'s own original type fresh
                        // and excludes every arm condition's own known-
                        // single-value type from it directly, so a bug
                        // in some unrelated narrowing fact can't
                        // accidentally suppress or misfire this
                        // diagnostic. `expand_enum_cases` first, since a
                        // plain enum-typed subject (`EnumClass`
                        // _)`, the overwhelmingly common real shape) has
                        // no case list of its own for `narrow_type_
                        // excluding` to filter against otherwise - only
                        // its already-`Union`-shaped form (every real
                        // case as its own member) lets that function's
                        // existing "filter out excluded members, `Never`
                        // once none remain" `Union` handling do the
                        // actual work. If anything is still left once
                        // every arm's condition has been excluded and
                        // there's no `default`, some real value would
                        // fall through to a runtime `UnhandledMatchError`.
                        let subject_ty = self.infer_expr(subject, locals);
                        let mut remaining = self.expand_enum_cases(&subject_ty);
                        for (conds, _) in arms.iter().filter_map(MatchArm::conditioned) {
                            for c in arenas.expr_ids(&conds).iter().map(|id| {
                                arenas
                                    .expr(*id)
                                    .expect("match condition belongs to its retained file owner")
                            }) {
                                let cond_ty = self.infer_expr(c, locals);
                                if cond_ty.is_single_value() {
                                    remaining = remaining.without(&cond_ty);
                                }
                            }
                        }
                        if !remaining.is_never()
                            && let Some(before) = &before_match
                        {
                            self.record_throw_state(unhandled_match_error(), before);
                        }
                        if !remaining.is_never() && !exempted {
                            self.report(
                                format!(
                                    "Match expression does not handle remaining value: {remaining}"
                                ),
                                self.expr_span(e.id),
                                4,
                                "match.unhandled",
                            );
                        }
                    }
                }
                if let Some(d) = default_expr {
                    let mut default_locals = accumulated.clone();
                    result_types.push(self.check_expr(d, &mut default_locals, label));
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&self.relation(), &m, &default_locals),
                        None => default_locals,
                    });
                }
                if let Some(m) = merged {
                    *locals = m;
                }
                // Same "don't let an unknown arm compose into a partial
                // Union that looks more precise than it is" rule Ternary
                // applies (`Type::union` absorbs `Mixed`), and the same
                // flattening `Type::union` gives every other multi-branch
                // construct (`Ternary`, `??`, ...). A raw union
                // built by hand here would leave a nested `Union` sitting
                // as one opaque, unflattened member whenever one arm's
                // result was already a Union - a nested Union member
                // breaks any consumer that walks a Union's members
                // expecting each to be a flat leaf type
                // (`return_type_too_wide`'s `union_members`, among
                // others).
                result_types
                    .into_iter()
                    .union_all_with(&self.relation())
                    .unwrap_or(Type::never())
            }
            ExprKind::Closure(closure_id) => {
                let Some(syntax_owner) = self.syntax_tree_owner() else {
                    return Type::mixed();
                };
                let arenas = syntax_owner.arenas();
                let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) else {
                    return Type::mixed();
                };
                let params = arenas.params(closure.params.clone());
                let uses = arenas.closure_uses(&closure.uses);
                let body = arenas.stmt_ids(&closure.body);
                let goto_labels = &closure.goto_labels;
                let is_generator = closure.is_generator;
                let is_static = closure.is_static;
                let return_type = closure.return_type.as_deref();
                let attrs = arenas.attributes(closure.attrs.clone());
                // Mirrors real PHPStan's `ClosureAttributesRule` -
                // existence plus target-validity for a closure
                // expression's own `#[Attr]` group.
                self.check_attributes(attrs, label);
                self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_FUNCTION, "function");
                // Unlike an arrow function, a plain closure does NOT
                // automatically see the enclosing scope - only variables
                // explicitly named in `use (...)` are captured (by value
                // or by reference; either way, the type it starts with is
                // whatever the outer scope currently has it as). `$this`
                // is the one implicit exception: a non-static closure
                // declared inside a method still auto-binds it, the same
                // as real PHP.
                // Each variable captured by reference, with what the closure is entered
                // with in it.
                let mut by_ref_entries: Vec<(Name, Type)> = Vec::new();
                let mut closure_locals: Locals = locals.closure_scope();
                for u in uses {
                    // Mirrors real PHPStan's
                    // `InvalidLexicalVariablesInClosureUseRule` - `$this`,
                    // a superglobal, or a name already used as a
                    // parameter can never be captured via `use (...)` at
                    // all (a real PHP compile error), distinct from - and
                    // checked before - the plain "undefined variable"
                    // case below.
                    if u.name == "this" {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                "Cannot use $this as lexical variable.".to_string(),
                                u.name.span,
                                0,
                                "closure.useThis",
                            )
                            .non_ignorable(),
                        );
                    } else if is_superglobal(&u.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                format!(
                                    "Cannot use superglobal variable ${} as lexical variable.",
                                    u.name
                                ),
                                u.name.span,
                                0,
                                "closure.useSuperGlobal",
                            )
                            .non_ignorable(),
                        );
                    } else if params.iter().any(|p| p.name.name == u.name.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(format!(
                                    "Cannot use lexical variable ${} since a parameter with the same name already exists.",
                                    u.name
                                ),
                                u.name.span,
                                0,
                                "closure.useDuplicate",
                            )
                            .non_ignorable(),
                        );
                    } else if !u.by_ref && !locals.contains_var(&u.name) {
                        self.report(
                            format!("undefined variable ${}", u.name),
                            u.name.span,
                            0,
                            "variable.undefined",
                        );
                    }
                    let ty = locals.get_var(&u.name).cloned().unwrap_or(Type::mixed());
                    // A captured variable's real declaration is wherever
                    // the outer scope already says it is - copied across,
                    // not fabricated fresh at this `use (...)` clause.
                    let provenance = locals
                        .get_provenance_var(&u.name)
                        .cloned()
                        .unwrap_or_default();
                    // The `use (...)` clause's capture token is itself a
                    // real reference to the outer `$x` - same probe hook
                    // `ExprKind::Variable` has.
                    if self.probe_span.get() == Some(u.name.span) {
                        *self.probe_var_provenance.borrow_mut() =
                            Some(provenance.iter().copied().collect());
                    }
                    // Hovering the capture token itself must show its
                    // type too, not just resolve go-to-definition - the
                    // two probes (`probe_hit`/`probe_var_provenance`) are
                    // independent.
                    self.probe_hit_at(u.name.span, &ty);
                    // A capture by reference is entered with what anything
                    // sharing it may leave (`captures`); it can still gain
                    // keys from anything else holding the reference before
                    // the closure runs.
                    let ty = if u.by_ref {
                        let ty = self.capture_entry(self.expr_span(e.id), &u.name.name, ty);
                        by_ref_entries.push((u.name.name.clone(), ty.clone()));
                        ty.unsealed_shapes()
                    } else {
                        ty
                    };
                    closure_locals.insert_with_provenance_var(&u.name, ty, provenance);
                }
                if !is_static && let Some(this_ty) = locals.get_var("this") {
                    closure_locals.insert_var("this", this_ty.clone());
                }
                // `record_branch_implied_facts`'s own facts don't
                // otherwise survive into a closure body at all -
                // `fresh_scope()` starts `branch_implied_facts` empty,
                // same as every other per-scope side-channel. A captured
                // discriminant tested again inside the closure (`Rate
                // Service::rate`'s own `$userAnonymous && $anonymousId
                // !== $savedAnonymousId` replaying the outer `if
                // ($userAnonymous) { $savedAnonymousId = ...; }`) still
                // needs the fact available to replay. The *discriminant*
                // side needs no filtering - the closure can only ever
                // test a subject it actually captured, so a fact keyed to
                // an uncaptured subject simply never gets looked up - but
                // the *implied* side does: an implied fact naming a
                // variable the closure never captured would otherwise
                // spuriously `.insert()` it into `closure_locals`,
                // masking a real `variable.undefined` on an unrelated,
                // genuinely-uncaptured reference to the same bare name.
                let captured_names: rustc_hash::FxHashSet<&str> =
                    uses.iter().map(|u| u.name.as_ref()).collect();
                for (key, implied) in locals.branch_implied_facts().iter() {
                    let filtered: Vec<(LocalKey, Type)> = implied
                        .iter()
                        .filter(|(name, _)| match locals.data(*name) {
                            KeyData::Var(variable) => captured_names.contains(variable.as_str()),
                            _ => false,
                        })
                        .cloned()
                        .collect();
                    if !filtered.is_empty() {
                        closure_locals
                            .branch_implied_facts_mut()
                            .insert(key.clone(), std::rc::Rc::new(filtered));
                    }
                }
                let closure_label = format!("{label} (closure)");
                for p in params {
                    let (ty, case_refs) =
                        self.resolve_type_hint_with_case_refs(p.type_hint.as_ref());
                    // Mirrors real PHPStan's
                    // `ExistingClassesInClosureTypehintsRule` - checked on
                    // the un-array-wrapped element type below (a variadic
                    // parameter's declared type, not the `array` it
                    // collects into) so a missing class inside e.g.
                    // `...Foo $items` is still reported by its real name,
                    // not `array`.
                    self.check_type_class_names_exist(
                        &ty,
                        format!("parameter ${} of anonymous function", p.name),
                        p.span,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, p.span);
                    let ty = if p.variadic {
                        Type::array(Type::unknown_array_key(), ty)
                    } else {
                        ty
                    };
                    closure_locals.declare_var(&p.name, ty, p.name.span);
                    closure_locals.set_native_var(&p.name, self.closure_param_native(arenas, p));
                }
                let (declared_return_ty, declared_return_case_refs) =
                    self.resolve_type_hint_with_case_refs(return_type);
                let declared_return = Some(declared_return_ty);
                self.check_type_class_names_exist(
                    declared_return.as_ref().unwrap_or(&Type::mixed()),
                    "return type of anonymous function",
                    self.expr_span(e.id),
                );
                self.check_type_trait_refs(
                    declared_return.as_ref().unwrap_or(&Type::mixed()),
                    "return type of anonymous function",
                    self.expr_span(e.id),
                    "return.trait",
                );
                self.check_type_class_name_case(&declared_return_case_refs, self.expr_span(e.id));
                // `return.noParent` - `declared_return` above never went
                // through `resolve_pseudo` (unlike `current_declared_
                // return_type` further down), so a fresh, one-off resolve
                // against the closure's enclosing class
                // (`self.current_class`, not a `class_name` parameter - a
                // closure has no signature of its own) is needed here
                // too.
                if let Some(c) = self.current_class.clone() {
                    let resolved_return =
                        self.resolve_pseudo(declared_return.as_ref().unwrap_or(&Type::mixed()), &c);
                    self.check_no_parent_ref(
                        &resolved_return,
                        "return type of anonymous function",
                        self.expr_span(e.id),
                        "return.noParent",
                    );
                }
                // The identical `@internal` type-hint check a named
                // function/method's param/return list already gets.
                let current_class_for_internal_check = self.current_class.clone();
                self.check_internal_param_and_return_types_raw(
                    params,
                    return_type,
                    self.expr_span(e.id),
                    false,
                    current_class_for_internal_check.as_deref(),
                    &closure_label,
                );
                self.check_restricted_param_and_return_types_raw(
                    params,
                    return_type,
                    self.expr_span(e.id),
                    current_class_for_internal_check.as_deref(),
                );
                // A generator-returning closure needs the same
                // current_function_is_generator save/set/restore
                // check_function does for a named function/method -
                // check_block never routes through check_function at
                // all.
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = is_generator;
                let previous_declared_return_type = self.current_declared_return_type.take();
                self.current_declared_return_type =
                    declared_return.as_ref().map(|t| match &self.current_class {
                        Some(c) => self.resolve_pseudo(t, c),
                        None => t.clone(),
                    });
                // A closure is a genuinely separate callable with its
                // own `@throws` contract, independent of whatever the
                // *enclosing* function declares - cleared, not
                // inherited, same reasoning as every other
                // `current_function_*` field already cleared just below
                // for this same arm.
                let previous_declared_throws_type = self.current_declared_throws_type.take();
                let previous_goto_targets = std::mem::replace(
                    &mut self.current_function_goto_targets,
                    GotoState::new(goto_labels.clone()),
                );
                let previous_assigned_variables = self.enter_assigned_variables(arenas, body);
                let enclosing_scope =
                    self.enter_shared_scope(crate::checker::shared_scope::SharedScope {
                        closure: true,
                        ..crate::checker::shared_scope::SharedScope::of(
                            arenas,
                            body,
                            uses,
                            Some((
                                self.current_class.as_deref().map(Name::from),
                                Name::from("{closure}"),
                            )),
                            false,
                        )
                    });
                // Real PHPStan exempts an anonymous function entirely
                // from both by-ref-parameter rules (`isInAnonymousFunction()`
                // bails in `ParameterOutAssignedTypeRule`/
                // `ParameterOutExecutionEndTypeRule`) - a closure
                // capturing an outer by-ref parameter via `use (&$x)`
                // and reassigning it inside its own body is a genuinely
                // different question from the *enclosing* function's own
                // by-ref contract, so this must not silently check the
                // closure body's assignments against it. Emptied for the
                // closure body's own `check_block` pass below, restored
                // after, the same shape as every other `current_function_*`
                // field here.
                let previous_by_ref_params =
                    std::mem::take(&mut self.current_function_by_ref_params);
                let previous_by_ref_exit_snapshots =
                    std::mem::take(&mut self.current_function_by_ref_exit_snapshots);
                let previous_by_ref_params_with_tag =
                    std::mem::take(&mut self.current_function_by_ref_params_with_tag);
                // Every `use (&$x)` capture needs its own exit-snapshot
                // recorded (see `current_function_byref_snapshot_names`'s
                // own doc comment) - unlike `current_function_by_ref_
                // params` just above, this must NOT stay empty for a
                // closure, or the write-back loop below has nothing but
                // `closure_locals`'s single end-of-body snapshot to work
                // from, silently losing any mutation that happened on an
                // early-return path (the common `if (cond) { $x[] =
                // ...; return ...; } return ...;` guard-clause shape).
                let previous_byref_snapshot_names =
                    std::mem::take(&mut self.current_function_byref_snapshot_names);
                self.current_function_byref_snapshot_names = uses
                    .iter()
                    .filter(|u| u.by_ref)
                    .map(|u| u.name.name.clone())
                    .collect();
                // A closure's own throw points never belong to the
                // enclosing function's count (calling the closure, not
                // merely declaring it, is what would matter, and this
                // project doesn't model that) - starts fresh, discarded
                // on restore, same shape as
                // `current_function_is_generator` above.
                let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
                let previous_try_throw_states = std::mem::take(&mut self.try_throw_states);
                let previous_unannotated_call =
                    std::mem::take(&mut self.current_function_has_unannotated_call);
                let previous_explicit_throw_points =
                    std::mem::take(&mut self.current_function_explicit_throw_points);
                let previous_exit_points = std::mem::take(&mut self.current_function_exit_points);
                // The closure's own `return` statements belong to the
                // closure, not the enclosing function's too-wide-return
                // accumulator - saved/restored the same way, and read back
                // below as the closure's own inferred return type.
                let previous_return_points =
                    std::mem::take(&mut self.current_function_return_points);
                let previous_saw_explicit_return =
                    std::mem::take(&mut self.current_function_saw_explicit_return);
                self.check_generator_return_type(declared_return.as_ref(), self.expr_span(e.id));
                // A closure literal may run zero-to-many times (any
                // callback-accepting builtin, or simply being called more
                // than once through an ordinary variable) - a single
                // `check_block` pass here would only ever see one
                // captured-by-ref variable's own *first* mutation, not a
                // fixed point across repeated invocations (real corpus
                // shape: `SearchService::positionalCondition`'s own
                // `use (&$values)` closure passed to
                // `preg_replace_callback`, mutated across however many
                // regex matches the pattern finds). Routes through the
                // same convergence primitive an ordinary loop body
                // already gets - `LoopZeroIterations::Possible` performs
                // exactly the "union the fixed-point result with never-
                // having-run" merge the by-ref-`use` handling below used
                // to do by hand, so `closure_locals` already comes back
                // correctly converged for that too.
                let mut body_terminates = false;
                let by_ref_names: Vec<Name> = uses
                    .iter()
                    .filter(|u| u.by_ref)
                    .map(|u| u.name.name.clone())
                    .collect();
                let mut entry_locals = closure_locals.clone();
                let (converged, _) =
                    self.check_capturing_scope(&mut entry_locals, |this, entry| {
                        this.converge_loop_body(
                            entry,
                            declared_return.as_ref(),
                            &closure_label,
                            LoopZeroIterations::Possible {
                                subject_exempt: None,
                                subject_key: None,
                            },
                            |this, pass_locals, declared_return, label, _pass| {
                                // Only the kept (last) pass's return points
                                // count, the same way only its diagnostics
                                // survive.
                                this.current_function_return_points.clear();
                                this.current_function_saw_explicit_return = false;
                                body_terminates = this
                                    .check_block(body, pass_locals, declared_return, label)
                                    .completion
                                    .terminates;
                                // The next run starts from what each capture held where
                                // the body returned, too.
                                this.fold_returned_captures(&by_ref_names, pass_locals);
                            },
                        )
                    });
                closure_locals = converged;
                let closure_return_points = std::mem::replace(
                    &mut self.current_function_return_points,
                    previous_return_points,
                );
                let closure_saw_explicit_return = std::mem::replace(
                    &mut self.current_function_saw_explicit_return,
                    previous_saw_explicit_return,
                );
                let closure_is_generator = self.current_function_is_generator;
                let closure_falls_off_end = !body_terminates;
                // One final snapshot from the fully-converged post-body
                // state - covers both "falls off the end with no
                // explicit return" and "never actually invoked"
                // uniformly, on top of whatever mid-body `StmtKind::
                // Return` sites already recorded via `check_block`
                // above (`stmt.rs`'s own `record_by_ref_exit_snapshot`
                // call sites, which fire for a closure body exactly the
                // same as a named function's, no special-casing needed
                // there).
                self.record_by_ref_exit_snapshot(&closure_locals);
                self.check_closure_missing_return(
                    body_terminates,
                    return_type.is_some(),
                    declared_return.as_ref(),
                    self.expr_span(e.id),
                );
                // Captured before the restore below overwrites the live
                // field back to the *outer* scope's own snapshots - the
                // write-back loop further down needs this closure's own
                // accumulated data, not whatever the enclosing function
                // had going into this closure literal.
                let closure_byref_exit_snapshots = std::mem::replace(
                    &mut self.current_function_by_ref_exit_snapshots,
                    previous_by_ref_exit_snapshots,
                );
                self.current_function_is_generator = previous_generator_flag;
                self.current_declared_return_type = previous_declared_return_type;
                self.current_declared_throws_type = previous_declared_throws_type;
                self.current_function_throw_points = previous_throw_points;
                self.try_throw_states = previous_try_throw_states;
                self.current_function_has_unannotated_call = previous_unannotated_call;
                self.current_function_explicit_throw_points = previous_explicit_throw_points;
                self.current_function_exit_points = previous_exit_points;
                self.current_function_goto_targets = previous_goto_targets;
                self.current_function_assigned_variables = previous_assigned_variables;
                self.leave_shared_scope(enclosing_scope);
                self.current_function_by_ref_params = previous_by_ref_params;
                self.current_function_by_ref_params_with_tag = previous_by_ref_params_with_tag;
                self.current_function_byref_snapshot_names = previous_byref_snapshot_names;
                // A `use(...)` capture never referenced anywhere in the
                // closure's own body. `ClosureUse` carries no span of its
                // own (see its doc comment), so every finding here
                // anchors to the whole closure expression's span instead
                // of the specific `$x` token.
                self.check_unused_closure_uses(uses, body, self.expr_span(e.id));
                // A `use (&$x)` capture is a real reference - any
                // mutation the closure body makes to `$x` is visible in
                // the declaring scope too, from the closure literal's
                // own point onward. `mutated` is the union of every real
                // exit point's own snapshot (`closure_byref_exit_
                // snapshots`, captured just above) - NOT just `closure_
                // locals`'s own single post-convergence state, which
                // only ever reflects whichever statement the body last
                // fell through to. For the common `if (cond) { $x[] =
                // ...; return ...; } return ...;` guard-clause shape,
                // that's never the branch that actually mutated `$x` -
                // every real mutation happened right before an early
                // `return`, invisible to a single end-of-body read. Falls
                // back to the old single-snapshot read only if somehow
                // nothing was recorded (shouldn't happen - the post-
                // convergence snapshot above is unconditional - but this
                // project's own "don't guess wrong" precedent prefers a
                // graceful degrade here over an `unwrap()`).
                self.write_back_captures(
                    self.expr_span(e.id),
                    uses,
                    &by_ref_entries,
                    &closure_byref_exit_snapshots,
                    &closure_locals,
                    locals,
                );
                let inferred_return = (!closure_is_generator).then(|| {
                    closure_inferred_return_type(
                        &self.relation(),
                        closure_return_points,
                        closure_saw_explicit_return,
                        closure_falls_off_end,
                    )
                });
                self.closure_literal_type(params, return_type, inferred_return, None)
            }
            ExprKind::ArrowFn(closure_id) => {
                let Some(syntax_owner) = self.syntax_tree_owner() else {
                    return Type::mixed();
                };
                let arenas = syntax_owner.arenas();
                let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*closure_id) else {
                    return Type::mixed();
                };
                let params = arenas.params(arrow.params.clone());
                let Some(body) = arenas.expr(arrow.body) else {
                    return Type::mixed();
                };
                let is_generator = arrow.is_generator;
                let return_type = arrow.return_type.as_deref();
                let attrs = arenas.attributes(arrow.attrs.clone());
                // Mirrors real PHPStan's `ArrowFunctionAttributesRule` -
                // see the `Closure` arm's identical comment above.
                self.check_attributes(attrs, label);
                self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_FUNCTION, "function");
                // An arrow function DOES automatically capture the whole
                // enclosing scope by value (implicit, no `use (...)`
                // needed) - a plain clone of `locals` is exactly that.
                let mut arrow_locals = locals.clone();
                for p in params {
                    let (ty, case_refs) =
                        self.resolve_type_hint_with_case_refs(p.type_hint.as_ref());
                    // Mirrors real PHPStan's
                    // `ExistingClassesInArrowFunctionTypehintsRule` - same
                    // "check the element type, not the variadic-collected
                    // `array`" reasoning as the Closure arm above.
                    self.check_type_class_names_exist(
                        &ty,
                        format!("parameter ${} of arrow function", p.name),
                        p.span,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        format!("parameter ${} of arrow function", p.name),
                        p.span,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, p.span);
                    let ty = if p.variadic {
                        Type::array(Type::unknown_array_key(), ty)
                    } else {
                        ty
                    };
                    arrow_locals.declare_var(&p.name, ty, p.name.span);
                    arrow_locals.set_native_var(&p.name, self.closure_param_native(arenas, p));
                }
                // Same save/set/restore
                // `check_function`/the Closure literal arms do: without
                // this, checking a generator arrow function here would
                // leak the flag into whatever comes after it in the
                // enclosing scope's checking pass.
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = is_generator;
                let declared_return = return_type.map(|_| self.resolve_type_hint(return_type));
                let previous_declared_return_type = self.current_declared_return_type.take();
                self.current_declared_return_type =
                    declared_return.as_ref().map(|t| match &self.current_class {
                        Some(c) => self.resolve_pseudo(t, c),
                        None => t.clone(),
                    });
                // Same reasoning as the `Closure` arm above.
                let previous_declared_throws_type = self.current_declared_throws_type.take();
                let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
                let previous_try_throw_states = std::mem::take(&mut self.try_throw_states);
                let previous_unannotated_call =
                    std::mem::take(&mut self.current_function_has_unannotated_call);
                let previous_explicit_throw_points =
                    std::mem::take(&mut self.current_function_explicit_throw_points);
                let previous_exit_points = std::mem::take(&mut self.current_function_exit_points);
                // Same by-ref-parameter exemption as the `Closure` arm
                // above - an arrow function's own implicit by-value
                // capture (`arrow_locals` is a clone, not an alias) means
                // `$x = ...;` as its single body expression never
                // actually mutates the enclosing function's own by-ref
                // parameter either, so it must not be checked against
                // that parameter's own contract.
                let previous_by_ref_params =
                    std::mem::take(&mut self.current_function_by_ref_params);
                let previous_by_ref_exit_snapshots =
                    std::mem::take(&mut self.current_function_by_ref_exit_snapshots);
                let previous_by_ref_params_with_tag =
                    std::mem::take(&mut self.current_function_by_ref_params_with_tag);
                self.check_generator_return_type(declared_return.as_ref(), self.expr_span(e.id));
                let enclosing_scope =
                    self.enter_shared_scope(crate::checker::shared_scope::SharedScope {
                        closure: true,
                        ..crate::checker::shared_scope::SharedScope::of_arrow_function(
                            self.syntax_arenas()
                                .expect("arrow checks require the syntax tree owner"),
                            body.id,
                        )
                    });
                let body_ty = self.check_expr(body, &mut arrow_locals, label);
                self.leave_shared_scope(enclosing_scope);
                self.current_function_is_generator = previous_generator_flag;
                self.current_declared_return_type = previous_declared_return_type;
                self.current_declared_throws_type = previous_declared_throws_type;
                self.current_function_throw_points = previous_throw_points;
                self.try_throw_states = previous_try_throw_states;
                self.current_function_has_unannotated_call = previous_unannotated_call;
                self.current_function_explicit_throw_points = previous_explicit_throw_points;
                self.current_function_exit_points = previous_exit_points;
                self.current_function_by_ref_params = previous_by_ref_params;
                self.current_function_by_ref_exit_snapshots = previous_by_ref_exit_snapshots;
                self.current_function_by_ref_params_with_tag = previous_by_ref_params_with_tag;
                // Its body is a single implicit-return expression, not a
                // StmtKind::Return - the equivalent of check_stmt's own
                // Return-vs-declared_return check, kept here since
                // there's no StmtKind::Return for it to share.
                if return_type.is_some() {
                    let (expected, expected_case_refs) =
                        self.resolve_type_hint_with_case_refs(return_type);
                    self.check_type_class_names_exist(
                        &expected,
                        "return type of arrow function",
                        self.expr_span(e.id),
                    );
                    self.check_type_class_name_case(&expected_case_refs, self.expr_span(e.id));
                    self.check_type_trait_refs(
                        &expected,
                        "return type of arrow function",
                        self.expr_span(e.id),
                        "return.trait",
                    );
                    if let Some(c) = self.current_class.clone() {
                        let resolved_return = self.resolve_pseudo(&expected, &c);
                        self.check_no_parent_ref(
                            &resolved_return,
                            "return type of arrow function",
                            self.expr_span(e.id),
                            "return.noParent",
                        );
                    }
                    // See `checker::calls`' own identical comment:
                    // `rejected_from` already accepts a `Mixed` body type
                    // below real PHPStan's own level 9/10.
                    if let Some(from) = self.rejected_from(&expected, &body_ty) {
                        self.report_found_from(
                            format!("return type mismatch: expected {expected}, got {body_ty}"),
                            self.expr_span(body.id),
                            3,
                            from,
                            "return.type",
                        );
                    }
                }
                // See the `Closure` arm's identical comment above.
                let current_class_for_internal_check = self.current_class.clone();
                self.check_internal_param_and_return_types_raw(
                    params,
                    return_type,
                    self.expr_span(e.id),
                    false,
                    current_class_for_internal_check.as_deref(),
                    label,
                );
                self.check_restricted_param_and_return_types_raw(
                    params,
                    return_type,
                    self.expr_span(e.id),
                    current_class_for_internal_check.as_deref(),
                );
                self.closure_literal_type(
                    params,
                    return_type,
                    (!is_generator).then(|| body_ty.clone()),
                    None,
                )
            }
            // `foo(...)`/`$obj->method(...)`/`Class::method(...)` (PHP 8.1
            // first-class-callable syntax) always wraps a
            // Call/MethodCall/StaticCall/DynamicMethodCall with an empty
            // argument list (this project's parser convention) - not
            // dispatching to that inner node's ordinary arm directly:
            // `check_args` would misread "no arguments were written" as
            // "zero arguments were passed," producing a false "expects at
            // least N argument(s)" diagnostic for any function/method
            // with required parameters. So this checks only existence -
            // the one thing that doesn't depend on argument count -
            // mirroring each inner arm's existence-check logic without
            // its argument handling.
            //
            // Also runs visibility/`@internal`/`@deprecated` (mirroring
            // real PHPStan's `StaticMethodCallableRule`, which just
            // re-runs the ordinary call-site checks against this shape):
            // both `Foo::method(...)` and `$obj->method(...)` produce a
            // real, callable `Closure` bound to that exact method the
            // same as an ordinary call would.
            ExprKind::FirstClassCallable(inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("first-class callables retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("first-class callable target belongs to its retained file owner");
                if !matches!(inner.kind, ExprKind::New(..))
                    && self.registry.php_supports_everywhere(
                        types::php_version::PhpFeature::FirstClassCallables,
                    ) == Some(false)
                {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            "First-class callables are supported only on PHP 8.1 and later."
                                .to_string(),
                            self.expr_span(e.id),
                            0,
                            "callable.notSupported",
                        )
                        .non_ignorable(),
                    );
                }
                match &inner.kind {
                    ExprKind::Call(callee, _) => {
                        let Some(callee) = arenas.expr(*callee) else {
                            return Type::mixed();
                        };
                        if let ExprKind::Name(range) = &callee.kind
                            && let Some(name) = arenas.qualified_name(range)
                        {
                            match self.registry.resolve_function(name) {
                                Some((resolved_name, sig)) => {
                                    self.check_internal_function_call(
                                        resolved_name,
                                        sig,
                                        self.expr_span(e.id),
                                    );
                                    // Closes a real, live bypass: a
                                    // banned function's own
                                    // first-class-callable syntax
                                    // (`banned_func(...)`) previously
                                    // went unchecked even though the
                                    // ordinary call site
                                    // (`banned_func()`) already was.
                                    self.check_restricted_function_call(
                                        resolved_name,
                                        &[],
                                        locals,
                                        self.expr_span(e.id),
                                    );
                                    self.check_deprecated_function_call(
                                        resolved_name,
                                        sig,
                                        self.expr_span(e.id),
                                    );
                                    self.record_function_usage_token(
                                        resolved_name,
                                        sig,
                                        self.expr_span(callee.id),
                                    );
                                }
                                None => {
                                    // Same `function_exists()`-guard
                                    // suppression as the ordinary-call
                                    // site above.
                                    let primary = name.candidates().0;
                                    let guarded = !locals.guarded_functions().is_empty()
                                        && locals.guarded_functions().contains(
                                            primary
                                                .rsplit('\\')
                                                .next()
                                                .unwrap_or(primary)
                                                .to_ascii_lowercase()
                                                .as_str(),
                                        );
                                    if !guarded {
                                        self.report(
                                            format!("call to undefined function {primary}()"),
                                            self.expr_span(e.id),
                                            0,
                                            "function.notFound",
                                        );
                                    }
                                }
                            }
                        } else {
                            self.check_expr(callee, locals, label);
                        }
                    }
                    ExprKind::MethodCall(recv, method, _, _) => {
                        let Some(recv) = arenas.expr(*recv) else {
                            return Type::mixed();
                        };
                        let recv_ty = self.check_expr(recv, locals, label);
                        let recv_class = recv_ty.first_object_class().cloned();
                        if let Some(class) = &recv_class
                            && self.registry.has_class(class.as_str())
                        {
                            if self.member_confirmed_missing(&recv_ty, |c| {
                                self.registry.lookup_method(c, method).is_some()
                                    || self.registry.lookup_method(c, "__call").is_some()
                            }) {
                                let mut builder = DiagnosticBuilder::new(
                                    format!("call to undefined method {recv_ty}::{method}()"),
                                    self.expr_span(e.id),
                                    0,
                                    "method.notFound",
                                );
                                if let Some(suggestion) = suggest::suggest_similar_name(
                                    method,
                                    self.registry.all_method_names(class),
                                ) {
                                    builder = builder.tip(format!("Did you mean {suggestion}()?"));
                                }
                                self.report_builder(builder);
                            } else if let Some((owner, sig)) =
                                self.lookup_method_with_owner(class, method)
                            {
                                self.check_member_visibility(MemberVisibilityCheck {
                                    member: MemberLabel {
                                        kind: MemberKind::Method,
                                        owner: &owner,
                                        name: method,
                                    },
                                    visibility: &sig.visibility(),
                                    span: self.expr_span(e.id),
                                });
                                self.check_internal_method_call(
                                    &owner,
                                    method,
                                    sig,
                                    false,
                                    self.expr_span(e.id),
                                );
                                self.check_restricted_method_call(
                                    &owner,
                                    method,
                                    false,
                                    &[],
                                    locals,
                                    self.expr_span(e.id),
                                );
                                self.check_deprecated_method_call(
                                    &owner,
                                    method,
                                    sig,
                                    false,
                                    self.expr_span(e.id),
                                );
                                self.record_member_usage_token(
                                    UsageTokenKind::Method,
                                    &owner,
                                    sig.deprecated().is_some(),
                                    sig.is_internal(),
                                    self.expr_span(e.id),
                                );
                                self.check_call_site_method_name_case(
                                    class,
                                    method,
                                    self.expr_span(e.id),
                                    Span {
                                        start: self.expr_span(recv.id).end,
                                        end: self.expr_span(e.id).end,
                                    },
                                );
                            }
                        }
                    }
                    ExprKind::StaticCall(target, method, _) => {
                        if let Some(class) = self.resolve_static_target_class(target, locals, label)
                            && self.registry.has_class(class.as_str())
                        {
                            self.check_call_site_method_name_case(
                                &class,
                                method,
                                self.expr_span(e.id),
                                Span {
                                    start: self.static_target_end(target),
                                    end: self.expr_span(e.id).end,
                                },
                            );
                            if self.registry.lookup_method(&class, method).is_none()
                                && self
                                    .registry
                                    .lookup_method(&class, "__callStatic")
                                    .is_none()
                            {
                                self.report(
                                    format!("call to undefined static method {class}::{method}()"),
                                    self.expr_span(e.id),
                                    0,
                                    "staticMethod.notFound",
                                );
                            } else if let Some((owner, sig)) =
                                self.lookup_method_with_owner(&class, method)
                            {
                                self.check_member_visibility(MemberVisibilityCheck {
                                    member: MemberLabel {
                                        kind: MemberKind::StaticMethod,
                                        owner: &owner,
                                        name: method,
                                    },
                                    visibility: &sig.visibility(),
                                    span: self.expr_span(e.id),
                                });
                                self.check_internal_method_call(
                                    &owner,
                                    method,
                                    sig,
                                    true,
                                    self.expr_span(e.id),
                                );
                                self.check_restricted_method_call(
                                    &owner,
                                    method,
                                    true,
                                    &[],
                                    locals,
                                    self.expr_span(e.id),
                                );
                                self.check_deprecated_method_call(
                                    &owner,
                                    method,
                                    sig,
                                    true,
                                    self.expr_span(e.id),
                                );
                                self.record_member_usage_token(
                                    UsageTokenKind::Method,
                                    &owner,
                                    sig.deprecated().is_some(),
                                    sig.is_internal(),
                                    self.expr_span(e.id),
                                );
                                if target.as_name().is_some_and(|n| n.is_keyword("static"))
                                    && sig.visibility() == Visibility::Private
                                {
                                    self.check_static_private_method_access(
                                        &owner,
                                        method,
                                        self.expr_span(e.id),
                                    );
                                }
                            }
                        }
                    }
                    ExprKind::DynamicMethodCall(recv, name_expr, _, _) => {
                        let Some(recv) = arenas.expr(*recv) else {
                            return Type::mixed();
                        };
                        let Some(name_expr) = arenas.expr(*name_expr) else {
                            return Type::mixed();
                        };
                        self.check_expr(recv, locals, label);
                        self.check_expr(name_expr, locals, label);
                    }
                    ExprKind::DynamicStaticCall(target, name_expr, _) => {
                        let Some(name_expr) = arenas.expr(*name_expr) else {
                            return Type::mixed();
                        };
                        self.resolve_static_target_class(target, locals, label);
                        self.check_expr(name_expr, locals, label);
                    }
                    ExprKind::New(..) => {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                "Cannot create callable from the new operator.".to_string(),
                                self.expr_span(e.id),
                                0,
                                "callable.notSupported",
                            )
                            .non_ignorable(),
                        );
                    }
                    _ => {
                        self.check_expr(inner, locals, label);
                    }
                }
                self.first_class_callable_type(inner, locals)
            }
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::int_literal(*n),
                Literal::Float(f) => Type::float_literal(*f),
                Literal::String(s) => self
                    .syntax_arenas()
                    .and_then(|arenas| arenas.owned_php_string(s))
                    .map(Type::string_literal)
                    .unwrap_or_else(Type::string),
                Literal::Bool(true) => Type::bool_literal(true),
                Literal::Bool(false) => Type::bool_literal(false),
                Literal::Null => Type::null(),
            },
            ExprKind::Name(name) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("name expressions retain their parsed file owner");
                let name = syntax_owner
                    .arenas()
                    .qualified_name(name)
                    .expect("name range belongs to its retained file owner");
                self.check_magic_constant_context(name, self.expr_span(e.id));
                match self.global_constant_type(name) {
                    Some(ty) => ty,
                    None => {
                        self.check_unknown_global_constant(name, locals, self.expr_span(e.id));
                        bare_name_literal_type(name)
                    }
                }
            }
        }
    }

    /// A property access's or method call's own type as the next link of
    /// its chain sees it: a nullsafe access earlier in the chain that met
    /// `null` skipped the rest of it, so that `null` isn't this link's.
    /// Anything else's type is its inferred one.
    pub(crate) fn chain_link_type(&self, e: &ExprNode, locals: &Locals) -> Type {
        if !matches!(
            e.kind,
            ExprKind::MethodCall(..)
                | ExprKind::PropertyAccess(..)
                | ExprKind::Index(_, Some(_))
                | ExprKind::Grouping(_)
        ) {
            return self.infer_expr(e, locals);
        }
        self.cached(e.id, locals, InferQuestion::ChainLink, || {
            self.chain_link_type_uncached(e, locals)
        })
    }

    fn chain_link_type_uncached(&self, e: &ExprNode, locals: &Locals) -> Type {
        match &e.kind {
            ExprKind::MethodCall(recv, method, args, _) => {
                let Some(owner) = self.syntax_tree_owner() else {
                    return Type::mixed();
                };
                let arenas = owner.arenas();
                let Some(recv) = arenas.expr(*recv) else {
                    return Type::mixed();
                };
                // What the method is looked up on is the receiver's object
                // part; a `null` one is reported where it's checked.
                let recv_ty = self.chain_link_type(recv, locals).without(&Type::null());
                self.method_call_return_type(
                    arenas,
                    recv,
                    &recv_ty,
                    method,
                    arenas.args(args.clone()),
                    locals,
                )
            }
            ExprKind::PropertyAccess(base, prop, _) => {
                let Some(owner) = self.syntax_tree_owner() else {
                    return Type::mixed();
                };
                let Some(base) = owner.arenas().expr(*base) else {
                    return Type::mixed();
                };
                if locals.may_have_member_narrowing()
                    && let Some(base_key) = self.find_expr_key(locals, base)
                    && let Some(narrowed) = locals
                        .find(&KeyData::Prop {
                            base: base_key,
                            prop: prop.name.clone(),
                        })
                        .and_then(|key| locals.get(key))
                {
                    return narrowed.clone();
                }
                let base_ty = self.chain_link_type(base, locals).without(&Type::null());
                // See `property_access_type`'s doc comment - shared
                // with `check_expr`'s identical arm.
                self.property_access_type(&base_ty, prop)
            }
            // A general array's element is taken as present; a key the
            // type says may be missing reads `V|null` (`offset_read_type`).
            ExprKind::Index(base, Some(idx)) => {
                let Some(owner) = self.syntax_tree_owner() else {
                    return Type::mixed();
                };
                let arenas = owner.arenas();
                let Some(base_expr) = arenas.expr(*base) else {
                    return Type::mixed();
                };
                let Some(idx_expr) = arenas.expr(*idx) else {
                    return Type::mixed();
                };
                // The narrowed key first: `apply_narrowing` writes a
                // narrowed type under `key_of`'s `"base[key]"`
                // (e.g. after `is_string($info['code'])`).
                if locals.may_have_member_narrowing()
                    && let Some(key) = self.find_expr_key(locals, e)
                    && let Some(narrowed) = locals.get(key)
                {
                    return narrowed.clone();
                }
                let key_ty = match crate::checker::literal_shape_key(arenas, idx_expr) {
                    Some(key) => key.to_type(),
                    None => self.infer_expr(idx_expr, locals),
                };
                self.offset_read_type(&self.chain_link_type(base_expr, locals), &key_ty)
            }
            // Parentheses don't end a chain.
            ExprKind::Grouping(inner) => {
                let Some(owner) = self.syntax_tree_owner() else {
                    return Type::mixed();
                };
                let Some(inner) = owner.arenas().expr(*inner) else {
                    return Type::mixed();
                };
                self.chain_link_type(inner, locals)
            }
            _ => self.infer_expr(e, locals),
        }
    }

    pub(crate) fn infer_expr(&self, e: &ExprNode, locals: &Locals) -> Type {
        // Inferring is `&self`, so nothing moves under one call: an
        // inference asked for outside an expression check remembers what it
        // finds of the operands for as long as it runs, as one inside does,
        // and not each level of a nested expression infers them again.
        let outermost = self.open_infer_scope();
        let ty = self.cached(e.id, locals, InferQuestion::Type, || {
            self.infer_expr_unguarded(e, locals)
        });
        if outermost {
            self.close_infer_scope();
        }
        ty
    }

    /// Starts remembering inferred types, unless that is going on already;
    /// whether it was started here.
    fn open_infer_scope(&self) -> bool {
        if self.infer_cache.borrow().is_some() {
            return false;
        }
        let spare = std::mem::take(&mut *self.infer_spare.borrow_mut());
        *self.infer_cache.borrow_mut() = Some(spare);
        true
    }

    /// Stops what `open_infer_scope` started, keeping the emptied table for
    /// the next.
    fn close_infer_scope(&self) {
        let mut cache = self.infer_cache.borrow_mut().take().unwrap_or_default();
        cache.clear();
        *self.infer_spare.borrow_mut() = cache;
    }

    /// `answer`'s answer to `question` about `id`, from `infer_cache` while
    /// an expression check runs.
    fn cached(
        &self,
        id: ExprId,
        locals: &Locals,
        question: InferQuestion,
        answer: impl FnOnce() -> Type,
    ) -> Type {
        // A completion probe may splice an expression parsed from its own
        // token slice into a cloned scope from the loaded file. Those two
        // trees have independent file-local ids; the short-lived probe
        // bypasses this one-file cache rather than mixing their slots.
        if self.is_synthetic_expr_id(id)
            || self.probe_span.get().is_some()
            || self.infer_cache.borrow().is_none()
        {
            return syntax::stack::guard(answer);
        }
        let version = locals.version();
        if let Some(ty) = self
            .infer_cache
            .borrow()
            .as_ref()
            .and_then(|c| c.get(id, version, question))
        {
            return ty.clone();
        }
        let ty = syntax::stack::guard(answer);
        if let Some(cache) = self.infer_cache.borrow_mut().as_mut() {
            cache.insert(id, version, question, ty.clone());
        }
        ty
    }

    fn infer_expr_unguarded(&self, e: &ExprNode, locals: &Locals) -> Type {
        match &e.kind {
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::int_literal(*n),
                Literal::Float(f) => Type::float_literal(*f),
                Literal::String(s) => self
                    .syntax_arenas()
                    .and_then(|arenas| arenas.owned_php_string(s))
                    .map(Type::string_literal)
                    .unwrap_or_else(Type::string),
                Literal::Bool(true) => Type::bool_literal(true),
                Literal::Bool(false) => Type::bool_literal(false),
                Literal::Null => Type::null(),
            },
            ExprKind::Variable(name) => locals
                .get_var(name)
                .cloned()
                .unwrap_or_else(|| unassigned_variable_type(name)),
            ExprKind::Grouping(inner)
            | ExprKind::ErrorSuppress(inner)
            | ExprKind::Clone(inner, _) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("wrapped expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("wrapped expression belongs to its retained file owner");
                self.infer_expr(inner, locals)
            }
            // First-class-callable syntax (`strlen(...)`, `$obj->
            // method(...)`) produces a real `Closure` instance at runtime
            // - infer the same `Object("Closure", ...)` the native
            // `Closure` type hint resolves to (rather than the broader
            // `callable`, which `callable`-hinted parameters accept
            // via `accepts`'s separate leniency), so a strictly
            // `Closure`-typed parameter correctly accepts it. A closure/
            // arrow-fn literal additionally carries its own signature
            // (`closure_literal_type`).
            ExprKind::FirstClassCallable(inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("first-class callables retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("callable target belongs to its retained file owner");
                self.first_class_callable_type(inner, locals)
            }
            ExprKind::Closure(closure_id) => {
                let Some(arenas) = self.syntax_arenas() else {
                    return Type::mixed();
                };
                let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) else {
                    return Type::mixed();
                };
                let params = arenas.params(closure.params.clone());
                let inferred = self.closure_syntactic_return_type(arenas, closure, locals, None);
                self.closure_literal_type(params, closure.return_type.as_deref(), inferred, None)
            }
            ExprKind::ArrowFn(closure_id) => {
                let Some(arenas) = self.syntax_arenas() else {
                    return Type::mixed();
                };
                let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*closure_id) else {
                    return Type::mixed();
                };
                let params = arenas.params(arrow.params.clone());
                let body = arenas.expr(arrow.body);
                let inferred = body.and_then(|body| {
                    self.arrow_fn_inferred_return(params, body, arrow.is_generator, locals, None)
                });
                self.closure_literal_type(params, arrow.return_type.as_deref(), inferred, None)
            }
            ExprKind::Match { subject, arms } => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("match expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let arms = arenas.match_arms(arms.clone());
                let subject = arenas
                    .expr(*subject)
                    .expect("match subject belongs to its retained file owner");
                let default = match_default(arms).map(|id| {
                    arenas
                        .expr(id)
                        .expect("match default body belongs to its retained file owner")
                });
                // Same "don't let an unknown branch compose into a
                // partial Union that looks more precise than it is" rule
                // Ternary applies, generalized to N arms plus an optional
                // default - and the same per-arm narrowing `check_expr`'s
                // Match handling applies, so a `default` arm returning
                // the match's subject infers its narrowed type rather
                // than the subject's unnarrowed original type.
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut result_types: Vec<Type> = Vec::new();
                for (conds, result) in arms.iter().filter_map(MatchArm::conditioned) {
                    let arm_locals = self.narrow_match_arm_conditions(
                        arenas,
                        subject,
                        arenas.expr_ids(&conds),
                        &accumulated,
                        match_true,
                    );
                    let result = arenas
                        .expr(result)
                        .expect("match arm body belongs to its retained file owner");
                    result_types.push(self.infer_expr(result, &arm_locals));
                    if let [only_cond] = arenas.expr_ids(&conds) {
                        let only_cond = arenas
                            .expr(*only_cond)
                            .expect("match condition belongs to its retained file owner");
                        if match_true {
                            self.apply_narrowing(only_cond, &mut accumulated, false);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut accumulated,
                                false,
                            );
                        }
                    }
                }
                if let Some(d) = default {
                    result_types.push(self.infer_expr(d, &accumulated));
                }
                // See `check_expr`'s identical arm's doc comment for
                // why this must be a `Type::union` reduction, not a raw
                // dedup-then-union construction.
                result_types
                    .into_iter()
                    .union_all_with(&self.relation())
                    .unwrap_or(Type::never())
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used for
            // a function that always throws or exits.
            ExprKind::Throw(_) => Type::never(),
            ExprKind::NewAnonymousClass(class_id) => self
                .syntax_arenas()
                .map(|arenas| anonymous_class_type(arenas.anonymous_class_name(*class_id)))
                .unwrap_or_else(Type::mixed),
            ExprKind::ShellExec(_) => self.shell_exec_type(),
            ExprKind::Print(_) => Type::int_literal(1),
            ExprKind::Isset(_) | ExprKind::Empty(_) => Type::bool(),
            // `true` or `false` when the subject's type decides it.
            ExprKind::Instanceof(inner, target) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("instanceof expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let inner = arenas
                    .expr(*inner)
                    .expect("instanceof operand belongs to its retained file owner");
                let subject = self.infer_expr(inner, locals);
                match self.instanceof_outcome(arenas, &subject, target, locals) {
                    Some((always, _)) => Type::bool_literal(always),
                    None => Type::bool(),
                }
            }
            ExprKind::Exit(..) => Type::never(),
            ExprKind::Eval(_) | ExprKind::Include(..) => Type::mixed(),
            ExprKind::Cast(cast, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("cast expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("cast operand belongs to its retained file owner");
                let inner_ty = self.infer_expr(inner, locals);
                self.cast_type(cast.kind(), &inner_ty)
            }
            ExprKind::Unary(UnaryOp::Not, _) => Type::bool(),
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) -
            // the same implicit-numeric-coercion rule arithmetic
            // operators apply generally.
            ExprKind::Unary(op @ (UnaryOp::Neg | UnaryOp::Plus), inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                let inner_ty = self.infer_expr(inner, locals);
                if inner_ty.is_string().yes() {
                    Type::int()
                } else if *op == UnaryOp::Neg {
                    inner_ty.negate()
                } else {
                    inner_ty
                }
            }
            ExprKind::Unary(UnaryOp::BitNot, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                let inner_ty = self.infer_expr(inner, locals);
                self.bitwise_not_type(&inner_ty)
            }
            ExprKind::Unary(UnaryOp::PreInc, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                self.infer_expr(inner, locals).increment_type()
            }
            ExprKind::Unary(UnaryOp::PreDec, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                self.infer_expr(inner, locals).decrement_type()
            }
            ExprKind::Unary(_, inner) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("unary expressions retain their parsed file owner");
                let inner = syntax_owner
                    .arenas()
                    .expr(*inner)
                    .expect("unary operand belongs to its retained file owner");
                self.infer_expr(inner, locals)
            }
            ExprKind::Binary(op, l, r) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("binary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let l = arenas
                    .expr(*l)
                    .expect("left operand belongs to its retained file owner");
                let r = arenas
                    .expr(*r)
                    .expect("right operand belongs to its retained file owner");
                self.infer_binary(op, l, r, locals)
            }
            ExprKind::Assign(_, rhs) | ExprKind::AssignRef(_, rhs) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("assignment expressions retain their parsed file owner");
                let rhs = syntax_owner
                    .arenas()
                    .expr(*rhs)
                    .expect("assigned value belongs to its retained file owner");
                self.infer_expr(rhs, locals)
            }
            // `self`/`parent`/`static` aren't lexed as keywords (see the
            // parser: they're indistinguishable from any other identifier),
            // so a literal `ExprKind::Name("self")` has to be special-cased
            // here rather than showing up as an Object of that name.
            ExprKind::New(class_expr, args) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("instantiations retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let class_expr = arenas
                    .expr(*class_expr)
                    .expect("instantiation target belongs to its retained file owner");
                let args = arenas.args(args.clone());
                match &class_expr.kind {
                    ExprKind::Name(name) => {
                        let Some(name) = arenas.qualified_name(name) else {
                            return Type::mixed();
                        };
                        match name.written.as_ref() {
                            "self" | "parent" | "static" => {
                                let raw = match name.written.as_ref() {
                                    "self" => Type::self_ref(Vec::new()),
                                    "parent" => Type::parent_ref(),
                                    _ => Type::static_ref(Vec::new()),
                                };
                                match &self.current_class {
                                    Some(c) => self.resolve_pseudo(&raw, c),
                                    None => raw,
                                }
                            }
                            // `new ReflectionClass($view)`-shaped constructor-
                            // argument-driven class-level generic inference - the
                            // same `Relation::infer_template_bindings`/`TemplateBindings`
                            // primitive `infer_call_return_type` uses for
                            // ordinary call-site inference, just binding against
                            // a constructor's params instead of a function's.
                            // Rebind to the registry's canonical-case spelling
                            // first (mirroring `check_expr`'s identical fix)
                            // since `lookup_method`'s ancestor walk needs exact
                            // case.
                            _ => {
                                let canonical = self.resolve_class_name(name);
                                let templates = self
                                    .registry
                                    .class_by_name(canonical.as_str())
                                    .map(|info| info.templates().as_slice())
                                    .unwrap_or(&[]);
                                if templates.is_empty() {
                                    Type::object(canonical.into(), Vec::new())
                                } else {
                                    let mut bindings: TemplateBindings =
                                        TemplateBindings::default();
                                    if let Some(sig) =
                                        self.registry.lookup_method(&canonical, "__construct")
                                    {
                                        let resolved_args =
                                            resolve_call_args(arenas, args, sig.params());
                                        for (param, slot) in
                                            sig.params().iter().zip(resolved_args.iter())
                                        {
                                            let Some(arg) = slot else { continue };
                                            let arg_ty = self.infer_expr(arg, locals);
                                            // A bare string literal naming a
                                            // real, registered class binds a
                                            // `class-string<T>` slot exactly like
                                            // an explicit `Foo::class` would,
                                            // mirroring real PHPStan's
                                            // `ConstantStringType::isClassString()`.
                                            let promoted = param
                                                .ty()
                                                .promote_class_string_literals(&arg_ty, &|s| {
                                                    self.registry
                                                        .lookup_class(s)
                                                        .map(|(c, _)| c.to_string())
                                                });
                                            self.relation().infer_template_bindings(
                                                param.ty(),
                                                &promoted,
                                                &mut bindings,
                                            );
                                        }
                                    }
                                    // A template the constructor's arguments
                                    // never bound is left erased (trimmed off the
                                    // trailing end, same as a real bare `new Box()`
                                    // reference with fewer type arguments than
                                    // declared) rather than eagerly filled with
                                    // `Mixed`: `substitute_scoped`'s `@template
                                    // T = Default` fallback only triggers for a
                                    // name genuinely absent from the zip against
                                    // `templates`, not one explicitly bound to
                                    // `Mixed`. A template bound in the middle
                                    // with a later one still unbound has no such
                                    // "erase the tail" option (PHP's type-argument
                                    // syntax can't skip a middle position
                                    // either), so it does still fall back to
                                    // `Mixed` there.
                                    let last_bound = templates
                                        .iter()
                                        .rposition(|t| bindings.contains_key(t.as_ref()))
                                        .map(|i| i + 1)
                                        .unwrap_or(0);
                                    let info = self.registry.class_by_name(canonical.as_str());
                                    let bounds = self
                                        .registry
                                        .effective_template_bounds_by_name(canonical.as_str());
                                    let bound_args = templates[..last_bound]
                                        .iter()
                                        .map(|t| match bindings.get(t.as_ref()) {
                                            Some(ty) => match info {
                                                Some(info) => generalize_inferred_class_template(
                                                    info, &bounds, t, ty,
                                                ),
                                                None => ty.clone(),
                                            },
                                            None => Type::mixed(),
                                        })
                                        .collect();
                                    Type::object(canonical.into(), bound_args)
                                }
                            }
                        }
                    }
                    _ => Type::mixed(),
                }
            }
            ExprKind::Call(callee, args) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("calls retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let callee = arenas
                    .expr(*callee)
                    .expect("call callee belongs to its retained file owner");
                let args = arenas.args(args.clone());
                match &callee.kind {
                    ExprKind::Name(name) => match arenas.qualified_name(name) {
                        Some(name) => self
                            .registry
                            .resolve_function(name)
                            .map(|(resolved_name, sig)| {
                                self.infer_builtin_call_return_type(
                                    resolved_name,
                                    sig,
                                    args,
                                    locals,
                                )
                            })
                            .unwrap_or_else(|| {
                                self.call_fallback_return_type(arenas, callee, args, locals)
                            }),
                        None => Type::mixed(),
                    },
                    _ => self.call_fallback_return_type(arenas, callee, args, locals),
                }
            }
            ExprKind::MethodCall(..)
            | ExprKind::PropertyAccess(..)
            | ExprKind::Index(_, Some(_)) => {
                let link = self.chain_link_type(e, locals);
                // A nullsafe access in the chain that meets `null` makes the
                // whole chain `null`.
                if self
                    .syntax_arenas()
                    .is_some_and(|arenas| postfix_chain_has_nullsafe(arenas, e))
                    && !link.is_mixed()
                {
                    self.relation().union(link, Type::null())
                } else {
                    link
                }
            }
            ExprKind::StaticCall(target, method, args) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("static calls retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let args = arenas.args(args.clone());
                if let Some(property) = target.parent_hook_property(arenas) {
                    return self.parent_hook_call_type(property, method);
                }
                if self.collect_member_refs
                    && let Some(class) = self.infer_static_target_class(target, locals)
                    && let Some((owner, _)) = self.lookup_method_with_owner(&class, method)
                {
                    self.record_method_ref(self.expr_span(e.id), &owner, method);
                    // The `::cases()` escape hatch - see
                    // `Checker::escaped_enums`'s own doc comment. `owner`
                    // is already the registry's own canonically-declared
                    // name (same reasoning as `check_expr`'s identical
                    // hook).
                    if method.eq_ignore_ascii_case("cases")
                        && self
                            .registry
                            .class_by_name(owner.as_ref())
                            .is_some_and(|c| c.kind() == ClassKind::Enum)
                    {
                        self.record_enum_escape(&owner);
                    }
                }
                self.infer_static_target_class(target, locals)
                    .and_then(|class| {
                        self.static_call_method(target, &class, method)
                            .map(|(_, sig)| {
                                let sig = sig.as_ref();
                                let resolved_args = resolve_call_args(arenas, args, sig.params());
                                let resolved =
                                    self.infer_call_return_type(sig, &resolved_args, locals);
                                let resolved = self.narrow_enum_from_call(
                                    arenas, &class, method, args, resolved, locals,
                                );
                                // The `::from()`/`::tryFrom()` literal-argument
                                // signal - see `check_expr`'s
                                // identical fix just above `ExprKind::
                                // ClassConstFetch` for why this reads the
                                // already-computed type rather than
                                // threading a new span parameter through
                                // `narrow_enum_from_call`.
                                if let Some((case_class, case)) = resolved.as_enum_case() {
                                    self.record_member_ref(self.expr_span(e.id), case_class, case);
                                }
                                if let Some(ty) =
                                    self.class_argument_return(arenas, &class, method, args, locals)
                                {
                                    return ty;
                                }
                                self.resolve_static_call_return(&resolved, target, &class)
                            })
                    })
                    .unwrap_or(Type::mixed())
            }
            ExprKind::DynamicStaticCall(target, name, args) => {
                let Some(name) = self.syntax_arenas().and_then(|arenas| arenas.expr(*name)) else {
                    return Type::mixed();
                };
                match &self.infer_expr(name, locals).constant_strings_text()[..] {
                    [method] => {
                        let arena = self.synthetic_expr_arena();
                        let call = self.static_call(
                            &arena,
                            target,
                            method.to_string(),
                            args.clone(),
                            self.expr_span(e.id),
                        );
                        self.infer_expr(&call, locals)
                    }
                    _ => Type::mixed(),
                }
            }
            ExprKind::DynamicClassConstFetch(target, name) => {
                let names: Option<Vec<String>> = {
                    let Some(name) = self.syntax_arenas().and_then(|arenas| arenas.expr(*name))
                    else {
                        return Type::mixed();
                    };
                    let names = self.infer_expr(name, locals);
                    let names = names.constant_strings_text();
                    (!names.is_empty()).then(|| names.iter().map(|n| n.to_string()).collect())
                };
                let arena = self.synthetic_expr_arena();
                names
                    .into_iter()
                    .flatten()
                    .map(|member| {
                        let fetch =
                            self.constant_fetch(&arena, target, member, self.expr_span(e.id));
                        self.infer_expr(fetch, locals)
                    })
                    .union_all_with(&self.relation())
                    .unwrap_or(Type::mixed())
            }
            ExprKind::ClassNameFetch(target) => {
                match self.infer_static_target_class(target, locals) {
                    Some(class) => Type::class_string(Type::object(class.into(), Vec::new())),
                    None => Type::string(),
                }
            }
            ExprKind::DynamicStaticPropertyFetch(target, _) => {
                if self.collect_member_refs {
                    self.record_dynamic_static_member_escape(target);
                }
                Type::mixed()
            }
            ExprKind::StaticPropertyFetch(target, prop_name) => {
                // A static property (`self::$prop`) can be narrowed the
                // same way an instance property already is above - check
                // for a locally-narrowed override before falling back to
                // its own declared type.
                if locals.may_have_member_narrowing()
                    && let Some(class_name) = target.as_name()
                    && let Some(narrowed) = locals
                        .find(&locals.static_prop_data(class_name, &prop_name.name))
                        .and_then(|key| locals.get(key))
                {
                    return narrowed.clone();
                }
                let Some(class) = self.infer_static_target_class(target, locals) else {
                    return Type::mixed();
                };
                if self.collect_member_refs
                    && let Some((owner, _)) =
                        self.registry.lookup_property_with_owner(&class, prop_name)
                {
                    self.record_member_ref(self.expr_span(e.id), &owner, prop_name);
                }
                self.registry
                    .lookup_property(&class, prop_name)
                    .map(|p| self.resolve_pseudo(p.ty(), &class))
                    .unwrap_or(Type::mixed())
            }
            ExprKind::ClassConstFetch(target, member) => {
                let Some(class) = self.infer_static_target_class(target, locals) else {
                    return Type::mixed();
                };
                if self.collect_member_refs
                    && let Some((owner, _)) = self.registry.lookup_const_with_owner(&class, member)
                {
                    self.record_member_ref(self.expr_span(e.id), &owner, member);
                }
                self.registry
                    .lookup_const_type_with_owner(&class, member)
                    .map(|(owner, constant_type)| {
                        let ty = self.settings.constants.class_constant_type(
                            &owner,
                            member,
                            constant_type,
                        );
                        self.resolve_pseudo(&ty, &class)
                    })
                    .unwrap_or(Type::mixed())
            }
            ExprKind::Ternary(cond, then_, else_) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("ternary expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let cond = arenas
                    .expr(*cond)
                    .expect("ternary condition belongs to its retained file owner");
                let then_ = then_.as_ref().map(|id| {
                    arenas
                        .expr(*id)
                        .expect("ternary branch belongs to its retained file owner")
                });
                let else_ = arenas
                    .expr(*else_)
                    .expect("ternary else branch belongs to its retained file owner");
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.infer_expr(else_, &else_locals);
                match then_ {
                    Some(t) => {
                        let mut then_locals = locals.clone();
                        self.apply_narrowing(cond, &mut then_locals, true);
                        let then_ty = self.infer_expr(t, &then_locals);
                        // See the parallel arm in `check_expr` for why
                        // this must be `Type::union`, not a raw
                        // a raw union.
                        self.relation().union(then_ty, else_ty)
                    }
                    None => Type::mixed(),
                }
            }
            ExprKind::ArrayLiteral(items) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("array literals retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                let items = arenas.array_items(items.clone());
                self.infer_array_literal_type(arenas, items, locals)
            }
            // Routed through the identical `resolve_concat_type` engine
            // as `BinOp::Concat`, as a left-fold over each part (an
            // interpolated string is exactly a chain of concatenations) -
            // `check_expr`'s separate `Interpolated` arm walks each part
            // for a different purpose (implicit `__toString()`/array-to-
            // string checks), not reused here since it doesn't compute or
            // return a refined type. An empty `parts` list can't occur in
            // practice, but folds to plain `string` rather than
            // panicking if it somehow did.
            ExprKind::Interpolated(parts) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("interpolated expressions retain their parsed file owner");
                let arenas = syntax_owner.arenas();
                concat_string_parts(
                    arenas
                        .expr_ids(parts)
                        .iter()
                        .map(|part| {
                            self.infer_expr(
                                arenas.expr(*part).expect(
                                    "interpolation part belongs to its retained file owner",
                                ),
                                locals,
                            )
                            .to_string_type(self.registry)
                            .unwrap_or(Type::string())
                        })
                        .collect(),
                    self.registry,
                )
            }
            // A homogeneous array's element type applies regardless of
            // which key is accessed; optimistically assumes the access is
            // in bounds/present rather than widening to `V|null` for a
            // possibly-missing element - consistent with this checker's
            // general preference for fewer false positives over catching
            // every possible runtime-undefined access.
            ExprKind::Name(name) => self
                .syntax_arenas()
                .and_then(|arenas| arenas.qualified_name(name))
                .map(|name| {
                    self.global_constant_type(name)
                        .unwrap_or_else(|| bare_name_literal_type(name))
                })
                .unwrap_or(Type::mixed()),
            // Mirrors `check_expr`'s `ExprKind::Yield`/`YieldFrom` arms -
            // needed here too, since `assertType()`'s testing-DSL
            // recognition computes its "actual" type via `infer_expr`,
            // not `check_expr`.
            ExprKind::Yield(_, _) => self
                .current_declared_return_type
                .as_ref()
                .map(|t| generator_template_param(t, self.registry, "TSend"))
                .unwrap_or(Type::mixed()),
            ExprKind::YieldFrom(inner) => {
                let Some(inner) = self.syntax_arenas().and_then(|arenas| arenas.expr(*inner))
                else {
                    return Type::mixed();
                };
                generator_template_param(&self.infer_expr(inner, locals), self.registry, "TReturn")
            }
            _ => Type::mixed(),
        }
    }

    /// A closure/arrow-fn literal's own declared `: T` return hint isn't
    /// necessarily the most precise answer - unlike an ordinary named
    /// function/method (already reconciled against its own inferred body
    /// return type elsewhere), a literal's signature previously used the
    /// bare hint unconditionally. Real PHPStan doesn't: confirmed via
    /// `\PHPStan\dumpType()` that `array_map(static fn (OptionallyQualified
    /// Name $name): string => $name->getUnqualifiedName()->getValue(),
    /// ...)` infers the closure literal's own type as `Closure(Name):
    /// non-empty-string` - the body's own more precise `getValue(): non-
    /// empty-string`, not the bare `: string` hint - so `array_map()`'s
    /// result element type comes out `non-empty-string` too (a real
    /// Piwigo false positive fixed: `DbMaintenanceRepositoryTest`'s own
    /// `OptionallyQualifiedName::unquoted(non-empty-string $unqualifiedName,
    /// ...)` argument check). Only trusts `inferred` when it's a real,
    /// non-`Mixed` subtype of the declared hint - anything else (a body
    /// that doesn't type-check against its own declared hint at all, or
    /// simply couldn't be inferred) falls back to the hint unchanged,
    /// exactly today's prior behavior.
    fn prefer_inferred_over_declared_return(
        &self,
        hint: Option<&TypeHint>,
        inferred: Type,
    ) -> Type {
        let Some(hint) = hint else {
            return inferred;
        };
        let declared = self.resolve_type_hint(Some(hint));
        if !inferred.is_mixed() && self.accepts(&declared, &inferred).yes() {
            inferred
        } else {
            declared
        }
    }

    /// Infers a closure or arrow-fn's own return type *as if* its
    /// parameters were bound to `param_overrides` (positionally),
    /// instead of the `Mixed` they'd otherwise get standalone (a
    /// closure/arrow-fn's declared param, with no further context, has
    /// no type unless natively hinted) - this is what lets
    /// `array_map(fn($x) => $x * 2, $ints)` infer a real `array<int>`
    /// return instead of `array<mixed>`, and `array_reduce` bind both its
    /// callback's `$carry`/`$item` params at once. Only the common
    /// exact-arity, non-variadic, non-by-ref shape is handled - a
    /// mismatched param count, or any variadic/by-ref param, safely
    /// returns `None`, falling back to the caller's generic (imprecise
    /// but correct) behavior. Mirrors the closure-capture-scope
    /// construction `check_expr`'s `ExprKind::Closure`/`ExprKind::ArrowFn`
    /// arms do, but on this separate, read-only inference pass rather
    /// than the diagnostic-producing one.
    pub(crate) fn infer_closure_return_with_param_overrides(
        &self,
        callback: &ExprNode,
        param_overrides: &[Type],
        locals: &Locals,
    ) -> Option<Type> {
        let syntax_owner = self.syntax_tree_owner()?;
        let arenas = syntax_owner.arenas();
        match &callback.kind {
            ExprKind::ArrowFn(closure_id) => {
                let ClosurePayload::ArrowFn(arrow) = arenas.closure(*closure_id)? else {
                    return None;
                };
                let params = arenas.params(arrow.params.clone());
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut arrow_locals = locals.clone();
                for (p, ty) in params.iter().zip(param_overrides) {
                    arrow_locals.insert_var(&p.name, ty.clone());
                }
                let inferred = self.infer_expr(arenas.expr(arrow.body)?, &arrow_locals);
                Some(
                    self.prefer_inferred_over_declared_return(
                        arrow.return_type.as_deref(),
                        inferred,
                    ),
                )
            }
            ExprKind::Closure(closure_id) => {
                let ClosurePayload::Closure(closure) = arenas.closure(*closure_id)? else {
                    return None;
                };
                let params = arenas.params(closure.params.clone());
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut closure_locals: Locals = locals.closure_scope();
                for u in arenas.closure_uses(&closure.uses) {
                    let ty = locals.get_var(&u.name).cloned().unwrap_or(Type::mixed());
                    closure_locals.insert_var(&u.name, ty);
                }
                if !closure.is_static
                    && let Some(this_ty) = locals.get_var("this")
                {
                    closure_locals.insert_var("this", this_ty.clone());
                }
                for (p, ty) in params.iter().zip(param_overrides) {
                    closure_locals.insert_var(&p.name, ty.clone());
                }
                // Only the common single-implicit-return-statement body
                // shape is handled (mirrors check_offset_existence's
                // "only handle the shape that actually comes up, don't
                // guess at the rest" precedent) - a closure with a more
                // complex body falls back to the declared hint (still
                // `None` when there isn't one), never silently discarding
                // an explicit `: T` the caller could otherwise have kept
                // trusting - regressed once already: a multi-statement
                // body (an `if`-guard before the real `return`, a real
                // corpus shape) used to still return the hint here, since
                // the old code checked for one before ever looking at the
                // body at all.
                match arenas.stmt_ids(&closure.body) {
                    [stmt_id]
                        if let Some(StmtNode {
                            kind: StmtKind::Return(Some(expr_id)),
                            ..
                        }) = arenas.stmt(*stmt_id) =>
                    {
                        let inferred = self.infer_expr(arenas.expr(*expr_id)?, &closure_locals);
                        Some(self.prefer_inferred_over_declared_return(
                            closure.return_type.as_deref(),
                            inferred,
                        ))
                    }
                    _ => match closure.return_type.as_deref() {
                        Some(hint) => Some(self.resolve_type_hint(Some(hint))),
                        // No hint: what its return statements give, which
                        // is `void` for a body that returns nothing.
                        None => self.closure_syntactic_return_type(
                            arenas,
                            closure,
                            locals,
                            Some(param_overrides),
                        ),
                    },
                }
            }
            // A first-class callable (`intval(...)`, `TagId::from(...)`,
            // PHP 8.1) used as `array_map`'s callback would otherwise
            // fall to this function's `_ => None`, discarding the
            // list-ness/non-emptiness/mapped-element-type precision the
            // array_map special case provides. Resolves the wrapped
            // call's signature and binds `param_overrides` against its
            // declared parameter types the same way
            // `infer_call_return_type` does for an ordinary call, just
            // without real argument expressions to read types from.
            ExprKind::FirstClassCallable(inner) => {
                let owner = self
                    .syntax_tree_owner()
                    .expect("first-class callable retains its file owner");
                let inner = owner
                    .arenas()
                    .expr(*inner)
                    .expect("first-class callable target belongs to its retained file owner");
                let target = self.first_class_callable_target(&inner.kind, locals);
                if self.collect_member_refs
                    && let Some(FirstClassTarget {
                        class: Some(class),
                        name,
                        ..
                    }) = &target
                    && let Some((owner, _)) = self.lookup_method_with_owner(class, name)
                {
                    self.record_method_ref(self.expr_span(callback.id), &owner, name);
                }
                let (sig, class) = match target {
                    Some(t) => (Some(t.sig), t.class),
                    None => (None, None),
                };
                sig.map(|s| {
                    let mut bindings: TemplateBindings = TemplateBindings::default();
                    for (param, ty) in s.params().iter().zip(param_overrides) {
                        let promoted = param.ty().promote_class_string_literals(ty, &|s| {
                            self.registry.lookup_class(s).map(|(c, _)| c.to_string())
                        });
                        self.relation().infer_template_bindings(
                            param.ty(),
                            &promoted,
                            &mut bindings,
                        );
                    }
                    let substituted = s
                        .return_type()
                        .substitute_templates(&bindings, &self.relation());
                    match &class {
                        Some(class) => self.resolve_pseudo(&substituted, class),
                        None => substituted,
                    }
                })
            }
            _ => None,
        }
    }

    /// `textDocument/references`/`textDocument/rename`'s Tier-2 own
    /// dedicated collection pass, run once per file alongside
    /// `check_program` when `collect_member_refs` is on - NOT folded into
    /// `check_program`'s own ordinary statement walk, and never called
    /// for an ordinary diagnostic-only check. A parameter/property
    /// default value and a class/interface/trait/enum constant's value
    /// are never visited by `check_expr`'s own top-to-bottom statement
    /// walk at all (confirmed by direct trace: the only other `.default`
    /// consumers in this crate are unrelated unused-variable/`@api`-usage
    /// walkers, none of which resolve a true declaring owner) - each
    /// found expression is fed through `infer_expr` purely to trigger its
    /// own now-hooked `ClassConstFetch`/`StaticPropertyFetch`/`StaticCall` arms; the returned
    /// `Type` is discarded, only the `record_member_ref` side effect
    /// matters. `check_typed_class_constant_values` already runs
    /// `infer_expr` on a *typed* constant's own value independently -
    /// re-visiting it here too is harmless, since `member_ref_sites` is a
    /// `HashSet`.
    ///
    /// Recurses into every function/method's own body to reach a nested
    /// (conditionally-declared) function/class/enum - real, legal PHP.
    /// Deliberately does NOT chase into a `Closure`/`ArrowFn` literal's
    /// own parameter defaults - a real, narrower residual than the
    /// top-level/method-parameter gap this method's own trace found,
    /// disclosed rather than silently dropped (see this item's own
    /// PROGRESS notes).
    pub(crate) fn collect_default_and_const_value_member_refs(
        &mut self,
        arenas: &syntax::ast::NodeArenas,
        program: &[StmtId],
    ) {
        for &stmt in program {
            self.collect_member_ref_defaults_stmt(arenas, stmt);
        }
    }

    fn collect_member_ref_defaults_params(
        &mut self,
        arenas: &syntax::ast::NodeArenas,
        params: &[Param],
    ) {
        for p in params {
            if let Some(d) = p.default.and_then(|id| arenas.expr(id)) {
                self.infer_expr(d, &self.new_locals());
            }
        }
    }

    fn collect_member_ref_defaults_function(
        &mut self,
        arenas: &syntax::ast::NodeArenas,
        f: &FunctionDecl,
    ) {
        self.collect_member_ref_defaults_params(arenas, arenas.params(f.params.clone()));
        for &s in f.body_stmts(arenas) {
            self.collect_member_ref_defaults_stmt(arenas, s);
        }
    }

    /// Every declaration in `stmt`'s body, nested ones included.
    fn collect_member_ref_defaults_stmt(&mut self, arenas: &syntax::ast::NodeArenas, stmt: StmtId) {
        syntax::visit::arena::any_stmt_in_body(arenas, stmt, |s| {
            self.collect_member_ref_defaults_decl(arenas, s);
            false
        });
    }

    fn collect_member_ref_defaults_decl(
        &mut self,
        arenas: &syntax::ast::NodeArenas,
        stmt: &StmtNode,
    ) {
        match &stmt.kind {
            StmtKind::FunctionDecl(id) => {
                if let Some(function) = arenas.function(*id) {
                    self.collect_member_ref_defaults_function(arenas, function);
                }
            }
            StmtKind::ClassDecl(c) => {
                let c = arenas.class_decl(c.clone());
                let Some(def) = arenas.class(c.def) else {
                    return;
                };
                for p in def.properties(arenas) {
                    if let Some(d) = p.default.and_then(|id| arenas.expr(id)) {
                        self.infer_expr(d, &self.new_locals());
                    }
                }
                for cst in def.consts(arenas) {
                    if let Some(value) = arenas.expr(cst.value) {
                        self.infer_expr(value, &self.new_locals());
                    }
                }
                for m in arenas.methods(def.methods.clone()) {
                    self.collect_member_ref_defaults_function(arenas, &m.function);
                }
            }
            _ => {}
        }
    }

    /// `??`'s result is usually just `r`'s type (`ExprKind::Binary(
    /// BinOp::Coalesce, ...)` and this function's `BinOp::Coalesce` arm
    /// both start from `r_ty` alone) - widening away from that is
    /// deliberately narrow, not a blanket "always union `non_null(l)`
    /// with `r`" fix: that would pollute the common and currently-precise
    /// `$x ?? 'literal'` pattern (where `$x`'s real type is often
    /// genuinely `Mixed`) with unwanted `Mixed` noise across a much
    /// broader, currently-well-served set of call sites. Only widens when
    /// `l`'s real non-null type is array (generic/unshaped) and
    /// `r`'s type is a narrower array shape - `$details =
    /// $row->details ?? [];` where `$row->details: ?array` genuinely
    /// holds arbitrary keys at runtime, which would otherwise collapse to
    /// the literal empty `[]` shape from the fallback alone, making every
    /// later `$details['key']` read look like a typo.
    fn widen_coalesce_result(&self, l: &ExprNode, r_ty: Type, locals: &Locals) -> Type {
        // `$x ?? null` is a real, if unusual, no-op: if `$x` is null the
        // result is null either way, and otherwise the result is exactly
        // `$x` itself - so its type is exactly `$x`'s own type,
        // unnarrowed, not the literal `r_ty` (`null`) this function
        // otherwise trusts as the fallback's type. Otherwise an unshaped-
        // array element read (`$post['selection'] ?? null`, `$post:
        // array` with no shape, so `Mixed`) would collapse all the way
        // down to bare `null`, when code checking the original
        // expression with `is_array($selection_raw)` immediately after
        // should still be checking a real `Mixed`.
        if r_ty.is_null().yes() {
            let l_ty = self.infer_expr(l, locals);
            // The no-op reasoning above only holds when `l` is a bare
            // variable - `locals`'s tracked type for one already
            // reflects every way it can be null (a prior narrowing, an
            // untyped param, ...), so trusting it completely is correct.
            // An array/property access (or any other expression `??`
            // exists specifically because this checker can't otherwise
            // prove is always set) is different: `infer_expr` for
            // `$group[$index]` on an `array<int, string>`-typed `$group`
            // returns plain `string`, with no "the key might not exist"
            // notion folded in - the exact real-PHP case `?? null`
            // idiomatically guards against. Unioning `Null` back in here
            // is always safe for the already-covered `Mixed` case too
            // (`Type::union` collapses `Mixed | Null` back to plain
            // `Mixed`).
            return if matches!(l.kind, ExprKind::Variable(_)) {
                l_ty
            } else {
                self.relation().union(l_ty, Type::null())
            };
        }
        let l_ty = self.infer_expr(l, locals).without(&Type::null());
        // `$x ?? throw new Foo();` (or any other never-returning right
        // side: `exit`, a `never`-typed call) - falling through to the
        // generic `_ => r_ty` fallback below would make the whole
        // expression, and by extension any `$x = ... ?? throw ...;`
        // assignment statement built from it, infer as `never`
        // itself, which `Completion::terminates`'s `StmtKind::Expression` case
        // then reads as "this plain assignment statement always ends
        // control flow," wrongly marking every statement after it
        // unreachable. The real semantics: reaching anything past this
        // expression at all already proves the right side never ran, so
        // the result is exactly the left side's own non-null type, the
        // same as the `?? null` case above computes.
        if r_ty.is_never() {
            return l_ty;
        }
        // Still scoped to exactly array on the left, matching
        // this function's documented, deliberately narrow boundary (an
        // ArrayShape on the left stays uncovered) - also recognizes a
        // `list<T>`/`non-empty-array<T>`-typed left side (a real
        // `Intersection`), which would otherwise lose its type here the
        // same way a plain array would.
        let l_is_array = l_ty.is_array().yes() && l_ty.as_array_shape().is_none();
        match &r_ty {
            r if l_is_array && r.as_array_shape().is_some() => l_ty,
            // The general case must not discard `l_ty` entirely, keeping
            // only `r_ty` - that's wrong for `??`'s semantics regardless
            // of precision: the result is `$expr` itself when it's not
            // null (already computed above, with `Null` excluded) or
            // `$default` when it is - a real union of both outcomes, not
            // "the default's type, unconditionally."
            _ => self.relation().union(l_ty, r_ty),
        }
    }

    pub(crate) fn infer_binary(
        &self,
        op: &BinOp,
        l: &ExprNode,
        r: &ExprNode,
        locals: &Locals,
    ) -> Type {
        match op {
            // `resolve_concat_type` is a full port of real PHPStan's
            // `InitializerExprTypeResolver::resolveConcatType`: real
            // constant-folding for two literal operands, and AND/OR
            // composition of every accessory kind (`NonFalsyString`/
            // `NonEmptyString`/`LiteralString`/`LowercaseString`/
            // `UppercaseString`/`NumericString`), not just non-falsiness.
            BinOp::Concat => self
                .infer_expr(l, locals)
                .concat(&self.infer_expr(r, locals), self.registry),
            // Real constant folding, mirroring real PHPStan's
            // `BooleanAndHandler`/`BooleanOrHandler::resolveType` -
            // without this, `check_boolean_and_or`'s "whole result"
            // diagnostic (`booleanAnd.always*`/`booleanOr.always*`) could
            // never fire at all, since it has nothing but a permanent
            // bare `bool` to match against. A definitely-false left
            // side makes `&&`'s result definitely false without even
            // considering the right side; otherwise the right side is
            // inferred against `l`'s truthy-narrowed scope (the same
            // `apply_narrowing` every call site narrows into before
            // checking the right operand), since that's the only scope
            // the right side is ever actually reached in.
            BinOp::And | BinOp::LogicalAnd => {
                let l_ty = self.infer_expr(l, locals);
                if l_ty.is_false().yes() {
                    return Type::bool_literal(false);
                }
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, true);
                let r_ty = self.infer_expr(r, &narrowed);
                if r_ty.is_false().yes() {
                    Type::bool_literal(false)
                } else if l_ty.is_true().yes() && r_ty.is_true().yes() {
                    Type::bool_literal(true)
                } else {
                    Type::bool()
                }
            }
            BinOp::Or | BinOp::LogicalOr => {
                let l_ty = self.infer_expr(l, locals);
                if l_ty.is_true().yes() {
                    return Type::bool_literal(true);
                }
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, false);
                let r_ty = self.infer_expr(r, &narrowed);
                if r_ty.is_true().yes() {
                    Type::bool_literal(true)
                } else if l_ty.is_false().yes() && r_ty.is_false().yes() {
                    Type::bool_literal(false)
                } else {
                    Type::bool()
                }
            }
            // `xor` doesn't short-circuit, so both sides are always
            // inferred against the same, un-narrowed scope - confirmed
            // against `BinaryOpHandler::resolveType`'s own
            // `BinaryOp\LogicalXor` arm.
            BinOp::LogicalXor => {
                let l_ty = self.infer_expr(l, locals).to_boolean();
                let r_ty = self.infer_expr(r, locals).to_boolean();
                match (l_ty.bool_literal_value(), r_ty.bool_literal_value()) {
                    (Some(a), Some(b)) => Type::bool_literal(a != b),
                    _ => Type::bool(),
                }
            }
            BinOp::Identical | BinOp::NotIdentical => Type::bool(),
            // Mirrors real PHPStan's `ConstantLooseComparisonRule` -
            // `Type::loosely_equals`'s doc comment has the full
            // scope/version-gating reasoning.
            BinOp::Eq | BinOp::NotEq => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                match self
                    .registry
                    .php_version_consensus(|v| lt.loosely_equals(&rt, v))
                {
                    Some(equal) => {
                        let outcome = if matches!(op, BinOp::Eq) {
                            equal
                        } else {
                            !equal
                        };
                        if outcome {
                            Type::bool_literal(true)
                        } else {
                            Type::bool_literal(false)
                        }
                    }
                    None => Type::bool(),
                }
            }
            // Mirrors real PHPStan's
            // `NumberComparisonOperatorsConstantConditionRule`: the same
            // relationship as `Eq`/`NotEq` above has to
            // `ConstantLooseComparisonRule` - a standalone rule, never
            // folded into `if`/`while`'s generic constant-condition
            // family (its own `smaller.always*`/`greater.always*`/
            // `smallerOrEqual.always*`/`greaterOrEqual.always*`
            // identifiers, not `if.always*`).
            // `should_skip_constant_condition` (`checker/control_flow.rs`)
            // already excludes all four of these ops from the generic
            // `if`/`while` path - this constant-folding half only feeds
            // `check_constant_numeric_comparison`'s standalone diagnostic
            // below.
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ordering) = ordering_op(op)
                    && let Some(outcome) = self.ordering_outcome(&lt, ordering, &rt)
                {
                    if outcome {
                        Type::bool_literal(true)
                    } else {
                        Type::bool_literal(false)
                    }
                } else {
                    Type::bool()
                }
            }
            // The spaceship operator returns -1/0/1 (an int), not a bool -
            // its single most common real use is returning a value
            // directly from a comparator (`usort`/`uasort`/...).
            //
            // `GMP`/`BcMath\Number`'s overloaded spaceship still returns
            // the same `-1|0|1` range, just computed by the object's own
            // comparison rather than a plain numeric one -
            // `binary_operator_overload_result` produces the identical
            // `IntRange(-1, 1)` either way, so this also refines how
            // precisely non-overloaded spaceship is typed.
            BinOp::Spaceship
            | BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Mod
            | BinOp::Div => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                // An operand that never has a value leaves none for the result.
                if lt.is_never() || rt.is_never() {
                    return Type::never();
                }
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                // `<=>` gives what the comparison can come to: `-1`, `0`,
                // and `1` for greater or uncomparable.
                if *op == BinOp::Spaceship {
                    let outcomes =
                        self.relation()
                            .outcomes(&lt, &rt, self.comparison_php_version());
                    let mut values = Vec::new();
                    if outcomes.contains(types::Outcomes::LESS) {
                        values.push(Type::int_literal(-1));
                    }
                    if outcomes.contains(types::Outcomes::EQUAL) {
                        values.push(Type::int_literal(0));
                    }
                    if outcomes.contains(types::Outcomes::GREATER)
                        || outcomes.contains(types::Outcomes::UNCOMPARABLE)
                    {
                        values.push(Type::int_literal(1));
                    }
                    if !values.is_empty() {
                        return self.relation().union_of(values);
                    }
                }
                arithmetic_op(op)
                    .map_or_else(Type::mixed, |op| Type::binary_operation(op, &lt, &rt))
            }
            BinOp::Coalesce => {
                let r_ty = self.infer_expr(r, locals);
                self.widen_coalesce_result(l, r_ty, locals)
            }
            // `$value |> callable` calls callable($value) - its return
            // type depends on resolving whatever expression `callable` is
            // (a string, a closure, a first-class callable reference...)
            // to a real signature, which this checker doesn't attempt.
            BinOp::Pipe => Type::mixed(),
        }
    }
}

/// After checking `&&`/`||`'s right operand against a cloned, possibly-
/// narrowed `Locals`, merges back only the keys that are both new (absent
/// from `locals` before checking the right operand) and a real PHP
/// variable name (`is_real_variable_key`) - a genuine definitional side
/// effect (a by-ref out-param like `preg_match`'s `$matches`, or a plain
/// assignment) written as the right operand of `&&`/`and`/`||`/`or`
/// should persist regardless of the whole expression's truth value,
/// matching this codebase's "assume it might be defined" philosophy
/// elsewhere (`predefine_byref_or_unknown_args`).
///
/// Narrowing-derived synthetic keys (`key_of`'s own
/// `"base->prop"`/`"base[key]"` shapes) must **not** be merged this way,
/// even though they're just as "new" by the same test - they're
/// conditional on the right operand actually having run (true only when
/// the left operand was truthy), not a real, unconditional definition.
/// Merging them would leak conditional narrowing straight past the whole
/// `&&`/`||` expression: for `if ($arg->value instanceof Variable &&
/// $arg->value->name) {...} else {...}`, it would insert `$arg->value`'s
/// narrowed type into the real `locals` *before* the `if`'s own
/// then/else split even runs, contaminating the `else` branch (where the
/// condition was false, so `$arg->value` is not known to be a
/// `Variable`) and anything after the whole `if` statement.
/// `Locals::is_real_variable` is exactly the discriminator that excludes
/// them, and composes correctly for a chained `A and B and C`
/// (left-associative `And(And(A,B),C)`): the inner node's own filtered
/// merge already lands B's new plain-variable defs in the outer node's
/// real `locals` before the outer node clones it for `C`.
fn merge_new_variable_definitions(locals: &mut Locals, narrowed: &Locals) {
    for (key, ty) in narrowed.iter() {
        if narrowed.is_real_variable(key) && !locals.contains_key(key) {
            // `key`'s real declaration (a by-ref out-param, or a plain
            // assignment) already recorded its true provenance inside
            // `narrowed` when it happened - carried across unchanged
            // here, not fabricated fresh at this merge point.
            let provenance = narrowed.get_provenance(key).cloned().unwrap_or_default();
            locals.insert_with_provenance(key, ty.clone(), provenance);
        }
    }
}

/// Whether `ty` is a type PHP itself can never legally use as an array
/// key (`int`/`string`/`bool`/`float`/`null` all being legal - PHP
/// coerces the latter three at runtime) - `Some(description)` only when
/// every part of `ty` is confidently illegal, `None` for anything
/// ambiguous (`Mixed`, a union mixing legal and illegal members, an
/// unresolved template, ...), matching this project's "don't guess"
/// leniency elsewhere. An object covers both a real class
/// instance and this project's `resource`/`Closure`-as-object
/// representations - none of which PHP ever accepts as a key, even one
/// with a real `__toString()` (array-key coercion has no string-cast
/// fallback the way string context does). `php_version` is `None` (this
/// project's "no target configured, don't check" convention) unless the
/// caller has a real, configured `Registry::php_version` to check
/// against, mirroring real PHPStan's `AllowedArrayKeysTypes::getType`,
/// whose `float`/`null` legality depends on the target `PhpVersion`.
pub(crate) fn invalid_array_key_description(
    ty: &Type,
    php_version: Option<types::php_version::PhpVersion>,
) -> Option<String> {
    let deprecated = |feature: types::php_version::PhpFeature| {
        php_version.is_some_and(|v| feature.is_available(v))
    };
    let mut descs = Vec::new();
    for member in ty.union_members() {
        // A legal or ambiguous member leaves the whole key unreported.
        let desc = if member.is_array().yes() {
            "array".to_string()
        } else if member.is_object().yes()
            && !member.is_enum_case()
            && let Some(class) = member.single_object_class()
        {
            class.to_string()
        } else if *member == Type::float()
            && deprecated(types::php_version::PhpFeature::DeprecatesFloatArrayKey)
        {
            "float".to_string()
        } else if member.is_null().yes()
            && deprecated(types::php_version::PhpFeature::DeprecatesNullArrayOffset)
        {
            "null".to_string()
        } else {
            return None;
        };
        descs.push(desc);
    }
    Some(descs.join("|"))
}

impl<'a> Checker<'a> {
    /// The type of a call this project can't look up a real signature for.
    ///
    /// Takes `&self`/`locals` so a callee whose inferred type is a shaped
    /// `Closure(...)` (`callable(...)`) can fall back to that
    /// shape's declared return type instead of `Mixed` - the same fix
    /// `check_expr`'s dynamic-call branch makes on the diagnostic-producing
    /// side.
    fn call_fallback_return_type(
        &self,
        arenas: &syntax::ast::NodeArenas,
        callee: &ExprNode,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        self.infer_expr(callee, locals)
            .callable_sig()
            .map_or(Type::mixed(), |sig| {
                if sig.templates.is_empty() {
                    return (*sig.return_type).clone();
                }
                let arg_types: Vec<Type> = args
                    .iter()
                    .filter_map(|a| arenas.expr(a.value))
                    .map(|value| self.infer_expr(value, locals))
                    .collect();
                sig.instantiated_return(&arg_types, &self.relation())
            })
    }

    /// `@phpstan-self-out`: after `$x->method(...)`, `$x` has the tag's
    /// type, its templates bound as for a return type (the class's from
    /// the receiver, the method's from the arguments).
    fn apply_self_out(
        &mut self,
        recv: &ExprNode,
        sig: &FunctionReflection,
        resolved: &[Option<&ExprNode>],
        locals: &mut Locals,
        class: &str,
    ) {
        let Some(out) = sig.self_out() else {
            return;
        };
        let Some(key) = self.key_of_expr(locals, recv) else {
            return;
        };
        let as_return = sig.with_return_type(out.clone());
        let ty = self.infer_call_return_type(&as_return, resolved, locals);
        let ty = self.resolve_pseudo(&ty, class);
        locals.insert(key, ty);
    }

    /// What `parent::$property::get()` returns: the parent's property
    /// type. `set()` returns nothing.
    fn parent_hook_call_type(&self, property: &str, hook: &str) -> Type {
        if !hook.eq_ignore_ascii_case("get") {
            return Type::void();
        }
        let Some(current) = &self.current_class else {
            return Type::mixed();
        };
        self.registry
            .class_references_by_name(current.as_str())
            .and_then(|references| references.extends.first())
            .and_then(|parent| {
                let (owner, sig) = self.registry.lookup_property_with_owner(parent, property)?;
                Some(self.resolve_pseudo(sig.ty(), &owner))
            })
            .unwrap_or(Type::mixed())
    }

    /// The method a static call on `class` reaches, with its declaring
    /// class. Called through `parent::`/`self::`/`static::`, the class
    /// templates are those the calling class binds, as for `$this`.
    fn static_call_method(
        &self,
        target: &StaticTarget,
        class: &str,
        method: &str,
    ) -> Option<(Name, std::borrow::Cow<'a, FunctionReflection>)> {
        let (owner, sig) = self.lookup_method_with_owner(class, method)?;
        let sig = match &self.current_class {
            // Inside the calling class, its own templates stay templates:
            // `parent::` binds the parent's through the class's `@extends`.
            Some(current) if forwards_static_binding(target) => {
                let own = self
                    .registry
                    .class_by_name(current.as_str())
                    .map(|_| self.registry.own_template_args(current))
                    .unwrap_or_default();
                self.substitute_class_templates(sig, current, &own, &owner)
            }
            _ => std::borrow::Cow::Borrowed(sig),
        };
        Some((owner, sig))
    }

    /// A static call's return type with `self`/`static`/`parent`
    /// resolved. Called through `parent::`/`self::`/`static::`, PHP
    /// forwards the late static binding, so `static` is the calling class,
    /// not the target.
    fn resolve_static_call_return(&self, ty: &Type, target: &StaticTarget, class: &str) -> Type {
        let forwards = forwards_static_binding(target);
        match (&self.current_class, forwards) {
            (Some(current), true) => {
                let bound = ty.bind_static(current);
                self.resolve_pseudo(&bound, class)
            }
            _ => self.resolve_pseudo(ty, class),
        }
    }

    /// The function or method a first-class callable's call (`f(...)`,
    /// `$obj->m(...)`, `C::m(...)`) names.
    pub(crate) fn first_class_callable_target(
        &self,
        inner: &ExprKind,
        locals: &Locals,
    ) -> Option<FirstClassTarget<'a>> {
        match inner {
            ExprKind::Call(callee, _) => {
                let arenas = self.syntax_arenas()?;
                let callee = arenas.expr(*callee)?;
                let ExprKind::Name(name) = &callee.kind else {
                    return None;
                };
                let name = arenas.qualified_name(name)?;
                let (resolved, sig) = self.registry.resolve_function(name)?;
                let bare = resolved.rsplit('\\').next().unwrap_or(resolved);
                Some(FirstClassTarget {
                    sig,
                    class: None,
                    name: bare.to_string(),
                })
            }
            ExprKind::StaticCall(target, method, _) => {
                let class = self.infer_static_target_class(target, locals)?;
                let sig = self.registry.lookup_method(&class, method)?;
                Some(FirstClassTarget {
                    sig,
                    class: Some(class),
                    name: method.to_string(),
                })
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                let recv = self.syntax_arenas()?.expr(*recv)?;
                let recv_ty = self.infer_expr(recv, locals);
                let (sig, class) = self.find_method_receiver_class(&recv_ty, method);
                Some(FirstClassTarget {
                    sig: sig?,
                    class,
                    name: method.to_string(),
                })
            }
            _ => None,
        }
    }

    /// A first-class callable's `Closure(...)` type: the named function's
    /// or method's signature, its own templates the closure's; a plain
    /// `Closure` when what it names isn't known.
    fn first_class_callable_type(&self, inner: &ExprNode, locals: &Locals) -> Type {
        let Some(target) = self.first_class_callable_target(&inner.kind, locals) else {
            return Type::object("Closure".into(), Vec::new());
        };
        let sig = types::CallableSig {
            params: target
                .sig
                .params()
                .iter()
                .map(|p| types::CallableParam {
                    ty: p.ty().clone(),
                    variadic: p.variadic(),
                    optional: p.has_default(),
                    by_ref: p.by_ref(),
                })
                .collect(),
            return_type: Box::new(target.sig.return_type().clone()),
            templates: Vec::new(),
        }
        .with_own_templates(&|scope| scope.eq_ignore_ascii_case(&target.name));
        let closure = Type::closure_literal(sig);
        match &target.class {
            Some(class) => self.resolve_pseudo(&closure, class),
            None => closure,
        }
    }

    /// A closure/arrow-fn literal's own type (`Type::closure_literal`):
    /// its declared native parameter types and return type, `self`/
    /// `static` resolved against the enclosing class. `inferred_return`
    /// (what the body actually returns, when the caller has it) is the
    /// return of an unhinted literal, and narrows a hinted one's native
    /// return through `intersect_but_not_never` - what real PHPStan's
    /// `ClosureType` reports either way. An unhinted return with nothing
    /// inferred is `mixed`; an unhinted parameter is
    /// `mixed`, unless `overrides` supplies the type the surrounding
    /// context expects there (see `closure_param_type`). The
    /// callable-shape comparison treats `mixed` as "any".
    pub(crate) fn closure_literal_type(
        &self,
        params: &[Param],
        return_type: Option<&TypeHint>,
        inferred_return: Option<Type>,
        overrides: Option<&[Type]>,
    ) -> Type {
        let resolve = |hint: Option<&TypeHint>| {
            let ty = self.resolve_type_hint(hint);
            match &self.current_class {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            }
        };
        Type::closure_literal(types::CallableSig {
            params: params
                .iter()
                .enumerate()
                .map(|(i, p)| types::CallableParam {
                    ty: match p.type_hint {
                        Some(_) => resolve(p.type_hint.as_ref()),
                        None => self.closure_param_type(p, i, overrides),
                    },
                    variadic: p.variadic,
                    optional: p.default.is_some(),
                    by_ref: p.by_ref,
                })
                .collect(),
            return_type: Box::new(match (return_type, inferred_return) {
                (None, Some(inferred)) => inferred,
                (Some(hint), Some(inferred)) => self
                    .relation()
                    .intersect_but_not_never(&resolve(Some(hint)), inferred),
                (hint, None) => resolve(hint),
            }),
            templates: Vec::new(),
        })
    }

    /// `infer_expr`'s (read-only, no body walk) view of a non-generator
    /// closure literal's inferred return type: each `return`
    /// statement in the body (nested functions/classes excluded, and a
    /// nested closure's statements are never reached - they live inside an
    /// expression) inferred against a scope holding the parameters and the
    /// `use (...)` captures, combined by `closure_inferred_return_type`.
    /// It sees none of the body's own narrowing, unlike `check_expr`'s arm,
    /// which reads the closure's real return points - sound, just less
    /// precise where a return depends on a guard.
    fn closure_syntactic_return_type(
        &self,
        arenas: &NodeArenas,
        closure: &syntax::ast::ClosureExpr,
        locals: &Locals,
        overrides: Option<&[Type]>,
    ) -> Option<Type> {
        let syntax::ast::ClosureExpr {
            params,
            uses,
            body,
            is_generator,
            is_static,
            ..
        } = closure;
        let uses = arenas.closure_uses(uses);
        let params = arenas.params(params.clone());
        let body = arenas.stmt_ids(body);
        if *is_generator {
            return None;
        }
        let mut scope = locals.closure_scope();
        for u in uses {
            if let Some(ty) = locals.get_var(&u.name) {
                scope.insert_var(&u.name, ty.clone());
            }
        }
        if !is_static && let Some(this_ty) = locals.get_var("this") {
            scope.insert_var("this", this_ty.clone());
        }
        for (i, p) in params.iter().enumerate() {
            let ty = self.closure_param_type(p, i, overrides);
            let ty = if p.variadic {
                Type::array(Type::unknown_array_key(), ty)
            } else {
                ty
            };
            scope.insert_var(&p.name, ty);
        }
        // Every `return` in the body, last first.
        let arenas = self.syntax_arenas()?;
        struct Returns(Vec<StmtId>);
        impl syntax::visit::arena::Visitor for Returns {
            fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
            fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}
            fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}
            fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
                syntax::visit::arena::walk_stmt(self, arenas, id);
                if arenas
                    .stmt(id)
                    .is_some_and(|stmt| matches!(stmt.kind, StmtKind::Return(_)))
                {
                    self.0.push(id);
                }
            }
        }
        let mut returns = Returns(Vec::new());
        syntax::visit::arena::walk_stmts(&mut returns, arenas, body);
        let mut points = Vec::new();
        let mut saw_explicit_return = false;
        for id in returns.0.into_iter().rev() {
            saw_explicit_return = true;
            let Some(s) = arenas.stmt(id) else {
                continue;
            };
            points.push(match &s.kind {
                StmtKind::Return(Some(value)) => self.infer_expr(arenas.expr(*value)?, &scope),
                _ => Type::void(),
            });
        }
        let falls_off_end = !body
            .last()
            .and_then(|id| arenas.stmt(*id))
            .is_some_and(|s| matches!(s.kind, StmtKind::Return(_) | StmtKind::Throw(_)));
        Some(closure_inferred_return_type(
            &self.relation(),
            points,
            saw_explicit_return,
            falls_off_end,
        ))
    }

    /// A non-generator arrow fn's own inferred return type: its body
    /// expression inferred against the enclosing scope (captured by
    /// value, the same as `check_expr`'s arm) with its parameters bound.
    fn arrow_fn_inferred_return(
        &self,
        params: &[Param],
        body: &ExprNode,
        is_generator: bool,
        locals: &Locals,
        overrides: Option<&[Type]>,
    ) -> Option<Type> {
        if is_generator {
            return None;
        }
        let mut arrow_locals = locals.clone();
        for (i, p) in params.iter().enumerate() {
            let ty = self.closure_param_type(p, i, overrides);
            let ty = if p.variadic {
                Type::array(Type::unknown_array_key(), ty)
            } else {
                ty
            };
            arrow_locals.insert_var(&p.name, ty);
        }
        Some(self.infer_expr(body, &arrow_locals))
    }

    /// A closure/arrow-fn literal parameter's type: its own native hint
    /// when it has one, else the type the surrounding context expects at
    /// that position (`overrides`, a `Closure(int): string`-shaped
    /// parameter's own parameter types - real PHPStan infers an unhinted
    /// closure parameter from the expected callable the same way), else
    /// `mixed`.
    fn closure_param_type(&self, p: &Param, i: usize, overrides: Option<&[Type]>) -> Type {
        match (&p.type_hint, overrides.and_then(|o| o.get(i))) {
            (Some(_), _) => self.resolve_type_hint(p.type_hint.as_ref()),
            (None, Some(expected)) => expected.clone(),
            (None, None) => Type::mixed(),
        }
    }

    /// A closure/arrow-fn literal argument's type as seen by a parameter
    /// expecting a shaped callable (`Closure(int): string`, possibly one
    /// member of a union): its unhinted parameters typed from that shape,
    /// and its unhinted return inferred with those parameter types bound.
    /// `None` when `arg` isn't a literal or `expected` carries no shape -
    /// the caller's ordinary `infer_expr` applies then.
    pub(crate) fn infer_closure_literal_in_context(
        &self,
        arg: &ExprNode,
        expected: &Type,
        locals: &Locals,
    ) -> Option<Type> {
        let sig = expected
            .callable_sig()
            .or_else(|| expected.union_members().iter().find_map(Type::callable_sig))?;
        let overrides: Vec<Type> = sig.params.iter().map(|p| p.ty.clone()).collect();
        let syntax_owner = self.syntax_tree_owner()?;
        let arenas = syntax_owner.arenas();
        let arg = arenas.peeled(arg.id).and_then(|id| arenas.expr(id))?;
        match &arg.kind {
            ExprKind::ArrowFn(closure_id) => {
                let ClosurePayload::ArrowFn(arrow) = arenas.closure(*closure_id)? else {
                    return None;
                };
                let params = arenas.params(arrow.params.clone());
                let inferred =
                    self.infer_closure_return_with_param_overrides(arg, &overrides, locals);
                Some(self.closure_literal_type(
                    params,
                    arrow.return_type.as_deref(),
                    inferred,
                    Some(&overrides),
                ))
            }
            ExprKind::Closure(closure_id) => {
                let ClosurePayload::Closure(closure) = arenas.closure(*closure_id)? else {
                    return None;
                };
                let params = arenas.params(closure.params.clone());
                let inferred =
                    self.infer_closure_return_with_param_overrides(arg, &overrides, locals);
                Some(self.closure_literal_type(
                    params,
                    closure.return_type.as_deref(),
                    inferred,
                    Some(&overrides),
                ))
            }
            _ => None,
        }
    }
}

/// Whether `e` is the literal `false` - used to scope
/// `array_slice`/`array_reverse`'s `$preserve_keys` argument to exactly
/// the default-matching case (omitted, or explicitly `false`).
fn is_false_literal(e: &ExprNode) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Bool(false)))
}

/// The `true`-literal mirror of `is_false_literal` above - used by
/// `var_export()`'s dynamic-return-type narrowing to recognize its
/// `$return` argument's default-matching-but-inverted case (explicitly
/// `true`, unlike `$preserve_keys`'s "omitted or `false`" default).
fn is_true_literal(e: &ExprNode) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Bool(true)))
}

/// A synthetic `target::method(args)` call, for a computed method name
/// whose value is known.
fn unhandled_match_error() -> Type {
    Type::object("UnhandledMatchError".into(), Vec::new())
}

/// Whether a static call's target is `parent`, `self` or `static`.
fn forwards_static_binding(target: &StaticTarget) -> bool {
    matches!(
        target,
        StaticTarget::Name(n)
            if matches!(n.to_ascii_lowercase().as_str(), "parent" | "self" | "static")
    )
}

impl Checker<'_> {
    fn static_call<'s>(
        &self,
        arena: &'s bumpalo::Bump,
        target: &StaticTarget,
        method: String,
        args: ArgRange,
        span: Span,
    ) -> &'s crate::checker::synthetic::SyntheticExpr {
        self.synthetic_expr_in(
            arena,
            ExprKind::StaticCall(
                target.clone(),
                Ident {
                    name: method.as_str().into(),
                    span,
                    name_id: None,
                },
                args,
            ),
            span,
        )
    }

    /// A synthetic `target::NAME` fetch, for a computed constant name whose
    /// possible values are known.
    fn constant_fetch<'s>(
        &self,
        arena: &'s bumpalo::Bump,
        target: &StaticTarget,
        name: String,
        span: Span,
    ) -> &'s crate::checker::synthetic::SyntheticExpr {
        let kind = if name == "class" {
            ExprKind::ClassNameFetch(target.clone())
        } else {
            ExprKind::ClassConstFetch(
                target.clone(),
                Ident {
                    name: name.as_str().into(),
                    span,
                    name_id: None,
                },
            )
        };
        self.synthetic_expr_in(arena, kind, span)
    }
}

impl Checker<'_> {
    /// The type a cast of `inner_ty` produces: the coercion of the value
    /// where PHP can make one, the cast's plain target type otherwise.
    fn cast_type(&self, kind: CastKind, inner_ty: &Type) -> Type {
        let coerced = match kind {
            CastKind::Int => inner_ty.to_integer(self.registry),
            CastKind::Float => inner_ty.to_float(self.registry),
            CastKind::String => inner_ty.to_string_type(self.registry),
            CastKind::Bool => Some(inner_ty.to_boolean()),
            _ => None,
        };
        coerced.unwrap_or_else(|| cast_result_type(kind))
    }
}

/// The type a cast of this kind produces, whatever it casts.
fn cast_result_type(kind: CastKind) -> Type {
    match kind {
        CastKind::Int => Type::int(),
        CastKind::Float => Type::float(),
        CastKind::String => Type::string(),
        CastKind::Bool => Type::bool(),
        CastKind::Array => Type::array(Type::unknown_array_key(), Type::mixed()),
        CastKind::Object => Type::object_without_class(),
        CastKind::Unset => Type::null(),
        CastKind::Void => Type::mixed(),
    }
}

/// Doctrine's `AbstractQuery::getResult()` (and every real terminal
/// method sharing its own `HYDRATE_OBJECT` shape - `execute()`/
/// `toIterable()` when their own hydration-mode argument is omitted or
/// explicit) always produces a real, sequentially-keyed list, whether or
/// not the DQL row type resolved; this is the list of `mixed` used when
/// it didn't.
pub(crate) fn doctrine_query_result_list_type() -> Type {
    Type::list_of(Type::mixed())
}

impl<'a> Checker<'a> {
    /// `Foo::BAR` (`is_property` false) or `Foo::$bar` (true): the
    /// member's type, reporting an unknown class or member, visibility,
    /// deprecation and internal-use problems.
    fn check_static_member_fetch(
        &mut self,
        e: &ExprNode,
        target: &StaticTarget,
        member: &str,
        is_property: bool,
        locals: &mut Locals,
        label: &str,
    ) -> Type {
        let class_name = match target {
            StaticTarget::Name(name) => name,
            // `$obj::X` / `$obj::$x`: the class of the receiver's value.
            StaticTarget::Expr(receiver) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("static target belongs to its retained file owner");
                let receiver = syntax_owner
                    .arenas()
                    .expr(*receiver)
                    .expect("static target receiver belongs to its retained file owner");
                let Some(class) = receiver_class(self.check_expr(receiver, locals, label)) else {
                    return Type::mixed();
                };
                return self.static_member_of_class(e, &class, None, member, is_property);
            }
        };
        // A static property (`self::$prop`) can be narrowed the
        // same way an instance property already is - check for a
        // locally-narrowed override before the undefined-property
        // check/registry lookup below.
        if is_property
            && locals.may_have_member_narrowing()
            && let Some(narrowed) = locals
                .find(&locals.static_prop_data(class_name, &Name::from(member)))
                .and_then(|key| locals.get(key))
        {
            return narrowed.clone();
        }
        let Some(class) = self.resolve_static_class_name(class_name) else {
            return Type::mixed();
        };
        if Self::class_guarded(&class, locals) && self.registry.lookup_class(&class).is_none() {
            return Type::mixed();
        }
        self.static_member_of_class(e, &class, Some(class_name), member, is_property)
    }

    /// `check_static_member_fetch` once the class is known; `written` is
    /// the class name as written, absent for a receiver expression.
    fn static_member_of_class(
        &mut self,
        e: &ExprNode,
        class: &str,
        written: Option<&QualifiedName>,
        member: &str,
        is_property: bool,
    ) -> Type {
        let class = class.to_string();
        // Rebind to the registry's canonical-case spelling up
        // front, same as `ExprKind::New`'s identical fix - without
        // it, a differently-cased real reference (`base::CONST`
        // for a real `class Base`) always fell into the "class
        // doesn't exist" branch below before `check_class_name_case`
        // ever got a chance to run.
        let class = self
            .registry
            .lookup_class(&class)
            .map(|(canonical, _)| canonical.to_string())
            .unwrap_or(class);
        if !self.registry.has_class(class.as_str()) {
            // Mirrors real PHPStan's `AccessStaticPropertiesCheck`/
            // `ClassConstantRule`'s "class doesn't exist at all"
            // half, for both a static-property and a
            // class-constant reference on a genuinely undefined
            // class.
            let subject = if is_property {
                "static property access"
            } else {
                "class constant access"
            };
            self.report(
                format!("{subject} target class {class} not found"),
                self.expr_span(e.id),
                0,
                "class.notFound",
            );
            return Type::mixed();
        }
        // `class.nameCase`, against the class name as written.
        if let Some(written) = written {
            self.check_class_name_case(written);
        }
        if is_property {
            let prop_name = member;
            // Unlike an instance property, PHP has no legal way
            // to dynamically create a static property at all - no
            // "could be legitimate" case to stay lenient for
            // here, the way check_property_assignment's write
            // side has to for instance properties. A property
            // declared as an ordinary instance property is
            // equally invalid through `::` in real PHP ("Access
            // to undeclared static property"), so both share one
            // message.
            let prop_with_owner = self.registry.lookup_property_with_owner(&class, prop_name);
            let is_valid_static = prop_with_owner
                .as_ref()
                .is_some_and(|(_, sig)| sig.is_static());
            if !is_valid_static {
                self.report(
                    format!("access to undefined static property {class}::${prop_name}"),
                    self.expr_span(e.id),
                    0,
                    "staticProperty.notFound",
                );
            } else if let Some((owner, sig)) = &prop_with_owner {
                self.check_internal_property_access(owner, prop_name, sig, self.expr_span(e.id));
                self.check_restricted_property_access(owner, prop_name, true, self.expr_span(e.id));
                self.check_deprecated_property_access(owner, prop_name, sig, self.expr_span(e.id));
                self.record_member_usage_token(
                    UsageTokenKind::Property,
                    owner,
                    sig.deprecated().is_some(),
                    sig.is_internal(),
                    self.expr_span(e.id),
                );
                self.record_member_ref(self.expr_span(e.id), owner, prop_name);
                // A static property shares the exact same
                // visibility rules as an instance one ("Cannot
                // access private property" is a real fatal error
                // either way).
                self.check_member_visibility(MemberVisibilityCheck {
                    member: MemberLabel {
                        kind: MemberKind::StaticProperty,
                        owner,
                        name: prop_name,
                    },
                    visibility: &sig.visibility(),
                    span: self.expr_span(e.id),
                });
                // Mirrors real PHPStan's
                // `AccessPrivatePropertyThroughStaticRule` - same
                // late-static-binding unsafety as the identical
                // class-constant rule above.
                if written.is_some_and(|n| n.is_keyword("static"))
                    && sig.visibility() == Visibility::Private
                {
                    self.check_static_private_property_access(
                        owner,
                        prop_name,
                        self.expr_span(e.id),
                    );
                }
            }
            prop_with_owner
                .map(|(_, p)| self.resolve_pseudo(p.ty(), &class))
                .unwrap_or(Type::mixed())
        } else {
            let const_with_owner = self.registry.lookup_const_with_owner(&class, member);
            let const_type_with_owner = self.registry.lookup_const_type_with_owner(&class, member);
            let const_ty = const_type_with_owner
                .as_ref()
                .map(|(owner, constant_type)| {
                    self.settings
                        .constants
                        .class_constant_type(owner, member, constant_type)
                });
            if const_ty.is_none() {
                self.report(
                    format!("access to undefined constant {class}::{member}"),
                    self.expr_span(e.id),
                    0,
                    "classConstant.notFound",
                );
            } else if let Some((owner, cs)) = &const_with_owner {
                self.check_internal_class_constant_access(owner, member, cs, self.expr_span(e.id));
                self.check_restricted_class_constant_access(owner, member, self.expr_span(e.id));
                self.check_deprecated_class_constant_access(
                    owner,
                    member,
                    cs,
                    self.expr_span(e.id),
                );
                self.record_member_usage_token(
                    UsageTokenKind::ClassConstant,
                    owner,
                    cs.deprecated().is_some(),
                    cs.is_internal(),
                    self.expr_span(e.id),
                );
                self.record_member_ref(self.expr_span(e.id), owner, member);
                self.check_member_visibility(MemberVisibilityCheck {
                    member: MemberLabel {
                        kind: MemberKind::Constant,
                        owner,
                        name: member,
                    },
                    visibility: &cs.visibility(),
                    span: self.expr_span(e.id),
                });
                // Mirrors real PHPStan's
                // `AccessPrivateConstantThroughStaticRule`.
                if written.is_some_and(|n| n.is_keyword("static"))
                    && cs.visibility() == Visibility::Private
                {
                    self.check_static_private_constant_access(owner, member, self.expr_span(e.id));
                }
            }
            const_ty
                .map(|ty| self.resolve_pseudo(&ty, &class))
                .unwrap_or(Type::mixed())
        }
    }

    /// A static member reached through a computed name: when the class
    /// is an enum, any of its cases may be the one used - see
    /// `Checker::escaped_enums`.
    fn record_dynamic_static_member_escape(&self, target: &StaticTarget) {
        if let Some(raw) = target
            .as_name()
            .and_then(|n| self.resolve_static_class_name(n))
            && let Some((canonical, info)) = self.registry.lookup_class(&raw)
            && info.kind() == ClassKind::Enum
        {
            self.record_enum_escape(canonical);
        }
    }

    /// A `match(true) { $a, $b => X, ... }` arm listing more than one
    /// condition for the same result (real PHP semantics: the arm runs
    /// iff `$a || $b`) needs narrowing applied to its body just as much
    /// as a single-condition arm does - narrow by each listed condition
    /// independently (same single-condition logic, once per condition)
    /// starting from the same baseline, then OR-merge the results via
    /// `merge_scopes`, the same combinator used to merge sibling arms'
    /// locals, applied here across one arm's multiple conditions.
    fn narrow_match_arm_conditions(
        &self,
        arenas: &syntax::ast::NodeArenas,
        subject: &ExprNode,
        conds: &[ExprId],
        base: &Locals,
        match_true: bool,
    ) -> Locals {
        let narrow_one = |cond_id: &ExprId| {
            let cond = arenas
                .expr(*cond_id)
                .expect("match condition belongs to its retained file owner");
            let mut locals = base.clone();
            if match_true {
                self.apply_narrowing(cond, &mut locals, true);
            } else {
                self.apply_equality_narrowing(subject, cond, &mut locals, true);
            }
            locals
        };
        match conds {
            [] => base.clone(),
            [only_cond] => narrow_one(only_cond),
            multiple => multiple
                .iter()
                .map(narrow_one)
                .reduce(|a, b| merge_scopes(&self.relation(), &a, &b))
                .unwrap_or_else(|| base.clone()),
        }
    }

    /// `match.unhandled`'s own two real exemptions, mirroring real
    /// PHPStan's `isUnhandledMatchErrorCaught`/`hasUnhandledMatchErrorThrowsTag`
    /// exactly: a `match` with no case for the value it's given isn't
    /// actually unsound when the resulting `UnhandledMatchError` is
    /// either already caught by an enclosing `try` (`current_catch_types`,
    /// pushed/popped around a `try`'s own protected region in
    /// `check_try`) or already covered by the enclosing function's own
    /// declared `@throws` (`current_declared_throws_type`) - both real,
    /// if rare, ways to make the "your `match` might throw" fact this
    /// check exists for already an explicit, handled part of the
    /// contract rather than a silent runtime surprise. `accepts`
    /// (not a plain class-name string comparison) so a broader catch/
    /// `@throws` type - `\Throwable`, `\Error`, or a real union
    /// containing either - counts too, not just the exact
    /// `UnhandledMatchError` class name.
    fn match_unhandled_error_is_exempted(&self) -> bool {
        let unhandled_match_error = Type::object("UnhandledMatchError".into(), Vec::new());
        self.current_catch_types
            .iter()
            .any(|caught| self.accepts(caught, &unhandled_match_error).yes())
            || self
                .current_declared_throws_type
                .as_ref()
                .is_some_and(|declared| self.accepts(declared, &unhandled_match_error).yes())
    }

    /// `match(true)`'s own "is this condition provably true" oracle for
    /// `match.unhandled`'s last-arm exhaustiveness check just above -
    /// `inferred_constant_condition` alone isn't enough, since it
    /// deliberately *defers* a comparison-operator condition to that
    /// family's own dedicated always-true/false check
    /// (`should_skip_constant_condition`'s own doc comment,
    /// `control_flow/boolean_conditions.rs`) rather than resolving it
    /// generically - confirmed via a real `if ($x > 5) {} elseif ($x <=
    /// 5) {}` chain already correctly flagging `smallerOrEqual.alwaysTrue`,
    /// proving the *narrowing* itself was never the gap, only which
    /// function this check called to read it. Asks the same question
    /// `check_constant_numeric_comparison` reports on
    /// (`ordering_outcome`) as a plain predicate instead of that
    /// function's own side-effecting report. Scoped to the same four
    /// ordering operators that family covers - an `==`/`===`/
    /// `instanceof`/`&&`/`||` condition (all also skipped by
    /// `inferred_constant_condition`) isn't attempted here yet, each
    /// with its own separate dedicated-check family this could equally
    /// reach into later.
    fn condition_is_provably_true(&self, e: &ExprNode, locals: &Locals) -> bool {
        if self.inferred_constant_condition(e, locals) == Some(true) {
            return true;
        }
        let ExprKind::Binary(op @ (BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq), l, r) =
            &e.kind
        else {
            return false;
        };
        let Some(arenas) = self.syntax_arenas() else {
            return false;
        };
        let (Some(l), Some(r)) = (arenas.expr(*l), arenas.expr(*r)) else {
            return false;
        };
        let l_ty = self.infer_expr(l, locals);
        let r_ty = self.infer_expr(r, locals);
        ordering_op(op).and_then(|op| self.ordering_outcome(&l_ty, op, &r_ty)) == Some(true)
    }

    /// Whether `l op r` is always true or always false, as the comparison
    /// model answers it on every configured PHP version.
    pub(crate) fn ordering_outcome(
        &self,
        l: &Type,
        op: types::OrderingOp,
        r: &Type,
    ) -> Option<bool> {
        match self
            .relation()
            .ordering(l, op, r, self.comparison_php_version())
        {
            types::TrinaryLogic::Yes => Some(true),
            types::TrinaryLogic::No => Some(false),
            types::TrinaryLogic::Maybe => None,
        }
    }

    /// `property.nonObject`/`method.nonObject` for an access whose member
    /// name is computed (`$x->$name`, `$x->{$name}()`): the receiver must
    /// be able to have properties (or methods) at all; `?->` excuses
    /// `null`.
    fn check_dynamic_member_receiver(
        &mut self,
        receiver: &Type,
        operator: ObjectOperator,
        is_call: bool,
        span: Span,
    ) {
        let checked = if operator.is_nullsafe() {
            receiver.without(&Type::null())
        } else {
            receiver.clone()
        };
        let Some(level) = self.failing_level(&checked, 0, |m| {
            if is_call {
                self.relation().can_call_methods(m).no()
            } else {
                self.relation().can_access_properties(m).no()
            }
        }) else {
            // A `mixed` receiver, from the levels that check it.
            if let Some(from) = types::strict_mixed_receiver_from(&checked) {
                let (message, identifier) = if is_call {
                    ("Cannot call a dynamic method on mixed.", "method.nonObject")
                } else {
                    (
                        "Cannot access a dynamic property on mixed.",
                        "property.nonObject",
                    )
                };
                self.report_found_from(message.to_string(), span, 0, from, identifier);
            }
            return;
        };
        let (message, identifier) = if is_call {
            (
                format!(
                    "call to a dynamic method on {} {receiver}",
                    non_object_kind(level)
                ),
                "method.nonObject",
            )
        } else {
            (
                format!(
                    "access to a dynamic property on {} {receiver}",
                    non_object_kind(level)
                ),
                "property.nonObject",
            )
        };
        self.report(message, span, level, identifier);
    }

    /// `ty` without `excluded`, where a check for it failed. Excluding
    /// an enum case removes it from the enum's cases (`Suit` without
    /// `Suit::Hearts` is `Suit::Clubs|Suit::Spades`); an exclusion that
    /// removes nothing leaves `ty` as it was.
    pub(crate) fn exclude(&self, ty: &Type, excluded: &Type) -> Type {
        let names_a_case = excluded
            .union_members()
            .iter()
            .any(|m| m.as_enum_case().is_some());
        if !names_a_case {
            return ty.without(excluded);
        }
        let expanded = self.expand_enum_cases(ty);
        let remaining = expanded.without(excluded);
        if remaining == expanded {
            ty.clone()
        } else {
            remaining
        }
    }

    /// `match.unhandled`'s own case-expansion step: a plain enum-typed
    /// `EnumClass` has no case list of its own for
    /// `Type::without`'s existing `Union`-member-filtering to
    /// work against - only the fully-expanded `Union` of every real case
    /// (`EnumClass::CaseName`, already how a case is
    /// represented everywhere else in this checker - `ClassReflection::
    /// from_enum_decl` folds each case into `consts` with exactly that
    /// type) lets excluding each arm's own case value reduce it all the
    /// way to `never` once every case is covered. Recurses into a
    /// `Union` member-by-member (real shape: `Status|null`, a nullable
    /// enum parameter - without this, a fully-exhaustive `Status::A =>
    /// ..., Status::B => ..., null => ...` match false-positived
    /// `match.unhandled` on the un-expanded `Status` member, since
    /// excluding each individual case can never prove a *generic* enum
    /// `Object` redundant, only a real `EnumCase` sitting directly in
    /// the union already can). A member that isn't a bare `Object`
    /// naming a real, currently-registered enum (an interface/non-enum
    /// class, an unresolved name) is kept unchanged - this project's own
    /// "don't guess" precedent: the exhaustiveness check simply won't
    /// fire for that narrower shape, rather than report something
    /// unproven.
    pub(crate) fn expand_enum_cases(&self, ty: &Type) -> Type {
        syntax::stack::guard(|| {
            let members = ty.union_members();
            if members.len() > 1 {
                let expanded: Vec<Type> = members
                    .iter()
                    .flat_map(|m| self.expand_enum_cases(m).union_members().to_vec())
                    .collect();
                return match expanded.len() {
                    0 => Type::never(),
                    1 => expanded.into_iter().next().unwrap(),
                    _ => ty.with_members(expanded),
                };
            }
            if ty.as_enum_case().is_some() {
                return ty.clone();
            }
            let Some((class_name, _)) = ty.as_object() else {
                return ty.clone();
            };
            let Some(info) = self.registry.class_by_name(class_name.as_str()) else {
                return ty.clone();
            };
            let cases: Vec<Type> = info
                .consts()
                .iter()
                .filter_map(|(name, _)| {
                    self.registry
                        .lookup_const_type(class_name, name)
                        .and_then(Type::as_enum_case)
                })
                .filter(|(cls, _)| *cls == class_name)
                .map(|(cls, case)| Type::enum_case_ref(cls, case))
                .collect();
            match cases.len() {
                0 => ty.clone(),
                1 => cases.into_iter().next().unwrap(),
                _ => self.relation().union_of(cases),
            }
        })
    }
}

/// PHP's `?->` short-circuits the entire remaining postfix chain to
/// `null`, not just the single arrow it's written on - `$a?->b->c` means
/// `$a === null ? null : $a->b->c`, so the composite expression's type
/// must include `Null` even though only the first link is nullsafe. The
/// outer, plain `->c` node must forward that inherited nullability
/// rather than only checking its own `nullsafe` flag. Checks only the
/// direct base-chain lineage (not into unrelated subexpressions like
/// call arguments), and deliberately does not apply to a base that's
/// nullable for some other reason with no nullsafe anywhere in its chain
/// (e.g. a plain `$nullableVar->prop`) - that's a real mistake elsewhere,
/// and re-widening its result to include `Null` here would risk masking
/// further bugs rather than fixing anything.
pub(crate) fn postfix_chain_has_nullsafe(arenas: &NodeArenas, base: &ExprNode) -> bool {
    let Some(base) = arenas.peeled(base.id).and_then(|id| arenas.expr(id)) else {
        return false;
    };
    syntax::stack::guard(|| match &base.kind {
        ExprKind::PropertyAccess(inner, _, operator)
        | ExprKind::MethodCall(inner, _, _, operator)
        | ExprKind::DynamicPropertyAccess(inner, _, operator)
        | ExprKind::DynamicMethodCall(inner, _, _, operator) => {
            operator.is_nullsafe()
                || arenas
                    .expr(*inner)
                    .is_some_and(|inner| postfix_chain_has_nullsafe(arenas, inner))
        }
        ExprKind::Index(inner, _) => arenas
            .expr(*inner)
            .is_some_and(|inner| postfix_chain_has_nullsafe(arenas, inner)),
        _ => false,
    })
}

/// A non-generator closure literal's inferred return type, from the
/// types its `return` statements produced - real PHPStan's `ClosureType`
/// return: no `return` at all is `void`; explicit returns are unioned,
/// with `null` added when the body can also fall off its end (what PHP
/// really returns then - stricter than real PHPStan, which only counts
/// return statements); a bare `return;` mixed with value returns reads as
/// `null` there, since `void` only means "no value" when it's the whole
/// story.
fn closure_inferred_return_type(
    relation: &types::Relation,
    points: Vec<Type>,
    saw_explicit_return: bool,
    falls_off_end: bool,
) -> Type {
    if !saw_explicit_return {
        return Type::void();
    }
    let mut members = points;
    if falls_off_end {
        members.push(Type::void());
    }
    let has_value = members.iter().any(|m| !m.is_void());
    let mut members: Vec<Type> = members
        .into_iter()
        .map(|m| {
            if has_value && m.is_void() {
                Type::null()
            } else {
                m
            }
        })
        .collect();
    // `null` last, the order real PHPStan's own union description uses
    // (`int|null`, not `null|int`).
    members.sort_by_key(|m| m.is_null().yes());
    if members.is_empty() {
        return Type::void();
    }
    relation.union_of(members)
}

impl<'a> Checker<'a> {
    /// Where a static call's class part (`Foo` in `Foo::bar()`, or `$obj`
    /// in `$obj::bar()`) ends.
    fn static_target_end(&self, target: &StaticTarget) -> u32 {
        match target {
            StaticTarget::Name(name) => name.span.end,
            StaticTarget::Expr(expr) => {
                let syntax_owner = self
                    .syntax_tree_owner()
                    .expect("static target belongs to its retained file owner");
                syntax_owner
                    .arenas()
                    .expr_span(*expr)
                    .expect("static target receiver belongs to its retained file owner")
                    .end
            }
        }
    }
}

/// An interpolated string's type: its parts, each as a string,
/// concatenated.
fn concat_string_parts(parts: Vec<Type>, hierarchy: &dyn types::ClassHierarchy) -> Type {
    parts
        .into_iter()
        .reduce(|acc, part| acc.concat(&part, hierarchy))
        .unwrap_or(Type::string())
}

/// A `new class { ... }` expression's type: an instance of the class
/// under its registry name, or `mixed` before the file's anonymous
/// classes are named.
fn anonymous_class_type(name: Option<&Name>) -> Type {
    name.map_or(Type::mixed(), |name| Type::object_ref(name, &[]))
}

/// How a receiver failing at `level` is described: wholly a non-object at
/// level 0, only in part (a union member, `null`) above it.
fn non_object_kind(level: u8) -> &'static str {
    if level == 0 {
        "a non-object type"
    } else {
        "a possibly non-object type"
    }
}
