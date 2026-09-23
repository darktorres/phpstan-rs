use crate::ast::*;
use crate::checker::suggest;
use crate::checker::{
    ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION, ATTRIBUTE_TARGET_PROPERTY,
    CheckArgsCall, Checker, DiagnosticBuilder, Locals, LoopZeroIterations, MemberVisibilityCheck,
    TemplateBindings, UsageTokenKind, binop_sigil, body_contains_yield, collect_goto_targets,
    expr_contains_yield, generator_template_param, is_known_single_value, is_language_construct,
    is_real_variable_key, is_superglobal, literal_string_key, merge_scopes, narrow_type_excluding,
    narrowing_key, propagate_container_field_facts, purge_stale_narrowing_for, resolve_call_args,
    resolve_concat_type, string_type_has_accessory, superglobal_default_type, to_bitwise_not,
};
use crate::pest_config;
use crate::regex_groups;
use crate::registry::{FunctionSig, Registry};
use crate::token::Span;
use crate::types::{self, AccessoryKind, Type};

mod array_ops;
mod builtin_calls;
mod member_resolution;
mod preg_shapes;
mod probe_checks;
pub(crate) mod ranges;

use crate::checker::expr::ranges::{
    checked_bound_op, compare_range_outcome, div_int_range, loose_compare_literals, mod_int_range,
    mul_int_range, numeric_comparison_bounds, pow_int_range, shift_int_range, sub_int_range_full,
};
use crate::checker::type_union::union_types;
pub(crate) use ranges::{NullProbeVerdictFn, empty_probe_verdict, null_probe_verdict};
pub(crate) use ranges::{int_range_bounds, normalize_int_range};

// `checker::throws::dynamic` reuses this bitwise-or-flag-chain scan for
// `JSON_THROW_ON_ERROR`/`FILTER_THROW_ON_FAILURE`, the same helper
// `builtin_calls.rs`'s `json_encode`/`filter_var` return-type narrowing
// uses.
pub(crate) use builtin_calls::contains_flag_name;
pub(crate) use preg_shapes::build_matched_shape;
use preg_shapes::{
    PregFlags, parse_preg_flags, preg_leaf_type, preg_match_all_pattern_order_shape,
    preg_match_matches_shape,
};

/// Does unary `-`/`+`'s numeric-string-to-int coercion apply to this
/// operand's type - true for a bare `Type::String`/`Type::StringLiteral`,
/// or a `numeric-string`-shaped `Intersection` (`String` alongside an
/// accessory kind). Deliberately does NOT widen into a `Union`'s members
/// here, unlike `operators.rs`'s leniency-tuned `is_exactly_string_operand`
/// (tuned for suppressing a false invalid-operand flag, not this coercion
/// decision) - `int|string` should keep its own type, not collapse
/// entirely to `Int` just because one member is string-shaped.
fn is_string_shaped(ty: &Type) -> bool {
    matches!(ty, Type::String | Type::StringLiteral(_))
        || matches!(ty, Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::String)))
}

/// Unary `-` negates a literal/range operand's actual value - `+` is a
/// true no-op on the value (only ever coercing a numeric string), but `-`
/// must flip it: `-32768` infers as `IntLiteral(-32768)`, not
/// `IntLiteral(32768)`. `IntRange`'s min/max swap on negation (`-int<a,
/// b>` is `int<-b, -a>`) mirrors real PHPStan's `IntegerRangeType`
/// negation handling. Anything else (a bare `Type::Int`, `Type::Float`,
/// ...) has no specific value to flip, so it's returned unchanged.
fn negate_numeric_literal(ty: Type) -> Type {
    match ty {
        Type::IntLiteral(n) => Type::IntLiteral(-n),
        Type::FloatLiteral(f) => Type::FloatLiteral(-f),
        Type::IntRange(min, max) => Type::IntRange(max.map(|m| -m), min.map(|m| -m)),
        other => other,
    }
}

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
/// `apply_equality_narrowing`'s `target_ty == Type::Mixed(false)` bail-out.
fn bare_name_literal_type(name: &str) -> Type {
    match name.trim_start_matches('\\').to_ascii_lowercase().as_str() {
        "true" => Type::True,
        "false" => Type::False,
        "null" => Type::Null,
        _ => Type::Mixed(false),
    }
}

/// Does `ty`'s value-space unconditionally exclude `null`? `Mixed`/`Never`
/// are left alone (a `Mixed` slot could always turn out to hold `null` at
/// runtime; `Never` means unreachable code) - everything else is settled
/// by comparing against `narrow_type_excluding(ty, &Type::Null)` (the
/// same "subtract null" operation `??`/`?->`'s type inference relies on
/// elsewhere in this file): if removing `null` changes nothing, `ty`
/// never carried it to begin with.
pub(crate) fn type_definitely_excludes_null(ty: &Type) -> bool {
    !matches!(ty, Type::Mixed(_) | Type::Never) && narrow_type_excluding(ty, &Type::Null) == *ty
}

/// Whether `ty`'s value-space can never include a PHP object - real
/// PHPStan's `Type::canCallMethods()`/`canAccessProperties()` return "no"
/// for exactly this same set of scalar/array-family types (confirmed
/// reading each one's own `use PHPStan\Type\Traits\Non*TypeTrait` block:
/// `NullType`/`VoidType`/`BooleanType`/`IntegerType`/`FloatType`/
/// `StringType`/`ArrayType` all use `NonObjectTypeTrait`). Used by both
/// `method.nonObject` and `property.nonObject` - real PHPStan's own
/// `MethodCallCheck`/`AccessPropertiesCheck` gate on the identical
/// question. `Type::Callable` (could still resolve to a real `Closure`
/// object at runtime - a literal closure already infers as
/// `Type::Object("Closure", [])` directly, never `Type::Callable`),
/// `Type::TemplateParam`'s own bound, an unresolved `Type::Conditional`
/// (folded to `Union([if_true, if_false])`, its own doc comment's stated
/// fallback), `Type::ObjectShape` (a structural bag with no class name to
/// rule anything out), `Type::SelfRef`/`ParentRef`/`StaticRef` (resolved
/// away by `Checker::resolve_pseudo` well before reaching a real
/// expression's type in valid code), and `Type::Mixed(false)`/`Never` are all
/// left unflagged - matching real PHPStan's own permissive default for
/// `Mixed` below level 9 and vacuous-`Never` behavior
/// (`NeverType` itself overrides `canCallMethods` to unconditionally
/// return "yes"). A level-9/10 `Mixed` receiver (`checkExplicitMixed`/
/// `checkImplicitMixed`) is handled separately at each call site, via
/// `Registry::strict_mixed_receiver_level`, not by this predicate.
///
/// A `Union` is flagged the moment *any* **non-`Null`** member is
/// confirmed closed; an `Intersection` only when *every* member is - the
/// same rule this project's own `member_confirmed_missing` already uses
/// for the sibling `method.notFound`/`property.notFound` checks,
/// deliberately stricter than real PHPStan's own `checkUnionTypes`-gated
/// (off by default below level 7) member-filtering default. `Null` is
/// dropped from a `Union` before that evaluation specifically - real
/// PHPStan's `RuleLevelHelper::findTypeToCheckImplementation` strips
/// `Null` out of *any* type that isn't itself exactly `Null` before
/// running any of these checks at all (`checkNullables`, off by default
/// through level 7 - a *different*, unconditional-on-`checkUnionTypes`
/// preamble than the member-filtering above), so `Closure|null` is
/// checked as plain `Closure`, never as a `Union` with a closed `Null`
/// member - confirmed the hard way, by a real, pre-existing test
/// (`this_is_available_inside_a_closure_literal_passed_to_closure_bind`,
/// exercising `Closure::bind()`'s genuinely nullable real return type)
/// failing against the first implementation of this rule.
///
/// Used as-is by `property.nonObject` and `clone.nonObject` - real
/// PHPStan's `ObjectType::canAccessProperties()`/`isCloneable()` are both
/// an unconditional "yes" for every real class, no exceptions. `method.
/// nonObject` needs one further exception on top of this
/// (`type_can_never_call_methods` below) - `canCallMethods()` is the one
/// of the three that special-cases `stdClass`.
pub(crate) fn type_can_never_be_object(ty: &Type) -> bool {
    match ty {
        Type::Null
        | Type::Void
        | Type::Bool
        | Type::True
        | Type::False
        | Type::Int
        | Type::IntLiteral(_)
        | Type::IntRange(..)
        | Type::Float
        | Type::FloatLiteral(_)
        | Type::String
        | Type::StringLiteral(_)
        | Type::ClassString(_)
        | Type::Array(..)
        | Type::ArrayShape(_)
        | Type::HasOffsetValue(..) => true,
        Type::Union(members) => members
            .iter()
            .filter(|m| !matches!(m, Type::Null))
            .any(type_can_never_be_object),
        Type::Intersection(members) => members.iter().all(type_can_never_be_object),
        Type::TemplateParam(param) => type_can_never_be_object(&param.bound),
        Type::Conditional {
            if_true, if_false, ..
        } => type_can_never_be_object(if_true) || type_can_never_be_object(if_false),
        _ => false,
    }
}

/// `method.nonObject`'s own classifier - `type_can_never_be_object` plus
/// one further exception: real PHPStan's `ObjectType::canCallMethods()`
/// (`ObjectType.php:1072-1078`) special-cases `stdClass` to
/// unconditionally return "no" (a real, registered, permanently
/// zero-method class), unlike `canAccessProperties()`/`isCloneable()`,
/// which have no such exception - so `property.nonObject`/
/// `clone.nonObject` must keep using the plain, `stdClass`-unaware
/// function above, not this one.
pub(crate) fn type_can_never_call_methods(ty: &Type) -> bool {
    match ty {
        Type::Object(class, _) if class.as_str() == "stdClass" => true,
        Type::Union(members) => members
            .iter()
            .filter(|m| !matches!(m, Type::Null))
            .any(type_can_never_call_methods),
        Type::Intersection(members) => members.iter().all(type_can_never_call_methods),
        Type::TemplateParam(param) => type_can_never_call_methods(&param.bound),
        Type::Conditional {
            if_true, if_false, ..
        } => type_can_never_call_methods(if_true) || type_can_never_call_methods(if_false),
        _ => type_can_never_be_object(ty),
    }
}

impl<'a> Checker<'a> {
    /// A `new class { ... }` expression's own inferred type - looks its
    /// span up in `anonymous_class_names` (populated once, at parse time,
    /// by `registry::collect_anonymous_classes` - see that module's own
    /// doc comment) rather than recomputing its synthetic name here, so
    /// the checker and the registry entry it resolves against always
    /// agree by construction. Falls back to `Type::Mixed(false)` when the span
    /// isn't found - every existing test (which never calls
    /// `set_anonymous_class_names`) keeps today's behavior unchanged.
    fn anonymous_class_type(&self, span: Span) -> Type {
        self.anonymous_class_names
            .get(&span)
            .map(|name| Type::Object(name.clone(), Vec::new()))
            .unwrap_or(Type::Mixed(false))
    }

    /// Thin wrapper over `check_expr_impl` - the real work - so every
    /// external `rule::Rule` registered via `Checker::set_external_rules`
    /// still fires at every recursion depth without each of
    /// `check_expr_impl`'s own many internal `self.check_expr(...)`
    /// recursive calls needing to know about rule dispatch at all (see
    /// `rule`'s own module doc comment).
    pub(crate) fn check_expr(&mut self, e: &Expr, locals: &mut Locals, label: &str) -> Type {
        let ty = self.check_expr_impl(e, locals, label);
        if !self.external_rules.is_empty() {
            self.dispatch_expr_rules(e, locals, label, &ty);
        }
        ty
    }

    fn dispatch_expr_rules(&mut self, e: &Expr, locals: &Locals, label: &str, ty: &Type) {
        let rules = self.external_rules;
        let mut found = Vec::new();
        for rule in rules {
            if rule.wants_expr(&e.kind) {
                let scope = crate::rule::Scope::new(self, locals, label);
                found.extend(rule.check_expr(&scope, e, ty));
            }
        }
        self.diagnostics.extend(found);
    }

    #[hotpath::measure]
    fn check_expr_impl(&mut self, e: &Expr, locals: &mut Locals, label: &str) -> Type {
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
        if !is_bare_statement && let ExprKind::Match { arms, default, .. } = &e.kind {
            self.check_void_match_usage(arms, default.as_deref(), locals, e.span, label);
        }
        match &e.kind {
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(name) = &callee.kind
                    && let Some(kind) = crate::checker::includes::include_kind(name)
                    && let [arg] = args.as_slice()
                {
                    self.check_include_path(kind, &arg.value, locals, e.span, label);
                }
                // `unset($x)` genuinely removes `$x` from scope in real
                // PHP - a real bug if it's used afterward. Checked before
                // anything else below: `predefine_byref_or_unknown_args`
                // treats *any* bare-variable argument to an unknown-
                // signature call as a definition site (a mechanism built
                // for a different reason - conservatively assuming an
                // unresolvable call might take the argument by
                // reference - that happens to also make `isset`/`empty`
                // safe to use on a maybe-undefined variable), which is
                // backwards for `unset` specifically: it would make
                // `unset($x)` look like it *defines* `$x` instead of
                // removing it.
                if let ExprKind::Name(name) = &callee.kind
                    && name == "unset"
                {
                    for a in args {
                        if let ExprKind::Variable(var_name) = &a.value.kind {
                            locals.remove(var_name);
                        } else {
                            self.check_expr(&a.value, locals, label);
                        }
                    }
                    return Type::Mixed(false);
                }
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
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "assertType" || name.ends_with("\\assertType"))
                    && let [expected_arg, actual_arg] = args.as_slice()
                    && let ExprKind::Literal(Literal::String(expected)) = &expected_arg.value.kind
                {
                    let actual = self.infer_expr(&actual_arg.value, locals).to_string();
                    if *actual != **expected {
                        self.report(label.to_string(), format!(
                                "assertType() mismatch: expected type \"{expected}\", actual type \"{actual}\""
                            ), actual_arg.value.span, 0, "phpstanRs.assertType");
                    }
                    self.check_expr(&actual_arg.value, locals, label);
                    return Type::Mixed(false);
                }
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "extract" || name == "eval")
                {
                    self.saw_dynamic_var_construct = true;
                }
                // `print $x` desugars to this same synthetic `Call(Name
                // ("print"), [$x])` node - checked here, since
                // `is_language_construct` below routes it away from
                // ordinary signature-based argument checking entirely
                // (`print` has no real signature to check against). The
                // operand must be string-coercible (`print.nonString`,
                // distinct wording from `echo.nonString` - see
                // `check_print_validity`'s doc comment), it triggers the
                // same implicit `__toString()` call `echo`/`(string)` do,
                // and `print` itself always evaluates to the literal
                // `int(1)`.
                if let ExprKind::Name(name) = &callee.kind
                    && name == "print"
                    && let [arg] = args.as_slice()
                {
                    let arg_ty = self.check_expr(&arg.value, locals, label);
                    self.check_print_validity(&arg_ty, arg.value.span, label);
                    self.check_implicit_tostring_usage(&arg_ty, arg.value.span, label);
                    return Type::IntLiteral(1);
                }
                // Resolved once up front (namespace/`use`-import rules
                // applied, with the same global-namespace fallback real
                // PHP gives an unqualified function call) so both the
                // arg-checking path and the undefined-call diagnostic
                // below see the same name(s) a real call would actually
                // resolve to. `is_language_construct` is checked against
                // the original name, before any namespace-qualification -
                // `isset`/`empty`/etc. are never real namespaced symbols,
                // unlike an actual function call, even though the parser
                // models both the same way.
                let resolved_name = match &callee.kind {
                    ExprKind::Name(name) if is_language_construct(name) => None,
                    ExprKind::Name(name) => {
                        let (primary, fallback) =
                            self.resolve_function_candidates_cached(callee.span, name);
                        if self.registry.lookup_function(&primary).is_some() {
                            Some(primary)
                        } else if let Some(global) = &fallback
                            && self.registry.lookup_function(global).is_some()
                        {
                            Some(global.clone())
                        } else {
                            Some(primary)
                        }
                    }
                    _ => None,
                };
                let sig = resolved_name
                    .as_deref()
                    .and_then(|name| self.registry.lookup_function(name));
                // `resolved_name` is already namespace-qualified the
                // same way `check_class_name_case`'s own `resolved`
                // variable is, so the identical canonical-vs-written
                // comparison applies unchanged.
                if self.registry.check_function_name_case
                    && let Some(resolved) = &resolved_name
                    && let Some((canonical, _)) = self.registry.lookup_function_with_name(resolved)
                    && canonical != resolved.as_ref()
                {
                    let mut builder = DiagnosticBuilder::new(
                        label.to_string(),
                        format!("Function {canonical} used with incorrect case: {resolved}."),
                        callee.span,
                        0,
                        "function.nameCase",
                    );
                    if let ExprKind::Name(written) = &callee.kind
                        && let Some((span, replacement, description)) =
                            Self::name_case_fix(written, canonical, callee.span)
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
                    .map(|s| resolve_call_args(args, &s.params))
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
                let array_shift_pop_pre_widen_ty: Option<Type> = matches!(
                    resolved_name.as_deref(),
                    Some("array_shift") | Some("array_pop")
                )
                .then(|| args.first())
                .flatten()
                .map(|a| self.infer_expr(&a.value, locals));
                self.predefine_byref_or_unknown_args(
                    resolved_name.as_deref(),
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
                let is_this_binding_call = matches!(&callee.kind, ExprKind::Name(name) if pest_config::is_this_binding_call(name));
                let pest_call_name: Option<&str> = match &callee.kind {
                    ExprKind::Name(name) if is_this_binding_call => Some(name.as_ref()),
                    _ => None,
                };
                // `isset($x['maybe_missing'])`/`empty($x['maybe_missing'])`
                // are exactly PHP's own sanctioned way to safely probe a
                // possibly-absent array key/property/variable - real PHP
                // raises no notice either way, and real PHPStan explicitly
                // never flags an "offset doesn't exist"-style diagnostic
                // for either construct's own direct argument(s), the same
                // "root of this expression chain isn't checked for
                // existence" leniency `check_expr_quiet_root` already
                // grants `??`'s left side. Scoped to these two names only,
                // matched on the *original* (pre-resolution) name exactly
                // like `is_language_construct` itself, since neither is a
                // real, resolvable function call.
                let is_existence_probe = matches!(&callee.kind, ExprKind::Name(name) if name == "isset" || name == "empty");
                // `isset($x)` and `empty($x)` each get their own verdict
                // function - `null_probe_verdict` (just nullability) for
                // the former, `empty_probe_verdict` (full falsiness, via
                // `type_truthiness`) for the latter, matching real
                // PHPStan's `EmptyRule` requiring both nullability and
                // falsiness to be unambiguous before it says anything.
                let existence_probe_kind: Option<(&str, &str, NullProbeVerdictFn)> =
                    match &callee.kind {
                        ExprKind::Name(name) if name == "isset" => {
                            Some(("isset", "in isset()", null_probe_verdict))
                        }
                        ExprKind::Name(name) if name == "empty" => {
                            Some(("empty", "in empty()", empty_probe_verdict))
                        }
                        _ => None,
                    };
                // `array_map($callback, $items)`'s contextual parameter-
                // type injection - computed once, up front, from
                // `$items`' element type (not per-argument inside the
                // loop below, since it's the other argument's type this
                // needs). Scoped to the common 2-arg single-array form,
                // matching `infer_builtin_call_return_type`'s identical
                // restriction for the same function.
                let array_map_elem_override = (resolved_name.as_deref() == Some("array_map")
                    && args.len() == 2)
                    .then(|| self.infer_expr(&args[1].value, locals))
                    .and_then(|ty| Self::array_element_type(&ty));
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
                let contextual_callback_override: Option<(usize, Vec<Type>)> =
                    resolved_name.as_deref().and_then(|name| match name {
                        "array_filter" if args.len() == 2 => {
                            let arr_ty = self.infer_expr(&args[0].value, locals);
                            Self::array_element_type(&arr_ty).map(|t| (1, vec![t]))
                        }
                        "usort" | "uasort" if args.len() == 2 => {
                            let arr_ty = self.infer_expr(&args[0].value, locals);
                            Self::array_element_type(&arr_ty).map(|t| (1, vec![t.clone(), t]))
                        }
                        "array_walk" | "array_find" | "array_find_key" | "array_any"
                        | "array_all"
                            if args.len() == 2 =>
                        {
                            let arr_ty = self.infer_expr(&args[0].value, locals);
                            match (
                                Self::array_element_type(&arr_ty),
                                Self::array_key_type(&arr_ty),
                            ) {
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
                            vec![Type::Array(
                                Box::new(Type::unknown_array_key()),
                                Box::new(Type::String),
                            )],
                        )),
                        _ => None,
                    });
                for (i, a) in args.iter().enumerate() {
                    // `argument.unpackNonIterable` - `foo(...$x)` where
                    // `$x` is confirmed to never be iterable at all.
                    // Checked via `infer_expr` (read-only, no side
                    // effects) rather than threading a captured type
                    // through every branch below - this arm's own
                    // per-argument dispatch already has four different
                    // shapes, none of which uniformly produce one.
                    if a.spread {
                        let spread_ty = self.infer_expr(&a.value, locals);
                        if self.type_confirmed_not_iterable(&spread_ty) {
                            self.report(
                                label.to_string(),
                                format!("Only iterables can be unpacked, {spread_ty} given."),
                                a.value.span,
                                3,
                                "argument.unpackNonIterable",
                            );
                        }
                    }
                    if is_this_binding_call
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                    {
                        let this_ty = self.pest_this_type.clone();
                        // Pest's `$this`-binding (a real `TestCase`-
                        // subclass instance, almost always) grants the
                        // same private/protected access real PHP's
                        // `Closure::bind`/`->bindTo()` third argument does,
                        // since `current_class` has no other way to
                        // reflect Pest's file-scoped binding.
                        let scope_class = match &this_ty {
                            Type::Object(name, _) => Some(name.clone()),
                            _ => None,
                        };
                        self.check_closure_literal_with_this_and_scope(
                            unwrap_grouping(&a.value),
                            locals,
                            label,
                            this_ty,
                            scope_class,
                        );
                        if self.registry.check_risky_tests
                            && let Some(name) = pest_call_name
                        {
                            self.check_pest_risky_test(
                                name,
                                unwrap_grouping(&a.value),
                                e.span,
                                label,
                            );
                        }
                    } else if is_existence_probe {
                        self.check_expr_quiet_root(&a.value, locals, label);
                        if let Some((identifier_prefix, operator_description, verdict_fn)) =
                            existence_probe_kind
                        {
                            self.check_pointless_null_probe(
                                &a.value,
                                locals,
                                label,
                                identifier_prefix,
                                operator_description,
                                verdict_fn,
                            );
                            self.check_pointless_offset_or_property_probe(
                                &a.value,
                                locals,
                                label,
                                identifier_prefix,
                                operator_description,
                                verdict_fn,
                            );
                        }
                    } else if i == 0
                        && let Some(elem_ty) = &array_map_elem_override
                        && self.check_closure_literal_with_param_overrides(
                            unwrap_grouping(&a.value),
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
                            unwrap_grouping(&a.value),
                            overrides,
                            locals,
                            label,
                        )
                    {
                        // Same as the `array_map` arm just above, for
                        // one of the other four contextually-checked
                        // functions.
                    } else if let Some(this_ty) = sig
                        .and_then(|s| s.params.get(i))
                        .and_then(|p| p.closure_this_type.clone())
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
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
                            unwrap_grouping(&a.value),
                            locals,
                            label,
                            this_ty,
                            None,
                        );
                    } else if let Some(callable_sig) = sig
                        .and_then(|s| s.params.get(i))
                        .and_then(|p| p.ty.callable_sig())
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                        && self.check_closure_literal_with_param_overrides(
                            unwrap_grouping(&a.value),
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
                        self.check_expr(&a.value, locals, label);
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
                    callee,
                    args,
                    locals,
                    is_exempt_last_condition,
                    e.span,
                    label,
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
                        let dynamic_throw = self.dynamic_function_throw_type(&bare, args, locals);
                        self.record_call_throw_point(dynamic_throw, Some(sig));
                        self.check_internal_function_call(name, sig, e.span, label);
                        self.check_restricted_function_call(name, args, locals, e.span, label);
                        self.check_deprecated_function_call(name, sig, e.span, label);
                        self.record_function_usage_token(name, sig, callee.span);
                        match name.as_ref() {
                            "array_filter" => {
                                self.check_array_filter_no_op(args, locals, label, e.span)
                            }
                            "array_values" => {
                                self.check_array_values_no_op(args, locals, label, e.span)
                            }
                            _ => {}
                        }
                        // A spread argument (`f(...$args)`) means the
                        // real argument count/types past it aren't known
                        // statically - `check_args` is spread-aware (it
                        // stops trusting a count/"missing" check at or
                        // past a spread's position), so it's always worth
                        // calling. `check_format_string_call` is NOT
                        // spread-aware (it walks `args[1..]` strictly
                        // positionally against the format string's
                        // specifier count) and stays gated on no spread
                        // being present, to avoid treating the spread
                        // argument itself as filling one specifier slot.
                        self.check_args(CheckArgsCall {
                            callee_label: name,
                            sig,
                            args,
                            resolved: &resolved,
                            locals,
                            label,
                            defining_class: None,
                            span: e.span,
                        });
                        self.check_castable_array_arguments(
                            name, sig, args, &resolved, locals, label,
                        );
                        self.check_impossible_haystack_values(name, args, locals, e.span, label);
                        if name.eq_ignore_ascii_case("define") {
                            self.check_define_value(args, locals, e.span, label);
                        }
                        if !args.iter().any(|a| a.spread)
                            && matches!(
                                name.as_ref(),
                                "sprintf" | "printf" | "fprintf" | "vsprintf" | "vprintf"
                            )
                        {
                            self.check_format_string_call(name, args, locals, label);
                        }
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
                            self.check_define_third_argument(args, e.span, label);
                        }
                        if name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .eq_ignore_ascii_case("random_int")
                        {
                            self.check_random_int_call(args, locals, label);
                        }
                        if name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .eq_ignore_ascii_case("filter_var")
                            && args.len() >= 3
                        {
                            self.check_filter_var_flag_conflict(args, e.span, label);
                        }
                        if name
                            .rsplit('\\')
                            .next()
                            .unwrap_or(name)
                            .eq_ignore_ascii_case("simplexml_load_string")
                        {
                            self.check_simplexml_load_string_call(args, e.span, label);
                        }
                        self.check_preg_pattern_validity(name, args, label);
                        {
                            let bare = name.rsplit('\\').next().unwrap_or(name);
                            if bare.eq_ignore_ascii_case("sscanf")
                                || bare.eq_ignore_ascii_case("fscanf")
                            {
                                self.check_scanf_argument_count(bare, args, label, e.span);
                            }
                            if bare.eq_ignore_ascii_case("call_user_func")
                                || bare.eq_ignore_ascii_case("call_user_func_array")
                            {
                                self.check_call_user_func(bare, args, locals, label, e.span);
                            }
                            self.check_preg_quote_delimiter(bare, args, label);
                        }
                        if !is_bare_statement {
                            self.check_useless_return_value_call(name, args, locals, e.span, label);
                        }
                    } else if locals.guarded_functions.is_empty()
                        || !locals.guarded_functions.contains(
                            &name
                                .rsplit('\\')
                                .next()
                                .unwrap_or(name)
                                .to_ascii_lowercase(),
                        )
                    {
                        // Since resolve_function_candidates always
                        // produces a definite name (unlike `ExprKind::New`'s
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
                            label.to_string(),
                            format!("call to undefined function {name}()"),
                            e.span,
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
                    if self.type_confirmed_not_callable(&callee_ty) {
                        self.report(
                            label.to_string(),
                            format!("Trying to invoke {callee_ty} but it's not a callable."),
                            callee.span,
                            2,
                            "callable.nonCallable",
                        );
                    }
                    if let Some(callable_sig) = callee_ty.callable_sig().cloned() {
                        self.check_dynamic_callable_args(
                            &callable_sig,
                            args,
                            locals,
                            label,
                            e.span,
                        );
                    } else {
                        self.check_named_callable_invocation(
                            &callee_ty, args, locals, e.span, label,
                        );
                    }
                    // A `Closure(...)`-shaped PHPDoc callable carries no
                    // `@throws` info of its own, and any other dynamic-
                    // callee shape has even less to go on - genuinely
                    // unbounded.
                    self.current_function_has_unannotated_call = true;
                }
                // `preg_match`'s by-ref `$matches` shape, including
                // `PREG_OFFSET_CAPTURE`/`PREG_UNMATCHED_AS_NULL`
                // recognition - `parse_preg_flags` bails (`None`) on any
                // other flag, leaving this whole check inert for it.
                // Overrides the generic widening
                // `predefine_byref_or_unknown_args` already did above,
                // rather than replacing it - anything outside this scope
                // keeps that call's Mixed-or-widened fallback unchanged.
                // Placed after `check_args` above, not right after
                // `predefine_byref_or_unknown_args`: `check_args` checks
                // whether the argument `$matches` is compatible with the
                // declared `?array<string> $matches` parameter (the
                // real phpstorm-stubs stub is more specific than the
                // bare native `?array` hint), and that check needs to see
                // `$matches`'s own *pre-call* type, not this override's
                // freshly-computed *post-call* shape - placing this
                // before `check_args` would make every `$matches` shape
                // whose leaf type isn't plain `string` (a `[string, int]`
                // tuple, or `string|null`) spuriously fail that check.
                if resolved_name.as_deref() == Some("preg_match")
                    && resolved.get(1).copied().flatten().is_some()
                    && let Some(pattern_arg) = resolved.first().copied().flatten()
                    && let Some(matches_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(matches_name) = &matches_arg.kind
                    && let Some(pattern_str) = self.constant_string_value(pattern_arg, locals)
                    && let Some(parsed) = regex_groups::parse_capture_groups(&pattern_str)
                    && let Some(flags) = resolved
                        .get(3)
                        .copied()
                        .flatten()
                        .map_or(Some(PregFlags::default()), |e| parse_preg_flags(e, false))
                {
                    let leaf = preg_leaf_type(flags.offset_capture);
                    let matched = build_matched_shape(
                        &parsed.groups,
                        &parsed.alternation_branch_counts,
                        &parsed.alternation_wrapped_in_optional_quantifier,
                        &leaf,
                        flags.unmatched_as_null,
                    );
                    // Unioned with an empty shape for the real
                    // `0`-return/no-match case (real PHP genuinely sets
                    // `$matches = []` then) - sound on its own only
                    // because `check_offset_existence`/`infer_expr`'s
                    // `Index` arm are `Union`-aware, and precision for the
                    // common `if (preg_match(...)) { use $matches }`
                    // idiom is recovered by `apply_narrowing`'s
                    // `preg_match_condition` recognizer, not by this
                    // unconditional type itself.
                    let shape = union_types(matched, Type::ArrayShape(vec![]));
                    purge_stale_narrowing_for(locals, matches_name);
                    // `$matches` is very commonly undeclared before this
                    // exact call - its real first appearance, so
                    // `declare` when there's nothing here yet to preserve
                    // (an already-existing `$matches` keeps its own real
                    // provenance instead).
                    if locals.contains_key(matches_name) {
                        locals.insert(matches_name.to_string(), shape);
                    } else {
                        locals.declare(matches_name.to_string(), shape, matches_arg.span);
                    }
                }
                // `preg_match_all`'s by-ref `$matches` shape - a sibling
                // special case to `preg_match`'s above, sharing the same
                // pattern-parsing/flag-parsing/leaf-type machinery but
                // building a structurally different shape
                // (`PREG_PATTERN_ORDER`, the real default, groups results
                // by capture group across every match; `PREG_SET_ORDER`
                // groups them by match, reusing `preg_match_matches_shape`
                // unchanged for each entry).
                if resolved_name.as_deref() == Some("preg_match_all")
                    && resolved.get(1).copied().flatten().is_some()
                    && let Some(pattern_arg) = resolved.first().copied().flatten()
                    && let Some(matches_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(matches_name) = &matches_arg.kind
                    && let Some(pattern_str) = self.constant_string_value(pattern_arg, locals)
                    && let Some(parsed) = regex_groups::parse_capture_groups(&pattern_str)
                    && let Some(flags) = resolved
                        .get(3)
                        .copied()
                        .flatten()
                        .map_or(Some(PregFlags::default()), |e| parse_preg_flags(e, true))
                {
                    let leaf = preg_leaf_type(flags.offset_capture);
                    let shape = if flags.set_order {
                        Type::Array(
                            Box::new(Type::unknown_array_key()),
                            Box::new(preg_match_matches_shape(
                                &parsed.groups,
                                &leaf,
                                flags.unmatched_as_null,
                            )),
                        )
                    } else {
                        preg_match_all_pattern_order_shape(
                            &parsed.groups,
                            &leaf,
                            flags.unmatched_as_null,
                        )
                    };
                    purge_stale_narrowing_for(locals, matches_name);
                    // Same reasoning as `preg_match`'s identical treatment
                    // above.
                    if locals.contains_key(matches_name) {
                        locals.insert(matches_name.to_string(), shape);
                    } else {
                        locals.declare(matches_name.to_string(), shape, matches_arg.span);
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
                if resolved_name.as_deref() == Some("proc_open")
                    && let Some(pipes_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(pipes_name) = &pipes_arg.kind
                {
                    let shape = Type::Array(
                        Box::new(Type::Int),
                        Box::new(Type::Object("resource".to_string(), Vec::new())),
                    );
                    purge_stale_narrowing_for(locals, pipes_name);
                    if locals.contains_key(pipes_name) {
                        locals.insert(pipes_name.to_string(), shape);
                    } else {
                        locals.declare(pipes_name.to_string(), shape, pipes_arg.span);
                    }
                }
                // Reuses the `resolved_name`/`sig` already resolved above
                // - the same lookup `infer_expr`'s own `Call` arm would
                // otherwise redundantly redo from scratch.
                let call_ty = sig
                    .map(|s| {
                        self.infer_builtin_call_return_type(
                            resolved_name.as_deref().unwrap_or_default(),
                            s,
                            args,
                            locals,
                        )
                    })
                    .unwrap_or_else(|| self.call_fallback_return_type(callee, args, locals));
                match array_shift_pop_pre_widen_ty.and_then(|pre_widen_ty| {
                    Self::array_element_type(&pre_widen_ty).map(|elem_ty| {
                        Self::array_pointer_result(&pre_widen_ty, elem_ty, Type::Null, false)
                    })
                }) {
                    Some(corrected) => corrected,
                    None => call_ty,
                }
            }
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
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
                        unwrap_grouping(recv).kind,
                        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                    ) {
                    let this_ty = args
                        .first()
                        .map(|a| self.infer_expr(&a.value, locals))
                        .unwrap_or(Type::Mixed(false));
                    let scope_class = args
                        .get(1)
                        .and_then(|a| self.literal_class_name_from_expr(&a.value));
                    self.check_closure_literal_with_this_and_scope(
                        unwrap_grouping(recv),
                        locals,
                        label,
                        this_ty,
                        scope_class,
                    );
                    self.infer_expr(recv, locals)
                } else {
                    self.check_expr(recv, locals, label)
                };
                // See `probe_span`'s doc comment.
                self.probe_hit_at(recv.span, &recv_ty);
                // `$x?->method()` is pointless once `$x`'s type already
                // proves it's never null - mirrors real PHPStan's
                // `NullsafeMethodCallRule` exactly. Guarded by
                // `loop_depth`/`static_var_names` for the same reasons
                // `check_pointless_null_probe` is - a receiver only ever
                // widened to nullable on a later loop iteration or an
                // earlier call still looks non-nullable on this single
                // pass.
                if *nullsafe
                    && self.loop_depth == 0
                    && !matches!(&recv.kind, ExprKind::Variable(n) if self.static_var_names.contains(n.as_ref()))
                    && type_definitely_excludes_null(&recv_ty)
                {
                    self.report(
                        label.to_string(),
                        format!(
                            "Using nullsafe method call on non-nullable type {recv_ty}. Use -> instead."
                        ),
                        e.span,
                        0,
                        "nullsafe.neverNull",
                    );
                }
                // A method lookup trusts the specific `ClassInfo` stored
                // under a fully-qualified name, not a bare one two
                // unrelated packages could collide on. `member_missing`
                // below picks up the "should this be flagged" question
                // for a `Union`/`Intersection` receiver (e.g. a multi-type
                // `catch` variable), using a representative member (the
                // first `Object` found) for argument-checking.
                let recv_class = match &recv_ty {
                    Type::Object(class, _) => Some(class.clone()),
                    Type::EnumCase(fqn, _) => Some(fqn.clone()),
                    Type::Union(members) | Type::Intersection(members) => {
                        members.iter().find_map(|m| match m {
                            Type::Object(class, _) => Some(class.clone()),
                            Type::EnumCase(fqn, _) => Some(fqn.clone()),
                            _ => None,
                        })
                    }
                    _ => None,
                };
                // `method.nonObject` - a receiver whose type is confirmed
                // to never be an object at all (a scalar, an array,
                // `stdClass`) gets flagged here and short-circuits the
                // rest of this arm's method-resolution logic entirely,
                // rather than falling through to `method.notFound` below.
                // `recv_class` above and `type_can_never_call_methods` are
                // *not* disjoint sets once a `Union` mixes a real class
                // with a closed member (e.g. `Foo|string`) - this early
                // return, not non-overlapping domains, is what keeps the
                // two checks from double-firing on the same call.
                // Nullsafe-aware the same way real PHPStan's own
                // `NullsafeOperatorHelper::getNullsafeShortcircuitedExprRespectingScope`
                // is: `$x?->method()` on a `null`-only `$x` short-circuits
                // to `null` rather than erroring, while plain
                // `$x->method()` on the same `$x` is a real bug.
                let nonobject_ty = if *nullsafe {
                    narrow_type_excluding(&recv_ty, &Type::Null)
                } else {
                    recv_ty.clone()
                };
                if type_can_never_call_methods(&nonobject_ty) {
                    self.report(
                        label.to_string(),
                        format!("call to method {method}() on a non-object type {recv_ty}"),
                        e.span,
                        0,
                        "method.nonObject",
                    );
                    for a in args {
                        self.check_expr(&a.value, locals, label);
                    }
                    return Type::Mixed(false);
                }
                if let Some(level) = self.registry.strict_mixed_receiver_level(&nonobject_ty) {
                    self.report(
                        label.to_string(),
                        format!("Cannot call method {method}() on mixed."),
                        e.span,
                        level,
                        "method.nonObject",
                    );
                    for a in args {
                        self.check_expr(&a.value, locals, label);
                    }
                    return Type::Mixed(false);
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
                let sig = receiver
                    .as_ref()
                    .map(|(_, _, _, sig)| std::borrow::Borrow::borrow(sig));
                if let Some((_, _, owner, _)) = &receiver
                    && let Some(s) = sig
                {
                    self.check_phpstan_api_method_call(owner, method, s, e.span, label);
                }
                if let Some(class) = &recv_class {
                    self.check_node_connecting_attribute(class, method, args, e.span, label);
                }
                // This method call's own throw contribution.
                if let Some((class, _, _, _)) = &receiver {
                    let dynamic_throw = self.dynamic_method_throw_type(class, method, args);
                    self.record_call_throw_point(dynamic_throw, sig);
                } else {
                    // No resolved receiver class at all - genuinely
                    // unbounded, same reasoning as a dynamic callee.
                    self.current_function_has_unannotated_call = true;
                }
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.maybe_record_param_name_hints(sig, args);
                self.predefine_byref_or_unknown_args(None, sig, args, &resolved, locals);
                for (i, a) in args.iter().enumerate() {
                    // `argument.unpackNonIterable` - see `ExprKind::Call`'s
                    // identical check above.
                    if a.spread {
                        let spread_ty = self.infer_expr(&a.value, locals);
                        if self.type_confirmed_not_iterable(&spread_ty) {
                            self.report(
                                label.to_string(),
                                format!("Only iterables can be unpacked, {spread_ty} given."),
                                a.value.span,
                                3,
                                "argument.unpackNonIterable",
                            );
                        }
                    }
                    let closure_this = sig
                        .and_then(|s| s.params.get(i))
                        .and_then(|p| p.closure_this_type.clone());
                    if let Some(this_ty) = closure_this
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                    {
                        // `@param-closure-this` - see `ExprKind::Call`'s
                        // identical branch.
                        self.check_closure_literal_with_this_and_scope(
                            unwrap_grouping(&a.value),
                            locals,
                            label,
                            this_ty,
                            None,
                        );
                    } else {
                        self.check_expr(&a.value, locals, label);
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
                    let method_exists_guarded = !locals.guarded_methods.is_empty()
                        && matches!(
                            &unwrap_grouping(recv).kind,
                            ExprKind::Variable(name)
                                if locals
                                    .guarded_methods
                                    .contains(&(name.to_string(), method.to_string()))
                        );
                    if !method_exists_guarded
                        && self.registry.classes.contains_key(class.as_str())
                        && self.member_confirmed_missing(&recv_ty, |c| {
                            self.registry.lookup_method(c, method).is_some()
                                || self.registry.lookup_method(c, "__call").is_some()
                        })
                    {
                        let mut builder = DiagnosticBuilder::new(
                            label.to_string(),
                            format!("call to undefined method {recv_ty}::{method}()"),
                            e.span,
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
                            kind: "method",
                            owner,
                            member_name: method,
                            visibility: &sig.visibility,
                            describe: &format!("method {owner}::{method}()"),
                            span: e.span,
                            label,
                        });
                        self.check_internal_method_call(owner, method, sig, false, e.span, label);
                        self.check_restricted_method_call(
                            owner, method, false, args, locals, e.span, label,
                        );
                        self.check_deprecated_method_call(owner, method, sig, false, e.span, label);
                        self.record_member_usage_token(
                            UsageTokenKind::Method,
                            owner,
                            sig.deprecated.is_some(),
                            sig.is_internal,
                            e.span,
                        );
                        self.record_member_ref(e.span, owner, method);
                        self.check_call_site_method_name_case(
                            class,
                            method,
                            e.span,
                            Span {
                                start: recv.span.end,
                                end: e.span.end,
                            },
                            label,
                        );
                        // `DOMDocument::createElement()`'s tag-name
                        // validation - independent of the internal/
                        // deprecated checks above.
                        self.check_dom_create_element(class, method, args, e.span, label);
                        // Sub-phase E of the live SQL query reflection
                        // phase (see delegated-tumbling-dongarra.md) -
                        // independent of the internal/deprecated checks
                        // above too.
                        self.check_dbal_parameter_binding(
                            class, method, recv, args, locals, e.span, label,
                        );
                        // Phase 82's named-argument-rename feature -
                        // see `checker::named_argument_renames`'s own
                        // module doc comment. Always on (not gated on
                        // `facts_only`) - this arm's whole body already
                        // runs unconditionally.
                        self.record_named_argument_call(
                            owner,
                            method,
                            &sig.visibility,
                            e.span,
                            args,
                        );
                        // See the identical comment on the plain-function
                        // Call arm above - `check_args` is spread-aware,
                        // so it's always worth calling.
                        let qualified = format!("{class}::{method}");
                        self.check_args(CheckArgsCall {
                            callee_label: &qualified,
                            sig,
                            args,
                            resolved: &resolved,
                            locals,
                            label,
                            defining_class: Some(class),
                            span: e.span,
                        });
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
                let result = self.method_call_return_type(recv, &recv_ty, method, args, locals);
                if let Some((class, _, _, s)) = &receiver {
                    let s: &FunctionSig = std::borrow::Borrow::borrow(s);
                    self.apply_self_out(recv, s, &resolved, locals, class);
                }
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if (*nullsafe || postfix_chain_has_nullsafe(recv)) && result != Type::Mixed(false) {
                    union_types(result, Type::Null)
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
                let sigil = binop_sigil(op);
                let l_ty = self.check_expr(l, locals, label);
                self.check_non_boolean_and_or_operand(
                    l,
                    &l_ty,
                    sigil,
                    "left",
                    "booleanAnd.leftNotBoolean",
                    label,
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
                    label,
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
                    label,
                    is_condition_position || is_bare_statement,
                );
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(op, l, r, locals)
            }
            ExprKind::Binary(op @ (BinOp::Or | BinOp::LogicalOr), l, r) => {
                let sigil = binop_sigil(op);
                let l_ty = self.check_expr(l, locals, label);
                self.check_non_boolean_and_or_operand(
                    l,
                    &l_ty,
                    sigil,
                    "left",
                    "booleanOr.leftNotBoolean",
                    label,
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
                    label,
                );
                // Mirrors real PHPStan's `BooleanOrConstantConditionRule` -
                // same reasoning as `&&` above.
                self.check_boolean_and_or(
                    op,
                    l,
                    r,
                    e,
                    locals,
                    label,
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
                self.check_expr_quiet_root(l, locals, label);
                self.check_pointless_null_probe(
                    l,
                    locals,
                    label,
                    "nullCoalesce",
                    "on left side of ??",
                    null_probe_verdict,
                );
                self.check_pointless_offset_or_property_probe(
                    l,
                    locals,
                    label,
                    "nullCoalesce",
                    "on left side of ??",
                    null_probe_verdict,
                );
                let r_ty = self.check_expr(r, locals, label);
                self.widen_coalesce_result(l, r_ty, locals)
            }
            ExprKind::Binary(op, l, r) => {
                let l_ty = self.check_expr(l, locals, label);
                let r_ty = self.check_expr(r, locals, label);
                self.check_binary_operand_validity(op, &l_ty, &r_ty, false, l.span, label);
                self.check_comparison_validity(op, &l_ty, &r_ty, l.span, label);
                self.check_duplicate_operands(op, l, r, label);
                self.check_division_by_literal_zero(op, r, &r_ty, r.span, label);
                self.check_constant_loose_comparison(op, l, r, &l_ty, &r_ty, e.span, label);
                self.check_constant_numeric_comparison(op, l, r, &l_ty, &r_ty, e.span, label);
                self.check_strict_comparison_of_different_types(
                    op,
                    l,
                    r,
                    &l_ty,
                    &r_ty,
                    e.span,
                    is_exempt_last_condition,
                    label,
                );
                // Mirrors real PHPStan's `LogicalXorConstantConditionRule` -
                // see `check_logical_xor`'s doc comment. `BinOp::And`/`Or`
                // are deliberately not handled here: they each have their
                // own, earlier-matching `ExprKind::Binary` arm above
                // (short-circuit-narrowing-aware), which this generic arm
                // never reaches for them.
                if *op == BinOp::LogicalXor {
                    self.check_logical_xor(l, r, locals, label);
                }
                if *op == BinOp::Pipe {
                    self.check_pipe_byref(r, locals, label);
                }
                self.infer_binary(op, l, r, locals)
            }
            ExprKind::Variable(name) => {
                if !locals.contains_key(name) && !is_superglobal(name) {
                    self.report(
                        label.to_string(),
                        format!("undefined variable ${name}"),
                        e.span,
                        0,
                        "variable.undefined",
                    );
                }
                // See `Checker::probe_var_provenance`'s doc comment - the
                // same "stash it when the span matches" pattern
                // `probe_hit` uses.
                if self.probe_span.get() == Some(e.span) {
                    *self.probe_var_provenance.borrow_mut() = locals
                        .get_provenance(name)
                        .map(|p| p.iter().copied().collect());
                }
                let ty = locals
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| superglobal_default_type(name));
                // A read's inferred type, for hover - see
                // `Checker::probe_hit_at`'s doc comment.
                self.probe_hit_at(e.span, &ty);
                ty
            }
            ExprKind::Unary(
                op @ (UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec),
                inner,
            ) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_incdec_validity(op, inner, &inner_ty, e.span, label);
                // See `advance_incdec_type`'s doc comment for the real
                // PHP null/bool/string semantics this computes.
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
                // `narrowing_key` purge every other assignment gets) and
                // a safe no-op for any `inner` shape it doesn't specially
                // handle (including `PropertyAccess` - a property's read
                // type is always its declared type, never a locally-
                // tracked literal default, so `$obj->prop++` has no
                // equivalent gap to fix here).
                let is_increment = matches!(op, UnaryOp::PreInc | UnaryOp::PostInc);
                let updated = advance_incdec_type(&inner_ty, is_increment);
                // `$x++`/`$x--` is real, visible rebinding syntax, the
                // same category as `$x += 1`.
                self.bind_assignment_target(inner, updated, locals);
                self.infer_expr(inner, locals)
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_non_boolean_condition(
                    inner,
                    &inner_ty,
                    "a negated boolean",
                    "booleanNot.exprNotBoolean",
                    label,
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
                            label.to_string(),
                            format!(
                                "Negated boolean expression is always {}.",
                                if result_value { "true" } else { "false" }
                            ),
                            inner.span,
                            4,
                            if result_value {
                                "booleanNot.alwaysTrue"
                            } else {
                                "booleanNot.alwaysFalse"
                            },
                        );
                    }
                }
                Type::Bool
            }
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) - the
            // same implicit-numeric-coercion rule arithmetic operators
            // apply generally.
            ExprKind::Unary(op @ (UnaryOp::Neg | UnaryOp::Plus), inner) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_unary_operand_validity(op, &inner_ty, inner.span, label);
                if is_string_shaped(&inner_ty) {
                    Type::Int
                } else if *op == UnaryOp::Neg {
                    negate_numeric_literal(inner_ty)
                } else {
                    inner_ty
                }
            }
            // Mirrors real PHPStan's `IntegerRangeType::toBitwiseNotType()` -
            // `~5`/`~$x` on a known int-literal/range-typed operand
            // computes the order-reversed complement range instead of
            // just passing the operand's type through unchanged (still
            // correct, if imprecise, for a plain unbounded `Int`/`String` -
            // `~"hello"` is a real, still-string byte complement).
            ExprKind::Unary(op @ UnaryOp::BitNot, inner) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_unary_operand_validity(op, &inner_ty, inner.span, label);
                to_bitwise_not(&inner_ty).unwrap_or(inner_ty)
            }
            ExprKind::Grouping(inner) => self.check_expr(inner, locals, label),
            ExprKind::Cast(ty, inner, deprecated_spelling) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_cast_validity(ty, &inner_ty, inner.span, label);
                if ty == "unset" {
                    self.check_unset_cast_supported(e.span, label);
                }
                if ty == "void" {
                    self.check_void_cast(e.span, label, is_bare_statement);
                }
                if *deprecated_spelling {
                    self.check_deprecated_cast_spelling(ty, e.span, label);
                }
                // An implicit `(string)` cast of an object triggers a
                // real `__toString()` call - see
                // `check_implicit_tostring_usage`'s doc comment.
                if ty == "string" {
                    self.check_implicit_tostring_usage(&inner_ty, e.span, label);
                }
                match ty.as_str() {
                    "int" => Type::Int,
                    "float" => Type::Float,
                    "string" => Type::String,
                    "bool" => Type::Bool,
                    "array" => Type::Array(
                        Box::new(Type::unknown_array_key()),
                        Box::new(Type::Mixed(false)),
                    ),
                    "object" => Type::Object("object".to_string(), Vec::new()),
                    "unset" => Type::Null,
                    _ => Type::Mixed(false),
                }
            }
            // See `ExprKind::ShellExec`'s own doc comment - a thin
            // wrapper around the identical `Call` a plain `shell_exec()`
            // invocation would itself produce, so every consumer besides
            // this one new check just delegates straight through.
            ExprKind::ShellExec(inner) => {
                self.check_backtick_deprecation(e.span, label);
                self.check_expr(inner, locals, label)
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used
            // for a function that always throws or exits.
            ExprKind::Throw(inner) => {
                // A `throw` expression (`$x ?? throw new Foo();`,
                // `default => throw ...`, an arrow function body, ...) is
                // the same language construct as a bare `throw $x;`
                // statement at runtime and accepts only a real Throwable
                // just as unconditionally (a real PHP fatal error
                // otherwise).
                let inner_ty = self.check_expr(inner, locals, label);
                if let Some(class) = self.thrown_value_not_throwable(&inner_ty) {
                    self.report(
                        label.to_string(),
                        format!("cannot throw {class}: it does not implement Throwable"),
                        inner.span,
                        0,
                        "throw.notThrowable",
                    );
                }
                // Mirrors `check_throw`'s identical recording for the
                // statement form.
                self.current_function_explicit_throw_points
                    .push((inner_ty.clone(), inner.span));
                self.current_function_throw_points.push(inner_ty);
                Type::Never
            }
            ExprKind::YieldFrom(inner) => {
                let inner_ty = self.check_expr(inner, locals, label);
                let result_ty = generator_template_param(&inner_ty, self.registry, "TReturn");
                self.check_yield_from_compatibility(
                    &inner_ty,
                    &result_ty,
                    e.span,
                    label,
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
                let inner_ty = self.check_expr(inner, locals, label);
                // `clone.nonObject` - shares `property.nonObject`'s own
                // `stdClass`-unaware classifier, not `method.nonObject`'s:
                // real PHPStan's `ObjectType::isCloneable()` (unlike
                // `canCallMethods()`) has no `stdClass` exception.
                if type_can_never_be_object(&inner_ty) {
                    self.report(
                        label.to_string(),
                        format!("Cannot clone non-object variable of type {inner_ty}."),
                        inner.span,
                        3,
                        "clone.nonObject",
                    );
                }
                if let Some(o) = overrides {
                    self.check_expr(o, locals, label);
                }
                inner_ty
            }
            ExprKind::Yield(key, value) => {
                if let Some(k) = key {
                    self.check_expr(k, locals, label);
                }
                if let Some(v) = value {
                    self.check_expr(v, locals, label);
                }
                self.current_declared_return_type
                    .as_ref()
                    .map(|t| generator_template_param(t, self.registry, "TSend"))
                    .unwrap_or(Type::Mixed(false))
            }
            ExprKind::Instanceof(inner, target) => {
                self.check_expr(inner, locals, label);
                match target {
                    InstanceofTarget::Expr(inner_target) => {
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
                    InstanceofTarget::Name(name, name_span) => {
                        if let Type::Object(class, _) = self.resolve_name_expr(name) {
                            if !self.class_name_exists(&class) {
                                self.report(
                                    label.to_string(),
                                    format!("instanceof check target class {class} not found"),
                                    e.span,
                                    0,
                                    "class.notFound",
                                );
                            } else {
                                // `class.nameCase` - `name` is the raw,
                                // as-written text, still a bare string
                                // right here in the AST.
                                self.check_class_name_case(label, name, *name_span);
                                self.check_internal_instanceof(&class, e.span, label);
                                self.check_restricted_class_name_usage(
                                    &class,
                                    "Instanceof references",
                                    e.span,
                                    label,
                                    crate::checker::restricted_symbol_usage::ClassUsageLocation::Other,
                                );
                            }
                        }
                    }
                }
                self.check_impossible_instanceof(
                    inner,
                    target,
                    e.span,
                    locals,
                    label,
                    is_exempt_last_condition,
                );
                Type::Mixed(false)
            }
            ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
                self.check_nullsafe_assignment_target(lhs, label);
                self.check_assignable_target(lhs, label);
                if matches!(&e.kind, ExprKind::AssignRef(..)) {
                    self.check_nullsafe_byref_target(rhs, label);
                    self.check_property_assign_by_ref(rhs, locals, label);
                }
                // `$r = &$a[];` legally creates the new element it refers to.
                let rhs_ty = match &rhs.kind {
                    ExprKind::Index(base, None) if matches!(&e.kind, ExprKind::AssignRef(..)) => {
                        self.check_expr(base, locals, label);
                        Type::Mixed(false)
                    }
                    _ => self.check_expr(rhs, locals, label),
                };
                // `offsetAccess.nonArray` - `[$a, $b] = $x;`/
                // `list($a, $b) = $x;` (both desugar to the same
                // `ExprKind::ArrayLiteral` target - no separate
                // `ExprKind::List` variant exists) destructuring a value
                // confirmed to never be array-like at all.
                if let ExprKind::ArrayLiteral(items) = &lhs.kind
                    && !self.type_confirmed_not_destructurable(&rhs_ty)
                {
                    self.check_destructuring_offsets(items, &rhs_ty, locals, label);
                }
                self.check_offset_assignment(lhs, &rhs_ty, false, locals, label);
                if matches!(&lhs.kind, ExprKind::ArrayLiteral(_))
                    && self.type_confirmed_not_destructurable(&rhs_ty)
                {
                    self.report(
                        label.to_string(),
                        format!("Cannot use array destructuring on {rhs_ty}."),
                        rhs.span,
                        3,
                        "offsetAccess.nonArray",
                    );
                }
                // `$this` is real PHP's reserved, implicitly-bound
                // reference to the current object - reassigning it is an
                // unconditional fatal error ("Cannot re-assign $this.").
                if matches!(&lhs.kind, ExprKind::Variable(v) if **v == *"this") {
                    self.report(
                        label.to_string(),
                        "cannot re-assign $this".to_string(),
                        lhs.span,
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
                self.check_assignment_target(lhs, locals, label);
                self.check_self_assignment(lhs, rhs, label);
                self.check_property_assignment(lhs, rhs, locals, label);
                // A bare by-ref-parameter variable directly assigned a
                // value its declared type can't hold. Also covers a
                // destructured by-ref target (`[$byRef] = $arr;`) - see
                // `check_by_ref_destructuring`'s own doc comment.
                self.check_param_out_assignment(lhs, &rhs_ty, label);
                self.check_by_ref_destructuring(lhs, &rhs_ty, label);
                let native = match (&e.kind, &lhs.kind) {
                    (ExprKind::Assign(..), ExprKind::Variable(name)) => {
                        Some((name.clone(), self.native_expr_type(rhs, locals)))
                    }
                    _ => None,
                };
                self.bind_assignment_target(lhs, rhs_ty.clone(), locals);
                if let Some((name, native)) = native {
                    locals.set_native(&name, native);
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
                // from the ordinary `narrowing_key` fast path, so this is
                // deliberately gated to the dynamic-key case only.
                if let ExprKind::Variable(target_name) = &lhs.kind
                    && let ExprKind::Index(base, Some(idx)) = &unwrap_grouping(rhs).kind
                    && let ExprKind::Variable(container_name) = &unwrap_grouping(base).kind
                    && literal_string_key(idx).is_none()
                {
                    propagate_container_field_facts(locals, target_name, container_name);
                }
                rhs_ty
            }
            ExprKind::CompoundAssign(op, lhs, rhs) => {
                self.check_nullsafe_assignment_target(lhs, label);
                self.check_assignable_target(lhs, label);
                // `$x ??= $default;` is defined as exactly
                // `$x = $x ?? $default;` - the same quiet-left-side
                // exemption as plain `??` applies to its own left side.
                let lhs_ty = if *op == BinOp::Coalesce {
                    if matches!(lhs.kind, ExprKind::Index(_, None)) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                "Cannot use [] for reading.".to_string(),
                                lhs.span,
                                0,
                                "offsetAccess.noDim",
                            )
                            .non_ignorable(),
                        );
                    }
                    let ty = self.check_expr_quiet_root(lhs, locals, label);
                    self.check_pointless_null_probe(
                        lhs,
                        locals,
                        label,
                        "nullCoalesce",
                        "on left side of ??=",
                        null_probe_verdict,
                    );
                    self.check_pointless_offset_or_property_probe(
                        lhs,
                        locals,
                        label,
                        "nullCoalesce",
                        "on left side of ??=",
                        null_probe_verdict,
                    );
                    ty
                } else {
                    self.check_expr(lhs, locals, label)
                };
                let rhs_ty = self.check_expr(rhs, locals, label);
                self.check_offset_assignment(lhs, &rhs_ty, true, locals, label);
                if *op != BinOp::Coalesce {
                    self.check_binary_operand_validity(op, &lhs_ty, &rhs_ty, true, lhs.span, label);
                    self.check_division_by_literal_zero(op, rhs, &rhs_ty, rhs.span, label);
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
                self.check_param_out_assignment(lhs, &ty, label);
                let native = match &lhs.kind {
                    ExprKind::Variable(name) => {
                        Some((name.clone(), self.native_expr_type(e, locals)))
                    }
                    _ => None,
                };
                self.bind_assignment_target(lhs, ty, locals);
                if let Some((name, native)) = native {
                    locals.set_native(&name, native);
                }
                Type::Mixed(false)
            }
            ExprKind::Ternary(cond, then_, else_) => {
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
                        label,
                    );
                }
                // Mirrors real PHPStan's `TernaryOperatorConstantConditionRule`,
                // same design as `If`/`While`/`DoWhile` (`check_if`) - no
                // exclusions beyond `inferred_constant_condition`'s shared
                // ones; this rule has none of `BooleanAnd`/`BooleanOr`'s
                // `isLast`/first-level-statement complexity.
                if let Some(value) = self.inferred_constant_condition(cond, locals) {
                    self.report(
                        label.to_string(),
                        format!(
                            "Ternary operator condition is always {}.",
                            if value { "true" } else { "false" }
                        ),
                        cond.span,
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
                // `union_types`, not a raw `Type::Union(vec![...])` -
                // either branch's type is commonly already a `Union`
                // itself (e.g. a stub-declared `resource|false`), and a
                // bare `vec![then_ty, else_ty]` nests it unflattened
                // instead of merging its members in: a later `!== false`
                // narrowing filters only the outer union's members, so it
                // can strip a bare `false` sitting alongside the nested
                // union but can't reach inside it to strip its own
                // `false` member.
                match then_ty {
                    Some(then_ty) => union_types(then_ty, else_ty),
                    None => Type::Mixed(false),
                }
            }
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    if let Some(k) = &item.key {
                        self.check_expr(k, locals, label);
                        self.check_array_item_key_type(k, locals, label);
                    }
                    let value_ty = self.check_expr(&item.value, locals, label);
                    // `arrayUnpacking.nonIterable` - `[...$x]` where
                    // `$x` is confirmed to never be iterable at all.
                    // Shares `foreach.nonIterable`'s own classifier.
                    if item.spread && self.type_confirmed_not_iterable(&value_ty) {
                        self.report(
                            label.to_string(),
                            format!("Only iterables can be unpacked, {value_ty} given."),
                            item.value.span,
                            3,
                            "arrayUnpacking.nonIterable",
                        );
                    }
                    if item.spread {
                        self.check_unpacked_string_keys(&value_ty, item.value.span, label);
                    }
                }
                self.check_array_literal_duplicate_keys(items, locals, label);
                self.infer_array_literal_type(items, locals)
            }
            ExprKind::Index(base, idx) => {
                let base_ty = self.check_expr(base, locals, label);
                match idx {
                    Some(i) => {
                        self.check_expr(i, locals, label);
                        self.check_offset_accessibility(&base_ty, i, label);
                        self.check_offset_existence(base, i, locals, label);
                        self.check_dim_fetch_key_type(base, i, locals, label);
                        // The `Index` counterpart of `PropertyAccess`'s
                        // "check the narrowed key first" handling above -
                        // without this, `check_expr` (unlike `infer_expr`,
                        // which already does this for `Index`) would
                        // re-derive the type from the base's declared/
                        // inferred shape, discarding whatever
                        // `apply_narrowing` had written under this exact
                        // `narrowing_key`. A bare `return $x['k'];` right
                        // after narrowing `$x['k']` would otherwise see
                        // the un-narrowed type.
                        if locals.may_have_member_narrowing()
                            && let Some(key) = narrowing_key(e)
                            && let Some(narrowed) = locals.get(&key)
                        {
                            return narrowed.clone();
                        }
                        // Calls the shared `index_result_type` directly,
                        // rather than a hand-maintained inline copy of it,
                        // so its `Type::Union`/`Type::Intersection` arms
                        // stay in sync - a container widened to a `Union`
                        // across a loop's multi-pass fixed-point
                        // convergence must resolve here exactly as
                        // `infer_expr`'s identical read does.
                        index_result_type(base_ty, literal_string_key(i).as_deref())
                    }
                    // `OffsetAccessWithoutDimForReadingRule` - every
                    // write target routes around this arm, and a
                    // by-reference argument slot is recorded ahead of it.
                    None => {
                        if self.by_ref_empty_dim_args.borrow().contains(&e.span) {
                            return Type::Mixed(false);
                        }
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                "Cannot use [] for reading.".to_string(),
                                e.span,
                                0,
                                "offsetAccess.noDim",
                            )
                            .non_ignorable(),
                        );
                        Type::Mixed(false)
                    }
                }
            }
            ExprKind::New(class_expr, args) => {
                // `new Foo()` never carries known type arguments itself -
                // real PHP's constructor syntax has no way to write
                // `new Foo<Bar>()` at all - but `infer_expr`'s `New` arm
                // binds them from the constructor's own arguments instead,
                // the way real PHPStan does, when `Foo` is itself generic
                // (`@template` on the class).
                let new_ty = self.infer_expr(e, locals);
                let resolved_class = match &new_ty {
                    Type::Object(class, _) => Some(class.clone()),
                    _ => None,
                };
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
                        .unwrap_or(class)
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
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.maybe_record_param_name_hints(ctor_sig, args);
                self.predefine_byref_or_unknown_args(None, ctor_sig, args, &resolved, locals);
                for (i, a) in args.iter().enumerate() {
                    // `argument.unpackNonIterable` - see `ExprKind::Call`'s
                    // identical check above.
                    if a.spread {
                        let spread_ty = self.infer_expr(&a.value, locals);
                        if self.type_confirmed_not_iterable(&spread_ty) {
                            self.report(
                                label.to_string(),
                                format!("Only iterables can be unpacked, {spread_ty} given."),
                                a.value.span,
                                3,
                                "argument.unpackNonIterable",
                            );
                        }
                    }
                    let closure_this = ctor_sig
                        .and_then(|s| s.params.get(i))
                        .and_then(|p| p.closure_this_type.clone());
                    if let Some(this_ty) = closure_this
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                    {
                        // `@param-closure-this` - see `ExprKind::Call`'s
                        // identical branch.
                        self.check_closure_literal_with_this_and_scope(
                            unwrap_grouping(&a.value),
                            locals,
                            label,
                            this_ty,
                            None,
                        );
                    } else {
                        self.check_expr(&a.value, locals, label);
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
                    if let ExprKind::Name(raw) = &class_expr.kind {
                        self.check_class_name_case(label, raw, class_expr.span);
                    }
                    // Called before the borrow just below (this check
                    // does its own, separate registry lookup) since it
                    // needs `&mut self`.
                    self.check_internal_instantiation(class, e.span, label);
                    self.check_restricted_class_name_usage(
                        class,
                        "Instantiation of",
                        e.span,
                        label,
                        crate::checker::restricted_symbol_usage::ClassUsageLocation::Other,
                    );
                    self.check_deprecated_instantiation(class, e.span, label);
                    self.record_instantiation_usage_token(class, class_expr.span);
                    // `DateInterval`'s duration-string constructor
                    // validation - independent of whether the class is
                    // otherwise instantiable, so this runs unconditionally
                    // before that dispatch.
                    self.check_date_interval_constructor(class, args, e.span, label);
                    // `DateTime`/`DateTimeImmutable`'s own constructor-
                    // string validation - same shape.
                    self.check_date_time_constructor(class, args, e.span, label);
                    // `SimpleXMLElement`'s construction well-formedness
                    // validation - same shape.
                    self.check_simplexml_element_constructor(class, args, e.span, label);
                    // This constructor's own throw contribution.
                    let dynamic_throw = self.dynamic_new_throw_type(class, args);
                    self.record_call_throw_point(dynamic_throw, ctor_sig);
                    let (is_name, is_static) = match &class_expr.kind {
                        ExprKind::Name(raw) => (true, raw.eq_ignore_ascii_case("static")),
                        _ => (false, false),
                    };
                    if self.registry.lookup_class(class).is_some() {
                        if self.check_instantiated_class(
                            class,
                            is_name,
                            is_static,
                            !args.is_empty(),
                            e.span,
                            label,
                        ) && let Some(sig) = ctor_sig
                        {
                            // See the identical comment on the plain-
                            // function Call arm above - `check_args` is
                            // spread-aware, so it's always worth calling.
                            self.check_args(CheckArgsCall {
                                callee_label: &format!("{class}::__construct"),
                                sig,
                                args,
                                resolved: &resolved,
                                locals,
                                label,
                                defining_class: Some(class),
                                span: e.span,
                            });
                        }
                    } else {
                        self.report(
                            label.to_string(),
                            format!("instantiated class {class} not found"),
                            e.span,
                            0,
                            "class.notFound",
                        );
                    }
                } else {
                    match &class_expr.kind {
                        ExprKind::Name(raw) => {
                            self.check_new_pseudo_target(raw, class_expr.span, label);
                        }
                        _ => {
                            let target_ty = self.infer_expr(class_expr, locals);
                            self.check_new_on_non_class_name(&target_ty, class_expr.span, label);
                        }
                    }
                }
                new_ty
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                let ctor_params = decl
                    .methods
                    .iter()
                    .find(|m| m.function.name.eq_ignore_ascii_case("__construct"))
                    .map(|m| m.function.params.as_slice())
                    .unwrap_or_default();
                for (i, a) in args.iter().enumerate() {
                    let param = match &a.name {
                        Some(name) => ctor_params.iter().find(|p| *p.name == **name),
                        None => ctor_params
                            .get(i)
                            .or_else(|| ctor_params.last().filter(|p| p.variadic)),
                    };
                    if matches!(a.value.kind, ExprKind::Index(_, None))
                        && param.is_some_and(|p| p.by_ref)
                    {
                        self.by_ref_empty_dim_args.borrow_mut().insert(a.value.span);
                    }
                    self.check_expr(&a.value, locals, label);
                }
                // The anonymous class's own methods are still worth
                // checking - check_function only needs a FunctionDecl and
                // a label to check against, independent of whether the
                // class itself is indexed into the registry under a
                // stable name.
                //
                // `ancestor_sig` looks up the same method name directly
                // against whichever real, already-registered interface/
                // parent this anonymous class implements/extends
                // (`extends` first, matching `Registry::
                // lookup_ancestor_method_with_owner`'s order), since an
                // anonymous class's synthetic name is never indexed into
                // the registry for `check_function`'s own `sig` lookup to
                // find otherwise. Passed only as `check_missing_function_
                // typehints`'s fallback (see `check_function_with_
                // ancestor_hint`'s doc comment) - this project still
                // doesn't otherwise type-check an anonymous class's body
                // against its implemented interfaces.
                let previous_anonymous_class = std::mem::replace(
                    &mut self.current_anonymous_class,
                    self.anonymous_class_names.get(&e.span).cloned(),
                );
                if let Some(name) = self.current_anonymous_class.clone() {
                    self.check_inheritance_target_kinds(&name, true, label, e.span);
                    self.check_magic_serialization_methods(&name, "anonymous class", label, e.span);
                }
                let registry_name = self
                    .current_anonymous_class
                    .clone()
                    .unwrap_or_else(|| decl.name.to_string());
                self.check_property_declarations(
                    &registry_name,
                    "class@anonymous",
                    ClassKind::Class,
                    false,
                    &decl.properties,
                    &decl.methods,
                );
                self.check_readonly_class_version(decl.is_readonly, true, label, e.span);
                for m in &decl.methods {
                    let ancestor_sig =
                        decl.extends
                            .iter()
                            .chain(decl.implements.iter())
                            .find_map(|name| {
                                self.registry
                                    .lookup_method(&self.name_ctx.resolve(name), &m.function.name)
                            });
                    self.check_function_with_ancestor_hint(
                        &m.function,
                        Some(&decl.name),
                        m.is_static,
                        true,
                        ancestor_sig,
                    );
                }
                self.current_anonymous_class = previous_anonymous_class;
                // Its own properties'/consts' attributes too -
                // method/param attributes are already covered inside
                // `check_function` above.
                for p in &decl.properties {
                    self.check_attributes(&p.attrs, &decl.name);
                    self.check_attribute_targets(
                        &p.attrs,
                        ATTRIBUTE_TARGET_PROPERTY,
                        "property",
                        &decl.name,
                    );
                }
                for cc in &decl.consts {
                    self.check_attributes(&cc.attrs, &decl.name);
                    self.check_attribute_targets(
                        &cc.attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                        &decl.name,
                    );
                }
                self.anonymous_class_type(e.span)
            }
            ExprKind::StaticCall(target, method, args) => {
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
                    .and_then(|class| self.registry.lookup_method_with_owner(class, method));
                let sig = sig_with_owner.as_ref().map(|(_, s)| *s);
                // Phase 92's own `::cases()` escape hatch - see
                // `Checker::escaped_enums`'s own doc comment. `owner`
                // (not `resolved_class`) is the registry's own
                // canonically-declared name, which matters here: a
                // differently-cased call site (`SUIT::cases()`) would
                // silently fail to match `self.registry.classes` if
                // keyed by `resolved_class` instead.
                if method.eq_ignore_ascii_case("cases")
                    && let Some((owner, _)) = &sig_with_owner
                    && self
                        .registry
                        .classes
                        .get(owner.as_ref())
                        .is_some_and(|c| c.kind == ClassKind::Enum)
                {
                    self.record_enum_escape(owner);
                }
                // This static call's own throw contribution.
                if let Some(class) = &resolved_class {
                    let dynamic_throw = self.dynamic_method_throw_type(class, method, args);
                    self.record_call_throw_point(dynamic_throw, sig);
                } else {
                    self.current_function_has_unannotated_call = true;
                }
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.maybe_record_param_name_hints(sig, args);
                self.predefine_byref_or_unknown_args(None, sig, args, &resolved, locals);
                // `Closure::bind($closure, $newThis, $scope)` makes
                // `$this` (and, for private/protected access, the
                // calling scope) available inside a closure that
                // otherwise wouldn't have it - real, idiomatic PHP for
                // reaching into an object's state from outside its class.
                // Only special-cased when the first argument is literally
                // a closure expression right here (not some arbitrary
                // `Type::Object("Closure", _)`-typed value) - the closure
                // body needs to still be reachable as an `Expr` to
                // re-check with the rebound `$this`.
                if resolved_class.as_deref() == Some("Closure")
                    && method == "bind"
                    && !args.is_empty()
                    && matches!(
                        unwrap_grouping(&args[0].value).kind,
                        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                    )
                {
                    let this_ty = args
                        .get(1)
                        .map(|a| self.infer_expr(&a.value, locals))
                        .unwrap_or(Type::Mixed(false));
                    let scope_class = args
                        .get(2)
                        .and_then(|a| self.literal_class_name_from_expr(&a.value));
                    self.check_closure_literal_with_this_and_scope(
                        unwrap_grouping(&args[0].value),
                        locals,
                        label,
                        this_ty,
                        scope_class,
                    );
                    for a in &args[1..] {
                        self.check_expr(&a.value, locals, label);
                        // `argument.unpackNonIterable` - see
                        // `ExprKind::Call`'s identical check above.
                        if a.spread {
                            let spread_ty = self.infer_expr(&a.value, locals);
                            if self.type_confirmed_not_iterable(&spread_ty) {
                                self.report(
                                    label.to_string(),
                                    format!("Only iterables can be unpacked, {spread_ty} given."),
                                    a.value.span,
                                    3,
                                    "argument.unpackNonIterable",
                                );
                            }
                        }
                    }
                } else {
                    for (i, a) in args.iter().enumerate() {
                        // `argument.unpackNonIterable` - see
                        // `ExprKind::Call`'s identical check above.
                        if a.spread {
                            let spread_ty = self.infer_expr(&a.value, locals);
                            if self.type_confirmed_not_iterable(&spread_ty) {
                                self.report(
                                    label.to_string(),
                                    format!("Only iterables can be unpacked, {spread_ty} given."),
                                    a.value.span,
                                    3,
                                    "argument.unpackNonIterable",
                                );
                            }
                        }
                        let closure_this = sig
                            .and_then(|s| s.params.get(i))
                            .and_then(|p| p.closure_this_type.clone());
                        if let Some(this_ty) = closure_this
                            && matches!(
                                unwrap_grouping(&a.value).kind,
                                ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                            )
                        {
                            // `@param-closure-this` - see
                            // `ExprKind::Call`'s identical branch.
                            self.check_closure_literal_with_this_and_scope(
                                unwrap_grouping(&a.value),
                                locals,
                                label,
                                this_ty,
                                None,
                            );
                        } else {
                            self.check_expr(&a.value, locals, label);
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
                        self.check_class_name_case(label, raw, e.span);
                    }
                    if let Some(sig) = sig {
                        if let Some((owner, _)) = &sig_with_owner {
                            self.check_member_visibility(MemberVisibilityCheck {
                                kind: "staticMethod",
                                owner,
                                member_name: method,
                                visibility: &sig.visibility,
                                describe: &format!("static method {owner}::{method}()"),
                                span: e.span,
                                label,
                            });
                            self.check_internal_method_call(
                                owner, method, sig, true, e.span, label,
                            );
                            self.check_restricted_method_call(
                                owner, method, true, args, locals, e.span, label,
                            );
                            self.check_deprecated_method_call(
                                owner, method, sig, true, e.span, label,
                            );
                            self.record_member_usage_token(
                                UsageTokenKind::Method,
                                owner,
                                sig.deprecated.is_some(),
                                sig.is_internal,
                                e.span,
                            );
                            self.record_member_ref(e.span, owner, method);
                            self.check_static_call_context(
                                target, class, owner, method, e.span, label,
                            );
                            // Mirrors real PHPStan's
                            // `CallPrivateMethodThroughStaticRule` - same
                            // late-static-binding unsafety as the
                            // identical class-constant/static-property
                            // rules.
                            if target.as_name() == Some("static")
                                && sig.visibility == Visibility::Private
                            {
                                self.check_static_private_method_access(
                                    owner, method, e.span, label,
                                );
                            }
                        }
                        self.check_call_site_method_name_case(
                            class,
                            method,
                            e.span,
                            Span {
                                start: static_target_end(target),
                                end: e.span.end,
                            },
                            label,
                        );
                        // See the identical comment on the plain-
                        // function Call arm above - `check_args` is
                        // spread-aware, so it's always worth calling.
                        let qualified = format!("{class}::{method}");
                        self.check_args(CheckArgsCall {
                            callee_label: &qualified,
                            sig,
                            args,
                            resolved: &resolved,
                            locals,
                            label,
                            defining_class: Some(class),
                            span: e.span,
                        });
                    } else if self.registry.classes.contains_key(class.as_str())
                        && self.registry.lookup_method(class, "__callStatic").is_none()
                    {
                        // Same leniency as MethodCall's own undefined-
                        // method check: only fires once the class itself
                        // is confirmed known, and __callStatic (the
                        // static equivalent of __call) suppresses it
                        // entirely, matching real PHPStan.
                        self.report(
                            label.to_string(),
                            format!("call to undefined static method {class}::{method}()"),
                            e.span,
                            0,
                            "staticMethod.notFound",
                        );
                    } else if !self.registry.classes.contains_key(class.as_str()) {
                        // Mirrors real PHPStan's `StaticMethodCallCheck`'s
                        // "class doesn't exist at all" half - neither
                        // branch above fires for a genuinely undefined
                        // class (no `sig` to check args against, and the
                        // `contains_key` guard on the undefined-method
                        // branch correctly excludes it too).
                        self.report(
                            label.to_string(),
                            format!("static method call target class {class} not found"),
                            e.span,
                            0,
                            "class.notFound",
                        );
                    }
                } else {
                    self.check_unresolved_static_call_target(target, method, locals, e.span, label);
                }
                match (&resolved_class, sig) {
                    (Some(class), Some(s)) => {
                        let resolved_ty = self.infer_call_return_type(s, &resolved, locals);
                        let resolved_ty =
                            self.narrow_enum_from_call(class, method, args, resolved_ty, locals);
                        // Phase 92's own `::from()`/`::tryFrom()` literal-
                        // argument signal - see `checker::whole_program::
                        // dead_enum_cases`'s own doc comment. Reads the
                        // type `narrow_enum_from_call` just computed
                        // rather than threading a new span parameter
                        // through it.
                        if let Type::EnumCase(ref case_class, ref case) = resolved_ty {
                            self.record_member_ref(e.span, case_class, case);
                        }
                        match self.class_argument_return(class, method, args, locals) {
                            Some(ty) => ty,
                            None => self.resolve_static_call_return(&resolved_ty, target, class),
                        }
                    }
                    _ => Type::Mixed(false),
                }
            }
            ExprKind::DynamicClassConstFetch(class_name, name) => {
                let name_ty = self.check_expr(name, locals, label);
                self.check_dynamic_class_constant_fetch(&name_ty, e.span, label);
                let literal_names: Option<Vec<String>> = match &name_ty {
                    Type::StringLiteral(s) => Some(vec![s.clone()]),
                    Type::Union(members) => members
                        .iter()
                        .map(|m| match m {
                            Type::StringLiteral(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect(),
                    _ => None,
                };
                let fetch = |member: String| Expr {
                    kind: ExprKind::StaticAccess(class_name.clone(), member),
                    span: e.span,
                };
                match literal_names {
                    Some(names) if !names.is_empty() => names
                        .into_iter()
                        .map(|n| self.check_expr(&fetch(n), locals, label))
                        .reduce(crate::checker::type_union::union_types)
                        .unwrap_or(Type::Mixed(false)),
                    _ => {
                        self.check_expr(&fetch("{dynamic}".to_string()), locals, label);
                        Type::Mixed(false)
                    }
                }
            }
            ExprKind::StaticAccess(class_name, member) => {
                // `Foo::class` (member == "class") always resolves to a
                // real string constant, not an actual member of the
                // class - and a fully dynamic member name (`Foo::{$e}`/
                // `Foo::$$e`) has no statically known name to check in
                // the first place. Both are the parser's own documented
                // encoding for these two cases, not something to check.
                if member == "class" {
                    return match self.resolve_static_class_name(class_name) {
                        Some(class) => Type::ClassString(Box::new(Type::Object(class, Vec::new()))),
                        // A genuinely dynamic target (`$obj::class`) -
                        // unresolvable, unchanged from before.
                        None => Type::String,
                    };
                }
                if member == "{dynamic}" {
                    // Phase 92's own dynamic-member escape hatch - see
                    // `Checker::escaped_enums`'s own doc comment. No
                    // `lookup_method_with_owner`-style call to piggyback
                    // an already-canonical name off here (the member
                    // name genuinely isn't known), so this calls
                    // `lookup_class` itself for the canonical form -
                    // `resolve_static_class_name` alone doesn't
                    // guarantee it.
                    if let Some(raw) = self.resolve_static_class_name(class_name)
                        && let Some((canonical, info)) = self.registry.lookup_class(&raw)
                        && info.kind == ClassKind::Enum
                    {
                        self.record_enum_escape(canonical);
                    }
                    return Type::Mixed(false);
                }
                // A static property (`self::$prop`) can be narrowed the
                // same way an instance property already is - check for a
                // locally-narrowed override before the undefined-property
                // check/registry lookup below.
                if member.starts_with('$')
                    && locals.may_have_member_narrowing()
                    && let Some(narrowed) = locals.get(&format!("{class_name}::{member}"))
                {
                    return narrowed.clone();
                }
                let Some(class) = self.resolve_static_class_name(class_name) else {
                    return Type::Mixed(false);
                };
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
                if !self.registry.classes.contains_key(class.as_str()) {
                    // Mirrors real PHPStan's `AccessStaticPropertiesCheck`/
                    // `ClassConstantRule`'s "class doesn't exist at all"
                    // half, for both a static-property and a
                    // class-constant reference on a genuinely undefined
                    // class.
                    let subject = if member.starts_with('$') {
                        "static property access"
                    } else {
                        "class constant access"
                    };
                    self.report(
                        label.to_string(),
                        format!("{subject} target class {class} not found"),
                        e.span,
                        0,
                        "class.notFound",
                    );
                    return Type::Mixed(false);
                }
                // `class.nameCase` - `class_name` is the raw, as-written
                // text (`resolve_static_class_name` above already
                // resolved `self`/`parent`/`static`/a variable target to
                // `class`, but never touches `class_name` itself).
                self.check_class_name_case(label, class_name, e.span);
                if let Some(prop_name) = member.strip_prefix('$') {
                    // Unlike an instance property, PHP has no legal way
                    // to dynamically create a static property at all - no
                    // "could be legitimate" case to stay lenient for
                    // here, the way check_property_assignment's write
                    // side has to for instance properties. A property
                    // declared as an ordinary instance property is
                    // equally invalid through `::` in real PHP ("Access
                    // to undeclared static property"), so both share one
                    // message.
                    let prop_with_owner =
                        self.registry.lookup_property_with_owner(&class, prop_name);
                    let is_valid_static = prop_with_owner
                        .as_ref()
                        .is_some_and(|(_, sig)| sig.is_static);
                    if !is_valid_static {
                        self.report(
                            label.to_string(),
                            format!("access to undefined static property {class}::${prop_name}"),
                            e.span,
                            0,
                            "staticProperty.notFound",
                        );
                    } else if let Some((owner, sig)) = &prop_with_owner {
                        self.check_internal_property_access(owner, prop_name, sig, e.span, label);
                        self.check_restricted_property_access(
                            owner, prop_name, true, e.span, label,
                        );
                        self.check_deprecated_property_access(owner, prop_name, sig, e.span, label);
                        self.record_member_usage_token(
                            UsageTokenKind::Property,
                            owner,
                            sig.deprecated.is_some(),
                            sig.is_internal,
                            e.span,
                        );
                        self.record_member_ref(e.span, owner, prop_name);
                        // A static property shares the exact same
                        // visibility rules as an instance one ("Cannot
                        // access private property" is a real fatal error
                        // either way).
                        self.check_member_visibility(MemberVisibilityCheck {
                            kind: "staticProperty",
                            owner,
                            member_name: prop_name,
                            visibility: &sig.visibility,
                            describe: &format!("static property {owner}::${prop_name}"),
                            span: e.span,
                            label,
                        });
                        // Mirrors real PHPStan's
                        // `AccessPrivatePropertyThroughStaticRule` - same
                        // late-static-binding unsafety as the identical
                        // class-constant rule above.
                        if class_name == "static" && sig.visibility == Visibility::Private {
                            self.check_static_private_property_access(
                                owner, prop_name, e.span, label,
                            );
                        }
                    }
                    prop_with_owner
                        .map(|(_, p)| self.resolve_pseudo(&p.ty, &class))
                        .unwrap_or(Type::Mixed(false))
                } else {
                    let const_with_owner = self.registry.lookup_const_with_owner(&class, member);
                    let const_ty = const_with_owner.as_ref().map(|(owner, cs)| {
                        self.registry
                            .dynamic_class_constant_type(owner, member, &cs.ty)
                            .unwrap_or_else(|| cs.ty.clone())
                    });
                    if const_ty.is_none() {
                        self.report(
                            label.to_string(),
                            format!("access to undefined constant {class}::{member}"),
                            e.span,
                            0,
                            "classConstant.notFound",
                        );
                    } else if let Some((owner, cs)) = &const_with_owner {
                        self.check_internal_class_constant_access(owner, member, cs, e.span, label);
                        self.check_restricted_class_constant_access(owner, member, e.span, label);
                        self.check_deprecated_class_constant_access(
                            owner, member, cs, e.span, label,
                        );
                        self.record_member_usage_token(
                            UsageTokenKind::ClassConstant,
                            owner,
                            cs.deprecated.is_some(),
                            cs.is_internal,
                            e.span,
                        );
                        self.record_member_ref(e.span, owner, member);
                        self.check_member_visibility(MemberVisibilityCheck {
                            kind: "classConstant",
                            owner,
                            member_name: member,
                            visibility: &cs.visibility,
                            describe: &format!("constant {owner}::{member}"),
                            span: e.span,
                            label,
                        });
                        // Mirrors real PHPStan's
                        // `AccessPrivateConstantThroughStaticRule`.
                        if class_name == "static" && cs.visibility == Visibility::Private {
                            self.check_static_private_constant_access(owner, member, e.span, label);
                        }
                    }
                    const_ty
                        .map(|ty| self.resolve_pseudo(&ty, &class))
                        .unwrap_or(Type::Mixed(false))
                }
            }
            ExprKind::DynamicVariable(name_expr) => {
                self.check_expr(name_expr, locals, label);
                Type::Mixed(false)
            }
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                // Unlike a write (check_property_assignment, and the
                // Assign/ArrayLiteral-destructuring arms below that
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
                self.probe_hit_at(base.span, &base_ty);
                // `$x?->prop` is pointless once `$x`'s type already
                // proves it's never null - mirrors real PHPStan's
                // `NullsafePropertyFetchRule` exactly. Guarded by
                // `loop_depth`/`static_var_names` - see the identical
                // guard on the `MethodCall` arm above for why.
                if *nullsafe
                    && self.loop_depth == 0
                    && !matches!(&base.kind, ExprKind::Variable(n) if self.static_var_names.contains(n.as_ref()))
                    && type_definitely_excludes_null(&base_ty)
                {
                    self.report(
                        label.to_string(),
                        format!(
                            "Using nullsafe property access on non-nullable type {base_ty}. Use -> instead."
                        ),
                        e.span,
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
                let is_pest_this =
                    matches!(&base.kind, ExprKind::Variable(name) if **name == *"this");
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
                let property_exists_guarded = !locals.guarded_properties.is_empty()
                    && matches!(
                        &base.kind,
                        ExprKind::Variable(name)
                            if locals
                                .guarded_properties
                                .contains(&(name.to_string(), prop.to_string()))
                    );
                // `property.nonObject` - same reasoning and nullsafe
                // handling as `method.nonObject` above, checked first so
                // it short-circuits before `property.notFound` ever gets
                // a chance to also fire on the same access.
                let nonobject_ty = if *nullsafe {
                    narrow_type_excluding(&base_ty, &Type::Null)
                } else {
                    base_ty.clone()
                };
                if type_can_never_be_object(&nonobject_ty) {
                    self.report(
                        label.to_string(),
                        format!("access to property ${prop} on a non-object type {base_ty}"),
                        e.span,
                        0,
                        "property.nonObject",
                    );
                } else if let Some(level) = self.registry.strict_mixed_receiver_level(&nonobject_ty)
                {
                    self.report(
                        label.to_string(),
                        format!("Cannot access property ${prop} on mixed."),
                        e.span,
                        level,
                        "property.nonObject",
                    );
                } else if !property_exists_guarded
                    && self.member_confirmed_missing(&base_ty, |c| {
                        c == "stdClass"
                            || self.registry.lookup_property(c, prop).is_some()
                            || self.registry.lookup_method(c, "__get").is_some()
                            || (is_pest_this && self.pest_hook_properties.contains(prop.as_ref()))
                    })
                {
                    self.report(
                        label.to_string(),
                        format!("access to undefined property {base_ty}::${prop}"),
                        e.span,
                        2,
                        "property.notFound",
                    );
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
                let owner_and_sig =
                    self.find_property_owner(&narrow_type_excluding(&base_ty, &Type::Null), prop);
                if let Some((owner, p)) = owner_and_sig {
                    self.check_member_visibility(MemberVisibilityCheck {
                        kind: "property",
                        owner: &owner,
                        member_name: prop,
                        visibility: &p.visibility,
                        describe: &format!("property {owner}::${prop}"),
                        span: e.span,
                        label,
                    });
                    if !p.is_readable()
                        && self.is_member_access_visible(&owner, &p.visibility, prop, "property")
                    {
                        self.report(
                            label.to_string(),
                            format!("Property {owner}::${prop} is not readable."),
                            e.span,
                            0,
                            "property.writeOnly",
                        );
                    }
                    self.check_internal_property_access(&owner, prop, p, e.span, label);
                    self.check_restricted_property_access(&owner, prop, false, e.span, label);
                    self.check_deprecated_property_access(&owner, prop, p, e.span, label);
                    self.record_member_usage_token(
                        UsageTokenKind::Property,
                        &owner,
                        p.deprecated.is_some(),
                        p.is_internal,
                        e.span,
                    );
                    self.record_member_ref(e.span, &owner, prop);
                }
                if locals.may_have_member_narrowing()
                    && let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                // `EnumCase`/`ObjectShape` precision and the general
                // `Object` lookup all live in `property_access_type`,
                // shared with `infer_expr`'s identical arm - see its doc
                // comment.
                let result =
                    self.property_access_type(&narrow_type_excluding(&base_ty, &Type::Null), prop);
                if (*nullsafe || postfix_chain_has_nullsafe(base)) && result != Type::Mixed(false) {
                    union_types(result, Type::Null)
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
            ExprKind::DynamicPropertyAccess(base, name_expr) => {
                let base_ty = self.check_expr(base, locals, label);
                self.check_expr(name_expr, locals, label);
                if let ExprKind::Literal(Literal::String(name)) = &name_expr.kind {
                    if let Some((owner, _)) = self.find_property_owner(&base_ty, name) {
                        self.record_member_ref(e.span, &owner, name);
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
                Type::Mixed(false)
            }
            // Same literal-name-is-real-usage/genuinely-dynamic-name-
            // escapes split as `DynamicPropertyAccess` just above.
            ExprKind::DynamicMethodCall(recv, name_expr, args) => {
                let recv_ty = self.check_expr(recv, locals, label);
                self.check_expr(name_expr, locals, label);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                if let ExprKind::Literal(Literal::String(name)) = &name_expr.kind {
                    if let Some((owner, _)) = self.find_method_owner(&recv_ty, name) {
                        self.record_member_ref(e.span, &owner, name);
                    }
                } else if self.collect_member_refs {
                    for class in self.escape_targets_for_receiver_type(&recv_ty) {
                        self.record_member_class_escape(&class);
                    }
                }
                Type::Mixed(false)
            }
            ExprKind::Interpolated(parts) => {
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
                for p in parts {
                    let part_ty = self.check_expr(p, locals, label);
                    if matches!(&p.kind, ExprKind::Literal(Literal::String(_))) {
                        continue;
                    }
                    self.check_implicit_tostring_usage(&part_ty, p.span, label);
                    if self.type_to_string(&part_ty).is_none() {
                        self.report(
                            label.to_string(),
                            format!(
                                "Part of encapsed string ({part_ty}) cannot be cast to string."
                            ),
                            p.span,
                            2,
                            "encapsedStringPart.nonString",
                        );
                    }
                }
                Type::String
            }
            ExprKind::Match {
                subject,
                arms,
                default,
            } => {
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
                let arms_count = arms.len();
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
                // case list of its own for `narrow_type_excluding` to
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
                    Type::True
                } else {
                    self.expand_enum_cases(&self.infer_expr(subject, locals))
                };
                for (i, (conds, result)) in arms.iter().enumerate() {
                    for c in conds {
                        self.check_expr(c, locals, label);
                    }
                    if self.loop_depth == 0 && !next_arm_is_dead {
                        for c in conds {
                            let cond_ty = self.infer_expr(c, &accumulated);
                            let Some(outcome) = self
                                .registry
                                .identical_result(&remaining_subject_ty, &cond_ty)
                            else {
                                continue;
                            };
                            if !outcome {
                                self.report(
                                    label.to_string(),
                                    format!(
                                        "Match arm comparison between {remaining_subject_ty} and {cond_ty} is always false."
                                    ),
                                    c.span,
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
                                self.report(
                                    label.to_string(),
                                    format!(
                                        "Match arm comparison between {remaining_subject_ty} and {cond_ty} is always true."
                                    ),
                                    c.span,
                                    4,
                                    "match.alwaysTrue",
                                );
                            }
                        }
                    }
                    let mut arm_locals =
                        self.narrow_match_arm_conditions(subject, conds, &accumulated, match_true);
                    result_types.push(self.check_expr(result, &mut arm_locals, label));
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&m, &arm_locals),
                        None => arm_locals.clone(),
                    });
                    if i == arms_count - 1 {
                        accumulated_before_last_arm = accumulated.clone();
                    }
                    if match_true {
                        // A skipped arm had every condition false.
                        for c in conds {
                            self.apply_narrowing(c, &mut accumulated, false);
                        }
                    } else {
                        for c in conds {
                            self.apply_equality_narrowing(subject, c, &mut accumulated, false);
                            // `remaining_subject_ty`'s own incremental
                            // exclusion - see this loop's own leading
                            // comment for why this can't just reuse
                            // `accumulated`'s value above.
                            let cond_ty = self.infer_expr(c, locals);
                            if is_known_single_value(&cond_ty) {
                                remaining_subject_ty =
                                    narrow_type_excluding(&remaining_subject_ty, &cond_ty);
                            }
                        }
                    }
                }
                // `match.unhandled` - real PHPStan's own `MatchExpressionRule`
                // (`vendor/phpstan-src/src/Rules/Comparison/MatchExpressionRule.php:160-181`),
                // including both of its real exemptions (`isUnhandledMatchErrorCaught`/
                // `hasUnhandledMatchErrorThrowsTag`, `match_unhandled_error_is_exempted`
                // below) and its `match(true)` handling.
                if default.is_none() && !self.match_unhandled_error_is_exempted() {
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
                        let last_is_exhaustive = arms.last().is_some_and(|(conds, _)| {
                            matches!(conds.as_slice(), [only_cond]
                                if self.condition_is_provably_true(only_cond, &accumulated_before_last_arm))
                        });
                        if !last_is_exhaustive {
                            // `$matchCondition` is the literal `true` -
                            // real PHPStan's own `$remainingType` for
                            // this shape is always just that same
                            // constant (narrowing doesn't apply to a
                            // literal), so its message is this same
                            // degenerate-but-accurate "true" regardless
                            // of which arm(s) are actually unhandled.
                            self.report(
                                label.to_string(),
                                "Match expression does not handle remaining value: true"
                                    .to_string(),
                                e.span,
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
                        // plain enum-typed subject (`Type::Object(EnumClass,
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
                        for (conds, _) in arms.iter() {
                            for c in conds {
                                let cond_ty = self.infer_expr(c, locals);
                                if is_known_single_value(&cond_ty) {
                                    remaining = narrow_type_excluding(&remaining, &cond_ty);
                                }
                            }
                        }
                        if remaining != Type::Never {
                            self.report(
                                label.to_string(),
                                format!(
                                    "Match expression does not handle remaining value: {remaining}"
                                ),
                                e.span,
                                4,
                                "match.unhandled",
                            );
                        }
                    }
                }
                if let Some(d) = default {
                    let mut default_locals = accumulated.clone();
                    result_types.push(self.check_expr(d, &mut default_locals, label));
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&m, &default_locals),
                        None => default_locals,
                    });
                }
                if let Some(m) = merged {
                    *locals = m;
                }
                // Same "don't let an unknown arm compose into a partial
                // Union that looks more precise than it is" rule Ternary
                // applies (`union_types` absorbs `Mixed`), and the same
                // flattening `union_types` gives every other multi-branch
                // construct (`Ternary`, `??`, ...). A raw `Type::Union`
                // built by hand here would leave a nested `Union` sitting
                // as one opaque, unflattened member whenever one arm's
                // result was already a Union - a nested Union member
                // breaks any consumer that walks a Union's members
                // expecting each to be a flat leaf type
                // (`return_type_too_wide`'s `flatten_union`, among
                // others).
                result_types
                    .into_iter()
                    .reduce(union_types)
                    .unwrap_or(Type::Never)
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                attrs,
                ..
            } => {
                // Mirrors real PHPStan's `ClosureAttributesRule` -
                // existence plus target-validity for a closure
                // expression's own `#[Attr]` group.
                self.check_attributes(attrs, label);
                self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_FUNCTION, "function", label);
                // Unlike an arrow function, a plain closure does NOT
                // automatically see the enclosing scope - only variables
                // explicitly named in `use (...)` are captured (by value
                // or by reference; either way, the type it starts with is
                // whatever the outer scope currently has it as). `$this`
                // is the one implicit exception: a non-static closure
                // declared inside a method still auto-binds it, the same
                // as real PHP.
                let mut closure_locals: Locals = locals.fresh_scope();
                for u in uses {
                    // Mirrors real PHPStan's
                    // `InvalidLexicalVariablesInClosureUseRule` - `$this`,
                    // a superglobal, or a name already used as a
                    // parameter can never be captured via `use (...)` at
                    // all (a real PHP compile error), distinct from - and
                    // checked before - the plain "undefined variable"
                    // case below.
                    if u.name == "this".into() {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                "Cannot use $this as lexical variable.".to_string(),
                                u.span,
                                0,
                                "closure.useThis",
                            )
                            .non_ignorable(),
                        );
                    } else if is_superglobal(&u.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                format!(
                                    "Cannot use superglobal variable ${} as lexical variable.",
                                    u.name
                                ),
                                u.span,
                                0,
                                "closure.useSuperGlobal",
                            )
                            .non_ignorable(),
                        );
                    } else if params.iter().any(|p| p.name == u.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                format!(
                                    "Cannot use lexical variable ${} since a parameter with the same name already exists.",
                                    u.name
                                ),
                                u.span,
                                0,
                                "closure.useDuplicate",
                            )
                            .non_ignorable(),
                        );
                    } else if !u.by_ref && !locals.contains_key(&u.name) {
                        self.report(
                            label.to_string(),
                            format!("undefined variable ${}", u.name),
                            u.span,
                            0,
                            "variable.undefined",
                        );
                    }
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed(false));
                    // A captured variable's real declaration is wherever
                    // the outer scope already says it is - copied across,
                    // not fabricated fresh at this `use (...)` clause.
                    let provenance = locals.get_provenance(&u.name).cloned().unwrap_or_default();
                    // The `use (...)` clause's capture token is itself a
                    // real reference to the outer `$x` - same probe hook
                    // `ExprKind::Variable` has.
                    if self.probe_span.get() == Some(u.span) {
                        *self.probe_var_provenance.borrow_mut() =
                            Some(provenance.iter().copied().collect());
                    }
                    // Hovering the capture token itself must show its
                    // type too, not just resolve go-to-definition - the
                    // two probes (`probe_hit`/`probe_var_provenance`) are
                    // independent.
                    self.probe_hit_at(u.span, &ty);
                    // A by-reference capture can still gain keys from
                    // anything else holding the reference before the
                    // closure runs.
                    let ty = if u.by_ref {
                        unseal_array_shapes(&ty)
                    } else {
                        ty
                    };
                    closure_locals.insert_with_provenance(u.name.to_string(), ty, provenance);
                }
                if !is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
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
                let captured_names: std::collections::HashSet<&str> =
                    uses.iter().map(|u| u.name.as_ref()).collect();
                for (key, implied) in locals.branch_implied_facts.iter() {
                    let filtered: Vec<(String, Type)> = implied
                        .iter()
                        .filter(|(name, _)| captured_names.contains(name.as_str()))
                        .cloned()
                        .collect();
                    if !filtered.is_empty() {
                        closure_locals
                            .branch_implied_facts
                            .insert(key.clone(), std::rc::Rc::new(filtered));
                    }
                }
                let closure_label = format!("{label} (closure)");
                for p in params {
                    let (ty, case_refs) =
                        self.resolve_type_hint_with_case_refs(p.type_hint.as_deref());
                    // Mirrors real PHPStan's
                    // `ExistingClassesInClosureTypehintsRule` - checked on
                    // the un-array-wrapped element type below (a variadic
                    // parameter's declared type, not the `array` it
                    // collects into) so a missing class inside e.g.
                    // `...Foo $items` is still reported by its real name,
                    // not `array`.
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        &closure_label,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        &format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        &closure_label,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, &closure_label, p.span);
                    let ty = if p.variadic {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
                    } else {
                        ty
                    };
                    closure_locals.declare(p.name.to_string(), ty, p.name_span);
                    closure_locals.set_native(&p.name, self.closure_param_native(p));
                }
                let (declared_return_ty, declared_return_case_refs) =
                    self.resolve_type_hint_with_case_refs(return_type.as_deref());
                let declared_return = Some(declared_return_ty);
                self.check_type_class_names_exist(
                    declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
                    "return type of anonymous function",
                    e.span,
                    &closure_label,
                );
                self.check_type_trait_refs(
                    declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
                    "return type of anonymous function",
                    e.span,
                    &closure_label,
                    "return.trait",
                );
                self.check_type_class_name_case(&declared_return_case_refs, &closure_label, e.span);
                // `return.noParent` - `declared_return` above never went
                // through `resolve_pseudo` (unlike `current_declared_
                // return_type` further down), so a fresh, one-off resolve
                // against the closure's enclosing class
                // (`self.current_class`, not a `class_name` parameter - a
                // closure has no signature of its own) is needed here
                // too.
                if let Some(c) = self.current_class.clone() {
                    let resolved_return = self.resolve_pseudo(
                        declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
                        &c,
                    );
                    self.check_no_parent_ref(
                        &resolved_return,
                        "return type of anonymous function",
                        e.span,
                        &closure_label,
                        "return.noParent",
                    );
                }
                // The identical `@internal` type-hint check a named
                // function/method's param/return list already gets.
                let current_class_for_internal_check = self.current_class.clone();
                self.check_internal_param_and_return_types_raw(
                    params,
                    return_type.as_deref(),
                    e.span,
                    false,
                    current_class_for_internal_check.as_deref(),
                    &closure_label,
                );
                self.check_restricted_param_and_return_types_raw(
                    params,
                    return_type.as_deref(),
                    e.span,
                    current_class_for_internal_check.as_deref(),
                    &closure_label,
                );
                // A generator-returning closure needs the same
                // current_function_is_generator save/set/restore
                // check_function does for a named function/method -
                // check_block never routes through check_function at
                // all.
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = body_contains_yield(body);
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
                    collect_goto_targets(body),
                );
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
                    .map(|u| u.name.clone())
                    .collect();
                // A closure's own throw points never belong to the
                // enclosing function's count (calling the closure, not
                // merely declaring it, is what would matter, and this
                // project doesn't model that) - starts fresh, discarded
                // on restore, same shape as
                // `current_function_is_generator` above.
                let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
                let previous_unannotated_call =
                    std::mem::take(&mut self.current_function_has_unannotated_call);
                let previous_explicit_throw_points =
                    std::mem::take(&mut self.current_function_explicit_throw_points);
                // The closure's own `return` statements belong to the
                // closure, not the enclosing function's too-wide-return
                // accumulator - saved/restored the same way, and read back
                // below as the closure's own inferred return type.
                let previous_return_points =
                    std::mem::take(&mut self.current_function_return_points);
                let previous_saw_explicit_return =
                    std::mem::take(&mut self.current_function_saw_explicit_return);
                self.check_generator_return_type(declared_return.as_ref(), e.span, &closure_label);
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
                let (converged, _) = self.converge_loop_body(
                    &closure_locals,
                    declared_return.as_ref(),
                    &closure_label,
                    LoopZeroIterations::Possible {
                        subject_exempt: None,
                    },
                    |this, pass_locals, declared_return, label, _pass| {
                        // Only the kept (last) pass's return points count,
                        // the same way only its diagnostics survive.
                        this.current_function_return_points.clear();
                        this.current_function_saw_explicit_return = false;
                        this.check_block(body, pass_locals, declared_return, label);
                    },
                );
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
                let closure_falls_off_end = !body
                    .iter()
                    .any(|s| self.always_terminates(s, &closure_locals));
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
                    body,
                    return_type.is_some(),
                    declared_return.as_ref(),
                    &closure_locals,
                    e.span,
                    &closure_label,
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
                self.current_function_has_unannotated_call = previous_unannotated_call;
                self.current_function_explicit_throw_points = previous_explicit_throw_points;
                self.current_function_goto_targets = previous_goto_targets;
                self.current_function_by_ref_params = previous_by_ref_params;
                self.current_function_by_ref_params_with_tag = previous_by_ref_params_with_tag;
                self.current_function_byref_snapshot_names = previous_byref_snapshot_names;
                // A `use(...)` capture never referenced anywhere in the
                // closure's own body. `ClosureUse` carries no span of its
                // own (see its doc comment), so every finding here
                // anchors to the whole closure expression's span instead
                // of the specific `$x` token.
                self.check_unused_closure_uses(uses, body, &closure_label, e.span);
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
                for u in uses {
                    if !u.by_ref {
                        continue;
                    }
                    let mutated = closure_byref_exit_snapshots
                        .get(&u.name)
                        .and_then(|snapshots| snapshots.iter().cloned().reduce(union_types))
                        .unwrap_or_else(|| {
                            closure_locals
                                .get(&u.name)
                                .cloned()
                                .unwrap_or(Type::Mixed(false))
                        });
                    if !locals.contains_key(&u.name) {
                        // Real PHP auto-vivifies `$x` in the outer/
                        // declaring scope the moment `use (&$x)` captures
                        // an as-yet-undefined variable - the same by-ref-
                        // argument auto-vivification this project models
                        // in `predefine_byref_or_unknown_args`, now given
                        // a real, navigable declaration site.
                        locals.declare(u.name.to_string(), mutated, u.span);
                        // `locals.declare` above is this variable's only
                        // provenance site in this case (it didn't exist
                        // in the outer scope before), so the probe hook
                        // above (fired before this variable existed in
                        // `locals` at all) would have stashed an empty
                        // set - correct it now that the real site is
                        // known.
                        if self.probe_span.get() == Some(u.span) {
                            *self.probe_var_provenance.borrow_mut() =
                                Some([u.span].into_iter().collect());
                        }
                    } else {
                        locals.insert(u.name.to_string(), mutated);
                        // The closure may write anything to it.
                        locals.clear_native(&u.name);
                    }
                }
                let inferred_return = (!closure_is_generator).then(|| {
                    closure_inferred_return_type(
                        closure_return_points,
                        closure_saw_explicit_return,
                        closure_falls_off_end,
                    )
                });
                self.closure_literal_type(params, return_type.as_deref(), inferred_return, None)
            }
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                attrs,
                ..
            } => {
                // Mirrors real PHPStan's `ArrowFunctionAttributesRule` -
                // see the `Closure` arm's identical comment above.
                self.check_attributes(attrs, label);
                self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_FUNCTION, "function", label);
                // An arrow function DOES automatically capture the whole
                // enclosing scope by value (implicit, no `use (...)`
                // needed) - a plain clone of `locals` is exactly that.
                let mut arrow_locals = locals.clone();
                for p in params {
                    let (ty, case_refs) =
                        self.resolve_type_hint_with_case_refs(p.type_hint.as_deref());
                    // Mirrors real PHPStan's
                    // `ExistingClassesInArrowFunctionTypehintsRule` - same
                    // "check the element type, not the variadic-collected
                    // `array`" reasoning as the Closure arm above.
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of arrow function", p.name),
                        p.span,
                        label,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        &format!("parameter ${} of arrow function", p.name),
                        p.span,
                        label,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, label, p.span);
                    let ty = if p.variadic {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
                    } else {
                        ty
                    };
                    arrow_locals.declare(p.name.to_string(), ty, p.name_span);
                    arrow_locals.set_native(&p.name, self.closure_param_native(p));
                }
                // An arrow function's body is a single expression, not
                // a `Vec<Stmt>` `body_contains_yield` can walk -
                // `expr_contains_yield` is that same walk's expression-
                // only counterpart. Same save/set/restore
                // `check_function`/the Closure literal arms do: without
                // this, checking a generator arrow function here would
                // leak the flag into whatever comes after it in the
                // enclosing scope's checking pass.
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = expr_contains_yield(body);
                let declared_return = return_type
                    .as_deref()
                    .map(|_| self.resolve_type_hint(return_type.as_deref()));
                let previous_declared_return_type = self.current_declared_return_type.take();
                self.current_declared_return_type =
                    declared_return.as_ref().map(|t| match &self.current_class {
                        Some(c) => self.resolve_pseudo(t, c),
                        None => t.clone(),
                    });
                // Same reasoning as the `Closure` arm above.
                let previous_declared_throws_type = self.current_declared_throws_type.take();
                let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
                let previous_unannotated_call =
                    std::mem::take(&mut self.current_function_has_unannotated_call);
                let previous_explicit_throw_points =
                    std::mem::take(&mut self.current_function_explicit_throw_points);
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
                self.check_generator_return_type(declared_return.as_ref(), e.span, label);
                let body_ty = self.check_expr(body, &mut arrow_locals, label);
                self.current_function_is_generator = previous_generator_flag;
                self.current_declared_return_type = previous_declared_return_type;
                self.current_declared_throws_type = previous_declared_throws_type;
                self.current_function_throw_points = previous_throw_points;
                self.current_function_has_unannotated_call = previous_unannotated_call;
                self.current_function_explicit_throw_points = previous_explicit_throw_points;
                self.current_function_by_ref_params = previous_by_ref_params;
                self.current_function_by_ref_exit_snapshots = previous_by_ref_exit_snapshots;
                self.current_function_by_ref_params_with_tag = previous_by_ref_params_with_tag;
                // Its body is a single implicit-return expression, not a
                // StmtKind::Return - the equivalent of check_stmt's own
                // Return-vs-declared_return check, kept here since
                // there's no StmtKind::Return for it to share.
                if return_type.is_some() {
                    let (expected, expected_case_refs) =
                        self.resolve_type_hint_with_case_refs(return_type.as_deref());
                    self.check_type_class_names_exist(
                        &expected,
                        "return type of arrow function",
                        e.span,
                        label,
                    );
                    self.check_type_class_name_case(&expected_case_refs, label, e.span);
                    self.check_type_trait_refs(
                        &expected,
                        "return type of arrow function",
                        e.span,
                        label,
                        "return.trait",
                    );
                    if let Some(c) = self.current_class.clone() {
                        let resolved_return = self.resolve_pseudo(&expected, &c);
                        self.check_no_parent_ref(
                            &resolved_return,
                            "return type of arrow function",
                            e.span,
                            label,
                            "return.noParent",
                        );
                    }
                    // See `checker::calls`' own identical comment:
                    // `accepts_at_configured_level` already accepts a
                    // `Mixed` body type unconditionally below real
                    // PHPStan's own level 9/10.
                    if !self
                        .registry
                        .accepts_at_configured_level(&body_ty, &expected)
                    {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got {body_ty}"),
                            body.span,
                            3,
                            "return.type",
                        );
                    }
                }
                // See the `Closure` arm's identical comment above.
                let current_class_for_internal_check = self.current_class.clone();
                self.check_internal_param_and_return_types_raw(
                    params,
                    return_type.as_deref(),
                    e.span,
                    false,
                    current_class_for_internal_check.as_deref(),
                    label,
                );
                self.check_restricted_param_and_return_types_raw(
                    params,
                    return_type.as_deref(),
                    e.span,
                    current_class_for_internal_check.as_deref(),
                    label,
                );
                self.closure_literal_type(
                    params,
                    return_type.as_deref(),
                    (!expr_contains_yield(body)).then(|| body_ty.clone()),
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
                if !matches!(inner.kind, ExprKind::New(..))
                    && self.registry.php_version.is_some_and(|v| {
                        !crate::php_version::PhpFeature::FirstClassCallables.is_available(v)
                    })
                {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            label.to_string(),
                            "First-class callables are supported only on PHP 8.1 and later."
                                .to_string(),
                            e.span,
                            0,
                            "callable.notSupported",
                        )
                        .non_ignorable(),
                    );
                }
                match &inner.kind {
                    ExprKind::Call(callee, _) => {
                        if let ExprKind::Name(name) = &callee.kind {
                            if !is_language_construct(name) {
                                let (primary, fallback) =
                                    self.name_ctx.resolve_function_candidates(name);
                                let resolved_sig = self
                                    .registry
                                    .lookup_function(&primary)
                                    .map(|s| (primary.clone(), s))
                                    .or_else(|| {
                                        fallback.as_deref().and_then(|g| {
                                            self.registry
                                                .lookup_function(g)
                                                .map(|s| (g.to_string(), s))
                                        })
                                    });
                                match resolved_sig {
                                    Some((resolved_name, sig)) => {
                                        self.check_internal_function_call(
                                            &resolved_name,
                                            sig,
                                            e.span,
                                            label,
                                        );
                                        // Closes a real, live bypass: a
                                        // banned function's own
                                        // first-class-callable syntax
                                        // (`banned_func(...)`) previously
                                        // went unchecked even though the
                                        // ordinary call site
                                        // (`banned_func()`) already was.
                                        self.check_restricted_function_call(
                                            &resolved_name,
                                            &[],
                                            locals,
                                            e.span,
                                            label,
                                        );
                                        self.check_deprecated_function_call(
                                            &resolved_name,
                                            sig,
                                            e.span,
                                            label,
                                        );
                                        self.record_function_usage_token(
                                            &resolved_name,
                                            sig,
                                            callee.span,
                                        );
                                    }
                                    None => {
                                        // Same `function_exists()`-guard
                                        // suppression as the ordinary-call
                                        // site above.
                                        let guarded = !locals.guarded_functions.is_empty()
                                            && locals.guarded_functions.contains(
                                                primary
                                                    .rsplit('\\')
                                                    .next()
                                                    .unwrap_or(&primary)
                                                    .to_ascii_lowercase()
                                                    .as_str(),
                                            );
                                        if !guarded {
                                            self.report(
                                                label.to_string(),
                                                format!("call to undefined function {primary}()"),
                                                e.span,
                                                0,
                                                "function.notFound",
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            self.check_expr(callee, locals, label);
                        }
                    }
                    ExprKind::MethodCall(recv, method, _, _) => {
                        let recv_ty = self.check_expr(recv, locals, label);
                        let recv_class = match &recv_ty {
                            Type::Object(class, _) => Some(class.clone()),
                            Type::Union(members) | Type::Intersection(members) => {
                                members.iter().find_map(|m| match m {
                                    Type::Object(class, _) => Some(class.clone()),
                                    _ => None,
                                })
                            }
                            _ => None,
                        };
                        if let Some(class) = &recv_class
                            && self.registry.classes.contains_key(class.as_str())
                        {
                            if self.member_confirmed_missing(&recv_ty, |c| {
                                self.registry.lookup_method(c, method).is_some()
                                    || self.registry.lookup_method(c, "__call").is_some()
                            }) {
                                let mut builder = DiagnosticBuilder::new(
                                    label.to_string(),
                                    format!("call to undefined method {recv_ty}::{method}()"),
                                    e.span,
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
                                self.registry.lookup_method_with_owner(class, method)
                            {
                                self.check_member_visibility(MemberVisibilityCheck {
                                    kind: "method",
                                    owner: &owner,
                                    member_name: method,
                                    visibility: &sig.visibility,
                                    describe: &format!("method {owner}::{method}()"),
                                    span: e.span,
                                    label,
                                });
                                self.check_internal_method_call(
                                    &owner, method, sig, false, e.span, label,
                                );
                                self.check_restricted_method_call(
                                    &owner,
                                    method,
                                    false,
                                    &[],
                                    locals,
                                    e.span,
                                    label,
                                );
                                self.check_deprecated_method_call(
                                    &owner, method, sig, false, e.span, label,
                                );
                                self.record_member_usage_token(
                                    UsageTokenKind::Method,
                                    &owner,
                                    sig.deprecated.is_some(),
                                    sig.is_internal,
                                    e.span,
                                );
                                self.check_call_site_method_name_case(
                                    class,
                                    method,
                                    e.span,
                                    Span {
                                        start: recv.span.end,
                                        end: e.span.end,
                                    },
                                    label,
                                );
                            }
                        }
                    }
                    ExprKind::StaticCall(target, method, _) => {
                        if let Some(class) = self.resolve_static_target_class(target, locals, label)
                            && self.registry.classes.contains_key(class.as_str())
                        {
                            self.check_call_site_method_name_case(
                                &class,
                                method,
                                e.span,
                                Span {
                                    start: static_target_end(target),
                                    end: e.span.end,
                                },
                                label,
                            );
                            if self.registry.lookup_method(&class, method).is_none()
                                && self
                                    .registry
                                    .lookup_method(&class, "__callStatic")
                                    .is_none()
                            {
                                self.report(
                                    label.to_string(),
                                    format!("call to undefined static method {class}::{method}()"),
                                    e.span,
                                    0,
                                    "staticMethod.notFound",
                                );
                            } else if let Some((owner, sig)) =
                                self.registry.lookup_method_with_owner(&class, method)
                            {
                                self.check_member_visibility(MemberVisibilityCheck {
                                    kind: "staticMethod",
                                    owner: &owner,
                                    member_name: method,
                                    visibility: &sig.visibility,
                                    describe: &format!("static method {owner}::{method}()"),
                                    span: e.span,
                                    label,
                                });
                                self.check_internal_method_call(
                                    &owner, method, sig, true, e.span, label,
                                );
                                self.check_restricted_method_call(
                                    &owner,
                                    method,
                                    true,
                                    &[],
                                    locals,
                                    e.span,
                                    label,
                                );
                                self.check_deprecated_method_call(
                                    &owner, method, sig, true, e.span, label,
                                );
                                self.record_member_usage_token(
                                    UsageTokenKind::Method,
                                    &owner,
                                    sig.deprecated.is_some(),
                                    sig.is_internal,
                                    e.span,
                                );
                                if target.as_name() == Some("static")
                                    && sig.visibility == Visibility::Private
                                {
                                    self.check_static_private_method_access(
                                        &owner, method, e.span, label,
                                    );
                                }
                            }
                        }
                    }
                    ExprKind::DynamicMethodCall(recv, name_expr, _) => {
                        self.check_expr(recv, locals, label);
                        self.check_expr(name_expr, locals, label);
                    }
                    ExprKind::New(..) => {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                "Cannot create callable from the new operator.".to_string(),
                                e.span,
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
                Type::Object("Closure".to_string(), Vec::new())
            }
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.to_string()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            ExprKind::Name(name) => {
                self.check_magic_constant_context(name, e.span, label);
                match self.global_constant_type(name) {
                    Some(ty) => ty,
                    None => {
                        self.check_unknown_global_constant(name, locals, e.span, label);
                        bare_name_literal_type(name)
                    }
                }
            }
        }
    }

    pub(crate) fn infer_expr(&self, e: &Expr, locals: &Locals) -> Type {
        match &e.kind {
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.to_string()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            ExprKind::Variable(name) => locals
                .get(name)
                .cloned()
                .unwrap_or_else(|| superglobal_default_type(name)),
            ExprKind::Grouping(inner) | ExprKind::Clone(inner, _) => self.infer_expr(inner, locals),
            // First-class-callable syntax (`strlen(...)`, `$obj->
            // method(...)`) produces a real `Closure` instance at runtime
            // - infer the same `Object("Closure", ...)` the native
            // `Closure` type hint resolves to (rather than the broader
            // `Type::Callable`, which `callable`-hinted parameters accept
            // via `is_assignable`'s separate leniency), so a strictly
            // `Closure`-typed parameter correctly accepts it. A closure/
            // arrow-fn literal additionally carries its own signature
            // (`closure_literal_type`).
            ExprKind::FirstClassCallable(_) => Type::Object("Closure".to_string(), Vec::new()),
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                let inferred = self
                    .closure_syntactic_return_type(params, uses, body, *is_static, locals, None);
                self.closure_literal_type(params, return_type.as_deref(), inferred, None)
            }
            ExprKind::ArrowFn {
                params,
                return_type,
                body,
                ..
            } => {
                let inferred = self.arrow_fn_inferred_return(params, body, locals, None);
                self.closure_literal_type(params, return_type.as_deref(), inferred, None)
            }
            ExprKind::Match {
                subject,
                arms,
                default,
            } => {
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
                for (conds, result) in arms {
                    let arm_locals =
                        self.narrow_match_arm_conditions(subject, conds, &accumulated, match_true);
                    result_types.push(self.infer_expr(result, &arm_locals));
                    if let [only_cond] = conds.as_slice() {
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
                // why this must be a `union_types` reduction, not a raw
                // dedup-then-`Type::Union(deduped)` construction.
                result_types
                    .into_iter()
                    .reduce(union_types)
                    .unwrap_or(Type::Never)
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used for
            // a function that always throws or exits.
            ExprKind::Throw(_) => Type::Never,
            ExprKind::NewAnonymousClass(..) => self.anonymous_class_type(e.span),
            ExprKind::ShellExec(inner) => self.infer_expr(inner, locals),
            ExprKind::Cast(ty, _, _) => match ty.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "string" => Type::String,
                "bool" => Type::Bool,
                "array" => Type::Array(
                    Box::new(Type::unknown_array_key()),
                    Box::new(Type::Mixed(false)),
                ),
                "object" => Type::Object("object".to_string(), Vec::new()),
                "unset" => Type::Null,
                _ => Type::Mixed(false),
            },
            ExprKind::Unary(UnaryOp::Not, _) => Type::Bool,
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) -
            // the same implicit-numeric-coercion rule arithmetic
            // operators apply generally.
            ExprKind::Unary(op @ (UnaryOp::Neg | UnaryOp::Plus), inner) => {
                let inner_ty = self.infer_expr(inner, locals);
                if is_string_shaped(&inner_ty) {
                    Type::Int
                } else if *op == UnaryOp::Neg {
                    negate_numeric_literal(inner_ty)
                } else {
                    inner_ty
                }
            }
            ExprKind::Unary(UnaryOp::BitNot, inner) => {
                let inner_ty = self.infer_expr(inner, locals);
                to_bitwise_not(&inner_ty).unwrap_or(inner_ty)
            }
            ExprKind::Unary(_, inner) => self.infer_expr(inner, locals),
            ExprKind::Binary(op, l, r) => self.infer_binary(op, l, r, locals),
            ExprKind::Assign(_, rhs) | ExprKind::AssignRef(_, rhs) => self.infer_expr(rhs, locals),
            // `self`/`parent`/`static` aren't lexed as keywords (see the
            // parser: they're indistinguishable from any other identifier),
            // so a literal `ExprKind::Name("self")` has to be special-cased
            // here rather than showing up as an Object of that name.
            ExprKind::New(class_expr, args) => match &class_expr.kind {
                ExprKind::Name(name) => match name.as_str() {
                    "self" | "parent" | "static" => {
                        let raw = match name.as_str() {
                            "self" => Type::SelfRef,
                            "parent" => Type::ParentRef,
                            _ => Type::StaticRef,
                        };
                        match &self.current_class {
                            Some(c) => self.resolve_pseudo(&raw, c),
                            None => raw,
                        }
                    }
                    // `new ReflectionClass($view)`-shaped constructor-
                    // argument-driven class-level generic inference - the
                    // same `bind_template_param`/`TemplateBindings`
                    // primitive `infer_call_return_type` uses for
                    // ordinary call-site inference, just binding against
                    // a constructor's params instead of a function's.
                    // Rebind to the registry's canonical-case spelling
                    // first (mirroring `check_expr`'s identical fix)
                    // since `lookup_method`'s ancestor walk needs exact
                    // case.
                    _ => {
                        let resolved_name = self.name_ctx.resolve(name);
                        let canonical = self
                            .registry
                            .lookup_class(&resolved_name)
                            .map(|(canonical, _)| canonical.to_string())
                            .unwrap_or(resolved_name);
                        let templates = self
                            .registry
                            .classes
                            .get(canonical.as_str())
                            .map(|info| info.templates.as_slice())
                            .unwrap_or(&[]);
                        if templates.is_empty() {
                            Type::Object(canonical, Vec::new())
                        } else {
                            let mut bindings: TemplateBindings = TemplateBindings::default();
                            if let Some(sig) =
                                self.registry.lookup_method(&canonical, "__construct")
                            {
                                let resolved_args = resolve_call_args(args, &sig.params);
                                for (param, slot) in sig.params.iter().zip(resolved_args.iter()) {
                                    let Some(arg) = slot else { continue };
                                    let arg_ty = self.infer_expr(arg, locals);
                                    // A bare string literal naming a
                                    // real, registered class binds a
                                    // `class-string<T>` slot exactly like
                                    // an explicit `Foo::class` would,
                                    // mirroring real PHPStan's
                                    // `ConstantStringType::isClassString()`.
                                    let promoted = types::promote_class_string_literals(
                                        &param.ty,
                                        &arg_ty,
                                        &|s| {
                                            self.registry
                                                .lookup_class(s)
                                                .map(|(c, _)| c.to_string())
                                        },
                                    );
                                    types::bind_template_param(&param.ty, &promoted, &mut bindings);
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
                            let bound_args = templates[..last_bound]
                                .iter()
                                .map(|t| {
                                    bindings
                                        .get(t.as_ref())
                                        .cloned()
                                        .unwrap_or(Type::Mixed(false))
                                })
                                .collect();
                            Type::Object(canonical, bound_args)
                        }
                    }
                },
                _ => Type::Mixed(false),
            },
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if !is_language_construct(name) => {
                    let (primary, fallback) =
                        self.resolve_function_candidates_cached(callee.span, name);
                    self.registry
                        .lookup_function(&primary)
                        .map(|sig| (primary.as_ref(), sig))
                        .or_else(|| {
                            fallback
                                .as_deref()
                                .and_then(|g| self.registry.lookup_function(g).map(|sig| (g, sig)))
                        })
                        .map(|(resolved_name, sig)| {
                            self.infer_builtin_call_return_type(resolved_name, sig, args, locals)
                        })
                        .unwrap_or_else(|| self.call_fallback_return_type(callee, args, locals))
                }
                _ => self.call_fallback_return_type(callee, args, locals),
            },
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
                // A nullsafe receiver is virtually always nullable
                // (that's the entire reason to write `?->`), so its own
                // inferred type is virtually always `Union([Object(...),
                // Null])`, not a bare `Object` - strip `Null` out before
                // matching, the same real class the method is actually
                // being looked up on either way.
                let recv_ty = narrow_type_excluding(&self.infer_expr(recv, locals), &Type::Null);
                let result = self.method_call_return_type(recv, &recv_ty, method, args, locals);
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if (*nullsafe || postfix_chain_has_nullsafe(recv)) && result != Type::Mixed(false) {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            ExprKind::StaticCall(target, method, args) => {
                if self.collect_member_refs
                    && let Some(class) = self.infer_static_target_class(target, locals)
                    && let Some((owner, _)) = self.registry.lookup_method_with_owner(&class, method)
                {
                    self.record_member_ref(e.span, &owner, method);
                    // Phase 92's own `::cases()` escape hatch - see
                    // `Checker::escaped_enums`'s own doc comment. `owner`
                    // is already the registry's own canonically-declared
                    // name (same reasoning as `check_expr`'s identical
                    // hook).
                    if method.eq_ignore_ascii_case("cases")
                        && self
                            .registry
                            .classes
                            .get(owner.as_ref())
                            .is_some_and(|c| c.kind == ClassKind::Enum)
                    {
                        self.record_enum_escape(&owner);
                    }
                }
                self.infer_static_target_class(target, locals)
                    .and_then(|class| {
                        self.registry.lookup_method(&class, method).map(|sig| {
                            let resolved_args = resolve_call_args(args, &sig.params);
                            let resolved = self.infer_call_return_type(sig, &resolved_args, locals);
                            let resolved =
                                self.narrow_enum_from_call(&class, method, args, resolved, locals);
                            // Phase 92's own `::from()`/`::tryFrom()`
                            // literal-argument signal - see `check_expr`'s
                            // identical fix just above `ExprKind::
                            // StaticAccess` for why this reads the
                            // already-computed type rather than
                            // threading a new span parameter through
                            // `narrow_enum_from_call`.
                            if let Type::EnumCase(ref case_class, ref case) = resolved {
                                self.record_member_ref(e.span, case_class, case);
                            }
                            if let Some(ty) =
                                self.class_argument_return(&class, method, args, locals)
                            {
                                return ty;
                            }
                            self.resolve_static_call_return(&resolved, target, &class)
                        })
                    })
                    .unwrap_or(Type::Mixed(false))
            }
            ExprKind::DynamicClassConstFetch(class_name, name) => {
                let names: Option<Vec<String>> = match self.infer_expr(name, locals) {
                    Type::StringLiteral(s) => Some(vec![s]),
                    Type::Union(members) => members
                        .into_iter()
                        .map(|m| match m {
                            Type::StringLiteral(s) => Some(s),
                            _ => None,
                        })
                        .collect(),
                    _ => None,
                };
                names
                    .into_iter()
                    .flatten()
                    .map(|member| {
                        self.infer_expr(
                            &Expr {
                                kind: ExprKind::StaticAccess(class_name.clone(), member),
                                span: e.span,
                            },
                            locals,
                        )
                    })
                    .reduce(crate::checker::type_union::union_types)
                    .unwrap_or(Type::Mixed(false))
            }
            ExprKind::StaticAccess(class_name, member) => {
                if member == "class" {
                    return match self.resolve_static_class_name(class_name) {
                        Some(class) => Type::ClassString(Box::new(Type::Object(class, Vec::new()))),
                        None => Type::String,
                    };
                }
                // A static property (`self::$prop`) can be narrowed the
                // same way an instance property already is above - check
                // for a locally-narrowed override before falling back to
                // its own declared type.
                if member.starts_with('$')
                    && locals.may_have_member_narrowing()
                    && let Some(narrowed) = locals.get(&format!("{class_name}::{member}"))
                {
                    return narrowed.clone();
                }
                if self.collect_member_refs
                    && let Some(class) = self.resolve_static_class_name(class_name)
                {
                    if member == "{dynamic}" {
                        // Phase 92's own dynamic-member escape hatch -
                        // see `check_expr`'s identical fix on this same
                        // `"{dynamic}"` sentinel for why this needs
                        // `lookup_class` for the canonical name rather
                        // than trusting `class` as-is.
                        if let Some((canonical, info)) = self.registry.lookup_class(&class)
                            && info.kind == ClassKind::Enum
                        {
                            self.record_enum_escape(canonical);
                        }
                    } else {
                        match member.strip_prefix('$') {
                            Some(prop_name) => {
                                if let Some((owner, _)) =
                                    self.registry.lookup_property_with_owner(&class, prop_name)
                                {
                                    self.record_member_ref(e.span, &owner, prop_name);
                                }
                            }
                            None => {
                                if let Some((owner, _)) =
                                    self.registry.lookup_const_with_owner(&class, member)
                                {
                                    self.record_member_ref(e.span, &owner, member);
                                }
                            }
                        }
                    }
                }
                self.resolve_static_class_name(class_name)
                    .and_then(|class| match member.strip_prefix('$') {
                        Some(prop_name) => self
                            .registry
                            .lookup_property(&class, prop_name)
                            .map(|p| self.resolve_pseudo(&p.ty, &class)),
                        None => self.registry.lookup_const_with_owner(&class, member).map(
                            |(owner, cs)| {
                                let ty = self
                                    .registry
                                    .dynamic_class_constant_type(&owner, member, &cs.ty)
                                    .unwrap_or_else(|| cs.ty.clone());
                                self.resolve_pseudo(&ty, &class)
                            },
                        ),
                    })
                    .unwrap_or(Type::Mixed(false))
            }
            ExprKind::Ternary(cond, then_, else_) => {
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.infer_expr(else_, &else_locals);
                match then_ {
                    Some(t) => {
                        let mut then_locals = locals.clone();
                        self.apply_narrowing(cond, &mut then_locals, true);
                        let then_ty = self.infer_expr(t, &then_locals);
                        // See the parallel arm in `check_expr` for why
                        // this must be `union_types`, not a raw
                        // `Type::Union(vec![...])`.
                        union_types(then_ty, else_ty)
                    }
                    None => Type::Mixed(false),
                }
            }
            ExprKind::ArrayLiteral(items) => self.infer_array_literal_type(items, locals),
            // Routed through the identical `resolve_concat_type` engine
            // as `BinOp::Concat`, as a left-fold over each part (an
            // interpolated string is exactly a chain of concatenations) -
            // `check_expr`'s separate `Interpolated` arm walks each part
            // for a different purpose (implicit `__toString()`/array-to-
            // string checks), not reused here since it doesn't compute or
            // return a refined type. An empty `parts` list can't occur in
            // practice, but folds to plain `Type::String` rather than
            // panicking if it somehow did.
            ExprKind::Interpolated(parts) => parts
                .iter()
                .map(|p| self.infer_expr(p, locals))
                .reduce(|acc, part_ty| resolve_concat_type(&acc, &part_ty))
                .unwrap_or(Type::String),
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                if locals.may_have_member_narrowing()
                    && let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                let base_ty = narrow_type_excluding(&self.infer_expr(base, locals), &Type::Null);
                // See `property_access_type`'s doc comment - shared
                // with `check_expr`'s identical arm.
                let result = self.property_access_type(&base_ty, prop);
                // Same implicit nullability as `?->method()` just above,
                // for `$x?->prop`.
                if (*nullsafe || postfix_chain_has_nullsafe(base)) && result != Type::Mixed(false) {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            // A homogeneous array's element type applies regardless of
            // which key is accessed; optimistically assumes the access is
            // in bounds/present rather than widening to `V|null` for a
            // possibly-missing element - consistent with this checker's
            // general preference for fewer false positives over catching
            // every possible runtime-undefined access.
            ExprKind::Index(base, Some(idx)) => {
                // The `Index` counterpart of `PropertyAccess`'s "check
                // the narrowed key first" fast path above - without this,
                // `apply_narrowing` writes a narrowed type under
                // `narrowing_key`'s `"base[key]"` format (e.g. after
                // `is_string($info['code'])`), but nothing reads it back:
                // this arm would re-derive the type from `$info`'s
                // declared/inferred shape, discarding the narrowing.
                if locals.may_have_member_narrowing()
                    && let Some(key) = narrowing_key(e)
                    && let Some(narrowed) = locals.get(&key)
                {
                    return narrowed.clone();
                }
                let key = literal_string_key(idx);
                index_result_type(self.infer_expr(base, locals), key.as_deref())
            }
            ExprKind::Name(name) => self
                .global_constant_type(name)
                .unwrap_or_else(|| bare_name_literal_type(name)),
            // Mirrors `check_expr`'s `ExprKind::Yield`/`YieldFrom` arms -
            // needed here too, since `assertType()`'s testing-DSL
            // recognition computes its "actual" type via `infer_expr`,
            // not `check_expr`.
            ExprKind::Yield(_, _) => self
                .current_declared_return_type
                .as_ref()
                .map(|t| generator_template_param(t, self.registry, "TSend"))
                .unwrap_or(Type::Mixed(false)),
            ExprKind::YieldFrom(inner) => {
                generator_template_param(&self.infer_expr(inner, locals), self.registry, "TReturn")
            }
            _ => Type::Mixed(false),
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
    fn prefer_inferred_over_declared_return(&self, hint: Option<&str>, inferred: Type) -> Type {
        let Some(hint) = hint else {
            return inferred;
        };
        let declared = self.resolve_type_hint(Some(hint));
        if inferred != Type::Mixed(false) && self.registry.is_assignable(&inferred, &declared) {
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
        callback: &Expr,
        param_overrides: &[Type],
        locals: &Locals,
    ) -> Option<Type> {
        match &callback.kind {
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                ..
            } => {
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut arrow_locals = locals.clone();
                for (p, ty) in params.iter().zip(param_overrides) {
                    arrow_locals.insert(p.name.to_string(), ty.clone());
                }
                let inferred = self.infer_expr(body, &arrow_locals);
                Some(self.prefer_inferred_over_declared_return(return_type.as_deref(), inferred))
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut closure_locals: Locals = Locals::default();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed(false));
                    closure_locals.insert(u.name.to_string(), ty);
                }
                if !*is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                for (p, ty) in params.iter().zip(param_overrides) {
                    closure_locals.insert(p.name.to_string(), ty.clone());
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
                match body.as_slice() {
                    [
                        Stmt {
                            kind: StmtKind::Return(Some(e)),
                            ..
                        },
                    ] => {
                        let inferred = self.infer_expr(e, &closure_locals);
                        Some(
                            self.prefer_inferred_over_declared_return(
                                return_type.as_deref(),
                                inferred,
                            ),
                        )
                    }
                    _ => return_type
                        .as_deref()
                        .map(|hint| self.resolve_type_hint(Some(hint))),
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
                // `class` is the owner to resolve a `self`/`static`
                // pseudo-return through (`TagId::tryFrom(...): ?self`) -
                // absent for a plain function, which can't return either.
                let (sig, class) = match &inner.kind {
                    ExprKind::Call(callee, _) => match &callee.kind {
                        ExprKind::Name(name) => {
                            let (primary, fallback) =
                                self.name_ctx.resolve_function_candidates(name);
                            let sig = self.registry.lookup_function(&primary).or_else(|| {
                                fallback
                                    .as_deref()
                                    .and_then(|g| self.registry.lookup_function(g))
                            });
                            (sig, None)
                        }
                        _ => (None, None),
                    },
                    ExprKind::StaticCall(target, method, _) => {
                        match self.infer_static_target_class(target, locals) {
                            Some(class) => {
                                if self.collect_member_refs
                                    && let Some((owner, _)) =
                                        self.registry.lookup_method_with_owner(&class, method)
                                {
                                    self.record_member_ref(callback.span, &owner, method);
                                }
                                let sig = self.registry.lookup_method(&class, method);
                                (sig, Some(class))
                            }
                            None => (None, None),
                        }
                    }
                    ExprKind::MethodCall(recv, method, _, _) => {
                        let recv_ty = self.infer_expr(recv, locals);
                        let result = self.find_method_receiver_class(&recv_ty, method);
                        if self.collect_member_refs
                            && let Some(class) = &result.1
                            && let Some((owner, _)) =
                                self.registry.lookup_method_with_owner(class, method)
                        {
                            self.record_member_ref(callback.span, &owner, method);
                        }
                        result
                    }
                    _ => (None, None),
                };
                sig.map(|s| {
                    let mut bindings: TemplateBindings = TemplateBindings::default();
                    for (param, ty) in s.params.iter().zip(param_overrides) {
                        let promoted = types::promote_class_string_literals(&param.ty, ty, &|s| {
                            self.registry.lookup_class(s).map(|(c, _)| c.to_string())
                        });
                        types::bind_template_param(&param.ty, &promoted, &mut bindings);
                    }
                    let substituted = types::substitute_template(&s.return_type, &bindings);
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
    /// own now-hooked `StaticAccess`/`StaticCall` arms; the returned
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
    pub(crate) fn collect_default_and_const_value_member_refs(&mut self, program: &[Stmt]) {
        for stmt in program {
            self.collect_member_ref_defaults_stmt(stmt);
        }
    }

    fn collect_member_ref_defaults_params(&mut self, params: &[Param]) {
        for p in params {
            if let Some(d) = &p.default {
                self.infer_expr(d, &Locals::default());
            }
        }
    }

    fn collect_member_ref_defaults_function(&mut self, f: &FunctionDecl) {
        self.collect_member_ref_defaults_params(&f.params);
        for s in f.body.iter() {
            self.collect_member_ref_defaults_stmt(s);
        }
    }

    fn collect_member_ref_defaults_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => self.collect_member_ref_defaults_function(f),
            StmtKind::ClassDecl(c) => {
                for p in &c.properties {
                    if let Some(d) = &p.default {
                        self.infer_expr(d, &Locals::default());
                    }
                }
                for cst in &c.consts {
                    self.infer_expr(&cst.value, &Locals::default());
                }
                for m in &c.methods {
                    self.collect_member_ref_defaults_function(&m.function);
                }
            }
            StmtKind::EnumDecl(e) => {
                for cst in &e.consts {
                    self.infer_expr(&cst.value, &Locals::default());
                }
                for m in &e.methods {
                    self.collect_member_ref_defaults_function(&m.function);
                }
            }
            _ => {}
        }
        for child in crate::ast_walk::stmt_children(stmt) {
            self.collect_member_ref_defaults_stmt(child);
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
    /// `l`'s real non-null type is `Type::Array` (generic/unshaped) and
    /// `r`'s type is a narrower `Type::ArrayShape` - `$details =
    /// $row->details ?? [];` where `$row->details: ?array` genuinely
    /// holds arbitrary keys at runtime, which would otherwise collapse to
    /// the literal empty `[]` shape from the fallback alone, making every
    /// later `$details['key']` read look like a typo.
    fn widen_coalesce_result(&self, l: &Expr, r_ty: Type, locals: &Locals) -> Type {
        // `$x ?? null` is a real, if unusual, no-op: if `$x` is null the
        // result is null either way, and otherwise the result is exactly
        // `$x` itself - so its type is exactly `$x`'s own type,
        // unnarrowed, not the literal `r_ty` (`Type::Null`) this function
        // otherwise trusts as the fallback's type. Otherwise an unshaped-
        // array element read (`$post['selection'] ?? null`, `$post:
        // array` with no shape, so `Mixed`) would collapse all the way
        // down to bare `Type::Null`, when code checking the original
        // expression with `is_array($selection_raw)` immediately after
        // should still be checking a real `Mixed`.
        if r_ty == Type::Null {
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
            // (`union_types` collapses `Mixed | Null` back to plain
            // `Mixed`).
            return if matches!(l.kind, ExprKind::Variable(_)) {
                l_ty
            } else {
                union_types(l_ty, Type::Null)
            };
        }
        let l_ty = narrow_type_excluding(&self.infer_expr(l, locals), &Type::Null);
        // `$x ?? throw new Foo();` (or any other never-returning right
        // side: `exit`, a `never`-typed call) - falling through to the
        // generic `_ => r_ty` fallback below would make the whole
        // expression, and by extension any `$x = ... ?? throw ...;`
        // assignment statement built from it, infer as `Type::Never`
        // itself, which `always_terminates`'s `StmtKind::Expression` case
        // then reads as "this plain assignment statement always ends
        // control flow," wrongly marking every statement after it
        // unreachable. The real semantics: reaching anything past this
        // expression at all already proves the right side never ran, so
        // the result is exactly the left side's own non-null type, the
        // same as the `?? null` case above computes.
        if r_ty == Type::Never {
            return l_ty;
        }
        // Still scoped to exactly `Type::Array` on the left, matching
        // this function's documented, deliberately narrow boundary (an
        // ArrayShape on the left stays uncovered) - also recognizes a
        // `list<T>`/`non-empty-array<T>`-typed left side (a real
        // `Intersection`), which would otherwise lose its type here the
        // same way a plain `Type::Array` would.
        let l_is_array = matches!(l_ty, Type::Array(_, _))
            || matches!(&l_ty, Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Array(_, _))));
        match &r_ty {
            Type::ArrayShape(_) if l_is_array => l_ty,
            // The general case must not discard `l_ty` entirely, keeping
            // only `r_ty` - that's wrong for `??`'s semantics regardless
            // of precision: the result is `$expr` itself when it's not
            // null (already computed above, with `Null` excluded) or
            // `$default` when it is - a real union of both outcomes, not
            // "the default's type, unconditionally."
            _ => union_types(l_ty, r_ty),
        }
    }

    pub(crate) fn infer_binary(&self, op: &BinOp, l: &Expr, r: &Expr, locals: &Locals) -> Type {
        match op {
            // `resolve_concat_type` is a full port of real PHPStan's
            // `InitializerExprTypeResolver::resolveConcatType`: real
            // constant-folding for two literal operands, and AND/OR
            // composition of every accessory kind (`NonFalsyString`/
            // `NonEmptyString`/`LiteralString`/`LowercaseString`/
            // `UppercaseString`/`NumericString`), not just non-falsiness.
            BinOp::Concat => {
                resolve_concat_type(&self.infer_expr(l, locals), &self.infer_expr(r, locals))
            }
            // Real constant folding, mirroring real PHPStan's
            // `BooleanAndHandler`/`BooleanOrHandler::resolveType` -
            // without this, `check_boolean_and_or`'s "whole result"
            // diagnostic (`booleanAnd.always*`/`booleanOr.always*`) could
            // never fire at all, since it has nothing but a permanent
            // bare `Type::Bool` to match against. A definitely-false left
            // side makes `&&`'s result definitely false without even
            // considering the right side; otherwise the right side is
            // inferred against `l`'s truthy-narrowed scope (the same
            // `apply_narrowing` every call site narrows into before
            // checking the right operand), since that's the only scope
            // the right side is ever actually reached in.
            BinOp::And | BinOp::LogicalAnd => {
                let l_ty = self.infer_expr(l, locals);
                if matches!(l_ty, Type::False) {
                    return Type::False;
                }
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, true);
                let r_ty = self.infer_expr(r, &narrowed);
                if matches!(r_ty, Type::False) {
                    Type::False
                } else if matches!(l_ty, Type::True) && matches!(r_ty, Type::True) {
                    Type::True
                } else {
                    Type::Bool
                }
            }
            BinOp::Or | BinOp::LogicalOr => {
                let l_ty = self.infer_expr(l, locals);
                if matches!(l_ty, Type::True) {
                    return Type::True;
                }
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, false);
                let r_ty = self.infer_expr(r, &narrowed);
                if matches!(r_ty, Type::True) {
                    Type::True
                } else if matches!(l_ty, Type::False) && matches!(r_ty, Type::False) {
                    Type::False
                } else {
                    Type::Bool
                }
            }
            // `xor` doesn't short-circuit, so both sides are always
            // inferred against the same, un-narrowed scope - confirmed
            // against `BinaryOpHandler::resolveType`'s own
            // `BinaryOp\LogicalXor` arm.
            BinOp::LogicalXor => match (self.infer_expr(l, locals), self.infer_expr(r, locals)) {
                (Type::True, Type::True) | (Type::False, Type::False) => Type::False,
                (Type::True, Type::False) | (Type::False, Type::True) => Type::True,
                _ => Type::Bool,
            },
            BinOp::Identical | BinOp::NotIdentical => Type::Bool,
            // Mirrors real PHPStan's `ConstantLooseComparisonRule` -
            // `loose_compare_literals`'s doc comment has the full
            // scope/version-gating reasoning.
            BinOp::Eq | BinOp::NotEq => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                match loose_compare_literals(&lt, &rt, self.registry.php_version) {
                    Some(equal) => {
                        let outcome = if matches!(op, BinOp::Eq) {
                            equal
                        } else {
                            !equal
                        };
                        if outcome { Type::True } else { Type::False }
                    }
                    None => Type::Bool,
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
                if let (Some(l_bounds), Some(r_bounds)) = (
                    numeric_comparison_bounds(&lt),
                    numeric_comparison_bounds(&rt),
                ) && let Some(outcome) = compare_range_outcome(op, l_bounds, r_bounds)
                {
                    if outcome { Type::True } else { Type::False }
                } else {
                    Type::Bool
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
            BinOp::Spaceship => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                self.binary_operator_overload_result(op, &lt, &rt)
                    .unwrap_or(Type::Int)
            }
            BinOp::Add => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                // `+` on two arrays is PHP's array-union operator, not
                // arithmetic - it keeps every key from the left operand,
                // filling in any key only present on the right. Computes
                // the key type as the union of both operands' iterable
                // key types (collapsing back to the shared type when they
                // already agree, mirroring real PHPStan's
                // `InitializerExprTypeResolver::getPlusType`'s general
                // non-constant-array case) and the value type as the
                // union of both operands' iterable value types, via the
                // same `array_key_type`/`array_element_type` helpers this
                // file's other array-consuming call sites use.
                // Deliberately doesn't port real PHPStan's constant-array
                // branch (a full `ConstantArrayTypeBuilder`-style
                // per-offset merge that preserves an exact literal
                // `array{...}` shape when both operands are literal
                // shapes) - a precision gap for `array{a: int} +
                // array{b: string}`-shaped operands, not a soundness gap:
                // the result is still a real, provably-correct supertype.
                // `is_array_like` sees through a `list<T>`/
                // `non-empty-array<T>`-typed operand too - without it, a
                // `list<T> + list<T>` would fall through to numeric
                // addition instead of array union.
                // One array operand is enough: any other operand than an
                // array is a `TypeError`, so the value is still an array.
                if Self::is_array_like(&lt) || Self::is_array_like(&rt) {
                    // A non-array operand's keys and values are unknown.
                    let both_arrays = Self::is_array_like(&lt) && Self::is_array_like(&rt);
                    let key_of = |t: &Type| both_arrays.then(|| Self::array_key_type(t)).flatten();
                    let key_ty = match (key_of(&lt), key_of(&rt)) {
                        _ if !both_arrays => Type::benevolent_array_key(),
                        (Some(lk), Some(rk)) if lk == rk => lk,
                        (Some(lk), Some(rk)) => union_types(lk, rk),
                        (Some(k), None) | (None, Some(k)) => k,
                        // Real PHPStan's own `+` implementation
                        // (`InitializerExprTypeResolver::getPlusType`)
                        // falls back to a real `BenevolentUnionType`
                        // here too, not a plain strict union - confirmed
                        // reading it directly.
                        (None, None) => Type::benevolent_array_key(),
                    };
                    let val_ty = union_types(
                        Self::array_element_type(&lt).unwrap_or(Type::Mixed(false)),
                        Self::array_element_type(&rt).unwrap_or(Type::Mixed(false)),
                    );
                    let mut result = Type::Array(Box::new(key_ty), Box::new(val_ty));
                    let has_accessory = |ty: &Type, kind: AccessoryKind| matches!(ty, Type::Intersection(members) if members.contains(&Type::Accessory(kind)));
                    let mut accessories = Vec::new();
                    if has_accessory(&lt, AccessoryKind::ArrayList)
                        && has_accessory(&rt, AccessoryKind::ArrayList)
                    {
                        accessories.push(Type::Accessory(AccessoryKind::ArrayList));
                    }
                    if has_accessory(&lt, AccessoryKind::NonEmptyArray)
                        || has_accessory(&rt, AccessoryKind::NonEmptyArray)
                    {
                        accessories.push(Type::Accessory(AccessoryKind::NonEmptyArray));
                    }
                    if !accessories.is_empty() {
                        let mut members = vec![result];
                        members.extend(accessories);
                        result = Type::Intersection(members);
                    }
                    return result;
                }
                if lt == Type::Float || rt == Type::Float {
                    return Type::Float;
                }
                // Real, if narrow, `int<min,max>` arithmetic, mirroring
                // real PHPStan's `InitializerExprTypeResolver::
                // getPlusType`/`integerRangeMath`'s `BinaryOp\Plus` arm:
                // an unbounded end propagates as unbounded; a bounded one
                // shifts by the same amount. Deliberately narrower than
                // real PHPStan's full range-arithmetic surface (`Mul`/
                // `Div`'s negative/positive-interval splitting isn't
                // replicated) - falls through to the plain `Int` answer
                // whenever either side isn't range-representable.
                if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                {
                    // `checked_add` via `checked_bound_op`, not plain `+`:
                    // real PHP silently promotes an overflowing int
                    // addition to `float` - plain `i64::+` here would
                    // instead panic/wrap on adversarial input like
                    // `int<1, PHP_INT_MAX> + int<1, 10>`.
                    let (min, min_overflow) = checked_bound_op(l_min, r_min, i64::checked_add);
                    let (max, max_overflow) = checked_bound_op(l_max, r_max, i64::checked_add);
                    let result = normalize_int_range(min, max);
                    return if min_overflow || max_overflow {
                        union_types(result, Type::Float)
                    } else {
                        result
                    };
                }
                Type::Int
            }
            BinOp::Sub => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                if lt == Type::Float || rt == Type::Float {
                    return Type::Float;
                }
                // A *constant* right operand (`$x - 1`) shifts both bounds
                // by the same amount with no swap ever needed - kept as
                // its own fast path (matching this project's pre-existing
                // shape) rather than routed through `sub_int_range_full`
                // below, which computes the identical answer for this
                // case but far less directly.
                if let (Some((l_min, l_max)), Some((Some(c), Some(rc)))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                    && c == rc
                {
                    // `checked_sub` via `checked_bound_op` - see the `Add`
                    // arm's own doc comment just above.
                    let (min, min_overflow) = checked_bound_op(l_min, Some(c), i64::checked_sub);
                    let (max, max_overflow) = checked_bound_op(l_max, Some(c), i64::checked_sub);
                    let result = normalize_int_range(min, max);
                    return if min_overflow || max_overflow {
                        union_types(result, Type::Float)
                    } else {
                        result
                    };
                }
                // The full range-minus-range case (`$x - $y`, both sides
                // genuinely range-representable, right side not a single
                // constant) - `sub_int_range_full`'s own doc comment has
                // the transcription details.
                if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                {
                    return sub_int_range_full(l_min, l_max, r_min, r_max);
                }
                Type::Int
            }
            BinOp::Mul => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                if lt == Type::Float || rt == Type::Float {
                    return Type::Float;
                }
                // Real, narrow range-arithmetic, the `Add`/`Sub` arms'
                // own family - `mul_int_range`'s own doc comment has the
                // transcription details. Falls through to plain `Int`
                // whenever either side isn't range-representable at all.
                if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                {
                    return mul_int_range(l_min, l_max, r_min, r_max);
                }
                Type::Int
            }
            BinOp::Pow => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                {
                    pow_int_range(l_min, l_max, r_min, r_max)
                } else {
                    Type::Int
                }
            }
            // `%` and the bitwise/shift operators always cast both
            // operands to int before operating, regardless of whether
            // either side was a float - unlike Add/Sub/Mul/Pow, there is
            // no float-promotion rule for these.
            //
            // Except when GMP (all five) or `BcMath\Number` (`%` only -
            // it doesn't overload the bitwise/shift operators at all, per
            // `binary_operator_overload_result`'s own gate) is genuinely
            // involved, which yields the object type back instead of a
            // plain `int`.
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                fold_bitwise_int_literals(op, &lt, &rt).unwrap_or(Type::Int)
            }
            BinOp::Shl | BinOp::Shr => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                // `shift_int_range`'s own doc comment has the transcription
                // details - only a single constant right operand (`r_min ==
                // r_max`) is range-preserving at all, matching real
                // PHPStan's identical restriction.
                if let (Some((l_min, l_max)), Some((Some(amount), Some(r_max)))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                    && amount == r_max
                {
                    return shift_int_range(l_min, l_max, amount, matches!(op, BinOp::Shl));
                }
                Type::Int
            }
            BinOp::Mod => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                // Real, narrow range-arithmetic, the `Add`/`Sub`/`Mul`
                // family - `mod_int_range`'s own doc comment has the
                // transcription details. Falls through to plain `Int`
                // whenever either side isn't range-representable at all.
                if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                {
                    mod_int_range(l_min, l_max, r_min, r_max)
                } else {
                    Type::Int
                }
            }
            // `/` gives `int|float` unconditionally whenever it can't
            // prove `float` is involved, even for two fully unknown
            // operands (not only when both are provably `int`), and
            // `float` whenever either operand is - the same
            // Add/Sub/Mul/Pow float-promotion shape above, with
            // `Union([Int, Float])` in place of a bare `Int` as the
            // "float not proven" answer.
            BinOp::Div => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                {
                    div_int_range(l_min, l_max, r_min, r_max)
                } else {
                    Type::Union(vec![Type::Int, Type::Float])
                }
            }
            BinOp::Coalesce => {
                let r_ty = self.infer_expr(r, locals);
                self.widen_coalesce_result(l, r_ty, locals)
            }
            // `$value |> callable` calls callable($value) - its return
            // type depends on resolving whatever expression `callable` is
            // (a string, a closure, a first-class callable reference...)
            // to a real signature, which this checker doesn't attempt.
            BinOp::Pipe => Type::Mixed(false),
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
/// Narrowing-derived synthetic keys (`narrowing_key`'s own
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
/// `is_real_variable_key` is exactly the discriminator that excludes
/// them, and composes correctly for a chained `A and B and C`
/// (left-associative `And(And(A,B),C)`): the inner node's own filtered
/// merge already lands B's new plain-variable defs in the outer node's
/// real `locals` before the outer node clones it for `C`.
fn merge_new_variable_definitions(locals: &mut Locals, narrowed: &Locals) {
    for (key, ty) in narrowed.iter() {
        if is_real_variable_key(key) && !locals.contains_key(key) {
            // `key`'s real declaration (a by-ref out-param, or a plain
            // assignment) already recorded its true provenance inside
            // `narrowed` when it happened - carried across unchanged
            // here, not fabricated fresh at this merge point.
            let provenance = narrowed.get_provenance(key).cloned().unwrap_or_default();
            locals.insert_with_provenance(key.clone(), ty.clone(), provenance);
        }
    }
}

/// Real PHP: incrementing `null` produces `int(1)`; decrementing `null`,
/// and either direction on a `bool`, is a no-op (the value is unchanged) -
/// only the null-to-int case actually changes the tracked type. A string
/// operand's exact value isn't modeled precisely enough to distinguish
/// numeric from alphanumeric-increment behavior, so it conservatively
/// stays a string either way.
///
/// Recurses through a `Union` the same established way `write_index_into`/
/// `index_result_type` do (advance each member independently,
/// `reduce(union_types)` back into one type) - without this, a container
/// widened to `Null|Int` across a loop's multi-pass fixed-point
/// convergence (the counter's first touch contributing the `Null`
/// branch, an already-incremented later pass contributing `Int`) would
/// stay frozen at that same `Null|Int` forever, even though incrementing
/// either member alone always produces a real number.
fn advance_incdec_type(ty: &Type, is_increment: bool) -> Type {
    match ty {
        Type::Null if is_increment => Type::Int,
        // A literal/range-typed operand must not pass straight through
        // unchanged: `++$x`/`--$x` on `IntLiteral(0)` needs to actually
        // advance, the same "value never applied" bug class
        // `negate_numeric_literal`'s doc comment covers for unary `-`. A
        // counter incremented inside a loop then checked afterward
        // (`$n = 0; foreach (...) { ...; $n++; } if ($n > 0)`) would
        // otherwise look "always false" against a check that only fires
        // once anything actually consumes the literal's exact value.
        Type::IntLiteral(n) => Type::IntLiteral(if is_increment { n + 1 } else { n - 1 }),
        Type::FloatLiteral(f) => Type::FloatLiteral(if is_increment { f + 1.0 } else { f - 1.0 }),
        Type::IntRange(min, max) => {
            let shift = |b: Option<i64>| b.map(|v| if is_increment { v + 1 } else { v - 1 });
            Type::IntRange(shift(*min), shift(*max))
        }
        Type::Union(members) => members
            .iter()
            .map(|m| advance_incdec_type(m, is_increment))
            .reduce(union_types)
            .unwrap_or(Type::Mixed(false)),
        other => other.clone(),
    }
}

/// The type of `container[key]` for a known container type - shared by
/// `infer_expr`'s `Index` arm and its own recursion into a
/// `Type::Union`'s members. `preg_match`'s `Union` of `ArrayShape`s (the
/// matched shape, unioned with an empty one for the no-match case) needs
/// this same per-key resolution combined across every member via
/// `union_types`, rather than silently dropping to `Mixed` for the whole
/// type just because it's a `Union` - general infrastructure for a
/// `Union` whose members agree on a key's type, not what recovers
/// precision for `preg_match`'s common case (that's `apply_narrowing`'s
/// `preg_match_condition` recognizer, `checker/narrowing.rs`).
pub(crate) fn index_result_type(container_ty: Type, key: Option<&str>) -> Type {
    match container_ty {
        Type::Array(_, inner) => *inner,
        // A genuinely empty shape has zero possible keys, so a dynamic
        // key read is unconditionally real PHP's "undefined array key"
        // warning, returning `null` - without this, an incremented,
        // auto-vivifying fresh dynamic key (`$counts = []; ... =
        // ++$counts[$c];`) would read back bare `Mixed` instead of
        // `Null`, so the increment arm's `Null -> Int` transition
        // (`advance_incdec_type`) would never engage.
        //
        // Scoped to a dynamic key only (`key.is_none()`), not a literal
        // one too - `preg_match`'s by-ref `$matches` shape is a `Union`
        // whose "no match" branch is exactly this same empty shape, and
        // a literal-keyed read there (`$matches[1]`, unconditionally,
        // with no narrowing) is this project's deliberate leniency:
        // absorbing into `Mixed` rather than sharpening to `|null` and
        // forcing every unnarrowed `$matches[N]` access to handle a null
        // case most callers never bother to. A literal key miss already
        // has its own "key not found" diagnostic path
        // (`check_offset_existence`) regardless of what this function
        // returns.
        Type::ArrayShape(fields) if fields.is_empty() && key.is_none() => Type::Null,
        Type::ArrayShape(fields) => key
            .and_then(|k| fields.into_iter().find(|(name, _, _)| name == k))
            .map(|(_, ty, _)| ty)
            .unwrap_or(Type::Mixed(false)),
        Type::Union(members) => members
            .into_iter()
            .map(|m| index_result_type(m, key))
            .reduce(union_types)
            .unwrap_or(Type::Mixed(false)),
        // A `list<T>`/`non-empty-array<T>`-typed value is a real
        // `Intersection([Array(T), Accessory(_)])`, not plain
        // `Array(T)` - without this, indexing into one (`$listArg[0]`)
        // would fall to `Mixed`.
        //
        // A container that also carries a `hasOffsetValue(key, value)`
        // member for this exact literal key (`write_literal_key_aware`'s
        // own doc comment - `checker::assignment`'s construction site)
        // must resolve to that tracked precise `value` instead of the
        // base `Array`'s own coarse element type - mirrors real
        // PHPStan's identical read-side resolution, confirmed via
        // `assertType()`. Checked before the generic `array_element_type`
        // fallback below, which has no way to single out one specific
        // key at all.
        Type::Intersection(members) => {
            let has_offset_value = key.and_then(|k| {
                members.iter().find_map(|m| match m {
                    Type::HasOffsetValue(mk, v) if mk == k => Some((**v).clone()),
                    _ => None,
                })
            });
            has_offset_value.unwrap_or_else(|| {
                members
                    .into_iter()
                    .find_map(|m| Checker::array_element_type(&m))
                    .unwrap_or(Type::Mixed(false))
            })
        }
        _ => Type::Mixed(false),
    }
}

/// Whether `ty` is provably a `non-falsy-string` (non-empty and never
/// exactly `"0"`) - a one-line delegate to the generalized
/// `string_type_has_accessory`.
fn is_non_falsy_string(ty: &Type) -> bool {
    string_type_has_accessory(ty, AccessoryKind::NonFalsyString)
}

/// Whether `ty` is confidently array-shaped for
/// `check_dim_fetch_key_type`'s purposes - a bare `Array`/`ArrayShape`,
/// or a `Union` where every member is (recursively, so a union of unions
/// still counts) - anything else is "don't know," matching this
/// project's leniency elsewhere rather than real PHPStan's tri-state
/// `isArray()` grading.
pub(crate) fn is_confidently_array(ty: &Type) -> bool {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) => true,
        Type::Union(members) => members.iter().all(is_confidently_array),
        _ => false,
    }
}

/// Whether `ty` is a type PHP itself can never legally use as an array
/// key (`int`/`string`/`bool`/`float`/`null` all being legal - PHP
/// coerces the latter three at runtime) - `Some(description)` only when
/// every part of `ty` is confidently illegal, `None` for anything
/// ambiguous (`Mixed`, a union mixing legal and illegal members, an
/// unresolved template, ...), matching this project's "don't guess"
/// leniency elsewhere. A `Type::Object` covers both a real class
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
    php_version: Option<crate::php_version::PhpVersion>,
) -> Option<String> {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) => Some("array".to_string()),
        Type::Object(name, _) => Some(name.clone()),
        Type::Float
            if php_version.is_some_and(|v| {
                crate::php_version::PhpFeature::DeprecatesFloatArrayKey.is_available(v)
            }) =>
        {
            Some("float".to_string())
        }
        Type::Null
            if php_version.is_some_and(|v| {
                crate::php_version::PhpFeature::DeprecatesNullArrayOffset.is_available(v)
            }) =>
        {
            Some("null".to_string())
        }
        Type::Union(members) => {
            let mut descs = Vec::new();
            for m in members {
                // At least one member being legal (or ambiguous) bails
                // out entirely - real PHPStan would report "possibly
                // invalid" here (its own tri-state `isSuperTypeOf`),
                // which this project has no equivalent for; skipping
                // rather than guessing matches its usual leniency.
                descs.push(invalid_array_key_description(m, php_version)?);
            }
            if descs.is_empty() {
                None
            } else {
                Some(descs.join("|"))
            }
        }
        // An accessory-refined intersection (`non-empty-string`, `list<T>`,
        // ...) is illegal only when its own base type is - a refined
        // *string* is still a perfectly legal key.
        Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Array(..))) => {
            Some("array".to_string())
        }
        _ => None,
    }
}

impl<'a> Checker<'a> {
    /// The type of a call this project can't look up a real signature for -
    /// `exit`/`die` are real PHP language constructs (`is_language_construct`)
    /// with no stub of their own, but unlike the others in that list
    /// (`isset`/`empty`/`unset`/`eval`/`include*`/`print`), PHP itself declares
    /// them `never`: control flow can't continue past one. `always_terminates`
    /// (`checker/control_flow.rs`) recognizes a terminating expression
    /// statement via exactly `infer_expr(e, locals) == Type::Never` - without
    /// this, a guard clause built on `exit()`/`die()` instead of `return`/
    /// `throw` (common in this codebase's CLI tools and test fixtures) would
    /// never be recognized as terminating, leaking its never-actually-
    /// reached branch's narrowing into the surrounding scope.
    ///
    /// Takes `&self`/`locals` so a callee whose inferred type is a shaped
    /// `Closure(...)` (`Type::Callable(Some(sig))`) can fall back to that
    /// shape's declared return type instead of `Mixed` - the same fix
    /// `check_expr`'s dynamic-call branch makes on the diagnostic-producing
    /// side.
    fn call_fallback_return_type(&self, callee: &Expr, args: &[Arg], locals: &Locals) -> Type {
        match &callee.kind {
            ExprKind::Name(name)
                if matches!(name.to_ascii_lowercase().as_str(), "exit" | "die") =>
            {
                Type::Never
            }
            // `print` always evaluates to the literal `int(1)` in real
            // PHP, mirroring `PrintHandler::resolveType`, which returns
            // `ConstantIntegerType(1)` unconditionally.
            ExprKind::Name(name) if name == "print" => Type::IntLiteral(1),
            // `isset(...)`/`empty(...)` are real PHP language constructs
            // that always evaluate to a real `bool` when read as a value,
            // not just narrowed as a condition - `is_language_construct`
            // routes both away from ever resolving a real signature here
            // (`sig` stays `None`, so this whole function is the only
            // place their call-expression type gets decided), and
            // `self.infer_expr(callee, locals)` on the bare `Name` below
            // has no special case for either, which would otherwise
            // silently collapse to `Mixed`.
            ExprKind::Name(name) if name == "isset" || name == "empty" => Type::Bool,
            _ => self
                .infer_expr(callee, locals)
                .callable_sig()
                .map_or(Type::Mixed(false), |sig| {
                    if sig.templates.is_empty() {
                        return (*sig.return_type).clone();
                    }
                    let arg_types: Vec<Type> = args
                        .iter()
                        .map(|a| self.infer_expr(&a.value, locals))
                        .collect();
                    sig.instantiated_return(&arg_types)
                }),
        }
    }

    /// `@phpstan-self-out`: after `$x->method(...)`, `$x` has the tag's
    /// type, its templates bound as for a return type (the class's from
    /// the receiver, the method's from the arguments).
    fn apply_self_out(
        &mut self,
        recv: &Expr,
        sig: &FunctionSig,
        resolved: &[Option<&Expr>],
        locals: &mut Locals,
        class: &str,
    ) {
        let Some(out) = &sig.self_out else {
            return;
        };
        let Some(key) = narrowing_key(recv) else {
            return;
        };
        let mut as_return = sig.clone();
        as_return.return_type = out.clone();
        let ty = self.infer_call_return_type(&as_return, resolved, locals);
        let ty = self.resolve_pseudo(&ty, class);
        locals.insert(key, ty);
    }

    /// A static call's return type with `self`/`static`/`parent`
    /// resolved. Called through `parent::`/`self::`/`static::`, PHP
    /// forwards the late static binding, so `static` is the calling class,
    /// not the target.
    fn resolve_static_call_return(&self, ty: &Type, target: &StaticTarget, class: &str) -> Type {
        let forwards = matches!(
            target,
            StaticTarget::Name(n, _)
                if matches!(n.to_ascii_lowercase().as_str(), "parent" | "self" | "static")
        );
        match (&self.current_class, forwards) {
            (Some(current), true) => {
                let bound = bind_static_ref(ty, current);
                self.resolve_pseudo(&bound, class)
            }
            _ => self.resolve_pseudo(ty, class),
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
        return_type: Option<&str>,
        inferred_return: Option<Type>,
        overrides: Option<&[Type]>,
    ) -> Type {
        let resolve = |hint: Option<&str>| {
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
                        Some(_) => resolve(p.type_hint.as_deref()),
                        None => self.closure_param_type(p, i, overrides),
                    },
                    variadic: p.variadic,
                    optional: p.default.is_some(),
                    by_ref: p.by_ref,
                })
                .collect(),
            return_type: Box::new(match (return_type, inferred_return) {
                (None, Some(inferred)) => inferred,
                (Some(hint), Some(inferred)) => {
                    intersect_but_not_never(self.registry, &resolve(Some(hint)), inferred)
                }
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
        params: &[Param],
        uses: &[ClosureUse],
        body: &[Stmt],
        is_static: bool,
        locals: &Locals,
        overrides: Option<&[Type]>,
    ) -> Option<Type> {
        if body_contains_yield(body) {
            return None;
        }
        let mut scope = locals.fresh_scope();
        for u in uses {
            if let Some(ty) = locals.get(&u.name) {
                scope.insert(u.name.to_string(), ty.clone());
            }
        }
        if !is_static && let Some(this_ty) = locals.get("this") {
            scope.insert("this".to_string(), this_ty.clone());
        }
        for (i, p) in params.iter().enumerate() {
            let ty = self.closure_param_type(p, i, overrides);
            let ty = if p.variadic {
                Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
            } else {
                ty
            };
            scope.insert(p.name.to_string(), ty);
        }
        let mut points = Vec::new();
        let mut saw_explicit_return = false;
        let mut stack: Vec<&Stmt> = body.iter().collect();
        while let Some(s) = stack.pop() {
            match &s.kind {
                StmtKind::Return(Some(value)) => {
                    saw_explicit_return = true;
                    points.push(self.infer_expr(value, &scope));
                }
                StmtKind::Return(None) => {
                    saw_explicit_return = true;
                    points.push(Type::Void);
                }
                _ => {}
            }
            stack.extend(crate::ast_walk::stmt_children(s));
        }
        let falls_off_end = !body
            .last()
            .is_some_and(|s| matches!(s.kind, StmtKind::Return(_) | StmtKind::Throw(_)));
        Some(closure_inferred_return_type(
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
        body: &Expr,
        locals: &Locals,
        overrides: Option<&[Type]>,
    ) -> Option<Type> {
        if expr_contains_yield(body) {
            return None;
        }
        let mut arrow_locals = locals.clone();
        for (i, p) in params.iter().enumerate() {
            let ty = self.closure_param_type(p, i, overrides);
            let ty = if p.variadic {
                Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
            } else {
                ty
            };
            arrow_locals.insert(p.name.to_string(), ty);
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
            (Some(_), _) => self.resolve_type_hint(p.type_hint.as_deref()),
            (None, Some(expected)) => expected.clone(),
            (None, None) => Type::Mixed(false),
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
        arg: &Expr,
        expected: &Type,
        locals: &Locals,
    ) -> Option<Type> {
        let sig = expected.callable_sig().or_else(|| match expected {
            Type::Union(members) => members.iter().find_map(Type::callable_sig),
            _ => None,
        })?;
        let overrides: Vec<Type> = sig.params.iter().map(|p| p.ty.clone()).collect();
        match &unwrap_grouping(arg).kind {
            ExprKind::ArrowFn {
                params,
                return_type,
                body,
                ..
            } => {
                let inferred =
                    self.arrow_fn_inferred_return(params, body, locals, Some(&overrides));
                Some(self.closure_literal_type(
                    params,
                    return_type.as_deref(),
                    inferred,
                    Some(&overrides),
                ))
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                let inferred = self.closure_syntactic_return_type(
                    params,
                    uses,
                    body,
                    *is_static,
                    locals,
                    Some(&overrides),
                );
                Some(self.closure_literal_type(
                    params,
                    return_type.as_deref(),
                    inferred,
                    Some(&overrides),
                ))
            }
            _ => None,
        }
    }
}

/// Whether a `Type::ArrayShape` key string is one PHP itself would have
/// already cast to a real integer at the point the array was built -
/// PHP's rule: a decimal string with no leading zero (unless it's
/// exactly `"0"`) and no leading `+`. Deliberately doesn't handle a
/// leading `-` (a negative-integer key string) - misclassifying one as a
/// string key here is the same safe-by-omission default this project
/// uses throughout rather than a wrong answer.
pub(crate) fn is_canonical_int_key(key: &str) -> bool {
    crate::types::canonical_int_key(key).is_some()
}

/// Whether `e` is the literal `false` - used to scope
/// `array_slice`/`array_reverse`'s `$preserve_keys` argument to exactly
/// the default-matching case (omitted, or explicitly `false`).
fn is_false_literal(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Bool(false)))
}

/// The `true`-literal mirror of `is_false_literal` above - used by
/// `var_export()`'s dynamic-return-type narrowing to recognize its
/// `$return` argument's default-matching-but-inverted case (explicitly
/// `true`, unlike `$preserve_keys`'s "omitted or `false`" default).
fn is_true_literal(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Bool(true)))
}

/// Doctrine's `AbstractQuery::getResult()` (and every real terminal
/// method sharing its own `HYDRATE_OBJECT` shape - `execute()`/
/// `toIterable()` when their own hydration-mode argument is omitted or
/// explicit) always produces a real, sequentially-keyed list - true
/// regardless of whether ROADMAP Phase 168's own DQL-aware row-
/// type inference (168b-168d) managed to resolve a real element type,
/// in which case `element_ty` is that resolved `Type`; `Type::Mixed(false)`
/// when it didn't (matches this project's pre-Phase-168 fallback
/// exactly, so an undecidable chain regresses nothing).
pub(crate) fn doctrine_query_result_list_type_of(element_ty: Type) -> Type {
    Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(element_ty)),
        Type::Accessory(AccessoryKind::ArrayList),
    ])
}

pub(crate) fn doctrine_query_result_list_type() -> Type {
    doctrine_query_result_list_type_of(Type::Mixed(false))
}

/// Peels through any `(...)`-grouping wrapper(s) to the real expression
/// underneath - `ExprKind::Grouping` is a genuine, distinct AST node,
/// not something parens collapse away during parsing, so a closure-
/// literal-shaped check like the `Closure::bind`/`->bindTo()` special-
/// casing needs to see through `(function () {...})` the same as a bare
/// `function () {...}`.
pub(crate) fn unwrap_grouping(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) => unwrap_grouping(inner),
        _ => e,
    }
}

impl<'a> Checker<'a> {
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
        subject: &Expr,
        conds: &[Expr],
        base: &Locals,
        match_true: bool,
    ) -> Locals {
        let narrow_one = |cond: &Expr| {
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
                .reduce(|a, b| merge_scopes(&a, &b))
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
    /// contract rather than a silent runtime surprise. `is_assignable`
    /// (not a plain class-name string comparison) so a broader catch/
    /// `@throws` type - `\Throwable`, `\Error`, or a real union
    /// containing either - counts too, not just the exact
    /// `UnhandledMatchError` class name.
    fn match_unhandled_error_is_exempted(&self) -> bool {
        let unhandled_match_error = Type::Object("UnhandledMatchError".to_string(), Vec::new());
        self.current_catch_types
            .iter()
            .any(|caught| self.registry.is_assignable(&unhandled_match_error, caught))
            || self
                .current_declared_throws_type
                .as_ref()
                .is_some_and(|declared| {
                    self.registry
                        .is_assignable(&unhandled_match_error, declared)
                })
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
    /// function this check called to read it. Reaches for that same
    /// dedicated machinery directly (`compare_range_outcome`/
    /// `numeric_comparison_bounds`, already used by `check_constant_
    /// numeric_comparison` to report `smaller.alwaysTrue`/`greater.
    /// alwaysFalse`/etc.) as a plain predicate instead of that
    /// function's own side-effecting report. Scoped to the same four
    /// ordering operators that family covers - an `==`/`===`/
    /// `instanceof`/`&&`/`||` condition (all also skipped by
    /// `inferred_constant_condition`) isn't attempted here yet, each
    /// with its own separate dedicated-check family this could equally
    /// reach into later.
    fn condition_is_provably_true(&self, e: &Expr, locals: &Locals) -> bool {
        if self.inferred_constant_condition(e, locals) == Some(true) {
            return true;
        }
        let ExprKind::Binary(op @ (BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq), l, r) =
            &e.kind
        else {
            return false;
        };
        let l_ty = self.infer_expr(l, locals);
        let r_ty = self.infer_expr(r, locals);
        let Some(l_bounds) = numeric_comparison_bounds(&l_ty) else {
            return false;
        };
        let Some(r_bounds) = numeric_comparison_bounds(&r_ty) else {
            return false;
        };
        compare_range_outcome(op, l_bounds, r_bounds) == Some(true)
    }

    /// `match.unhandled`'s own case-expansion step: a plain enum-typed
    /// `Type::Object(EnumClass, _)` has no case list of its own for
    /// `narrow_type_excluding`'s existing `Union`-member-filtering to
    /// work against - only the fully-expanded `Union` of every real case
    /// (`Type::EnumCase(EnumClass, CaseName)`, already how a case is
    /// represented everywhere else in this checker - `ClassInfo::
    /// from_enum_decl` folds each case into `consts` with exactly that
    /// type) lets excluding each arm's own case value reduce it all the
    /// way to `Type::Never` once every case is covered. Recurses into a
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
    fn expand_enum_cases(&self, ty: &Type) -> Type {
        match ty {
            Type::Union(members) => {
                let mut expanded = Vec::new();
                for m in members {
                    match self.expand_enum_cases(m) {
                        Type::Union(inner) => expanded.extend(inner),
                        other => expanded.push(other),
                    }
                }
                match expanded.len() {
                    0 => Type::Never,
                    1 => expanded.into_iter().next().unwrap(),
                    _ => Type::Union(expanded),
                }
            }
            Type::Object(class_name, _) => {
                let Some(info) = self.registry.classes.get(class_name.as_str()) else {
                    return ty.clone();
                };
                let cases: Vec<Type> = info
                    .consts
                    .values()
                    .filter_map(|c| match &c.ty {
                        Type::EnumCase(cls, case) if cls == class_name => {
                            Some(Type::EnumCase(cls.clone(), case.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                match cases.len() {
                    0 => ty.clone(),
                    1 => cases.into_iter().next().unwrap(),
                    _ => Type::Union(cases),
                }
            }
            other => other.clone(),
        }
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
pub(crate) fn postfix_chain_has_nullsafe(base: &Expr) -> bool {
    match &base.kind {
        ExprKind::PropertyAccess(inner, _, nullsafe) => {
            *nullsafe || postfix_chain_has_nullsafe(inner)
        }
        ExprKind::MethodCall(inner, _, _, nullsafe) => {
            *nullsafe || postfix_chain_has_nullsafe(inner)
        }
        ExprKind::Grouping(inner) => postfix_chain_has_nullsafe(inner),
        _ => false,
    }
}

/// Real PHPStan's `MutatingScope::intersectButNotNever`: a hinted
/// closure's return is its native type narrowed by what the body
/// actually returns. An inferred type the native one can't hold at all
/// (a fall-off-the-end `void` under `: array`) leaves the native type
/// as-is, and a nullable native type keeps its `null`. An `int` returned
/// under `: float` is coerced at runtime, so it reads as the `float`.
fn intersect_but_not_never(registry: &Registry, native: &Type, inferred: Type) -> Type {
    let native_members: Vec<&Type> = match native {
        Type::Union(ms) => ms.iter().collect(),
        other => vec![other],
    };
    let inferred_members = match inferred {
        Type::Union(ms) => ms,
        other => vec![other],
    };
    let mut kept: Vec<Type> = Vec::new();
    for m in inferred_members {
        if matches!(m, Type::Mixed(_)) {
            return native.clone();
        }
        let int_like = matches!(m, Type::Int | Type::IntLiteral(_) | Type::IntRange(..));
        let fits = !matches!(m, Type::Void)
            && native_members
                .iter()
                .any(|n| !(int_like && matches!(n, Type::Float)) && registry.is_assignable(&m, n));
        if fits {
            kept.push(m);
            continue;
        }
        kept.extend(
            native_members
                .iter()
                .filter(|n| !matches!(n, Type::Null) && registry.is_assignable(n, &m))
                .map(|n| (*n).clone()),
        );
    }
    if kept.is_empty() {
        return native.clone();
    }
    if native_members.iter().any(|n| matches!(n, Type::Null)) {
        kept.push(Type::Null);
    }
    kept.sort_by_key(|m| *m == Type::Null);
    kept.into_iter()
        .reduce(union_types)
        .unwrap_or_else(|| native.clone())
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
    points: Vec<Type>,
    saw_explicit_return: bool,
    falls_off_end: bool,
) -> Type {
    if !saw_explicit_return {
        return Type::Void;
    }
    let mut members = points;
    if falls_off_end {
        members.push(Type::Void);
    }
    let has_value = members.iter().any(|m| *m != Type::Void);
    let mut members: Vec<Type> = members
        .into_iter()
        .map(|m| {
            if has_value && m == Type::Void {
                Type::Null
            } else {
                m
            }
        })
        .collect();
    // `null` last, the order real PHPStan's own union description uses
    // (`int|null`, not `null|int`).
    members.sort_by_key(|m| *m == Type::Null);
    members
        .into_iter()
        .reduce(union_types)
        .unwrap_or(Type::Void)
}

/// A bitwise `&`/`|`/`^` over int literals, folded to its exact values
/// (`PREG_SPLIT_NO_EMPTY | PREG_SPLIT_DELIM_CAPTURE` is `3`), at most 64
/// combinations. `$x & k` with a non-negative literal `k` is `int<0, k>`
/// whatever `$x` is.
fn fold_bitwise_int_literals(op: &BinOp, l: &Type, r: &Type) -> Option<Type> {
    fn literals(ty: &Type) -> Option<Vec<i64>> {
        match ty {
            Type::IntLiteral(n) => Some(vec![*n]),
            Type::True => Some(vec![1]),
            Type::False | Type::Null => Some(vec![0]),
            Type::Union(members) => members.iter().try_fold(Vec::new(), |mut acc, m| {
                acc.extend(literals(m)?);
                Some(acc)
            }),
            _ => None,
        }
    }
    let apply = |a: i64, b: i64| match op {
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        _ => a ^ b,
    };
    match (literals(l), literals(r)) {
        (Some(ls), Some(rs)) if ls.len() * rs.len() <= 64 => {
            let mut values: Vec<i64> = ls
                .iter()
                .flat_map(|a| rs.iter().map(move |b| apply(*a, *b)))
                .collect();
            values.sort_unstable();
            values.dedup();
            let mut members = values.into_iter().map(Type::IntLiteral);
            let first = members.next()?;
            Some(members.fold(first, union_types))
        }
        (_, Some(ks)) | (Some(ks), _)
            if matches!(op, BinOp::BitAnd) && ks.len() == 1 && ks[0] >= 0 =>
        {
            Some(Type::IntRange(Some(0), Some(ks[0])))
        }
        _ => None,
    }
}

/// `static` as `class`, everywhere in `ty`.
fn bind_static_ref(ty: &Type, class: &str) -> Type {
    match ty {
        Type::StaticRef => Type::Object(class.to_string(), Vec::new()),
        other => other.map_children(&mut |t| bind_static_ref(t, class)),
    }
}

/// `ty` with every sealed array shape marked unsealed (`types::parse`'s
/// `"..."` field), so a key it doesn't list isn't proven absent.
pub(crate) fn unseal_array_shapes(ty: &Type) -> Type {
    match ty {
        Type::ArrayShape(fields) if !fields.iter().any(|(k, _, _)| k == "...") => {
            let mut fields = fields.clone();
            fields.push(("...".to_string(), Type::Mixed(false), false));
            Type::ArrayShape(fields)
        }
        Type::Union(members) => Type::Union(members.iter().map(unseal_array_shapes).collect()),
        other => other.clone(),
    }
}

/// Where a static call's class part (`Foo` in `Foo::bar()`, or `$obj` in
/// `$obj::bar()`) ends.
fn static_target_end(target: &StaticTarget) -> crate::token::Position {
    match target {
        StaticTarget::Name(_, span) => span.end,
        StaticTarget::Expr(expr) => expr.span.end,
    }
}
