use crate::ast::*;
use crate::checker::{
    ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION, ATTRIBUTE_TARGET_PROPERTY,
    CheckArgsCall, Checker, DiagnosticBuilder, Locals, MemberVisibilityCheck, TemplateBindings,
    body_contains_yield, collect_goto_targets, expr_contains_yield, is_language_construct,
    is_real_variable_key, is_superglobal, literal_string_key, literal_string_value, merge_scopes,
    narrow_type_excluding, narrowing_key, purge_stale_narrowing_for, resolve_call_args,
    superglobal_default_type, to_bitwise_not,
};
use crate::pest_config;
use crate::regex_groups;
use crate::types::{self, AccessoryKind, Type};

mod array_ops;
mod builtin_calls;
mod member_resolution;
mod preg_shapes;
mod probe_checks;
mod ranges;

use crate::checker::expr::ranges::{
    compare_range_outcome, div_int_range, loose_compare_literals, mod_int_range, mul_int_range,
    numeric_comparison_bounds, pow_int_range, shift_int_range, sub_int_range_full,
};
use crate::checker::type_union::union_types;
pub(crate) use ranges::{NullProbeVerdictFn, empty_probe_verdict, null_probe_verdict};
pub(crate) use ranges::{int_range_bounds, normalize_int_range};

pub(crate) use preg_shapes::build_matched_shape;
use preg_shapes::{
    PregFlags, parse_preg_flags, preg_leaf_type, preg_match_all_pattern_order_shape,
    preg_match_matches_shape,
};

/// Phase 52: does unary `-`/`+`'s own numeric-string-to-int coercion
/// apply to this operand's type - true for a bare `Type::String`/
/// `Type::StringLiteral`, or a `numeric-string`-shaped `Intersection`
/// (`String` alongside an accessory kind). Deliberately does NOT widen
/// into a `Union`'s own members here (unlike `operators.rs`'s own
/// leniency-tuned `is_exactly_string_operand`, tuned for suppressing a
/// false invalid-operand flag rather than for this coercion decision) -
/// `int|string` should keep its own original type, not collapse
/// entirely to `Int` just because one member happens to be
/// string-shaped.
fn is_string_shaped(ty: &Type) -> bool {
    matches!(ty, Type::String | Type::StringLiteral(_))
        || matches!(ty, Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::String)))
}

/// Real corpus find (`Piwigo\Site\SiteRepository::findGalleriesUrlById`'s
/// own `$id < -32768`): unary `-`'s own type-inference arm returned
/// `inner_ty` completely unchanged for *both* `Neg` and `Plus` - correct
/// for `+` (a true no-op on the value, only ever coercing a numeric
/// string), but wrong for `-`, which was silently never negating a
/// literal/range operand's own *value* at all - `-32768` inferred as
/// `IntLiteral(32768)` (positive!), not `IntLiteral(-32768)`. Every
/// consumer that only cares "is this an int" never noticed; this
/// project's own range-based narrowing/constant-comparison machinery
/// (which cares about the exact value) did, the moment anything started
/// consuming it that way. `IntRange`'s own min/max swap on negation
/// (`-int<a, b>` is `int<-b, -a>`, negation reverses order) mirrors real
/// PHPStan's own `IntegerRangeType::equal()`-adjacent negation handling.
/// Anything else (a bare `Type::Int`, `Type::Float`, ...) has no
/// specific value to flip, so it's returned unchanged - exactly what the
/// old code already did for those cases, correctly.
fn negate_numeric_literal(ty: Type) -> Type {
    match ty {
        Type::IntLiteral(n) => Type::IntLiteral(-n),
        Type::FloatLiteral(f) => Type::FloatLiteral(-f),
        Type::IntRange(min, max) => Type::IntRange(max.map(|m| -m), min.map(|m| -m)),
        other => other,
    }
}

/// Phase 85: a bare `ExprKind::Name` with no registered global constant
/// (`Checker::global_constant_type`, checked first at both call sites
/// below - an *unresolved* name still stays `Mixed`, e.g. `PHP_EOL`/
/// `M_PI`, real PHP-engine-provided constants this project's own
/// registry has no declaration to index at all) - except `true`/`false`/
/// `null`, real PHP *language* keywords rather than ordinary constants,
/// and the one shape this project's own parser doesn't already fold
/// into `ExprKind::Literal` itself: a fully-qualified reference
/// (`\false`/`\true`/`\null`) parses as a plain `Name`, not the
/// `Keyword::False`/`True`/`Null` token `parse_primary` matches directly
/// for the unqualified form (real PHP itself treats both forms
/// identically - `\false` unambiguously names the same global
/// pseudo-constant `false` does, never a user-defined one, since PHP
/// doesn't allow declaring a constant with one of these three names). A
/// real, common corpus shape: Rector's own namespace-prefixing transform
/// (`RectorPrefix...`) rewrites every unqualified global name to its
/// fully-qualified form, including these three, across a large fraction
/// of this corpus's `vendor/rector/rector/vendor/` and similar prefixed
/// trees - previously silently `Mixed`, which fed straight into
/// `apply_equality_narrowing`'s own `target_ty == Type::Mixed` bail-out,
/// silently skipping every `\false === $x = fn()`-shaped guard's own
/// narrowing (Symfony\Component\Filesystem\Path::getDirectory's real
/// shape, found via Phase 85's own corpus check).
fn bare_name_literal_type(name: &str) -> Type {
    match name.trim_start_matches('\\').to_ascii_lowercase().as_str() {
        "true" => Type::True,
        "false" => Type::False,
        "null" => Type::Null,
        _ => Type::Mixed,
    }
}

/// Phase 60: does `ty`'s own value-space unconditionally exclude `null`?
/// `Mixed`/`Never` are left alone (a `Mixed` slot could always turn out
/// to hold `null` at runtime; `Never` means unreachable code, not a real
/// value to reason about) - everything else is settled by comparing
/// against `narrow_type_excluding(ty, &Type::Null)` (the same "subtract
/// null" operation `??`/`?->`'s own type inference already relies on
/// elsewhere in this file): if removing `null` changes nothing, `ty`
/// never carried it to begin with.
pub(crate) fn type_definitely_excludes_null(ty: &Type) -> bool {
    !matches!(ty, Type::Mixed | Type::Never) && narrow_type_excluding(ty, &Type::Null) == *ty
}

impl<'a> Checker<'a> {
    #[hotpath::measure]
    pub(crate) fn check_expr(&mut self, e: &Expr, locals: &mut Locals, label: &str) -> Type {
        // Phase 71: see `Checker::in_bare_statement_position`'s own doc
        // comment - taken (not just read) so it reads `true` only for
        // the exact top-level expression `StmtKind::Expression` set it
        // for, never for anything this match's own arms go on to
        // recursively check via a nested `check_expr` call.
        let is_bare_statement = std::mem::take(&mut self.in_bare_statement_position);
        // Phase 15 continuation: see `Checker::in_condition_position`'s
        // own doc comment - same "take, not just read" reasoning as
        // `is_bare_statement` just above.
        let is_condition_position = std::mem::take(&mut self.in_condition_position);
        let is_exempt_last_condition = std::mem::take(&mut self.in_exempt_last_condition);
        match &e.kind {
            ExprKind::Call(callee, args) => {
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
                    return Type::Mixed;
                }
                // `PHPStan\Testing\assertType('expected type string', $expr)`
                // is real PHPStan's own testing-DSL function, used
                // throughout its vendored `tests/PHPStan/Analyser/nsrt/`
                // fixture corpus (see Item 2 of the architecture-audit
                // plan) to pin down an exact expected inferred type at a
                // point of interest. Recognizing it here - comparing this
                // project's own `infer_expr` result (rendered via `Type`'s
                // existing PHPStan-style `Display` impl) against the
                // literal expected string - turns that whole corpus into
                // directly-runnable ground truth against this project's
                // own checker, with no separate test harness needed.
                // Real PHPStan itself treats this the same way (a no-op
                // stub function whose call sites its *own* internal
                // tooling specially recognizes), so this is additive,
                // not a real diagnostic anyone would trip on outside of
                // a fixture file that imports it.
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "assertType" || name.ends_with("\\assertType"))
                    && let [expected_arg, actual_arg] = args.as_slice()
                    && let ExprKind::Literal(Literal::String(expected)) = &expected_arg.value.kind
                {
                    let actual = self.infer_expr(&actual_arg.value, locals).to_string();
                    if &actual != expected {
                        self.report(label.to_string(), format!(
                                "assertType() mismatch: expected type \"{expected}\", actual type \"{actual}\""
                            ), actual_arg.value.span, 0, "phpstanRs.assertType");
                    }
                    self.check_expr(&actual_arg.value, locals, label);
                    return Type::Mixed;
                }
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "extract" || name == "eval")
                {
                    self.saw_dynamic_var_construct = true;
                }
                // `print $x` desugars to this same synthetic `Call(Name
                // ("print"), [$x])` node (see the parser's own comment on
                // that decision) - checked here, since `is_language_
                // construct` below routes it away from ordinary
                // signature-based argument checking entirely (`print`
                // has no real signature to check against). Real PHPStan's
                // own `PrintRule`/`PrintHandler` (`vendor/phpstan-src`):
                // the operand must be string-coercible (`print.nonString`,
                // distinct wording from `echo.nonString` - see
                // `check_print_validity`'s own doc comment), it triggers
                // the same implicit `__toString()` call `echo`/`(string)`
                // do, and `print` itself always evaluates to the literal
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
                // existing arg-checking path and the undefined-call
                // diagnostic below see the same name(s) a real call would
                // actually resolve to - previously `lookup_function` used
                // the raw, unnormalized name, so e.g. `\class_exists(...)`
                // silently skipped arg-checking even when the plain name
                // was a known signature. `is_language_construct` is
                // checked against the *original* name, before any
                // namespace-qualification - `isset`/`empty`/etc. are
                // never real namespaced symbols, unlike an actual function
                // call, even though the parser models both the same way.
                let resolved_name = match &callee.kind {
                    ExprKind::Name(name) if is_language_construct(name) => None,
                    ExprKind::Name(name) => {
                        let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
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
                // Item 1: resolves the call's own arguments against the
                // real signature (named/reordered-named arguments no
                // longer get checked as if they were positional in
                // written order) before any consumer below sees them.
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                // Real corpus find: `array_pop`/`array_shift`'s own
                // by-ref first argument gets widened right below
                // (`predefine_byref_or_unknown_args`'s own dedicated
                // arm for these two - correctly dropping any known
                // non-emptiness, since removing an element can leave
                // `[]`) *before* this same call's own return type is
                // computed further down - reading the *already-widened*
                // array there made a provably non-empty array (Gettext's
                // own `array_shift(explode(...))`, `explode()`'s own
                // return type unconditionally non-empty) look like it
                // could return `null`, when the value actually removed
                // and returned is the pre-widening array's own,
                // definitely-present first/last element. Captured here,
                // before the widening runs, and substituted back in once
                // the ordinary return-type computation below has run.
                //
                // Disclosed residual, not fully closed by this: fixes
                // the common `$x = array_shift($arr); needsFoo($x);`
                // shape (this expression's own returned type, consumed
                // by an assignment or `return`) but not `needsFoo(
                // array_shift($arr))` (the identical call passed
                // *directly* as another call's own argument) - argument-
                // type checking (`check_args`, `checker/calls.rs`)
                // independently re-derives each argument's type via
                // `infer_expr` from whatever `locals` holds *then*,
                // rather than reusing this already-corrected value, so
                // it still sees the post-widening (nullable) answer.
                // Fixing that would mean threading each argument's
                // already-checked type through to `check_args` instead
                // of letting it re-infer independently - a real,
                // separate architecture change, out of scope here.
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
                // Finding 1: `it`/`test`/`describe`/`beforeEach`/
                // `afterEach`/`beforeAll`/`afterAll` rebind `$this` inside
                // their closure/arrow-fn argument at runtime (Pest's own
                // `Closure::bindTo()`) - reuses the same
                // check_closure_literal_with_this helper the
                // Closure::bind/bindTo special case below already uses
                // for the identical "externally-bound $this" shape. The
                // base scanner never spells out Pest's own function names
                // - it only asks pest_config::is_this_binding_call. Only
                // a closure/arrow-fn *literal* argument is recognized,
                // same restriction the existing bind/bindTo case already
                // has - `it('...', $variableHoldingAClosure)` falls
                // through to plain check_expr, unrebound.
                let is_this_binding_call = matches!(&callee.kind, ExprKind::Name(name) if pest_config::is_this_binding_call(name));
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
                // Phase 60 follow-up: `isset($x)` and `empty($x)` each get
                // their own verdict function - `null_probe_verdict` (just
                // nullability) for the former, `empty_probe_verdict` (full
                // falsiness, via `type_truthiness`) for the latter,
                // matching real PHPStan's own `EmptyRule` requiring BOTH
                // nullability *and* falsiness to be unambiguous before it
                // says anything.
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
                // Phase 53: `array_map($callback, $items)`'s own
                // contextual parameter-type injection - computed once,
                // up front, from `$items`' own element type (not
                // per-argument inside the loop below, since it's the
                // *other* argument's own type this needs). Scoped to the
                // common 2-arg single-array form, matching `infer_
                // builtin_call_return_type`'s own identical restriction
                // for the same function.
                let array_map_elem_override = (resolved_name.as_deref() == Some("array_map")
                    && args.len() == 2)
                    .then(|| self.infer_expr(&args[1].value, locals))
                    .and_then(|ty| Self::array_element_type(&ty));
                // Phase 53 follow-up: the same contextual parameter-type
                // injection as `array_map` just above, extended to
                // `array_filter`/`usort`/`uasort`/`array_walk`/
                // `preg_replace_callback` - each computes its own
                // `(callback argument index, param overrides)` pair once
                // up front, the identical shape `array_map_elem_override`
                // already established, just generalized to a callback
                // that isn't always argument 0 and doesn't always take
                // exactly one parameter. `check_closure_literal_with_
                // param_overrides`'s own exact-arity requirement already
                // handles every mode/shape this doesn't specifically
                // recognize (e.g. `array_filter`'s `ARRAY_FILTER_USE_*`
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
                        "array_walk" if args.len() == 2 => {
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
                    if is_this_binding_call
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                    {
                        let this_ty = self.pest_this_type.clone();
                        // Phase 57: Pest's own `$this`-binding (a real
                        // `TestCase`-subclass instance, almost always)
                        // grants the same private/protected access real
                        // PHP's `Closure::bind`/`->bindTo()` third
                        // argument does - without this, every real
                        // corpus `it(...)`/`test(...)` closure calling
                        // an inherited protected `TestCase` method
                        // (`$this->expectErrorLog(...)`, a genuine,
                        // common idiom) was wrongly flagged, since
                        // `current_class` has no other way to reflect
                        // Pest's own file-scoped binding.
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
                        // Phase 36: `@param-closure-this` - a Closure/
                        // arrow-fn literal argument at this parameter's
                        // position gets `$this` seeded to the tag's own
                        // declared type, the same mechanism `Closure::
                        // bind`/`->bindTo()` already use. Only fires for a
                        // literal (matching real PHPStan's own
                        // `resolveClosureThisType` restriction) - a
                        // closure passed by variable falls through to the
                        // plain `check_expr` below, unchanged.
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
                if let Some(name) = &resolved_name {
                    if let Some(sig) = sig {
                        // Phase 67: `@internal`.
                        self.check_internal_function_call(name, sig, e.span, label);
                        // Phase 39: `@deprecated`.
                        self.check_deprecated_function_call(name, sig, e.span, label);
                        // Phase 68: no-op array_filter()/array_values().
                        match name.as_str() {
                            "array_filter" => {
                                self.check_array_filter_no_op(args, locals, label, e.span)
                            }
                            "array_values" => {
                                self.check_array_values_no_op(args, locals, label, e.span)
                            }
                            _ => {}
                        }
                        // Finding 29 / Phase 49: a spread argument
                        // (`f(...$args)`) means the real argument
                        // count/types past it aren't known statically -
                        // `check_args` itself is now spread-aware (Phase
                        // 49 Item 2: it stops trusting a count/"missing"
                        // check at or past a spread's own position,
                        // rather than this call site skipping the whole
                        // check the way it previously had to), so it's
                        // always worth calling. `check_format_string_call`
                        // is NOT spread-aware (it walks `args[1..]`
                        // strictly positionally against the format
                        // string's own specifier count) and stays gated
                        // on no spread being present, to avoid treating
                        // the spread argument itself as filling one
                        // specifier slot.
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
                        if !args.iter().any(|a| a.spread)
                            && matches!(
                                name.as_str(),
                                "sprintf" | "printf" | "fprintf" | "vsprintf" | "vprintf"
                            )
                        {
                            self.check_format_string_call(name, args, locals, label);
                        }
                        // Phase 71: real PHP itself resolves `define`
                        // case-insensitively like any other function
                        // call - matched against just the last
                        // namespace segment, the same as every other
                        // by-name builtin special-case here.
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
                        {
                            let bare = name.rsplit('\\').next().unwrap_or(name);
                            if bare.eq_ignore_ascii_case("sscanf")
                                || bare.eq_ignore_ascii_case("fscanf")
                            {
                                self.check_scanf_argument_count(bare, args, label, e.span);
                            }
                        }
                        if !is_bare_statement {
                            self.check_useless_return_value_call(name, args, locals, e.span, label);
                        }
                    } else {
                        // Since resolve_function_candidates always
                        // produces a definite name (there's no "can't
                        // resolve" case left for functions the way
                        // `ExprKind::New`'s class resolution still has for a
                        // multi-segment name with no matching import),
                        // every miss here is a real undefined-function
                        // finding.
                        self.report(
                            label.to_string(),
                            format!("call to undefined function {name}()"),
                            e.span,
                            0,
                            "function.notFound",
                        );
                    }
                }
                // Phase 12 Findings A/B/C: `preg_match`'s by-ref
                // `$matches` shape, including `PREG_OFFSET_CAPTURE`/
                // `PREG_UNMATCHED_AS_NULL` recognition - `parse_preg_flags`
                // bails (`None`) on any other flag, leaving this whole
                // check inert for it. Overrides the generic widening
                // `predefine_byref_or_unknown_args` already did above,
                // rather than replacing it - anything outside this scope
                // keeps that call's own (Mixed-or-widened) fallback
                // unchanged. Deliberately placed *after* `check_args`
                // above, not right after `predefine_byref_or_unknown_args`
                // like an earlier draft had it - a real bug found by
                // testing, not just reasoned about: `check_args` checks
                // whether the *argument* `$matches` is compatible with
                // the declared `?array<string> $matches` parameter (the
                // real phpstorm-stubs stub is more specific than the
                // bare native `?array` hint), and that check needs to see
                // `$matches`'s own *pre-call* type, not this override's
                // freshly-computed *post-call* shape - placing this
                // before `check_args` made every `$matches` shape whose
                // leaf type isn't plain `string` (Finding B's `[string,
                // int]` tuple, Finding C's `string|null`) spuriously fail
                // that check, since Finding A's own all-`string`-leaves
                // shape had accidentally masked the bug by coincidentally
                // matching the declared type exactly.
                if resolved_name.as_deref() == Some("preg_match")
                    && resolved.get(1).copied().flatten().is_some()
                    && let Some(pattern_arg) = resolved.first().copied().flatten()
                    && let Some(matches_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(matches_name) = &matches_arg.kind
                    && let Some(pattern_str) = literal_string_value(pattern_arg)
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
                    // Phase 12 Finding E, Part 3: unioned with an empty
                    // shape for the real `0`-return/no-match case (real
                    // PHP genuinely sets `$matches = []` then) - sound on
                    // its own only because `check_shape_key_access`/
                    // `infer_expr`'s own `Index` arm are now `Union`-aware
                    // (Part 1), and precision for the extremely common
                    // `if (preg_match(...)) { use $matches }` idiom is
                    // recovered by `apply_narrowing`'s own new
                    // `preg_match_condition` recognizer (Part 2), not by
                    // this unconditional type itself.
                    let shape = union_types(matched, Type::ArrayShape(vec![]));
                    purge_stale_narrowing_for(locals, matches_name);
                    // Phase 17 follow-up: `$matches` is very commonly
                    // undeclared before this exact call - its real
                    // first appearance, so `declare` when there's
                    // nothing here yet to preserve (an already-existing
                    // `$matches` keeps its own real provenance instead,
                    // matching this file's own "call-effected type
                    // change, not a new declaration" rule elsewhere).
                    if locals.contains_key(matches_name) {
                        locals.insert(matches_name.clone(), shape);
                    } else {
                        locals.declare(matches_name.clone(), shape, matches_arg.span);
                    }
                }
                // Phase 12 Finding D: `preg_match_all`'s by-ref
                // `$matches` shape - a sibling special case to
                // `preg_match`'s own above, sharing the same pattern-
                // parsing/flag-parsing/leaf-type machinery but building a
                // structurally different shape (`PREG_PATTERN_ORDER`, the
                // real default, groups results by capture group across
                // every match; `PREG_SET_ORDER` groups them by match,
                // reusing `preg_match_matches_shape` unchanged for each
                // entry). Placed after `check_args`, same reasoning as
                // Finding A/B/C just above.
                if resolved_name.as_deref() == Some("preg_match_all")
                    && resolved.get(1).copied().flatten().is_some()
                    && let Some(pattern_arg) = resolved.first().copied().flatten()
                    && let Some(matches_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(matches_name) = &matches_arg.kind
                    && let Some(pattern_str) = literal_string_value(pattern_arg)
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
                    // Phase 17 follow-up: same reasoning as `preg_match`'s
                    // own identical treatment just above.
                    if locals.contains_key(matches_name) {
                        locals.insert(matches_name.clone(), shape);
                    } else {
                        locals.declare(matches_name.clone(), shape, matches_arg.span);
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
                    .unwrap_or_else(|| call_fallback_return_type(callee));
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
                // Finding 49's other real shape: `(function () {...})
                // ->bindTo($newThis, $scope)` - same idea as
                // `Closure::bind`, just on the receiver instead of the
                // first argument.
                //
                // The `bindTo` branch can't reuse `check_expr`'s own
                // return value the way the ordinary branch below does -
                // `check_closure_literal_with_this` checks the closure's
                // *body* under a rebound `$this`, it doesn't compute
                // `recv`'s own type - so `recv` still needs its own
                // `infer_expr` call there specifically.
                let recv_ty = if method == "bindTo"
                    && matches!(
                        unwrap_grouping(recv).kind,
                        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                    ) {
                    let this_ty = args
                        .first()
                        .map(|a| self.infer_expr(&a.value, locals))
                        .unwrap_or(Type::Mixed);
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
                // Phase 17 Stage B: see `probe_span`'s own doc comment.
                if self.probe_span == Some(recv.span) {
                    self.probe_hit = Some(recv_ty.clone());
                }
                // Phase 60: `$x?->method()` is pointless once `$x`'s own
                // type already proves it's never null - mirrors real
                // PHPStan's `NullsafeMethodCallRule` exactly (same
                // identifier, same "only when definitely never null, not
                // when definitely always null either" split). Guarded by
                // `loop_depth`/`static_var_names` for the same reasons
                // `check_pointless_null_probe` is - a receiver only ever
                // widened to nullable on a later loop iteration or an
                // earlier call still looks non-nullable on this single
                // pass.
                if *nullsafe
                    && self.loop_depth == 0
                    && !matches!(&recv.kind, ExprKind::Variable(n) if self.static_var_names.contains(n))
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
                // Re-enabled (Phase 9.5): the registry is now
                // namespace-aware, so a method lookup trusts the specific
                // `ClassInfo` stored under a *fully-qualified* name, not a
                // bare one two unrelated packages could collide on - see
                // the module-level doc comment's "Known limitation" note,
                // now resolved, for the full history.
                //
                // A `Union`/`Intersection` receiver (e.g. a multi-type
                // `catch` variable, or an ordinarily union-typed
                // parameter) used to fall to `_ => None` here entirely,
                // silently skipping this whole check - `member_missing`
                // below picks up the "should this be flagged" question
                // for those cases instead, using a representative member
                // (the first `Object` found) for argument-checking.
                let recv_class = match &recv_ty {
                    Type::Object(class, type_args) => Some((class.clone(), type_args.clone())),
                    // Phase 79: an enum can't be generic, so there's no
                    // real template-args list to carry through here -
                    // same as every other synthesized enum-instance type
                    // this checker builds (`cases()`'s own element type,
                    // `self_ty` in `ClassInfo::from_enum_decl`).
                    Type::EnumCase(fqn, _) => Some((fqn.clone(), Vec::new())),
                    Type::Union(members) | Type::Intersection(members) => {
                        members.iter().find_map(|m| match m {
                            Type::Object(class, type_args) => {
                                Some((class.clone(), type_args.clone()))
                            }
                            Type::EnumCase(fqn, _) => Some((fqn.clone(), Vec::new())),
                            _ => None,
                        })
                    }
                    _ => None,
                };
                // Real bug found and fixed this session: `sig` (and every
                // class-name-dependent decision below it - argument-type
                // checking, visibility/`@internal`/`@deprecated`, the
                // Doctrine special case, and the final return-type
                // resolution) used to derive `class`/`owner` from
                // `recv_class` above, which is picked purely to answer
                // "is there any concrete class at all" (forward order,
                // regardless of whether it actually has the method) - the
                // same anti-pattern this session's own `find_method_
                // receiver_class`/`find_property_receiver` already fix
                // elsewhere. `find_method_receiver` searches every member
                // (`Intersection` in reverse, preferring the narrower,
                // just-`instanceof`-checked type - see `method_call_
                // return_type`'s own doc comment for the real corpus bug
                // this ordering fixes) for one that genuinely resolves
                // the method, carrying its own type-args and declaring
                // owner through in one pass. Substituted through the
                // receiver's own carried type arguments (Phase 10),
                // walking up to whichever ancestor actually declares the
                // method if it's inherited (Phase 10.5) - a method
                // declared `add(T $item)` on `Collection<User>`, or
                // inherited from `Collection` by a concrete
                // `IntCollection extends Collection<int>`, gets checked
                // against `User`/`int`, not an unbound `T`/`Mixed`.
                let receiver = self.find_method_receiver(&recv_ty, method);
                let sig = receiver.as_ref().map(|(class, type_args, owner, sig)| {
                    self.substitute_class_templates(sig, class, type_args, owner)
                });
                let resolved = sig
                    .as_ref()
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(None, sig.as_ref(), args, &resolved, locals);
                for (i, a) in args.iter().enumerate() {
                    let closure_this = sig
                        .as_ref()
                        .and_then(|s| s.params.get(i))
                        .and_then(|p| p.closure_this_type.clone());
                    if let Some(this_ty) = closure_this
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                    {
                        // Phase 36: `@param-closure-this` - see
                        // `ExprKind::Call`'s own identical branch.
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
                if let Some((class, _)) = &recv_class {
                    // Checked *before* falling back to whether the
                    // representative member's own `sig` was found: for a
                    // `Union`, the representative is just the first
                    // `Object` member encountered, which could easily be
                    // one that *does* have the method even though another
                    // member doesn't - the real question is never "does
                    // the first member have it," it's "is it missing from
                    // any member (Union) / every member (Intersection)."
                    // A class not known to the registry at all (an
                    // unresolvable/uninstalled dependency, the same gap
                    // `ExprKind::New`'s check documents) is left unchecked
                    // rather than risking a false positive - only fires
                    // once the class itself is confirmed known. `__call`
                    // anywhere in the chain means any method name is
                    // legitimately callable at runtime, so its presence
                    // suppresses this entirely, the same way real
                    // PHPStan treats it.
                    if self.registry.classes.contains_key(class)
                        && self.member_confirmed_missing(&recv_ty, |c| {
                            self.registry.lookup_method(c, method).is_some()
                                || self.registry.lookup_method(c, "__call").is_some()
                        })
                    {
                        self.report(
                            label.to_string(),
                            format!("call to undefined method {recv_ty}::{method}()"),
                            e.span,
                            0,
                            "method.notFound",
                        );
                    } else if let Some((class, _, owner, _)) = &receiver
                        && let Some(sig) = &sig
                    {
                        // Phase 57: visibility. `owner`/`sig` both come
                        // from the same `find_method_receiver` lookup
                        // now, guaranteed consistent with each other -
                        // no separate re-lookup needed.
                        self.check_member_visibility(MemberVisibilityCheck {
                            kind: "method",
                            owner,
                            member_name: method,
                            visibility: &sig.visibility,
                            describe: &format!("method {owner}::{method}()"),
                            span: e.span,
                            label,
                        });
                        // Phase 67: `@internal`.
                        self.check_internal_method_call(owner, method, sig, false, e.span, label);
                        // Phase 39: `@deprecated`.
                        self.check_deprecated_method_call(owner, method, sig, false, e.span, label);
                        // Finding 29 / Phase 49: see the identical
                        // comment on the plain-function Call arm above -
                        // `check_args` is spread-aware now, so it's
                        // always worth calling.
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
                let result = match &receiver {
                    Some((class, _, _, _)) if is_doctrine_query_get_result(self, class, method) => {
                        doctrine_query_result_list_type()
                    }
                    Some((class, _, _, _)) => sig
                        .as_ref()
                        .map(|s| {
                            let resolved_ty = self.infer_call_return_type(s, &resolved, locals);
                            self.resolve_pseudo(&resolved_ty, class)
                        })
                        .unwrap_or(Type::Mixed),
                    None => Type::Mixed,
                };
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if *nullsafe && result != Type::Mixed {
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
            ExprKind::Binary(BinOp::And, l, r) => {
                self.check_expr(l, locals, label);
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, true);
                self.check_expr(r, &mut narrowed, label);
                // Phase 15, `BooleanAndConstantConditionRule`: real
                // inferred-type trust - see `check_boolean_and_or`'s own
                // doc comment (`checker/control_flow.rs`). Passes the
                // *original*, pre-narrowing `locals` (not `narrowed`) -
                // `check_boolean_and_or` narrows its own right-side/
                // whole-result checks internally now; an earlier version
                // passed the already-narrowed scope for every one of its
                // three checks, including the left one, which was a real,
                // now-fixed bug (see that function's own doc comment).
                // `is_first_level` is `is_condition_position ||
                // is_bare_statement` - confirmed directly against real
                // PHPStan's own `isInFirstLevelStatement()`: a bare `$a &&
                // $b;` used as its own statement (its result discarded)
                // is checked with `ExpressionContext::createTopLevel()`
                // the exact same way an `if`/`while`'s own direct
                // condition is, not just the condition sites this
                // project's own `in_condition_position` already covers.
                // Real corpus false positive this fixes: `Piwigo\Url\
                // UrlService::makeSectionInUrl`'s `isset($x) or
                // trigger_error(...);` (`trigger_error()`'s own stub
                // return type is exactly `true`, so the bare `or`'s own
                // whole-result looked constantly true).
                self.check_boolean_and_or(
                    &BinOp::And,
                    l,
                    r,
                    e,
                    locals,
                    label,
                    is_condition_position || is_bare_statement,
                );
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(&BinOp::And, l, r, locals)
            }
            ExprKind::Binary(BinOp::Or, l, r) => {
                self.check_expr(l, locals, label);
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, false);
                self.check_expr(r, &mut narrowed, label);
                // Phase 15, `BooleanOrConstantConditionRule`: same
                // reasoning as `&&` above - passes the original,
                // pre-narrowing `locals`, and the same `is_bare_statement`
                // inclusion in `is_first_level`.
                self.check_boolean_and_or(
                    &BinOp::Or,
                    l,
                    r,
                    e,
                    locals,
                    label,
                    is_condition_position || is_bare_statement,
                );
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(&BinOp::Or, l, r, locals)
            }
            // `??` is built on isset()-style quiet semantics - its left
            // side is evaluated without ever raising an undefined-
            // variable/undefined-property notice, even when the root
            // variable was never assigned anywhere in scope. Real PHP
            // silences this for the whole dereference chain on the left
            // (`$config['key'] ?? 'default'` is just as warning-free as
            // a bare `$x ?? 'default'`), which is exactly what
            // `check_expr_quiet_root` walks. `??`'s own result type is
            // *usually* just the right side's type - reusing
            // `check_expr(r, ...)`'s return value directly avoids a real,
            // confirmed double-walk of `r` - except when `widen_coalesce_
            // result` (Phase 22 Finding 4) says the left side's own type
            // carries real information the right side's bare fallback
            // would otherwise discard.
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
                self.check_division_by_literal_zero(op, r, &r_ty, r.span, label);
                self.check_constant_loose_comparison(op, l, r, &l_ty, &r_ty, e.span, label);
                self.check_constant_numeric_comparison(op, l, r, &l_ty, &r_ty, e.span, label);
                // Phase 15, `LogicalXorConstantConditionRule`: real
                // inferred-type trust - see `check_logical_xor`'s own doc
                // comment (`checker/control_flow.rs`). `BinOp::And`/`Or`
                // are deliberately *not* handled here at all: they each
                // have their own, separate, earlier-matching `ExprKind::
                // Binary` arm above (short-circuit-narrowing-aware,
                // matching real PHPStan's own `getRightScope()`), which
                // this generic arm - the same one every *other* binary
                // operator falls through to - never actually reaches for
                // them.
                if *op == BinOp::LogicalXor {
                    self.check_logical_xor(l, r, locals, label);
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
                // Phase 17 follow-up: see `Checker::probe_var_provenance`'s
                // own doc comment - the exact same "stash it when the span
                // matches" pattern `probe_hit` already uses.
                if self.probe_span == Some(e.span) {
                    self.probe_var_provenance = locals
                        .get_provenance(name)
                        .map(|p| p.iter().copied().collect());
                }
                locals
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| superglobal_default_type(name))
            }
            ExprKind::Unary(
                op @ (UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec),
                inner,
            ) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_incdec_validity(op, inner, &inner_ty, e.span, label);
                // Real PHP: incrementing `null` produces `int(1)`;
                // decrementing `null`, and either direction on a `bool`,
                // is a no-op (the value is unchanged) - only the
                // null-to-int case actually changes the tracked type.
                // A string operand's own exact value isn't modeled
                // precisely enough to distinguish numeric from
                // alphanumeric-increment behavior, so it conservatively
                // stays a string either way.
                if let ExprKind::Variable(name) = &inner.kind {
                    let current = locals.get(name).cloned().unwrap_or(Type::Mixed);
                    let is_increment = matches!(op, UnaryOp::PreInc | UnaryOp::PostInc);
                    let updated = match &current {
                        Type::Null if is_increment => Type::Int,
                        // Real corpus find: a literal/range-typed operand
                        // used to pass straight through unchanged here -
                        // `++$x`/`--$x` on `IntLiteral(0)` stayed exactly
                        // `IntLiteral(0)` forever, never actually
                        // advancing - the identical "value never applied"
                        // bug class `negate_numeric_literal`'s own doc
                        // comment already found for unary `-`. A counter
                        // incremented inside a loop then checked
                        // afterward (`$n = 0; foreach (...) { ...; $n++;
                        // } if ($n > 0)`, a real, extremely common idiom)
                        // looked "always false" against a check that only
                        // fires once anything actually consumes the
                        // literal's own exact value.
                        Type::IntLiteral(n) => {
                            Type::IntLiteral(if is_increment { n + 1 } else { n - 1 })
                        }
                        Type::FloatLiteral(f) => {
                            Type::FloatLiteral(if is_increment { f + 1.0 } else { f - 1.0 })
                        }
                        Type::IntRange(min, max) => {
                            let shift = |b: Option<i64>| {
                                b.map(|v| if is_increment { v + 1 } else { v - 1 })
                            };
                            Type::IntRange(shift(*min), shift(*max))
                        }
                        other => other.clone(),
                    };
                    purge_stale_narrowing_for(locals, name);
                    // Phase 17 follow-up: `$x++`/`$x--` is real, visible
                    // rebinding syntax, the same category as `$x += 1`.
                    locals.declare(name.clone(), updated, inner.span);
                }
                self.infer_expr(inner, locals)
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.check_expr(inner, locals, label);
                // Phase 15, `BooleanNotConstantConditionRule`: real
                // inferred-type trust, fires *anywhere* `!expr` appears
                // (not just as a condition - `should_skip_constant_
                // condition` excludes `!` from `if`/`while`/`ternary`'s
                // own generic check specifically so this standalone rule
                // is the one and only place it's caught, matching real
                // PHPStan's own separation exactly). Note the inversion:
                // negating an always-*true* inner expression makes the
                // whole `!expr` always *false*, and vice versa - confirmed
                // directly against `BooleanNotConstantConditionRule.php`,
                // not assumed. `is_exempt_last_condition` (only ever `true`
                // when this `!expr` is itself a genuine last-elseif-
                // without-else's own condition, set by `check_if`) mirrors
                // real PHPStan's own `isLast` exemption for this rule too -
                // confirmed it has one, unlike `Ternary`'s.
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
            // Phase 51: `IntegerRangeType::toBitwiseNotType()` - `~5`/
            // `~$x` on a known int-literal/range-typed operand computes
            // the real, order-reversed complement range instead of just
            // passing the operand's own type through unchanged (still
            // correct, if imprecise, for a plain unbounded `Int`/
            // `String` - `~"hello"` is a real, still-string byte
            // complement).
            ExprKind::Unary(op @ UnaryOp::BitNot, inner) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_unary_operand_validity(op, &inner_ty, inner.span, label);
                to_bitwise_not(&inner_ty).unwrap_or(inner_ty)
            }
            ExprKind::Grouping(inner) => self.check_expr(inner, locals, label),
            ExprKind::Cast(ty, inner) => {
                let inner_ty = self.check_expr(inner, locals, label);
                self.check_cast_validity(ty, &inner_ty, inner.span, label);
                // Phase 67/Phase 39 follow-up: an implicit `(string)` cast
                // of an object triggers a real `__toString()` call - see
                // `check_implicit_tostring_usage`'s own doc comment.
                if ty == "string" {
                    self.check_implicit_tostring_usage(&inner_ty, e.span, label);
                }
                match ty.as_str() {
                    "int" => Type::Int,
                    "float" => Type::Float,
                    "string" => Type::String,
                    "bool" => Type::Bool,
                    "array" => {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed))
                    }
                    "object" => Type::Object("object".to_string(), Vec::new()),
                    "unset" => Type::Null,
                    _ => Type::Mixed,
                }
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used
            // for a function that always throws or exits.
            ExprKind::Throw(inner) => {
                // Finding 57's own check, previously only wired for the
                // `StmtKind::Throw` (bare `throw $x;` statement) shape -
                // a `throw` *expression* (`$x ?? throw new Foo();`,
                // `default => throw ...`, an arrow function body, ...)
                // is the exact same language construct at runtime and
                // accepts only a real Throwable just as unconditionally
                // (a real PHP fatal error otherwise), but never ran this
                // check at all until now.
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
                Type::Never
            }
            ExprKind::YieldFrom(inner) => {
                self.check_expr(inner, locals, label);
                Type::Mixed
            }
            // Never given a stable nominal type by infer_expr either,
            // since multiple clone-with-overrides expressions could
            // produce runtime objects that don't share a common shape
            // this checker tracks - the clone target's own type is the
            // honest answer.
            ExprKind::Clone(inner, overrides) => {
                let inner_ty = self.check_expr(inner, locals, label);
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
                Type::Mixed
            }
            ExprKind::Instanceof(inner, target) => {
                self.check_expr(inner, locals, label);
                match target {
                    InstanceofTarget::Expr(inner_target) => {
                        self.check_expr(inner_target, locals, label);
                    }
                    // Phase 48: `ExistingClassInInstanceOfRule`'s own
                    // check - a dynamic target (`InstanceofTarget::Expr`,
                    // handled above) never names a concrete class to
                    // check existence for at all, only a literal one
                    // does. `resolve_name_expr` already handles `self`/
                    // `parent`/`static` (never flagged - see its own doc
                    // comment), so anything that comes back as an
                    // `Object` here is a genuine bare class-name
                    // reference.
                    InstanceofTarget::Name(name, _) => {
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
                                self.check_internal_instanceof(&class, e.span, label);
                            }
                        }
                    }
                }
                Type::Mixed
            }
            ExprKind::Assign(lhs, rhs) => {
                let rhs_ty = self.check_expr(rhs, locals, label);
                // Phase 65: `$this` is real PHP's own reserved,
                // implicitly-bound reference to the current object -
                // reassigning it is a real, unconditional fatal error
                // ("Cannot re-assign $this."), confirmed directly.
                if matches!(&lhs.kind, ExprKind::Variable(v) if v == "this") {
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
                self.check_property_assignment(lhs, rhs, locals, label);
                self.bind_assignment_target(lhs, rhs_ty.clone(), locals);
                rhs_ty
            }
            ExprKind::CompoundAssign(op, lhs, rhs) => {
                // `$x ??= $default;` is defined as exactly
                // `$x = $x ?? $default;` - the same quiet-left-side
                // exemption as plain `??` applies to its own left side.
                let lhs_ty = if *op == BinOp::Coalesce {
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
                if *op != BinOp::Coalesce {
                    self.check_binary_operand_validity(op, &lhs_ty, &rhs_ty, true, lhs.span, label);
                    self.check_division_by_literal_zero(op, rhs, &rhs_ty, rhs.span, label);
                }
                if let ExprKind::Variable(name) = &lhs.kind {
                    let ty = self.infer_binary(op, lhs, rhs, locals);
                    purge_stale_narrowing_for(locals, name);
                    // Phase 17 follow-up: `$x += 1`/`$x ??= ...` is a
                    // real rebind of `$x`.
                    locals.declare(name.clone(), ty, lhs.span);
                }
                Type::Mixed
            }
            ExprKind::Ternary(cond, then_, else_) => {
                self.in_condition_position = true;
                self.check_expr(cond, locals, label);
                // Phase 15, `TernaryOperatorConstantConditionRule`: real
                // inferred-type trust, same design as `If`/`While`/
                // `DoWhile` (`checker/stmt.rs`'s own `check_if`) - no
                // exclusions beyond `inferred_constant_condition`'s own
                // shared ones, confirmed directly against `vendor/
                // phpstan-src`: this one real rule has none of `BooleanAnd`
                // /`BooleanOr`'s own `isLast`/first-level-statement
                // complexity.
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
                // Deliberately **not** writing `merge_scopes(&then_locals,
                // &else_locals)` back into the real `locals` afterward,
                // unlike `StmtKind::If` - a real, previously-uninvestigated
                // bug found via a Phase 12 corpus re-check traced this
                // exact write-back as the root cause of a genuine
                // divergence between `check_expr` and `infer_expr` (which
                // can't do this at all - it only ever receives `&Locals`,
                // never `&mut`) for the *same* expression tree.
                // Minimized repro: `$collected === [] ? null :
                // preg_replace('/.../', '', $collected[0])` - evaluating
                // the *condition's own true branch* narrows `$collected`
                // (declared `list<string>`) down to the literal empty
                // shape `array{}` (`narrow_type_to`'s own blind
                // `target.clone()` for a non-`Union` original, correct in
                // isolation: if `$collected` really does equal `[]`, its
                // type genuinely is the empty array). Writing that back
                // into the outer scope via `merge_scopes` then unioned it
                // with the else branch's own unnarrowed `list<string>`,
                // permanently corrupting `$collected`'s tracked type to
                // `array{}|list<string>` for anything checked afterward -
                // here, a *second*, separate evaluation of the very same
                // ternary (`check_property_assignment`'s own redundant
                // `infer_expr` call on the same `rhs`), where indexing
                // into the now-corrupted union's empty-shape member
                // produced `Mixed`, defeating `preg_replace`'s own
                // subject-type-dependent precision and leaking a spurious
                // `array<string>` into the property-assignment mismatch.
                // A ternary is an expression, not a control-flow
                // statement: narrowing *within* evaluating `then_`/`else_`
                // themselves (already achieved by `then_locals`/
                // `else_locals` below) is exactly what the comment above
                // is about and needs no further help: once the ternary as
                // a whole finishes evaluating, only its *value* matters to
                // whatever contains it, never which hypothetical branch's
                // narrowing might have applied - the outer `locals` should
                // come out exactly as they went in.
                let mut then_locals = locals.clone();
                self.apply_narrowing(cond, &mut then_locals, true);
                let then_ty = then_
                    .as_ref()
                    .map(|t| self.check_expr(t, &mut then_locals, label));
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.check_expr(else_, &mut else_locals, label);
                // Phase 23: `union_types`, not a raw `Type::Union(vec![...])`
                // - either branch's own type is very commonly already a
                // `Union` itself (e.g. a stub-declared `resource|false`),
                // and a bare `vec![then_ty, else_ty]` nests it unflattened
                // instead of merging its members in - a later `!== false`
                // narrowing filters only the *outer* union's members, so
                // it can strip a bare `false` sitting alongside the nested
                // union but can't reach inside that union to strip its own
                // `false` member. `union_types` already independently
                // implements the same "either side Mixed => Mixed" /
                // "equal sides collapse" rules this match used to
                // duplicate by hand, so this also simplifies the two arms
                // into one call.
                match then_ty {
                    Some(then_ty) => union_types(then_ty, else_ty),
                    None => Type::Mixed,
                }
            }
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    if let Some(k) = &item.key {
                        self.check_expr(k, locals, label);
                        self.check_array_item_key_type(k, locals, label);
                    }
                    self.check_expr(&item.value, locals, label);
                }
                self.check_array_literal_duplicate_keys(items, label);
                self.infer_array_literal_type(items, locals)
            }
            ExprKind::Index(base, idx) => {
                let base_ty = self.check_expr(base, locals, label);
                match idx {
                    Some(i) => {
                        self.check_expr(i, locals, label);
                        self.check_shape_key_access(base, i, locals, label);
                        self.check_dim_fetch_key_type(base, i, locals, label);
                        // Phase 32 Finding F (found while testing it): the
                        // `Index` counterpart of `PropertyAccess`'s own
                        // "check the narrowed key first" handling just
                        // above - `check_expr` (unlike `infer_expr`, which
                        // already does this for `Index`) went straight to
                        // re-deriving the type from the base's own
                        // declared/inferred shape, silently discarding
                        // whatever `apply_narrowing` had written under
                        // this exact `narrowing_key`. Real impact: a bare
                        // `return $x['k'];` right after narrowing
                        // `$x['k']` saw the un-narrowed type, while
                        // passing the same expression as a function
                        // argument (going through `infer_expr` for the
                        // argument-type comparison) already didn't.
                        if let Some(key) = narrowing_key(e)
                            && let Some(narrowed) = locals.get(&key)
                        {
                            return narrowed.clone();
                        }
                        match base_ty {
                            Type::Array(_, inner) => *inner,
                            Type::ArrayShape(fields) => literal_string_key(i)
                                .and_then(|key| {
                                    fields.into_iter().find(|(name, _, _)| *name == key)
                                })
                                .map(|(_, ty, _)| ty)
                                .unwrap_or(Type::Mixed),
                            _ => Type::Mixed,
                        }
                    }
                    None => Type::Mixed,
                }
            }
            ExprKind::New(_, args) => {
                // `new Foo()` never carries known type arguments itself -
                // real PHP's constructor syntax has no way to write
                // `new Foo<Bar>()` at all, and inferring them from the
                // constructor's own arguments (the way real PHPStan often
                // does) isn't attempted here, so this is always `Object`
                // with an empty arg list.
                let new_ty = self.infer_expr(e, locals);
                let resolved_class = match &new_ty {
                    Type::Object(class, _) => Some(class.clone()),
                    _ => None,
                };
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
                self.predefine_byref_or_unknown_args(None, ctor_sig, args, &resolved, locals);
                for (i, a) in args.iter().enumerate() {
                    let closure_this = ctor_sig
                        .and_then(|s| s.params.get(i))
                        .and_then(|p| p.closure_this_type.clone());
                    if let Some(this_ty) = closure_this
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                    {
                        // Phase 36: `@param-closure-this` - see
                        // `ExprKind::Call`'s own identical branch.
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
                    // `resolved_class` is already the fully-qualified name
                    // (`infer_expr`'s New arm resolves it via `name_ctx`),
                    // so every miss here is a real undefined-class finding
                    // - no "can't resolve a namespaced name" case left, the
                    // way there used to be before Phase 9.5's namespace
                    // support.
                    // Phase 67: `@internal` - called before the borrow
                    // just below (this check does its own, separate
                    // registry lookup) since it needs `&mut self`.
                    self.check_internal_instantiation(class, e.span, label);
                    // Phase 39 follow-up: `@deprecated`.
                    self.check_deprecated_instantiation(class, e.span, label);
                    // Phase 78: `DateInterval`'s own duration-string
                    // constructor validation - independent of whether
                    // the class is otherwise instantiable at all, so
                    // this runs unconditionally before that dispatch.
                    self.check_date_interval_constructor(class, args, e.span, label);
                    if let Some(info) = self.registry.classes.get(class) {
                        if info.kind == ClassKind::Interface {
                            self.report(
                                label.to_string(),
                                format!("cannot instantiate interface {class}"),
                                e.span,
                                0,
                                "new.interface",
                            );
                        } else if info.is_abstract {
                            self.report_builder(
                                DiagnosticBuilder::new(
                                    label.to_string(),
                                    format!("cannot instantiate abstract class {class}"),
                                    e.span,
                                    0,
                                    "new.abstract",
                                )
                                .tip(
                                    "either implement every inherited abstract method, or instantiate a concrete subclass instead",
                                ),
                            );
                        } else if let Some(sig) = ctor_sig {
                            // Finding 29 / Phase 49: see the identical
                            // comment on the plain-function Call arm
                            // above - `check_args` is spread-aware now,
                            // so it's always worth calling.
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
                }
                new_ty
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                // The anonymous class's own methods are still worth
                // checking, even though the class itself isn't indexed
                // into the registry under any stable name - check_function
                // only needs a FunctionDecl and a label to check against.
                //
                // Phase 90: `ancestor_sig` looks up the same method name
                // directly against whichever real, already-registered
                // interface/parent this anonymous class implements/
                // extends (`extends` first, matching `Registry::
                // lookup_ancestor_method_with_owner`'s own order) - real
                // corpus false positive this fixes: the extremely common
                // test-double shape (`new class implements
                // HtmlRenderingInterface { public function
                // nameCompare(array $a, array $b): int {...} }`, no own
                // docblock, relying entirely on the interface's own
                // `@param array<string, mixed>`) was never checked
                // against the interface's own docblock at all, since an
                // anonymous class's synthetic name is never indexed into
                // the registry for `check_function`'s own `sig` lookup to
                // find. Passed only as `check_missing_function_typehints`'s
                // own fallback (see `check_function_with_ancestor_hint`'s
                // doc comment for why it isn't substituted in more
                // broadly) - this project still doesn't otherwise type-
                // check an anonymous class's body against its own
                // implemented interfaces at all (a separate, larger gap).
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
                // Phase 37: its own properties'/consts' attributes too -
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
                Type::Mixed
            }
            ExprKind::StaticCall(class_name, method, args) => {
                // Mirrors ExprKind::MethodCall's own structure exactly -
                // resolve first (self/parent/static-aware), predefine
                // byref/unknown args using the real signature before
                // checking them, then the existence/arg-type check.
                let resolved_class = self.resolve_static_class_name(class_name);
                // Phase 57: `_with_owner`, not the plain `lookup_method`
                // this used to call - visibility needs the *declaring*
                // class, which can differ from `resolved_class` for an
                // inherited method (`sig` itself stays exactly what it
                // was before; only the extra `owner` half is new).
                let sig_with_owner = resolved_class
                    .as_ref()
                    .and_then(|class| self.registry.lookup_method_with_owner(class, method));
                let sig = sig_with_owner.as_ref().map(|(_, s)| *s);
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(None, sig, args, &resolved, locals);
                // Finding 49: `Closure::bind($closure, $newThis, $scope)`
                // makes `$this` (and, for private/protected access, the
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
                        .unwrap_or(Type::Mixed);
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
                    }
                } else {
                    for (i, a) in args.iter().enumerate() {
                        let closure_this = sig
                            .and_then(|s| s.params.get(i))
                            .and_then(|p| p.closure_this_type.clone());
                        if let Some(this_ty) = closure_this
                            && matches!(
                                unwrap_grouping(&a.value).kind,
                                ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                            )
                        {
                            // Phase 36: `@param-closure-this` - see
                            // `ExprKind::Call`'s own identical branch.
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
                    if let Some(sig) = sig {
                        // Phase 57: visibility.
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
                            // Phase 67: `@internal`.
                            self.check_internal_method_call(
                                owner, method, sig, true, e.span, label,
                            );
                            // Phase 39: `@deprecated`.
                            self.check_deprecated_method_call(
                                owner, method, sig, true, e.span, label,
                            );
                            // Phase 57 follow-up:
                            // `CallPrivateMethodThroughStaticRule` - same
                            // late-static-binding unsafety as the
                            // identical class-constant/static-property
                            // rules.
                            if class_name == "static" && sig.visibility == Visibility::Private {
                                self.check_static_private_method_access(
                                    owner, method, e.span, label,
                                );
                            }
                        }
                        // Finding 29 / Phase 49: see the identical
                        // comment on the plain-function Call arm above -
                        // `check_args` is spread-aware now, so it's
                        // always worth calling.
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
                    } else if self.registry.classes.contains_key(class)
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
                    }
                }
                match (&resolved_class, sig) {
                    (Some(class), Some(s)) => {
                        let resolved_ty = self.infer_call_return_type(s, &resolved, locals);
                        let resolved_ty =
                            self.narrow_enum_from_call(class, method, args, resolved_ty, locals);
                        self.resolve_pseudo(&resolved_ty, class)
                    }
                    _ => Type::Mixed,
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
                    return Type::Mixed;
                }
                // A static property (`self::$prop`) can be narrowed the
                // same way an instance property already is - check for a
                // locally-narrowed override before the undefined-property
                // check/registry lookup below.
                if member.starts_with('$')
                    && let Some(narrowed) = locals.get(&format!("{class_name}::{member}"))
                {
                    return narrowed.clone();
                }
                let Some(class) = self.resolve_static_class_name(class_name) else {
                    return Type::Mixed;
                };
                if !self.registry.classes.contains_key(&class) {
                    return Type::Mixed;
                }
                if let Some(prop_name) = member.strip_prefix('$') {
                    // Unlike an instance property, PHP has no legal way
                    // to dynamically create a static property at all -
                    // no "could be legitimate" case to stay lenient for
                    // here, the way check_property_assignment's write
                    // side has to for instance properties. Finding 9: a
                    // property that exists but was declared as an
                    // ordinary *instance* property is equally invalid
                    // through `::` in real PHP ("Access to undeclared
                    // static property") - from the caller's own
                    // perspective there's no real static property by
                    // this name either way, so both share one message.
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
                        // Phase 67: `@internal`.
                        self.check_internal_property_access(owner, prop_name, sig, e.span, label);
                        // Phase 39 follow-up: `@deprecated`.
                        self.check_deprecated_property_access(owner, prop_name, sig, e.span, label);
                        // Phase 57 follow-up: a static property shares
                        // the exact same visibility rules as an instance
                        // one - confirmed directly via `php -r` as a real
                        // "Cannot access private property" fatal error.
                        self.check_member_visibility(MemberVisibilityCheck {
                            kind: "staticProperty",
                            owner,
                            member_name: prop_name,
                            visibility: &sig.visibility,
                            describe: &format!("static property {owner}::${prop_name}"),
                            span: e.span,
                            label,
                        });
                        // Phase 57 follow-up:
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
                        .unwrap_or(Type::Mixed)
                } else {
                    let const_with_owner = self.registry.lookup_const_with_owner(&class, member);
                    let const_ty = const_with_owner.as_ref().map(|(_, cs)| cs.ty.clone());
                    if const_ty.is_none() {
                        self.report(
                            label.to_string(),
                            format!("access to undefined constant {class}::{member}"),
                            e.span,
                            0,
                            "classConstant.notFound",
                        );
                    } else if let Some((owner, cs)) = &const_with_owner {
                        // Phase 67: `@internal`.
                        self.check_internal_class_constant_access(owner, member, cs, e.span, label);
                        // Phase 39 follow-up: `@deprecated`.
                        self.check_deprecated_class_constant_access(
                            owner, member, cs, e.span, label,
                        );
                        // Phase 57's own still-open item: constant
                        // visibility.
                        self.check_member_visibility(MemberVisibilityCheck {
                            kind: "classConstant",
                            owner,
                            member_name: member,
                            visibility: &cs.visibility,
                            describe: &format!("constant {owner}::{member}"),
                            span: e.span,
                            label,
                        });
                        // Phase 57 follow-up: `AccessPrivateConstantThroughStaticRule`.
                        if class_name == "static" && cs.visibility == Visibility::Private {
                            self.check_static_private_constant_access(owner, member, e.span, label);
                        }
                    }
                    const_ty
                        .map(|ty| self.resolve_pseudo(&ty, &class))
                        .unwrap_or(Type::Mixed)
                }
            }
            ExprKind::DynamicVariable(name_expr) => {
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                // Unlike a *write* (check_property_assignment, and the
                // Assign/ArrayLiteral-destructuring arms below that
                // deliberately route a PropertyAccess target through only
                // its base, not here) - a genuinely undeclared property
                // creates nothing on read, it just returns null with a
                // warning in real PHP (unless __get exists). So there's
                // no "could be a legitimate dynamic property" case to
                // stay lenient for here, the way there is on write.
                // Same `Union`/`Intersection` handling as `MethodCall`
                // just above, and the identical reasoning for why: a
                // multi-type `catch` variable, or an ordinarily
                // union-typed parameter, used to skip this check
                // entirely rather than fall to `_ => {}`.
                let base_ty = self.check_expr(base, locals, label);
                // Phase 17 Stage B: see `probe_span`'s own doc comment.
                if self.probe_span == Some(base.span) {
                    self.probe_hit = Some(base_ty.clone());
                }
                // Phase 60: `$x?->prop` is pointless once `$x`'s own type
                // already proves it's never null - mirrors real PHPStan's
                // `NullsafePropertyFetchRule` exactly (same identifier,
                // same "only when definitely never null, not when
                // definitely always null either" split). Guarded by
                // `loop_depth`/`static_var_names` - see the identical
                // guard on the `MethodCall` arm above for why.
                if *nullsafe
                    && self.loop_depth == 0
                    && !matches!(&base.kind, ExprKind::Variable(n) if self.static_var_names.contains(n))
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
                // Phase 22 Finding 1: a Pest `beforeEach(...)` closure's own
                // `$this->prop = ...` legitimately defines a dynamic
                // property every other Pest closure in the file/directory
                // scope can read, real PHPUnit\Framework\TestCase
                // declaration or not - see `pest_hook_properties`'s own
                // doc comment. Keyed off the literal `$this` expression
                // (unlike the real `pestphp/pest-plugin-phpstan`
                // extension, which resolves by the base's type) - a
                // disclosed, evidence-free simplification.
                let is_pest_this = matches!(&base.kind, ExprKind::Variable(name) if name == "this");
                // Phase 34 Finding D: `stdClass` (a real, registered,
                // zero-property class in the vendored stubs -
                // `Core/Core_c.php`'s own `class stdClass {}`) is PHP's
                // canonical "arbitrary property bag" - real PHP (and
                // real PHPStan) never flag a dynamic property read or
                // write on it, by design. Scoped to *this* property-
                // access call site only, not `member_confirmed_missing`
                // itself, which two method-call checks below also share
                // - an undefined *method* call on `stdClass` is still a
                // real fatal error real PHPStan also flags.
                if self.member_confirmed_missing(&base_ty, |c| {
                    c == "stdClass"
                        || self.registry.lookup_property(c, prop).is_some()
                        || self.registry.lookup_method(c, "__get").is_some()
                        || (is_pest_this && self.pest_hook_properties.contains(prop.as_str()))
                }) {
                    self.report(
                        label.to_string(),
                        format!("access to undefined property {base_ty}::${prop}"),
                        e.span,
                        2,
                        "property.notFound",
                    );
                }
                // Phase 57: visibility - only meaningful once the
                // property is confirmed to exist at all (the block just
                // above already handles "doesn't exist"). Real bug found
                // and fixed this session: this used to match `Type::
                // Object` directly, with no `Union`/`Intersection` case at
                // all, silently skipping the check entirely for either
                // shape. Deliberately *not* the same "just the first
                // Object/EnumCase member, regardless of whether it
                // actually has the property" shortcut `MethodCall`'s own
                // `recv_class` extraction settles for a few hundred lines
                // up: for an `Intersection` specifically, the property
                // can easily live on a *different* member than whichever
                // happens to come first (`View&Foo` where `View` is an
                // unrelated interface and `$secret` is `Foo`'s own private
                // property - the first-found `View` would never have it,
                // silently skipping the check for exactly the shape this
                // fix exists to catch). `find_property_owner` recurses
                // through both `Union` and `Intersection`, keeping the
                // first member that actually declares it.
                let owner_and_sig = self
                    .find_property_owner(&narrow_type_excluding(&base_ty, &Type::Null), prop)
                    .map(|(owner, p)| (owner, p.clone()));
                if let Some((owner, p)) = &owner_and_sig {
                    let visibility = p.visibility.clone();
                    self.check_member_visibility(MemberVisibilityCheck {
                        kind: "property",
                        owner,
                        member_name: prop,
                        visibility: &visibility,
                        describe: &format!("property {owner}::${prop}"),
                        span: e.span,
                        label,
                    });
                    // Phase 67: `@internal`.
                    self.check_internal_property_access(owner, prop, p, e.span, label);
                    // Phase 39 follow-up: `@deprecated`.
                    self.check_deprecated_property_access(owner, prop, p, e.span, label);
                }
                if let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                // Phase 79/80's own `EnumCase`/`ObjectShape` precision and
                // the general `Object` lookup all now live in
                // `property_access_type` (shared with `infer_expr`'s own
                // identical arm) - see its own doc comment for the real
                // bug this factoring-out fixed (`Union`/`Intersection`
                // bases silently falling to `Mixed`).
                let result =
                    self.property_access_type(&narrow_type_excluding(&base_ty, &Type::Null), prop);
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            ExprKind::DynamicPropertyAccess(base, name_expr) => {
                self.check_expr(base, locals, label);
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            ExprKind::DynamicMethodCall(recv, name_expr, args) => {
                self.check_expr(recv, locals, label);
                self.check_expr(name_expr, locals, label);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                Type::Mixed
            }
            ExprKind::Interpolated(parts) => {
                // Real PHPStan's own `InvalidPartOfEncapsedStringRule`
                // (`encapsedStringPart.nonString`) - an interpolated
                // `"{$x}"` part implicitly casts `$x` to string exactly
                // like an explicit `(string)`/`echo`/`print` would (down
                // to triggering the same `__toString()` call), so an
                // array or an object with no usable `__toString()` there
                // is a real, confirmed PHP fatal error ("Array to string
                // conversion" / "Object of class X could not be
                // converted to string"), not just a style nit. A raw
                // literal text chunk between substitutions is itself
                // modeled as a synthetic `Literal(String(_))` part
                // (`build_interpolated`'s own doc comment) - skipped
                // here the same way real PHPStan's own rule skips a
                // plain `InterpolatedStringPart`, since it was never a
                // real expression to begin with.
                //
                // One known, deliberate departure from real PHPStan's
                // own message text: its wording embeds the part's
                // original PHP source (`Part $obj->prop (Foo) of...`),
                // printed via its own dedicated `ExprPrinter` - this
                // project has no general expression-to-source printer to
                // match that with, so the message here names only the
                // type, not the source snippet.
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
                // always safely assume every earlier arm's own condition
                // was false. `match(true) { $cond => ... }` is a common,
                // deliberate `switch`-replacement idiom specifically
                // because each arm's own condition is checked with `===`
                // - treat the arm's condition itself as an `if`-style
                // condition in that case; otherwise narrow via an
                // implicit `subject === cond` equality, the same as an
                // ordinary `switch` case. Only attempted for a
                // single-condition arm (`1, 2 => ...` multi-value arms
                // fall back to unnarrowed, rather than guessing at a
                // combined "or" narrowing).
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut merged: Option<Locals> = None;
                let mut result_types: Vec<Type> = Vec::new();
                for (conds, result) in arms {
                    for c in conds {
                        self.check_expr(c, locals, label);
                    }
                    let mut arm_locals = accumulated.clone();
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut arm_locals, true);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut arm_locals,
                                true,
                            );
                        }
                    }
                    result_types.push(self.check_expr(result, &mut arm_locals, label));
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&m, &arm_locals),
                        None => arm_locals.clone(),
                    });
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
                // applies.
                if result_types.contains(&Type::Mixed) {
                    Type::Mixed
                } else {
                    let mut deduped: Vec<Type> = Vec::new();
                    for t in result_types {
                        if !deduped.contains(&t) {
                            deduped.push(t);
                        }
                    }
                    match deduped.len() {
                        0 => Type::Never,
                        1 => deduped.into_iter().next().unwrap(),
                        _ => Type::Union(deduped),
                    }
                }
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
                // Phase 37: `ClosureAttributesRule` - existence (already
                // checked for every other attribute-bearing declaration)
                // plus target-validity, both missing entirely for a
                // closure expression's own `#[Attr]` group until now (a
                // real, previously-undetected gap: this arm never called
                // `check_attributes` at all, existence-only or otherwise).
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
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    // Phase 17 follow-up: a captured variable's own real
                    // declaration is wherever the *outer* scope already
                    // says it is - copied across, not fabricated fresh at
                    // this `use (...)` clause.
                    let provenance = locals.get_provenance(&u.name).cloned().unwrap_or_default();
                    closure_locals.insert_with_provenance(u.name.clone(), ty, provenance);
                }
                if !is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                let closure_label = format!("{label} (closure)");
                for p in params {
                    let ty = self.resolve_type_hint(p.type_hint.as_deref());
                    // Phase 48: `ExistingClassesInClosureTypehintsRule`'s
                    // own check - checked on the un-array-wrapped element
                    // type below (a variadic parameter's own declared
                    // type, not the `array` it collects into) so a
                    // missing class inside e.g. `...Foo $items` is still
                    // reported by its own real name, not `array`.
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        &closure_label,
                    );
                    let ty = if p.variadic {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
                    } else {
                        ty
                    };
                    closure_locals.declare(p.name.clone(), ty, p.name_span);
                }
                let declared_return = Some(self.resolve_type_hint(return_type.as_deref()));
                self.check_type_class_names_exist(
                    declared_return.as_ref().unwrap_or(&Type::Mixed),
                    "return type of anonymous function",
                    e.span,
                    &closure_label,
                );
                // Finding 3: a generator-returning closure (`function ():
                // Generator { if ($done) { return; } yield $x; }`, a
                // plausible callback shape) needs the same
                // current_function_is_generator save/set/restore
                // check_function does for a named function/method -
                // check_block never routes through check_function at all.
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = body_contains_yield(body);
                let previous_goto_targets = std::mem::replace(
                    &mut self.current_function_goto_targets,
                    collect_goto_targets(body),
                );
                self.check_generator_return_type(declared_return.as_ref(), e.span, &closure_label);
                self.check_block(
                    body,
                    &mut closure_locals,
                    declared_return.as_ref(),
                    &closure_label,
                );
                self.current_function_is_generator = previous_generator_flag;
                self.current_function_goto_targets = previous_goto_targets;
                // Phase 59: a `use(...)` capture never referenced anywhere
                // in the closure's own body it was just checked against
                // above. `ClosureUse` carries no span of its own (see its
                // own doc comment), so every finding here anchors to the
                // whole closure expression's span instead of the specific
                // `$x` token.
                self.check_unused_closure_uses(uses, body, &closure_label, e.span);
                // A `use (&$x)` capture is a real reference - any
                // mutation the closure body makes to `$x` is visible in
                // the *declaring* scope too, from the closure literal's
                // own point onward (conservatively: real PHP doesn't
                // guarantee the closure is ever actually called, or
                // called only once, so this widens rather than replaces
                // outright - `union_types` with the pre-closure type
                // covers "never called" the same way an `if` without an
                // `else` already does for its own branch). A real
                // Piwigo-corpus round found this missing entirely
                // (`$seen = []; $client = new MockHttpClient(function (...)
                // use (&$seen) { $seen[] = [...]; }); ...; $seen[0]`
                // - a real, common test-double-recording idiom) - without
                // it, every by-ref-captured variable's outer type stayed
                // frozen at whatever it was *before* the closure literal,
                // regardless of what the closure body's own checked
                // effects on `closure_locals` actually were.
                for u in uses {
                    if !u.by_ref {
                        continue;
                    }
                    let mutated = closure_locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    if !locals.contains_key(&u.name) {
                        // Phase 17: real PHP auto-vivifies `$x` in the
                        // outer/declaring scope the moment `use (&$x)`
                        // captures an as-yet-undefined variable - the same
                        // by-ref-argument auto-vivification this project
                        // already models in
                        // `predefine_byref_or_unknown_args`, now given a
                        // real, navigable declaration site instead of
                        // silently gaining a value with no provenance at
                        // all.
                        locals.declare(u.name.clone(), mutated, u.span);
                    } else {
                        let original = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                        locals.insert(u.name.clone(), union_types(original, mutated));
                    }
                }
                Type::Object("Closure".to_string(), Vec::new())
            }
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                attrs,
                ..
            } => {
                // Phase 37: `ArrowFunctionAttributesRule` - see the
                // `Closure` arm's own identical comment just above.
                self.check_attributes(attrs, label);
                self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_FUNCTION, "function", label);
                // An arrow function DOES automatically capture the whole
                // enclosing scope by value (implicit, no `use (...)`
                // needed) - a plain clone of `locals` is exactly that.
                let mut arrow_locals = locals.clone();
                for p in params {
                    let ty = self.resolve_type_hint(p.type_hint.as_deref());
                    // Phase 48:
                    // `ExistingClassesInArrowFunctionTypehintsRule`'s own
                    // check - same "check the element type, not the
                    // variadic-collected `array`" reasoning as the
                    // Closure arm above.
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of arrow function", p.name),
                        p.span,
                        label,
                    );
                    let ty = if p.variadic {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
                    } else {
                        ty
                    };
                    arrow_locals.declare(p.name.clone(), ty, p.name_span);
                }
                // Phase 70: an arrow function's own body is a single
                // expression, not a `Vec<Stmt>` `body_contains_yield`
                // can walk - `expr_contains_yield` is that same walk's
                // expression-only counterpart. Same save/set/restore
                // `check_function`/the Closure literal arms already do:
                // `check_expr` never routes through `check_function`
                // either, so without this, checking a generator arrow
                // function here would leak the flag into whatever comes
                // after it in the *enclosing* scope's own checking pass.
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = expr_contains_yield(body);
                let declared_return = return_type
                    .as_deref()
                    .map(|_| self.resolve_type_hint(return_type.as_deref()));
                self.check_generator_return_type(declared_return.as_ref(), e.span, label);
                let body_ty = self.check_expr(body, &mut arrow_locals, label);
                self.current_function_is_generator = previous_generator_flag;
                // Its body is a single implicit-return expression, not a
                // StmtKind::Return - the equivalent of check_stmt's own
                // Return-vs-declared_return check, kept here since
                // there's no StmtKind::Return for it to share.
                if return_type.is_some() {
                    let expected = self.resolve_type_hint(return_type.as_deref());
                    self.check_type_class_names_exist(
                        &expected,
                        "return type of arrow function",
                        e.span,
                        label,
                    );
                    if body_ty != Type::Mixed && !self.registry.is_assignable(&body_ty, &expected) {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got {body_ty}"),
                            body.span,
                            3,
                            "return.type",
                        );
                    }
                }
                Type::Object("Closure".to_string(), Vec::new())
            }
            // Finding 28: `foo(...)`/`$obj->method(...)`/`Class::method(...)`
            // (PHP 8.1 first-class-callable syntax) always wraps a
            // Call/MethodCall/StaticCall/DynamicMethodCall with an empty
            // argument list (this project's own parser convention) - not
            // dispatching to that inner node's own ordinary arm directly:
            // its own `check_args` would misread "no arguments were
            // written" as "zero arguments were passed to this call,"
            // producing a false "expects at least N argument(s)"
            // diagnostic for any real function/method with required
            // parameters. So this checks only existence - the one thing
            // that doesn't depend on argument count at all - mirroring
            // each inner arm's own existence-check logic without its
            // argument handling.
            //
            // Also runs visibility/`@internal`/`@deprecated` (real
            // PHPStan's own version, per its own `StaticMethodCallableRule`,
            // just re-runs the *ordinary* call-site checks against this
            // same shape) - a real, previously-undetected gap: `Foo::
            // method(...)`/`$obj->method(...)` used to run none of them,
            // even though both produce a real, callable `Closure` bound to
            // that exact method the same as an ordinary call would.
            ExprKind::FirstClassCallable(inner) => {
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
                                        self.check_deprecated_function_call(
                                            &resolved_name,
                                            sig,
                                            e.span,
                                            label,
                                        );
                                    }
                                    None => {
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
                            && self.registry.classes.contains_key(class)
                        {
                            if self.member_confirmed_missing(&recv_ty, |c| {
                                self.registry.lookup_method(c, method).is_some()
                                    || self.registry.lookup_method(c, "__call").is_some()
                            }) {
                                self.report(
                                    label.to_string(),
                                    format!("call to undefined method {recv_ty}::{method}()"),
                                    e.span,
                                    0,
                                    "method.notFound",
                                );
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
                                self.check_deprecated_method_call(
                                    &owner, method, sig, false, e.span, label,
                                );
                            }
                        }
                    }
                    ExprKind::StaticCall(class_name, method, _) => {
                        if let Some(class) = self.resolve_static_class_name(class_name)
                            && self.registry.classes.contains_key(&class)
                        {
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
                                self.check_deprecated_method_call(
                                    &owner, method, sig, true, e.span, label,
                                );
                                if class_name == "static" && sig.visibility == Visibility::Private {
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
                    _ => {
                        self.check_expr(inner, locals, label);
                    }
                }
                Type::Object("Closure".to_string(), Vec::new())
            }
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.clone()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            ExprKind::Name(name) => self
                .global_constant_type(name)
                .unwrap_or_else(|| bare_name_literal_type(name)),
        }
    }

    pub(crate) fn infer_expr(&self, e: &Expr, locals: &Locals) -> Type {
        match &e.kind {
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.clone()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            ExprKind::Variable(name) => locals
                .get(name)
                .cloned()
                .unwrap_or_else(|| superglobal_default_type(name)),
            ExprKind::Grouping(inner) | ExprKind::Clone(inner, _) => self.infer_expr(inner, locals),
            // A closure/arrow-fn literal, and first-class-callable syntax
            // (`strlen(...)`, `$obj->method(...)`), all produce a real
            // `Closure` instance at runtime in PHP - infer the same
            // `Object("Closure", ...)` the native `Closure` type hint
            // itself now resolves to (rather than the broader
            // `Type::Callable`, which `callable`-hinted parameters
            // accept via `is_assignable`'s own separate leniency), so a
            // strictly `Closure`-typed parameter correctly accepts them.
            ExprKind::FirstClassCallable(_)
            | ExprKind::Closure { .. }
            | ExprKind::ArrowFn { .. } => Type::Object("Closure".to_string(), Vec::new()),
            ExprKind::Match {
                subject,
                arms,
                default,
            } => {
                // Same "don't let an unknown branch compose into a
                // partial Union that looks more precise than it is" rule
                // Ternary already applies, generalized to N arms plus an
                // optional default - and the same per-arm narrowing
                // `check_expr`'s own Match handling applies, so a
                // `default` arm returning the match's own subject infers
                // its real, narrowed type rather than the subject's
                // unnarrowed original type.
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut result_types: Vec<Type> = Vec::new();
                for (conds, result) in arms {
                    let mut arm_locals = accumulated.clone();
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut arm_locals, true);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut arm_locals,
                                true,
                            );
                        }
                    }
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
                if result_types.contains(&Type::Mixed) {
                    Type::Mixed
                } else {
                    let mut deduped: Vec<Type> = Vec::new();
                    for t in result_types {
                        if !deduped.contains(&t) {
                            deduped.push(t);
                        }
                    }
                    match deduped.len() {
                        0 => Type::Never,
                        1 => deduped.into_iter().next().unwrap(),
                        _ => Type::Union(deduped),
                    }
                }
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used for
            // a function that always throws or exits.
            ExprKind::Throw(_) => Type::Never,
            // Never given a stable nominal type, since multiple such
            // expressions in the same file could share the placeholder
            // name - Mixed is the honest answer, not an oversight.
            ExprKind::NewAnonymousClass(..) => Type::Mixed,
            ExprKind::Cast(ty, _) => match ty.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "string" => Type::String,
                "bool" => Type::Bool,
                "array" => Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
                "object" => Type::Object("object".to_string(), Vec::new()),
                "unset" => Type::Null,
                _ => Type::Mixed,
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
            ExprKind::Assign(_, rhs) => self.infer_expr(rhs, locals),
            // `self`/`parent`/`static` aren't lexed as keywords (see the
            // parser: they're indistinguishable from any other identifier),
            // so a literal `ExprKind::Name("self")` has to be special-cased
            // here rather than showing up as an Object of that name.
            ExprKind::New(class_expr, _) => match &class_expr.kind {
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
                    _ => Type::Object(self.name_ctx.resolve(name), Vec::new()),
                },
                _ => Type::Mixed,
            },
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if !is_language_construct(name) => {
                    let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                    self.registry
                        .lookup_function(&primary)
                        .map(|sig| (primary.as_str(), sig))
                        .or_else(|| {
                            fallback
                                .as_deref()
                                .and_then(|g| self.registry.lookup_function(g).map(|sig| (g, sig)))
                        })
                        .map(|(resolved_name, sig)| {
                            self.infer_builtin_call_return_type(resolved_name, sig, args, locals)
                        })
                        .unwrap_or_else(|| call_fallback_return_type(callee))
                }
                _ => call_fallback_return_type(callee),
            },
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
                // A nullsafe receiver is virtually always nullable
                // (that's the entire reason to write `?->`), so its own
                // inferred type is virtually always `Union([Object(...),
                // Null])`, not a bare `Object` - strip `Null` out before
                // matching, the same real class the method is actually
                // being looked up on either way.
                let recv_ty = narrow_type_excluding(&self.infer_expr(recv, locals), &Type::Null);
                let result = self.method_call_return_type(&recv_ty, method, args, locals);
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            ExprKind::StaticCall(class_name, method, args) => self
                .resolve_static_class_name(class_name)
                .and_then(|class| {
                    self.registry.lookup_method(&class, method).map(|sig| {
                        let resolved_args = resolve_call_args(args, &sig.params);
                        let resolved = self.infer_call_return_type(sig, &resolved_args, locals);
                        let resolved =
                            self.narrow_enum_from_call(&class, method, args, resolved, locals);
                        self.resolve_pseudo(&resolved, &class)
                    })
                })
                .unwrap_or(Type::Mixed),
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
                    && let Some(narrowed) = locals.get(&format!("{class_name}::{member}"))
                {
                    return narrowed.clone();
                }
                self.resolve_static_class_name(class_name)
                    .and_then(|class| match member.strip_prefix('$') {
                        Some(prop_name) => self
                            .registry
                            .lookup_property(&class, prop_name)
                            .map(|p| self.resolve_pseudo(&p.ty, &class)),
                        None => self
                            .registry
                            .lookup_const(&class, member)
                            .map(|cs| self.resolve_pseudo(&cs.ty, &class)),
                    })
                    .unwrap_or(Type::Mixed)
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
                        // Phase 23: see the parallel arm in `check_expr`
                        // for why this must be `union_types`, not a raw
                        // `Type::Union(vec![...])`.
                        union_types(then_ty, else_ty)
                    }
                    None => Type::Mixed,
                }
            }
            ExprKind::ArrayLiteral(items) => self.infer_array_literal_type(items, locals),
            // Same reasoning as `BinOp::Concat`'s own identical check
            // just below - real corpus false positive this fix closes:
            // an interpolated string with a provably non-falsy literal
            // segment (`"prefix-{$suffix}"`) always produces a real
            // `non-falsy-string`, the same way `'prefix-' . $suffix`
            // already did, but this arm unconditionally returned bare
            // `Type::String` regardless of any part's own content -
            // `check_expr`'s own separate `Interpolated` arm already
            // walks each part for a *different* purpose (implicit
            // `__toString()`/array-to-string checks), not reused here
            // since it doesn't compute or return a refined type.
            ExprKind::Interpolated(parts) => {
                if parts
                    .iter()
                    .any(|p| is_non_falsy_string(&self.infer_expr(p, locals)))
                {
                    Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::NonFalsyString),
                    ])
                } else {
                    Type::String
                }
            }
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                if let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                let base_ty = narrow_type_excluding(&self.infer_expr(base, locals), &Type::Null);
                // See `property_access_type`'s own doc comment - shared
                // with `check_expr`'s identical arm.
                let result = self.property_access_type(&base_ty, prop);
                // Same implicit nullability as `?->method()` just above,
                // for `$x?->prop`.
                if *nullsafe && result != Type::Mixed {
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
                // Phase 22: the `Index` counterpart of `PropertyAccess`'s
                // own "check the narrowed key first" fast path just above
                // - without this, `apply_narrowing` faithfully *wrote* a
                // narrowed type under `narrowing_key`'s `"base[key]"`
                // format (e.g. after `is_string($info['code'])`), but
                // nothing ever *read* it back: this arm went straight to
                // re-deriving the type from `$info`'s own declared/
                // inferred shape, silently discarding the narrowing.
                if let Some(key) = narrowing_key(e)
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
            _ => Type::Mixed,
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
    /// returns `None`, falling back to the caller's own generic
    /// (imprecise but correct) behavior. Mirrors the closure-capture-
    /// scope construction `check_expr`'s own `ExprKind::Closure`/
    /// `ExprKind::ArrowFn` arms already do, but on this separate,
    /// read-only inference pass rather than the diagnostic-producing one.
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
                if return_type.is_some() {
                    return Some(self.resolve_type_hint(return_type.as_deref()));
                }
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut arrow_locals = locals.clone();
                for (p, ty) in params.iter().zip(param_overrides) {
                    arrow_locals.insert(p.name.clone(), ty.clone());
                }
                Some(self.infer_expr(body, &arrow_locals))
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                if return_type.is_some() {
                    return Some(self.resolve_type_hint(return_type.as_deref()));
                }
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut closure_locals: Locals = Locals::default();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    closure_locals.insert(u.name.clone(), ty);
                }
                if !*is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                for (p, ty) in params.iter().zip(param_overrides) {
                    closure_locals.insert(p.name.clone(), ty.clone());
                }
                // Only the common single-implicit-return-statement body
                // shape is handled (mirrors check_shape_key_access's own
                // "only handle the shape that actually comes up, don't
                // guess at the rest" precedent) - a closure with a more
                // complex body safely falls back to None too.
                match body.as_slice() {
                    [
                        Stmt {
                            kind: StmtKind::Return(Some(e)),
                            ..
                        },
                    ] => Some(self.infer_expr(e, &closure_locals)),
                    _ => None,
                }
            }
            // A round of Piwigo-corpus bucketing found a first-class
            // callable (`intval(...)`, `TagId::from(...)`, PHP 8.1) used
            // as `array_map`'s own callback fell to this function's own
            // `_ => None` fallback, discarding every bit of precision
            // (both list-ness/non-emptiness *and* the mapped element
            // type) the array_map special case above this one otherwise
            // provides - an extremely common idiom in this corpus (over
            // 140 first-party call sites: `array_map(intval(...), ...)`
            // alone is 76). Resolves the wrapped call's own signature and
            // binds `param_overrides` against its declared parameter
            // types the same way `infer_call_return_type` already does
            // for an ordinary call, just without real argument
            // expressions to read types from.
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
                    ExprKind::StaticCall(class_name, method, _) => {
                        match self.resolve_static_class_name(class_name) {
                            Some(class) => {
                                let sig = self.registry.lookup_method(&class, method);
                                (sig, Some(class))
                            }
                            None => (None, None),
                        }
                    }
                    ExprKind::MethodCall(recv, method, _, _) => {
                        let recv_ty = self.infer_expr(recv, locals);
                        self.find_method_receiver_class(&recv_ty, method)
                    }
                    _ => (None, None),
                };
                sig.map(|s| {
                    let mut bindings: TemplateBindings = TemplateBindings::default();
                    for (param, ty) in s.params.iter().zip(param_overrides) {
                        types::bind_template_param(&param.ty, ty, &mut bindings);
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

    /// Phase 22 Finding 4: `??`'s result is *usually* just `r`'s own
    /// type (`ExprKind::Binary(BinOp::Coalesce, ...)` and this function's
    /// own `BinOp::Coalesce` arm both start from `r_ty` alone) - widening
    /// away from that is deliberately narrow, not a blanket "always union
    /// `non_null(l)` with `r`" fix: that would pollute the extremely
    /// common and currently-precise `$x ?? 'literal'` pattern (where
    /// `$x`'s real type is often genuinely `Mixed`) with unwanted `Mixed`
    /// noise across a much broader, currently-well-served set of call
    /// sites. Only widens when `l`'s real non-null type is `Type::Array`
    /// (generic/unshaped) and `r`'s own type is a narrower
    /// `Type::ArrayShape` - `$details = $row->details ?? [];` where
    /// `$row->details: ?array` genuinely holds arbitrary keys at runtime
    /// collapsed to the literal empty `[]` shape from the fallback alone,
    /// so every later `$details['key']` read looked like a typo.
    /// Deliberately not covering the sibling cases disclosed in
    /// ROADMAP.md's own Phase 22 write-up (`l`'s non-null type being a
    /// *different* non-empty `ArrayShape`, or `r` being a `Union`
    /// containing one) - no corpus evidence either occurs, same class of
    /// bug, left for a future pass if one turns up.
    fn widen_coalesce_result(&self, l: &Expr, r_ty: Type, locals: &Locals) -> Type {
        // `$x ?? null` is a real, if unusual, no-op: if `$x` is null the
        // result is null either way, and otherwise the result is
        // exactly `$x` itself - so its own type is exactly `$x`'s own
        // type, unnarrowed, not the literal `r_ty` (`Type::Null`) this
        // function otherwise trusts as the fallback's own type. A round
        // of Piwigo-corpus bucketing found this collapsing an
        // unshaped-array element read (`$post['selection'] ?? null`,
        // `$post: array` with no shape, so `Mixed`) all the way down to
        // bare `Type::Null` - real corpus code checks the *original*
        // expression with `is_array($selection_raw)` immediately after,
        // which should still be checking a real `Mixed`, not a value
        // this checker had already convinced itself could only be null.
        if r_ty == Type::Null {
            let l_ty = self.infer_expr(l, locals);
            // Phase 94: the no-op reasoning above only actually holds
            // when `l` is a bare variable - `locals`'s own tracked type
            // for one already reflects every way it can be null (a
            // prior narrowing, an untyped param, ...), so trusting it
            // completely is correct. An array/property access (or any
            // other expression `??` exists specifically *because* this
            // checker can't otherwise prove is always set) is different:
            // `infer_expr` for `$group[$index]` on an `array<int,
            // string>`-typed `$group` returns plain `string`, with no
            // notion of "the key might not exist" folded in at all - the
            // exact real-PHP case `?? null` idiomatically guards
            // against. Real corpus false positive: `$val = $group[$index]
            // ?? null; return $val ?? '';` - `$val`'s own stored type
            // came out non-nullable, so the second, genuinely-needed
            // `?? ''` was flagged as checking an already-non-nullable
            // variable. Unioning `Null` back in here is always safe for
            // the already-covered `Mixed` case too (`union_types` already
            // collapses `Mixed | Null` back to plain `Mixed`).
            return if matches!(l.kind, ExprKind::Variable(_)) {
                l_ty
            } else {
                union_types(l_ty, Type::Null)
            };
        }
        let l_ty = narrow_type_excluding(&self.infer_expr(l, locals), &Type::Null);
        // `$x ?? throw new Foo();` (or any other never-returning right
        // side: `exit`, a `never`-typed call) - a real, corpus-evidenced
        // idiom (`revolt/event-loop`'s own `AbstractDriver::enable()`/
        // `isEnabled()`/`isReferenced()`, `latte/latte`'s own parser,
        // several others). Found the hard way: falling through to the
        // generic `_ => r_ty` fallback below made the *whole* expression
        // - and by extension any `$x = ... ?? throw ...;` assignment
        // statement built from it - infer as `Type::Never` itself,
        // which `always_terminates`'s own `StmtKind::Expression` case
        // (`self.infer_expr(e, locals) == Type::Never`) then read as
        // "this plain assignment statement always ends control flow,"
        // wrongly marking every real statement after it unreachable
        // (`deadCode.unreachable`'s own corpus verification is what
        // surfaced this). The real semantics: reaching anything *past*
        // this expression at all already proves the right side never
        // ran, so the result is exactly the left side's own (non-null)
        // type, the same as the `?? null` case above already computes.
        if r_ty == Type::Never {
            return l_ty;
        }
        // Phase 14: still scoped to exactly `Type::Array` on the left,
        // matching this function's own documented, deliberately narrow
        // boundary (an ArrayShape on the left stays uncovered, same as
        // before) - only widened to also recognize a `list<T>`/
        // `non-empty-array<T>`-typed left side (a real `Intersection`
        // now), which would otherwise lose its real type here the exact
        // same way a plain `Type::Array` used to.
        let l_is_array = matches!(l_ty, Type::Array(_, _))
            || matches!(&l_ty, Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Array(_, _))));
        match &r_ty {
            Type::ArrayShape(_) if l_is_array => l_ty,
            // Phase 15 continuation: the general case was discarding
            // `l_ty` entirely, keeping only `r_ty` - wrong for `??`'s own
            // semantics regardless of precision: the result is `$expr`
            // itself when it's not null (already computed above, with
            // `Null` excluded) *or* `$default` when it is - a real union
            // of both outcomes, not "the default's type, unconditionally"
            // (which happened to look like a narrow-imprecision problem -
            // `$x = $expr ?? false;` collapsing to the bare literal
            // `false` whenever `$expr`'s own type isn't confidently known
            // - but was actually just this fallback throwing `l_ty` away
            // outright, confirmed by reading the surrounding early-return
            // cases above, which *do* union `Null` back into `l_ty` for
            // the `?? null` case specifically; this arm never had the
            // equivalent treatment for every other right-hand type).
            _ => union_types(l_ty, r_ty),
        }
    }

    pub(crate) fn infer_binary(&self, op: &BinOp, l: &Expr, r: &Expr, locals: &Locals) -> Type {
        match op {
            // Round 12: verified via dumpType() - concatenating any two
            // strings where *either* side is provably non-falsy (a real
            // string literal other than `''`/`'0'`, or already a
            // `non-falsy-string`) always produces a `non-falsy-string`
            // result, regardless of the other side's own type - real
            // corpus shape: `sys_get_temp_dir() . '/'`, `dirname(...) .
            // '/backups/'`, `'.' . $ext`, each feeding a PHPUnit
            // `assertStringStartsWith()`/`assertStringEndsWith()` whose
            // stub declares its needle as `non-empty-string`.
            BinOp::Concat => {
                if is_non_falsy_string(&self.infer_expr(l, locals))
                    || is_non_falsy_string(&self.infer_expr(r, locals))
                {
                    Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::NonFalsyString),
                    ])
                } else {
                    Type::String
                }
            }
            // Phase 15 continuation: real constant folding, confirmed
            // directly against `vendor/phpstan-src`'s own
            // `BooleanAndHandler`/`BooleanOrHandler::resolveType` -
            // without this, `check_boolean_and_or`'s own "whole result"
            // diagnostic (`booleanAnd.always*`/`booleanOr.always*`) could
            // never fire at all, since it has nothing but a permanent
            // bare `Type::Bool` to ever match against. Mirrors real
            // PHPStan's exact short-circuit algorithm: a definitely-false
            // left side makes `&&`'s result definitely false without
            // even considering the right side; otherwise the right side
            // is inferred against `l`'s own truthy-narrowed scope (the
            // same `apply_narrowing` every call site already narrows
            // into before checking the right operand) since that's the
            // only scope the right side is ever actually reached in.
            BinOp::And => {
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
            BinOp::Or => {
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
            // Phase 55 follow-up, `ConstantLooseComparisonRule`:
            // `loose_compare_literals`'s own doc comment has the full
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
            // Phase 15 follow-up, real PHPStan's own
            // `NumberComparisonOperatorsConstantConditionRule`: the exact
            // same relationship as `Eq`/`NotEq` just above has to its own
            // `ConstantLooseComparisonRule` - a real, standalone rule,
            // never folded into `if`/`while`'s own generic constant-
            // condition family (confirmed directly against `vendor/
            // phpstan-src`: it's its own `#[RegisteredRule]`, with its
            // own `smaller.always*`/`greater.always*`/`smallerOrEqual.
            // always*`/`greaterOrEqual.always*` identifiers, not `if.
            // always*`). `should_skip_constant_condition` (`checker/
            // control_flow.rs`) already excludes all four of these ops
            // from the generic `if`/`while` path for exactly this reason -
            // this constant-folding half only feeds `check_constant_
            // numeric_comparison`'s own standalone diagnostic below, not
            // that generic path at all.
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
            // Phase 81: `GMP`/`BcMath\Number`'s own overloaded spaceship
            // still returns the same `-1|0|1` range, just computed by the
            // object's own comparison rather than a plain numeric one -
            // `binary_operator_overload_result` produces the identical
            // `IntRange(-1, 1)` either way, so this only refines *how*
            // precisely non-overloaded spaceship is typed too (a real,
            // if incidental, improvement over the previous bare `Int`).
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
                // filling in any key only present on the right. The real
                // per-key result type isn't something `Type::Array`/
                // `Type::ArrayShape` can express precisely today, so
                // `Array(Mixed)` is the honest answer.
                // Phase 14: `is_array_like` sees through a `list<T>`/
                // `non-empty-array<T>`-typed operand too - without it, a
                // `list<T> + list<T>` silently fell through to numeric
                // addition instead of array union.
                if Self::is_array_like(&lt) && Self::is_array_like(&rt) {
                    return Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed));
                }
                if lt == Type::Float || rt == Type::Float {
                    return Type::Float;
                }
                // Phase 15 continuation: real, if narrow, `int<min,max>`
                // arithmetic - confirmed directly against `vendor/
                // phpstan-src`'s own `InitializerExprTypeResolver::
                // getPlusType`/`integerRangeMath`'s `BinaryOp\Plus` arm
                // (an unbounded end propagates as unbounded; a bounded one
                // shifts by the same amount, exactly mirrored here). Real
                // corpus false positive this fixes: `strpos()`'s own
                // `$offset` parameter (typed `int<0, max>`) collapsing to
                // bare `Int` the moment it's reassigned via `$offset =
                // $pos + 1;` inside a loop - surfaced only once the
                // `While`-condition convergence fix (this session) made
                // `strpos()`'s own later, real re-check of `$offset`
                // visible at all. Deliberately narrower than real
                // PHPStan's own full range-arithmetic surface (`Mul`/`Div`'s
                // own negative/positive-interval splitting isn't
                // replicated - no corpus evidence needs it, and it's
                // meaningfully more involved to transcribe correctly) -
                // falls through to the existing plain `Int` answer
                // whenever either side isn't range-representable.
                if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (int_range_bounds(&lt), int_range_bounds(&rt))
                {
                    let min = l_min.zip(r_min).map(|(a, b)| a + b);
                    let max = l_max.zip(r_max).map(|(a, b)| a + b);
                    return normalize_int_range(min, max);
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
                    let min = l_min.map(|m| m - c);
                    let max = l_max.map(|m| m - c);
                    return normalize_int_range(min, max);
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
            // Phase 81: except when GMP (all five) or `BcMath\Number`
            // (`%` only - it doesn't overload the bitwise/shift operators
            // at all, per `binary_operator_overload_result`'s own gate)
            // is genuinely involved, which yields the object type back
            // instead of a plain `int`.
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                self.binary_operator_overload_result(op, &lt, &rt)
                    .unwrap_or(Type::Int)
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
            // Phase 13 Finding C: verified against real PHPStan's own
            // dumpType() - `/` gives `int|float` unconditionally whenever
            // it can't prove `float` is involved, even for two fully
            // unknown operands (not only when both are provably `int`),
            // and `float` whenever either operand is. Mirrors the
            // Add/Sub/Mul/Pow float-promotion shape directly above,
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
            BinOp::Pipe => Type::Mixed,
        }
    }
}

/// Phase 22 Finding 6: after checking `&&`/`||`'s right operand against a
/// cloned, possibly-narrowed `Locals`, merges back only the keys that are
/// both new (absent from `locals` before checking the right operand) and
/// a real PHP variable name (`is_real_variable_key`) - a genuine
/// definitional side effect (a by-ref out-param like `preg_match`'s
/// `$matches`, or a plain assignment) written as the right operand of
/// `&&`/`and`/`||`/`or` should persist regardless of the whole
/// expression's truth value, matching this codebase's "assume it might
/// be defined" philosophy elsewhere (`predefine_byref_or_unknown_args`).
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
            // Phase 17 follow-up: `key`'s own real declaration (a
            // by-ref out-param, or a plain assignment) already recorded
            // its true provenance inside `narrowed` when it happened -
            // carried across unchanged here, not fabricated fresh at
            // this merge point (which isn't itself where the variable
            // was declared).
            let provenance = narrowed.get_provenance(key).cloned().unwrap_or_default();
            locals.insert_with_provenance(key.clone(), ty.clone(), provenance);
        }
    }
}

/// The type of `container[key]` for a known container type - shared by
/// `infer_expr`'s own `Index` arm and its own recursion into a
/// `Type::Union`'s members. Phase 12 Finding E: `preg_match`'s own
/// `Union` of `ArrayShape`s (the matched shape, unioned with an empty one
/// for the no-match case) needs this same per-key resolution combined
/// across every member via `union_types`, not silently dropped to
/// `Mixed` for the whole type the moment it's a `Union` at all - though
/// for that *specific* pair of members, the combined result still lands
/// on `Mixed` anyway (the empty-shape member never has any real group's
/// key, so its own fallback below is always `Mixed`, which `union_types`
/// unconditionally absorbs into) - real, general infrastructure for a
/// `Union` whose members *agree* on a key's type, not what recovers
/// precision for `preg_match`'s own common case (that's `apply_narrowing`'s
/// own new `preg_match_condition` recognizer, `checker/narrowing.rs`).
fn index_result_type(container_ty: Type, key: Option<&str>) -> Type {
    match container_ty {
        Type::Array(_, inner) => *inner,
        Type::ArrayShape(fields) => key
            .and_then(|k| fields.into_iter().find(|(name, _, _)| name == k))
            .map(|(_, ty, _)| ty)
            .unwrap_or(Type::Mixed),
        Type::Union(members) => members
            .into_iter()
            .map(|m| index_result_type(m, key))
            .reduce(union_types)
            .unwrap_or(Type::Mixed),
        // Phase 14: a `list<T>`/`non-empty-array<T>`-typed value is a
        // real `Intersection([Array(T), Accessory(_)])` now, not plain
        // `Array(T)` - without this, indexing into one (`$listArg[0]`)
        // silently fell to `Mixed` below.
        Type::Intersection(members) => members
            .into_iter()
            .find_map(|m| Checker::array_element_type(&m))
            .unwrap_or(Type::Mixed),
        _ => Type::Mixed,
    }
}

/// Whether `ty` is provably a `non-falsy-string` (non-empty and never
/// exactly `"0"`) - shared by `BinOp::Concat` and `ExprKind::
/// Interpolated`'s own identical "does any one operand/part guarantee
/// the whole result is non-falsy" check, factored out so both build the
/// same `Intersection([String, Accessory(NonFalsyString)])` target type
/// and run the same `is_subtype` check against it, rather than each
/// constructing it independently.
fn is_non_falsy_string(ty: &Type) -> bool {
    let non_falsy_string = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::NonFalsyString),
    ]);
    types::is_subtype(ty, &non_falsy_string)
}

/// Phase 50 (reattempt): whether `ty` is confidently array-shaped for
/// `check_dim_fetch_key_type`'s own purposes - a bare `Array`/
/// `ArrayShape`, or a `Union` where *every* member is (recursively, so a
/// union of unions still counts) - anything else (`Mixed`, an object,
/// or a `Union` mixing array and non-array members) is "don't know,"
/// matching this project's leniency elsewhere rather than real
/// PHPStan's own tri-state `isArray()` grading.
pub(crate) fn is_confidently_array(ty: &Type) -> bool {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) => true,
        Type::Union(members) => members.iter().all(is_confidently_array),
        _ => false,
    }
}

/// Phase 50: whether `ty` is a type PHP itself can NEVER legally use as
/// an array key (`int`/`string`/`bool`/`float`/`null` all being legal -
/// PHP coerces the latter three at runtime) - `Some(description)` only
/// when every part of `ty` is confidently illegal, `None` (silently
/// allowed) for anything ambiguous (`Mixed`, a union mixing legal and
/// illegal members, an unresolved template, ...), matching this
/// project's "don't guess" leniency everywhere else. A `Type::Object`
/// covers both a real class instance and this project's own `resource`/
/// `Closure`-as-object representations - none of which PHP ever accepts
/// as a key either, even one with a real `__toString()` (array-key
/// coercion has no string-cast fallback the way string CONTEXT does).
pub(crate) fn invalid_array_key_description(ty: &Type) -> Option<String> {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) => Some("array".to_string()),
        Type::Object(name, _) => Some(name.clone()),
        Type::Union(members) => {
            let mut descs = Vec::new();
            for m in members {
                // At least one member being legal (or ambiguous) bails
                // out entirely - real PHPStan would report "possibly
                // invalid" here (its own tri-state `isSuperTypeOf`),
                // which this project has no equivalent for; skipping
                // rather than guessing matches its usual leniency.
                descs.push(invalid_array_key_description(m)?);
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

/// The type of a call this project can't look up a real signature for -
/// `exit`/`die` are real PHP language constructs (`is_language_construct`)
/// with no stub of their own, but unlike the others in that list
/// (`isset`/`empty`/`unset`/`eval`/`include*`/`print`), PHP itself declares
/// them `never`: control flow can't continue past one. `always_terminates`
/// (`checker/control_flow.rs`) recognizes a terminating expression
/// statement via exactly `infer_expr(e, locals) == Type::Never` - without
/// this, a guard clause built on `exit()`/`die()` instead of `return`/
/// `throw` (extremely common in this codebase's CLI tools and test
/// fixtures) was never recognized as terminating, leaking its own
/// never-actually-reached branch's narrowing into the surrounding scope.
fn call_fallback_return_type(callee: &Expr) -> Type {
    match &callee.kind {
        ExprKind::Name(name) if matches!(name.to_ascii_lowercase().as_str(), "exit" | "die") => {
            Type::Never
        }
        // `print` always evaluates to the literal `int(1)` in real PHP -
        // confirmed directly against `vendor/phpstan-src`'s own
        // `PrintHandler::resolveType`, which returns `new
        // ConstantIntegerType(1)` unconditionally.
        ExprKind::Name(name) if name == "print" => Type::IntLiteral(1),
        _ => Type::Mixed,
    }
}

/// Whether a `Type::ArrayShape` key string is one PHP itself would have
/// already cast to a real integer at the point the array was built -
/// PHP's own rule: a decimal string with no leading zero (unless it's
/// exactly `"0"`) and no leading `+`. Deliberately doesn't handle a
/// leading `-` (a negative-integer key string, real but rare enough as a
/// literal shape key that no evidence calls for it) - misclassifying one
/// as a string key here is the same safe-by-omission default this
/// project uses throughout rather than a wrong answer.
pub(crate) fn is_canonical_int_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|b| b.is_ascii_digit())
        && (key == "0" || !key.starts_with('0'))
}

/// Phase 14: whether `e` is the literal `false` - used to scope
/// `array_slice`/`array_reverse`'s own `$preserve_keys` argument to
/// exactly the default-matching case (omitted, or explicitly `false`).
fn is_false_literal(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Bool(false)))
}

/// The `true`-literal mirror of `is_false_literal` just above - used by
/// `var_export()`'s own dynamic-return-type narrowing (`builtin_calls.rs`)
/// to recognize its `$return` argument's default-matching-but-inverted
/// case (explicitly `true`, unlike `$preserve_keys`'s "omitted or
/// `false`" default).
fn is_true_literal(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Bool(true)))
}

/// Whether `class::$method()` is Doctrine's own `AbstractQuery::getResult()`
/// (or an override/inheritor of it, e.g. `Query::getResult()`) - the native
/// stub declares a bare `mixed` return with no docblock refinement at all.
/// Real PHPStan's own `phpstan-doctrine` extension gives this a much
/// richer, query-builder-aware entity type (full DQL SELECT/FROM
/// introspection, tracking `indexBy()` too) this project has no way to
/// replicate - a sizable feature on its own - but the narrower, still
/// real and bounded fact it always proves is worth having on its own:
/// a non-`indexBy()`'d query's own result is always a real,
/// sequentially-keyed list, confirmed via dumpType(). Zero `indexBy()`
/// call sites exist anywhere in this corpus, the one case that would
/// break this simplification.
pub(crate) fn is_doctrine_query_get_result(
    checker: &Checker<'_>,
    class: &str,
    method: &str,
) -> bool {
    method == "getResult"
        && checker
            .registry
            .is_subclass_of(class, "Doctrine\\ORM\\AbstractQuery")
}

pub(crate) fn doctrine_query_result_list_type() -> Type {
    Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
        Type::Accessory(AccessoryKind::ArrayList),
    ])
}

/// Peels through any `(...)`-grouping wrapper(s) to the real expression
/// underneath - `ExprKind::Grouping` is a genuine, distinct AST node
/// (not something parens collapse away during parsing), so a
/// closure-literal-shaped check like Finding 49's own `Closure::bind`/
/// `->bindTo()` special-casing needs to see through
/// `(function () {...})` the same as a bare `function () {...}`.
pub(crate) fn unwrap_grouping(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) => unwrap_grouping(inner),
        _ => e,
    }
}
