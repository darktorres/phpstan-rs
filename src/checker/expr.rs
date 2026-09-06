use crate::ast::*;
use crate::checker::{
    CheckArgsCall, Checker, DiagnosticBuilder, Locals, MemberVisibilityCheck, TemplateBindings,
    body_contains_yield, expr_contains_yield, is_language_construct, is_real_variable_key,
    is_superglobal, literal_string_key, literal_string_value, merge_scopes, narrow_type_excluding,
    narrowing_key, purge_stale_narrowing_for, resolve_call_args, superglobal_default_type,
    to_bitwise_not,
};
use crate::pest_config;
use crate::regex_groups;
use crate::registry::FunctionSig;
use crate::token::Span;
use crate::types::{self, AccessoryKind, Type};

mod builtin_calls;
mod preg_shapes;

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

/// Phase 85: a bare `ExprKind::Name` this project has no general global-
/// constant table for (`PHP_EOL`, `M_PI`, ... all stay `Mixed`, a
/// deliberate, unrelated scope boundary this doesn't touch) - except
/// `true`/`false`/`null`, real PHP *language* keywords rather than
/// ordinary constants, and the one shape this project's own parser
/// doesn't already fold into `ExprKind::Literal` itself: a fully-
/// qualified reference (`\false`/`\true`/`\null`) parses as a plain
/// `Name`, not the `Keyword::False`/`True`/`Null` token `parse_primary`
/// matches directly for the unqualified form (real PHP itself treats
/// both forms identically - `\false` unambiguously names the same
/// global pseudo-constant `false` does, never a user-defined one, since
/// PHP doesn't allow declaring a constant with one of these three
/// names). A real, common corpus shape: Rector's own namespace-
/// prefixing transform (`RectorPrefix...`) rewrites every unqualified
/// global name to its fully-qualified form, including these three,
/// across a large fraction of this corpus's `vendor/rector/rector/
/// vendor/` and similar prefixed trees - previously silently `Mixed`,
/// which fed straight into `apply_equality_narrowing`'s own `target_ty
/// == Type::Mixed` bail-out, silently skipping every `\false ===
/// $x = fn()`-shaped guard's own narrowing (Symfony\Component\
/// Filesystem\Path::getDirectory's real shape, found via Phase 85's own
/// corpus check).
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
fn type_definitely_excludes_null(ty: &Type) -> bool {
    !matches!(ty, Type::Mixed | Type::Never) && narrow_type_excluding(ty, &Type::Null) == *ty
}

/// Phase 60: real PHPStan's own `isset()`/`empty()`/`??`/`??=` "does this
/// null-probe make sense" verdict, condensed to the one shape this
/// project scopes to (a bare, guaranteed-defined variable - see
/// `Checker::check_pointless_null_probe`'s own doc comment for why a
/// deeper chain is out of scope). `None` for a genuinely ambiguous type
/// (`Mixed`, or one that's nullable among other members) - real PHPStan's
/// `IssetCheck` draws the identical two-way split (`isNull()->yes()` vs.
/// `isNull()->no()`, `maybe()` staying silent either way).
fn null_probe_verdict(ty: &Type) -> Option<&'static str> {
    if *ty == Type::Null {
        return Some("is always null");
    }
    if type_definitely_excludes_null(ty) {
        Some("is not nullable")
    } else {
        None
    }
}

impl<'a> Checker<'a> {
    /// Checks an `isset()`/`??`-style "quiet" left-hand-side chain: walks
    /// down through `Index`/`PropertyAccess`/`DynamicPropertyAccess`,
    /// checking each level's own sub-expressions (an index's key, a
    /// dynamic property's name) normally, but suppressing the root
    /// variable's own "undefined variable" check and `PropertyAccess`'s
    /// own "access to undefined property" check at every level along the
    /// way - exactly what `isset()`/`??` themselves suppress in real PHP,
    /// and nothing more. `StaticAccess` is never part of this walk (an
    /// undeclared *static* property is a real, non-suppressible PHP
    /// fatal error), and anything past a function/method call is checked
    /// normally, since `isset()`/`??` don't evaluate quietly past one.
    pub(crate) fn check_expr_quiet_root(
        &mut self,
        e: &Expr,
        locals: &mut Locals,
        label: &str,
    ) -> Type {
        // Every arm here is consumed only for its side effects - see this
        // function's own doc comment and Phase 3's plan note: neither `??`
        // nor `??=` (its only two call sites) depend on the quiet-checked
        // left side's own type, so the exact value returned doesn't matter
        // beyond satisfying the type system post-unification.
        match &e.kind {
            ExprKind::Variable(_) => {
                // Deliberately not `check_expr` - a bare variable here is
                // exactly the "might not be defined" case `??` exists to
                // handle safely.
                Type::Mixed
            }
            ExprKind::Index(base, idx) => {
                self.check_expr_quiet_root(base, locals, label);
                if let Some(i) = idx {
                    self.check_expr(i, locals, label);
                }
                Type::Mixed
            }
            ExprKind::PropertyAccess(base, _, _) => {
                self.check_expr_quiet_root(base, locals, label);
                Type::Mixed
            }
            ExprKind::DynamicPropertyAccess(base, name_expr) => {
                self.check_expr_quiet_root(base, locals, label);
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            _ => self.check_expr(e, locals, label),
        }
    }

    /// Phase 60: `isset($x)`/`empty($x)`/`$x ?? ...`/`$x ??= ...` where
    /// `$x` is a bare, already-guaranteed-defined local - `locals`
    /// itself is this project's own "definitely bound on every path"
    /// signal (`merge_scopes` drops a variable entirely rather than
    /// carry it forward unsoundly when only one branch defines it) - and
    /// whose tracked type proves the null-probe can never change the
    /// outcome. Deliberately scoped to a bare `Variable` root only: a
    /// deeper chain (`isset($x['key'])`, `isset($x->prop)`) needs
    /// offset-existence/property-initialization dataflow this project
    /// doesn't model yet (real PHPStan gates the equivalent behind its
    /// own `checkAdvancedIsset` config flag) - left alone rather than
    /// risk a false positive from an unmodeled "might not exist" case.
    fn check_pointless_null_probe(
        &mut self,
        e: &Expr,
        locals: &Locals,
        label: &str,
        identifier_prefix: &str,
        operator_description: &str,
    ) {
        let ExprKind::Variable(name) = &e.kind else {
            return;
        };
        if is_superglobal(name) {
            return;
        }
        // Phase 60 corpus find: inside a loop body, `locals`'s own
        // tracked type only reflects a single, non-iterated pass through
        // it (see `Checker::loop_depth`'s own doc comment) - a variable
        // initialized to a narrow type once before the loop, then
        // self-referentially widened only via the very statement being
        // checked here (`$err ??= $e;` inside a loop's own `catch`,
        // Symfony's own `AbstractObjectNormalizer::
        // validateAndDenormalize` real shape), looks "always null" on
        // this first pass even though a real second-or-later iteration
        // would see otherwise.
        if self.loop_depth > 0 {
            return;
        }
        // Phase 60 corpus find: a `static` local can carry a value left
        // over from an earlier call - see `Checker::static_var_names`'s
        // own doc comment (real shape: Symfony's own `HttpClient::
        // create()`'s `static $curlVersion = null; $curlVersion ??=
        // curl_version();`, confirmed genuinely silent in real PHPStan
        // too).
        if self.static_var_names.contains(name) {
            return;
        }
        let Some(ty) = locals.get(name) else {
            return;
        };
        let Some(verdict) = null_probe_verdict(ty) else {
            return;
        };
        self.report(
            label.to_string(),
            format!("Variable ${name} {operator_description} always exists and {verdict}."),
            e.span,
            0,
            &format!("{identifier_prefix}.variable"),
        );
    }

    #[hotpath::measure]
    pub(crate) fn check_expr(&mut self, e: &Expr, locals: &mut Locals, label: &str) -> Type {
        // Phase 71: see `Checker::in_bare_statement_position`'s own doc
        // comment - taken (not just read) so it reads `true` only for
        // the exact top-level expression `StmtKind::Expression` set it
        // for, never for anything this match's own arms go on to
        // recursively check via a nested `check_expr` call.
        let is_bare_statement = std::mem::take(&mut self.in_bare_statement_position);
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
                // Phase 60: `isset($x)` specifically (not `empty($x)` too -
                // real PHPStan's own `EmptyRule` requires BOTH nullability
                // *and* falsy-ness to be unambiguous before it says
                // anything, since `empty()` tests falsiness, not just
                // null-ness - `toBoolean()->isFalse()` staying `maybe` for
                // an ordinary non-empty-unproven `array`/`string`/`int`
                // keeps real PHPStan silent on exactly the shape this
                // project's own `null_probe_verdict` would otherwise
                // wrongly flag. Left for a follow-up rather than modeled
                // here.
                let is_isset_probe =
                    matches!(&callee.kind, ExprKind::Name(name) if name == "isset");
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
                        if is_isset_probe {
                            self.check_pointless_null_probe(
                                &a.value,
                                locals,
                                label,
                                "isset",
                                "in isset()",
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
                    locals.insert(matches_name.clone(), shape);
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
                    locals.insert(matches_name.clone(), shape);
                }
                // Reuses the `resolved_name`/`sig` already resolved above
                // - the same lookup `infer_expr`'s own `Call` arm would
                // otherwise redundantly redo from scratch.
                sig.map(|s| {
                    self.infer_builtin_call_return_type(
                        resolved_name.as_deref().unwrap_or_default(),
                        s,
                        args,
                        locals,
                    )
                })
                .unwrap_or_else(|| call_fallback_return_type(callee))
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
                // Substituted through the receiver's own carried type
                // arguments (Phase 10), walking up to whichever ancestor
                // actually declares the method if it's inherited (Phase
                // 10.5) - a method declared `add(T $item)` on
                // `Collection<User>`, or inherited from `Collection` by a
                // concrete `IntCollection extends Collection<int>`, gets
                // checked against `User`/`int`, not an unbound `T`/`Mixed`.
                let sig = recv_class.as_ref().and_then(|(class, type_args)| {
                    self.registry
                        .lookup_method_with_owner(class, method)
                        .map(|(owner, sig)| {
                            self.substitute_class_templates(sig, class, type_args, &owner)
                        })
                });
                let resolved = sig
                    .as_ref()
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(None, sig.as_ref(), args, &resolved, locals);
                for a in args {
                    self.check_expr(&a.value, locals, label);
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
                    } else if let Some(sig) = &sig {
                        // Phase 57: visibility - a fresh, small lookup
                        // rather than threading `owner` out of `sig`'s
                        // own computation above (which discards it once
                        // `substitute_class_templates` has consumed it),
                        // matching this project's own established
                        // "cheap, pure lookup called twice for clarity"
                        // precedent elsewhere (e.g. `ExprKind::
                        // PropertyAccess`'s identical shape just above).
                        if let Some((owner, _)) =
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
                            // Phase 67: `@internal`.
                            self.check_internal_method_call(
                                &owner, method, sig, false, e.span, label,
                            );
                            // Phase 39: `@deprecated`.
                            self.check_deprecated_method_call(
                                &owner, method, sig, false, e.span, label,
                            );
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
                    }
                }
                let result = match &recv_class {
                    Some((class, _)) if is_doctrine_query_get_result(self, class, method) => {
                        doctrine_query_result_list_type()
                    }
                    Some((class, _)) => sig
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
                // Phase 15 item (2), `BooleanAndConstantConditionRule`:
                // investigated and declined alongside `StmtKind::If`'s own
                // identical check - see that arm's doc comment
                // (`checker/stmt.rs`) and ROADMAP.md.
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, true);
                self.check_expr(r, &mut narrowed, label);
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(&BinOp::And, l, r, locals)
            }
            ExprKind::Binary(BinOp::Or, l, r) => {
                self.check_expr(l, locals, label);
                // Phase 15 item (2), `BooleanOrConstantConditionRule`:
                // investigated and declined - same as `&&` above.
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, false);
                self.check_expr(r, &mut narrowed, label);
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
                );
                let r_ty = self.check_expr(r, locals, label);
                self.widen_coalesce_result(l, r_ty, locals)
            }
            ExprKind::Binary(op, l, r) => {
                let l_ty = self.check_expr(l, locals, label);
                let r_ty = self.check_expr(r, locals, label);
                self.check_binary_operand_validity(op, &l_ty, &r_ty, false, l.span, label);
                self.check_comparison_validity(op, &l_ty, &r_ty, l.span, label);
                // Phase 15 item (2), `LogicalXorConstantConditionRule`:
                // investigated and declined alongside `StmtKind::If`'s own
                // identical check - see that arm's doc comment
                // (`checker/stmt.rs`) and ROADMAP.md.
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
                        other => other.clone(),
                    };
                    purge_stale_narrowing_for(locals, name);
                    locals.insert(name.clone(), updated);
                }
                self.infer_expr(inner, locals)
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.check_expr(inner, locals, label);
                // Phase 15 item (2), `BooleanNotConstantConditionRule`:
                // investigated and declined alongside `StmtKind::If`'s own
                // identical check - see that arm's doc comment
                // (`checker/stmt.rs`) and ROADMAP.md.
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
                if let Type::Object(class, _) = self.check_expr(inner, locals, label)
                    && !self.registry.is_subclass_of(&class, "Throwable")
                {
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
                    InstanceofTarget::Name(name) => {
                        if let Type::Object(class, _) = self.resolve_name_expr(name)
                            && !self.class_name_exists(&class)
                        {
                            self.report(
                                label.to_string(),
                                format!("instanceof check target class {class} not found"),
                                e.span,
                                0,
                                "class.notFound",
                            );
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
                    );
                    ty
                } else {
                    self.check_expr(lhs, locals, label)
                };
                let rhs_ty = self.check_expr(rhs, locals, label);
                if *op != BinOp::Coalesce {
                    self.check_binary_operand_validity(op, &lhs_ty, &rhs_ty, true, lhs.span, label);
                }
                if let ExprKind::Variable(name) = &lhs.kind {
                    let ty = self.infer_binary(op, lhs, rhs, locals);
                    purge_stale_narrowing_for(locals, name);
                    locals.insert(name.clone(), ty);
                }
                Type::Mixed
            }
            ExprKind::Ternary(cond, then_, else_) => {
                self.check_expr(cond, locals, label);
                // Phase 15 item (2), `TernaryOperatorConstantConditionRule`:
                // investigated and declined alongside `StmtKind::If`'s own
                // identical check - see that arm's doc comment
                // (`checker/stmt.rs`) and ROADMAP.md.
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
                for a in args {
                    self.check_expr(&a.value, locals, label);
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
                for m in &decl.methods {
                    self.check_function(&m.function, Some(&decl.name), m.is_static);
                }
                // Phase 37: its own properties'/consts' attributes too -
                // method/param attributes are already covered inside
                // `check_function` above.
                for p in &decl.properties {
                    self.check_attributes(&p.attrs, &decl.name);
                }
                for cc in &decl.consts {
                    self.check_attributes(&cc.attrs, &decl.name);
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
                    for a in args {
                        self.check_expr(&a.value, locals, label);
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
                // above already handles "doesn't exist"), so this reuses
                // the identical class-narrowing shape the real type-
                // resolution just below performs anyway.
                if let Type::Object(class, _) = narrow_type_excluding(&base_ty, &Type::Null)
                    && let Some((owner, p)) = self.registry.lookup_property_with_owner(&class, prop)
                {
                    let visibility = p.visibility.clone();
                    self.check_member_visibility(MemberVisibilityCheck {
                        kind: "property",
                        owner: &owner,
                        member_name: prop,
                        visibility: &visibility,
                        describe: &format!("property {owner}::${prop}"),
                        span: e.span,
                        label,
                    });
                    // Phase 67: `@internal`.
                    self.check_internal_property_access(&owner, prop, p, e.span, label);
                    // Phase 39 follow-up: `@deprecated`.
                    self.check_deprecated_property_access(&owner, prop, p, e.span, label);
                }
                if let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                let result = match narrow_type_excluding(&base_ty, &Type::Null) {
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_property_with_owner(&class, prop)
                        .map(|(owner, p)| {
                            let bindings = self.class_template_bindings(&class, &type_args, &owner);
                            let resolved = types::substitute_template(&p.ty, &bindings);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    // Phase 79: `->name`/`->value` on a specific known
                    // case narrows past the enum-wide property type
                    // registered in `ClassInfo` (a generic `string` /
                    // union-of-every-case's-value) to this exact case's
                    // own literal - real PHPStan's own `->name`/`->value`
                    // precision (`EnumCaseObjectType`). Enums can't
                    // declare any instance property of their own in real
                    // PHP, so no other property name is ever legitimate
                    // here - the fallback still does an ordinary by-class
                    // lookup rather than assuming that, in case this
                    // checker's own synthesized `properties` map (or a
                    // future language change) ever disagrees.
                    Type::EnumCase(_, case) if prop == "name" => Type::StringLiteral(case),
                    Type::EnumCase(fqn, case) if prop == "value" => self
                        .registry
                        .enum_case_value(&fqn, &case)
                        .cloned()
                        .or_else(|| {
                            self.registry
                                .lookup_property(&fqn, "value")
                                .map(|p| p.ty.clone())
                        })
                        .unwrap_or(Type::Mixed),
                    Type::EnumCase(fqn, _) => self
                        .registry
                        .lookup_property_with_owner(&fqn, prop)
                        .map(|(owner, p)| self.resolve_pseudo(&p.ty, &owner))
                        .unwrap_or(Type::Mixed),
                    // Phase 80: a declared `object{...}` shape's own
                    // field, resolved by name directly - no registry
                    // lookup needed, the shape's own fields already carry
                    // real types. An unlisted property name falls to
                    // `Mixed` rather than a new "undefined property"
                    // diagnostic - this project doesn't add that check
                    // for `ArrayShape`'s own unlisted keys either (see
                    // Phase 50's own still-open `InvalidKeyInArrayDimFetchRule`
                    // note), matching the identical "don't guess a shape
                    // is exhaustive" caution here.
                    Type::ObjectShape(fields) => fields
                        .iter()
                        .find(|(name, _, _)| name == prop)
                        .map(|(_, ty, _)| ty.clone())
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
                };
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
                ..
            } => {
                // Unlike an arrow function, a plain closure does NOT
                // automatically see the enclosing scope - only variables
                // explicitly named in `use (...)` are captured (by value
                // or by reference; either way, the type it starts with is
                // whatever the outer scope currently has it as). `$this`
                // is the one implicit exception: a non-static closure
                // declared inside a method still auto-binds it, the same
                // as real PHP.
                let mut closure_locals: Locals = Locals::default();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    closure_locals.insert(u.name.clone(), ty);
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
                    closure_locals.insert(p.name.clone(), ty);
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
                self.check_generator_return_type(declared_return.as_ref(), e.span, &closure_label);
                self.check_block(
                    body,
                    &mut closure_locals,
                    declared_return.as_ref(),
                    &closure_label,
                );
                self.current_function_is_generator = previous_generator_flag;
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
                    let original = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    let mutated = closure_locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    locals.insert(u.name.clone(), union_types(original, mutated));
                }
                Type::Object("Closure".to_string(), Vec::new())
            }
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                ..
            } => {
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
                    arrow_locals.insert(p.name.clone(), ty);
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
            ExprKind::Name(name) => bare_name_literal_type(name),
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
            ExprKind::Unary(UnaryOp::Neg | UnaryOp::Plus, inner) => {
                let inner_ty = self.infer_expr(inner, locals);
                if is_string_shaped(&inner_ty) {
                    Type::Int
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
                let result = match recv_ty {
                    Type::Object(class, _)
                        if is_doctrine_query_get_result(self, &class, method) =>
                    {
                        doctrine_query_result_list_type()
                    }
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_method_with_owner(&class, method)
                        .map(|(owner, sig)| {
                            // Class-level substitution (Collection<User>'s
                            // own `T`, or an inherited method's
                            // owner-level `T` via Phase 10.5) runs first,
                            // then infer_call_return_type's existing
                            // per-call-argument binding handles any
                            // template the *method itself* separately
                            // declares - the two are independent
                            // parameter scopes that can both appear on
                            // the same signature.
                            let sig =
                                self.substitute_class_templates(sig, &class, &type_args, &owner);
                            let resolved_args = resolve_call_args(args, &sig.params);
                            let resolved =
                                self.infer_call_return_type(&sig, &resolved_args, locals);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    // Phase 79: an enum case is a real instance of its
                    // enum for method-call purposes (see
                    // `member_confirmed_missing`'s identical `EnumCase`
                    // arm) - no real template-args list to carry through,
                    // same as every other synthesized enum-instance type.
                    Type::EnumCase(fqn, _) => self
                        .registry
                        .lookup_method_with_owner(&fqn, method)
                        .map(|(owner, sig)| {
                            let resolved_args = resolve_call_args(args, &sig.params);
                            let resolved = self.infer_call_return_type(sig, &resolved_args, locals);
                            self.resolve_pseudo(&resolved, &owner)
                        })
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
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
                let result = match base_ty {
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_property_with_owner(&class, prop)
                        .map(|(owner, p)| {
                            let bindings = self.class_template_bindings(&class, &type_args, &owner);
                            let resolved = types::substitute_template(&p.ty, &bindings);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    // Phase 79: mirrors `check_expr`'s own identical
                    // `EnumCase` arm just above - see its comment.
                    Type::EnumCase(_, case) if prop == "name" => Type::StringLiteral(case),
                    Type::EnumCase(fqn, case) if prop == "value" => self
                        .registry
                        .enum_case_value(&fqn, &case)
                        .cloned()
                        .or_else(|| {
                            self.registry
                                .lookup_property(&fqn, "value")
                                .map(|p| p.ty.clone())
                        })
                        .unwrap_or(Type::Mixed),
                    Type::EnumCase(fqn, _) => self
                        .registry
                        .lookup_property_with_owner(&fqn, prop)
                        .map(|(owner, p)| self.resolve_pseudo(&p.ty, &owner))
                        .unwrap_or(Type::Mixed),
                    // Phase 80: mirrors `check_expr`'s own identical
                    // `ObjectShape` arm - see its comment.
                    Type::ObjectShape(fields) => fields
                        .iter()
                        .find(|(name, _, _)| name == prop)
                        .map(|(_, ty, _)| ty.clone())
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
                };
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
            ExprKind::Name(name) => bare_name_literal_type(name),
            _ => Type::Mixed,
        }
    }

    /// Phase 68: `DuplicateKeysInLiteralArraysRule` - a later literal key
    /// silently overwrites an earlier one at runtime (real PHP raises no
    /// warning either), almost always a real bug (a copy-pasted entry
    /// whose key was never updated) rather than an intentional
    /// "overwrite semantics" idiom. `infer_array_literal_type` just
    /// below already computes the exact same key-derivation/auto-index
    /// logic for its own overwrite-on-duplicate handling - this walks
    /// the same shape independently so it can report every duplicate
    /// found, rather than reusing that function's own discard-on-
    /// overwrite `Vec`. Bails on the *first* non-literal key, same
    /// "don't guess" leniency `infer_array_literal_type` already has -
    /// a computed key could coincide with any later key, so nothing
    /// past it can be soundly checked either.
    pub(crate) fn check_array_literal_duplicate_keys(&mut self, items: &[ArrayItem], label: &str) {
        if items.iter().any(|item| item.spread) {
            return;
        }
        // Key text -> (first occurrence's span, how many times it's been
        // seen so far). Real PHP coerces a numeric-string key ("5") to
        // the same int key (5) it names - both an int key's own
        // `.to_string()` and its numeric-string twin are spelled
        // identically here, so comparing the plain text alone (no
        // separate int/string discriminator) already gets this right,
        // the same simplification `infer_array_literal_type` below
        // relies on for its own overwrite-on-duplicate handling.
        let mut seen: Vec<(String, (Span, u32))> = Vec::new();
        let mut next_index: i64 = 0;
        for item in items {
            let item_span = item.key.as_ref().map_or(item.value.span, |k| k.span);
            let key = match &item.key {
                None => {
                    let k = next_index.to_string();
                    next_index += 1;
                    k
                }
                Some(k) => match &k.kind {
                    ExprKind::Literal(Literal::String(s)) => s.clone(),
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        n.to_string()
                    }
                    _ => return,
                },
            };
            match seen.iter_mut().find(|(k, _)| *k == key) {
                Some((_, (first_span, count))) => {
                    *count += 1;
                    if *count == 2 {
                        // `var_export`-style display: bare for a
                        // canonical int key, single-quoted otherwise.
                        let display = if is_canonical_int_key(&key) {
                            key.clone()
                        } else {
                            format!("'{}'", key.replace('\\', "\\\\").replace('\'', "\\'"))
                        };
                        self.report(
                            label.to_string(),
                            format!("Array has duplicate key {display}."),
                            *first_span,
                            0,
                            "array.duplicateKey",
                        );
                    }
                }
                None => seen.push((key, (item_span, 1))),
            }
        }
    }

    /// Finding 48: an array literal's own element types were never
    /// inferred precisely - every literal read as `Array(Mixed)`
    /// regardless of its actual contents, which silently defeated
    /// `strict_mixed_arrays` (Phase 11 Stage 5) for its own headline use
    /// case (checking a hand-written literal against a declared shape)
    /// and made any destructuring from an inline literal lose all
    /// per-position precision (Finding 42's own gap, from the same root
    /// cause). Builds a real `Type::ArrayShape`, keyed by each literal
    /// key (or, for a positional element, PHP's own real auto-index
    /// rule: one past the highest explicit integer key seen so far, not
    /// just a running count - `[5 => 'a', 'b']`'s `'b'` really is key
    /// `6` in real PHP). A spread element (`...$other`) or a computed,
    /// non-literal key both defeat precise inference for the *whole*
    /// literal - real PHP inserts a variable number of the spread
    /// value's own elements at that point, shifting every subsequent
    /// positional key by an amount this checker has no way to know
    /// statically, and a non-literal key has no fixed name to key a
    /// field by at all - degrading to the honest `Array(Mixed)` for
    /// those specific cases, matching the status quo everywhere else.
    pub(crate) fn infer_array_literal_type(&self, items: &[ArrayItem], locals: &Locals) -> Type {
        if items.iter().any(|item| item.spread) {
            return self.infer_spread_array_literal_type(items, locals);
        }
        let mut fields: Vec<(String, Type, bool)> = Vec::new();
        let mut next_index: i64 = 0;
        for item in items {
            let key = match &item.key {
                None => {
                    let k = next_index.to_string();
                    next_index += 1;
                    k
                }
                Some(k) => match &k.kind {
                    ExprKind::Literal(Literal::String(s)) => s.clone(),
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        n.to_string()
                    }
                    _ => {
                        return Type::Array(
                            Box::new(Type::unknown_array_key()),
                            Box::new(Type::Mixed),
                        );
                    }
                },
            };
            let value_ty = self.infer_expr(&item.value, locals);
            // A later duplicate key overwrites an earlier one, matching
            // real PHP's own array-literal semantics.
            match fields.iter_mut().find(|(k, _, _)| *k == key) {
                Some(field) => field.1 = value_ty,
                None => fields.push((key, value_ty, false)),
            }
        }
        Type::ArrayShape(fields)
    }

    /// A spread item (`[...$a, ...$b]`) can be tracked precisely as long
    /// as every spread source's own exact fields are known (an
    /// `ArrayShape`) - real PHP's own array-unpacking semantics
    /// (verified via `dumpType()`): a spread `ArrayShape`'s canonical int
    /// keys get renumbered sequentially into the result, exactly the same
    /// as a plain positional item; its non-canonical (string, or a
    /// non-sequential int) keys are kept and overwrite verbatim, the same
    /// `is_canonical_int_key` rule `infer_array_literal_type`'s own
    /// int-key handling already applies. `UserInfoWithThemeName::toArray()`
    /// (`[...$this->userInfo->toArray(), 'theme_name' => $this->themeName]`)
    /// is exactly this shape - merging two known shapes into one, not
    /// combining unknown-length arrays.
    ///
    /// The moment any spread source's fields *aren't* known this way (a
    /// plain `array<T>`/`list<T>`), the whole literal degrades to the
    /// coarser "element type plus list/non-empty markers" level instead -
    /// and every field already collected before that point degrades with
    /// it, keeping only its value type, not its specific key. Verified via
    /// `dumpType()`: `['x' => 1, ...$plainArray]` on a plain `array<int>`
    /// dumps as `non-empty-array<int>`, not a shape retaining `'x'`.
    /// Spreading two `list<T>`s stays a `list<T>`; a plain `array<T>`
    /// source (list-ness unproven) degrades list-ness for the whole
    /// result, matching `[...$list, ...$plainArray] -> array<T>` (not
    /// `list<T>`) exactly. A plain, unkeyed element always guarantees the
    /// result is non-empty; a spread source only does if it's itself
    /// provably non-empty.
    fn infer_spread_array_literal_type(&self, items: &[ArrayItem], locals: &Locals) -> Type {
        let mut fields: Vec<(String, Type, bool)> = Vec::new();
        let mut next_index: i64 = 0;
        let mut degraded = false;
        let mut elem_types: Vec<Type> = Vec::new();
        let mut list_safe = true;
        let mut known_non_empty = false;

        for item in items {
            let value_ty = self.infer_expr(&item.value, locals);
            if item.spread {
                if !degraded {
                    match &value_ty {
                        Type::ArrayShape(inner_fields) => {
                            for (k, t, opt) in inner_fields {
                                Self::merge_shape_field(
                                    &mut fields,
                                    &mut next_index,
                                    k,
                                    t.clone(),
                                    *opt,
                                );
                            }
                        }
                        _ => degraded = true,
                    }
                }
                if degraded {
                    elem_types.push(Self::array_element_type(&value_ty).unwrap_or(Type::Mixed));
                    if !Self::is_list_like(&value_ty) {
                        list_safe = false;
                    }
                    if Self::is_known_non_empty_array(&value_ty) {
                        known_non_empty = true;
                    }
                }
            } else {
                if !degraded {
                    match &item.key {
                        None => {
                            let key = next_index.to_string();
                            next_index += 1;
                            match fields.iter_mut().find(|(fk, _, _)| *fk == key) {
                                Some(field) => field.1 = value_ty.clone(),
                                None => fields.push((key, value_ty.clone(), false)),
                            }
                        }
                        Some(k) => match &k.kind {
                            ExprKind::Literal(Literal::String(s)) => {
                                match fields.iter_mut().find(|(fk, _, _)| fk == s) {
                                    Some(field) => field.1 = value_ty.clone(),
                                    None => fields.push((s.clone(), value_ty.clone(), false)),
                                }
                            }
                            ExprKind::Literal(Literal::Int(n)) => {
                                next_index = next_index.max(n + 1);
                                let key = n.to_string();
                                match fields.iter_mut().find(|(fk, _, _)| *fk == key) {
                                    Some(field) => field.1 = value_ty.clone(),
                                    None => fields.push((key, value_ty.clone(), false)),
                                }
                            }
                            _ => degraded = true,
                        },
                    }
                }
                elem_types.push(value_ty);
                known_non_empty = true;
                if item.key.is_some() {
                    list_safe = false;
                }
            }
        }

        if !degraded {
            return Type::ArrayShape(fields);
        }
        elem_types.extend(fields.into_iter().map(|(_, t, _)| t));
        let elem_ty = elem_types
            .into_iter()
            .reduce(union_types)
            .unwrap_or(Type::Mixed);
        let mut members = vec![Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(elem_ty),
        )];
        if list_safe {
            members.push(Type::Accessory(AccessoryKind::ArrayList));
        }
        if known_non_empty {
            members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
        }
        if members.len() == 1 {
            members.pop().unwrap()
        } else {
            Type::Intersection(members)
        }
    }

    /// Shared by a spread array literal and `array_merge()`'s own bespoke
    /// special case - both apply the identical real-PHP merge rule
    /// verified via dumpType(): a source field's canonical int key gets
    /// renumbered sequentially into the target (`next_index`); any other
    /// key is kept and fully overwrites a same-named target field
    /// verbatim, its own optional flag included.
    fn merge_shape_field(
        fields: &mut Vec<(String, Type, bool)>,
        next_index: &mut i64,
        key: &str,
        ty: Type,
        optional: bool,
    ) {
        let resolved_key = if is_canonical_int_key(key) {
            let assigned = next_index.to_string();
            *next_index += 1;
            assigned
        } else {
            key.to_string()
        };
        match fields.iter_mut().find(|(fk, _, _)| *fk == resolved_key) {
            Some(field) => *field = (resolved_key, ty, optional),
            None => fields.push((resolved_key, ty, optional)),
        }
    }

    /// Whether `ty` is already provably int-key-sequential from zero - a
    /// real `list<T>`/`non-empty-list<T>`, or an `ArrayShape` whose own
    /// keys happen to be canonical and sequential. Shares the exact
    /// `is_subtype`-side rule for an `ArrayShape` against
    /// `Accessory::ArrayList` rather than re-deriving it.
    pub(crate) fn is_list_like(ty: &Type) -> bool {
        match ty {
            Type::Intersection(members) => {
                members.contains(&Type::Accessory(AccessoryKind::ArrayList))
            }
            Type::ArrayShape(fields) => fields
                .iter()
                .enumerate()
                .all(|(i, (k, _, _))| k == &i.to_string()),
            // Every branch of a control-flow-merged `Union` must itself
            // be list-like - an `ArrayShape([])` branch (the common
            // "0 iterations" case) already satisfies this vacuously via
            // the `ArrayShape` arm above.
            Type::Union(members) => members.iter().all(Self::is_list_like),
            _ => false,
        }
    }

    /// Whether `ty` is already provably non-empty - a real
    /// `non-empty-array<T>`/`non-empty-list<T>`, or a non-empty
    /// `ArrayShape` literal.
    pub(crate) fn is_known_non_empty_array(ty: &Type) -> bool {
        match ty {
            Type::Intersection(members) => {
                members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray))
            }
            Type::ArrayShape(fields) => !fields.is_empty(),
            // Non-empty only if *every* branch is - a possibly-empty
            // branch (an `ArrayShape([])`, the common "0 iterations"
            // case) means the merged value isn't provably non-empty.
            Type::Union(members) => members.iter().all(Self::is_known_non_empty_array),
            _ => false,
        }
    }

    /// The mirror image of `is_known_non_empty_array` just above -
    /// whether `ty` is provably empty (an empty `ArrayShape` literal;
    /// nothing else in this project's own type system can prove zero
    /// elements, the same "don't guess" leniency `is_known_non_empty_
    /// array` already applies in the other direction).
    pub(crate) fn is_known_empty_array(ty: &Type) -> bool {
        match ty {
            Type::ArrayShape(fields) => fields.is_empty(),
            Type::Union(members) => members.iter().all(Self::is_known_empty_array),
            _ => false,
        }
    }

    /// Phase 71: `UselessFunctionReturnValueRule` -
    /// `var_export`/`print_r`/`highlight_string` all share the same
    /// `bool $return = false` last parameter: printing directly and
    /// returning a useless `null`/`true` when it's omitted or literally
    /// `false`, or returning the real string instead of printing when
    /// it's `true`. Gated on `!is_bare_statement` at the one call site
    /// (see `Checker::in_bare_statement_position`'s own doc comment) -
    /// a bare `var_export($x);` statement genuinely wants the print
    /// side effect and legitimately discards the useless return value,
    /// the normal way to use any of these three; only flagged once
    /// something else tries to *use* that return value instead
    /// (assigned, returned, passed as an argument, ...).
    fn check_useless_return_value_call(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &Locals,
        span: Span,
        label: &str,
    ) {
        let bare_name = name.rsplit('\\').next().unwrap_or(name);
        let useless_value = if bare_name.eq_ignore_ascii_case("var_export") {
            "null"
        } else if bare_name.eq_ignore_ascii_case("print_r")
            || bare_name.eq_ignore_ascii_case("highlight_string")
        {
            "true"
        } else {
            return;
        };
        // Only fires when the print-vs-return flag is definitely absent
        // or definitely `false` - a genuinely unknown (non-literal,
        // non-narrowed) second argument stays silent, "don't guess": it
        // really could be `true` at runtime.
        let is_useless = match args.len() {
            0 => return,
            1 => true,
            _ => self.infer_expr(&args[1].value, locals) == Type::False,
        };
        if !is_useless {
            return;
        }
        self.report(
            label.to_string(),
            format!(
                "Return value of function {bare_name}() is always {useless_value} and the result is printed instead of being returned. Pass in true as parameter #2 $return to return the output instead."
            ),
            span,
            0,
            "function.uselessReturnValue",
        );
    }

    /// Phase 68: `ArrayFilterRule`, narrowed to just its `arrayFilter.
    /// empty` half (real PHP's own default no-callback "drop every falsy
    /// element" behavior on a provably-empty array). The other two real
    /// PHPStan sub-checks (`arrayFilter.same`/`arrayFilter.alwaysEmpty`,
    /// gated on the array's own *element* type being provably always-
    /// truthy/always-falsy) were dropped after corpus verification
    /// caught two live false positives - a dynamic-key index-assignment
    /// inside a loop's `if`-branch not correctly widening the tracked
    /// element type across the branch (`vendor/symfony/var-exporter/
    /// ProxyHelper.php`'s `$abstractProperties[$name] = new
    /// \ReflectionProperty(...)` vs `$abstractProperties[$name] =
    /// false;`, inferred as bare `array<false>` - the object-valued
    /// branch went missing entirely), and a `try { $x = ...; } finally {
    /// array_filter([..., $x]); }` shape not accounting for the
    /// possibility the assignment inside `try` never ran (Piwigo's own
    /// `tests/Integration/GroupServiceTest.php`, `$copyId = null;`
    /// reassigned only conditionally inside `try`, inferred as bare
    /// `GroupId` with the `null` branch dropped). Both are pre-existing
    /// gaps in this project's own variable-type tracking, not something
    /// to fix as part of this "cheap win" phase - `is_known_empty_array`
    /// below has no such exposure (purely syntactic, no flow-sensitive
    /// inference involved).
    pub(crate) fn check_array_filter_no_op(
        &mut self,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: Span,
    ) {
        if args.len() != 1 {
            return;
        }
        let arr_ty = self.infer_expr(&args[0].value, locals);
        if !Self::is_array_like(&arr_ty) {
            return;
        }
        if Self::is_known_empty_array(&arr_ty) {
            self.report(
                label.to_string(),
                format!("call to array_filter() with an empty array ({arr_ty}) has no effect"),
                span,
                0,
                "arrayFilter.empty",
            );
        }
    }

    /// Phase 68: `ArrayValuesRule` - a no-op both when the input is
    /// provably empty and when it's already list-shaped (`is_list_like`,
    /// the same accessory-marker check this call's own return-type
    /// inference already relies on to know it *always* produces a fresh
    /// list regardless of the input's own shape).
    pub(crate) fn check_array_values_no_op(
        &mut self,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: Span,
    ) {
        if args.len() != 1 {
            return;
        }
        let arr_ty = self.infer_expr(&args[0].value, locals);
        if !Self::is_array_like(&arr_ty) {
            return;
        }
        if Self::is_known_empty_array(&arr_ty) {
            self.report(
                label.to_string(),
                format!("call to array_values() with an empty array ({arr_ty}) has no effect"),
                span,
                0,
                "arrayValues.empty",
            );
        } else if Self::is_list_like(&arr_ty) {
            self.report(
                label.to_string(),
                format!(
                    "call to array_values() on {arr_ty}, which is already a list, has no effect"
                ),
                span,
                0,
                "arrayValues.list",
            );
        }
    }

    /// A generic function's return type may mention its own `@template`
    /// parameters (`TemplateParam`) - this binds each one against the
    /// actual argument at this call site (`identity(5)` binds `T` to
    /// `int`) and substitutes the result into the return type. A signature
    /// with no templates at all is untouched (the common case, cheap).
    pub(crate) fn infer_call_return_type(
        &self,
        sig: &FunctionSig,
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) -> Type {
        let mut bindings: TemplateBindings = TemplateBindings::default();
        for (param, slot) in sig.params.iter().zip(resolved.iter()) {
            let Some(arg) = slot else { continue };
            let arg_ty = self.infer_expr(arg, locals);
            types::bind_template_param(&param.ty, &arg_ty, &mut bindings);
        }
        types::substitute_template(&sig.return_type, &bindings)
    }

    /// Phase 79: `Suit::from(1)`/`Suit::tryFrom(1)` called with a literal
    /// argument that provably matches one specific case's own backing
    /// value narrows past the synthesized `self`/`?self` signature
    /// straight to that case's own singleton type -
    /// `Registry::enum_case_for_value` is a no-op (returns `None`) for
    /// every non-enum class and every enum whose cases this checker
    /// couldn't prove literal values for, so this never fires outside a
    /// real backed-enum `from`/`tryFrom` call. A literal that matches
    /// *no* known case is deliberately left alone (returns `fallback`
    /// unchanged) rather than guessing `never`/`null` - real PHP's own
    /// `from()` runtime-error path isn't modeled, and this project
    /// doesn't have every case's value proven literal often enough to
    /// safely conclude "none of them match." Shared by both the
    /// mutating `check_expr` and the pure `infer_expr` `StaticCall` arms
    /// so they stay in sync.
    fn narrow_enum_from_call(
        &self,
        class: &str,
        method: &str,
        args: &[Arg],
        fallback: Type,
        locals: &Locals,
    ) -> Type {
        if (method != "from" && method != "tryFrom") || args.len() != 1 {
            return fallback;
        }
        let arg_ty = self.infer_expr(&args[0].value, locals);
        match self.registry.enum_case_for_value(class, &arg_ty) {
            Some(case) => Type::EnumCase(class.to_string(), case.to_string()),
            None => fallback,
        }
    }

    /// Finding 48 made a literal array's own inferred type a real
    /// `Type::ArrayShape` instead of always `Array(Mixed)` - every one of
    /// Phase 12's own special cases below that used to pattern-match
    /// `Type::Array(elem_ty)` directly needs this instead, so
    /// `array_map(fn($x) => ..., [1, 2, 3])` (a literal, now a shape) keeps
    /// working the same way it already did for a plain `array<int>`-typed
    /// variable. A shape's own "effective element type" is the union of
    /// every field's type (`Mixed` for an empty literal - nothing to infer
    /// a narrower type from).
    /// Phase 50: real PHPStan's `InvalidKeyInArrayItemRule` - an array
    /// literal's own key expression (`[$key => $value]`) must be a type
    /// PHP itself accepts as an array key at runtime (`int`/`string`,
    /// plus `bool`/`float`/`null`, which PHP silently coerces) - anything
    /// else (an array, an object, ...) is a hard `TypeError` at runtime,
    /// not a static-analysis nicety. Mirrors real PHPStan's own
    /// `AllowedArrayKeysTypes::getType()` default (non-strict) allowed
    /// set; unlike real PHPStan, has no tri-state "possibly invalid"
    /// case - `invalid_array_key_description` only ever answers when
    /// every part of the key's own type is confidently illegal, the same
    /// "don't guess" leniency this project applies everywhere else, so a
    /// `Mixed`/unresolved/partially-legal key is silently allowed rather
    /// than flagged.
    fn check_array_item_key_type(&mut self, key_expr: &Expr, locals: &Locals, label: &str) {
        let key_ty = self.infer_expr(key_expr, locals);
        if let Some(desc) = invalid_array_key_description(&key_ty) {
            self.report(
                label.to_string(),
                format!("Invalid array key type {desc}."),
                key_expr.span,
                3,
                "array.invalidKey",
            );
        }
    }

    pub(crate) fn array_element_type(ty: &Type) -> Option<Type> {
        match ty {
            Type::Array(_, elem) => Some((**elem).clone()),
            Type::ArrayShape(fields) => Some(
                fields
                    .iter()
                    .map(|(_, t, _)| t.clone())
                    .reduce(union_types)
                    .unwrap_or(Type::Mixed),
            ),
            // Phase 14: a `list<T>`/`non-empty-array<T>`-typed value is a
            // real `Intersection([Array(T), Accessory(_)])` now, not
            // plain `Array(T)` - without this, every one of this
            // function's own callers (`array_map`/`array_reduce`/
            // `array_chunk`/`array_column`) silently stopped recognizing
            // such a value as an array at all.
            Type::Intersection(members) => members.iter().find_map(Self::array_element_type),
            // A round of Piwigo-corpus bucketing found this was missing
            // the single most common way a real variable's type actually
            // arrives here: a `Union` of array-like branches from a
            // control-flow merge (`$x = []; if (...) { foreach (...) {
            // $x[] = ...; } }` - the "0 iterations" branch stays
            // `array{}`, the "≥1" branch becomes a real list, and an
            // `if`/loop join leaves `$x` a `Union` of the two). A
            // genuinely empty `ArrayShape` member contributes no real
            // element-type constraint (it holds zero elements in that
            // branch) and is skipped rather than folded in as `Mixed`,
            // which would otherwise collapse the *whole* merged element
            // type down to `Mixed` the moment any branch was ever empty.
            // Only defined when every other member is itself array-like -
            // a `list<T>|null` union has no sensible single "element
            // type" to report.
            Type::Union(members) => {
                let mut elem_types = Vec::new();
                for m in members {
                    if matches!(m, Type::ArrayShape(fields) if fields.is_empty()) {
                        continue;
                    }
                    elem_types.push(Self::array_element_type(m)?);
                }
                Some(
                    elem_types
                        .into_iter()
                        .reduce(union_types)
                        .unwrap_or(Type::Mixed),
                )
            }
            _ => None,
        }
    }

    /// Phase 73: `array_element_type`'s own key-side counterpart -
    /// deliberately narrower, since only a bare `Type::Array` (or one
    /// wrapped in the `list<T>`/`non-empty-array<T>` `Intersection`
    /// shape) carries a real key type to report at all. An `ArrayShape`
    /// has no such field (each field's own key is a literal `String`,
    /// not a `Type`) - `is_list_like` already covers its one precise,
    /// commonly-useful key fact (are the keys exactly `0, 1, 2, ...`),
    /// so this returns `None` for one rather than trying to synthesize
    /// a real `Type::Int`/`Type::String`/`unknown_array_key()` from its
    /// field names, which none of this phase's own callers need.
    pub(crate) fn array_key_type(ty: &Type) -> Option<Type> {
        match ty {
            Type::Array(key, _) => Some((**key).clone()),
            Type::Intersection(members) => members.iter().find_map(Self::array_key_type),
            _ => None,
        }
    }

    /// Phase 14: whether `ty` is array-shaped at all - for sites that
    /// only need "is this even an array," not the unwrapped element
    /// type `array_element_type` above provides. Sees through a
    /// `list<T>`/`non-empty-array<T>`-typed value the same way, and
    /// through a `Union` of such values (the same control-flow-merge
    /// shape `array_element_type`'s own `Union` arm above handles).
    pub(crate) fn is_array_like(ty: &Type) -> bool {
        matches!(ty, Type::Array(_, _) | Type::ArrayShape(_))
            || matches!(ty, Type::Intersection(members)
                if members.iter().any(|m| matches!(m, Type::Array(_, _) | Type::ArrayShape(_))))
            || matches!(ty, Type::Union(members) if members.iter().all(Self::is_array_like))
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
                        match self.infer_expr(recv, locals) {
                            Type::Object(class, _) => {
                                let sig = self.registry.lookup_method(&class, method);
                                (sig, Some(class))
                            }
                            _ => (None, None),
                        }
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
            return self.infer_expr(l, locals);
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
            _ => r_ty,
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
            BinOp::Eq
            | BinOp::Identical
            | BinOp::NotEq
            | BinOp::NotIdentical
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::LtEq
            | BinOp::GtEq
            | BinOp::And
            | BinOp::Or
            | BinOp::LogicalXor => Type::Bool,
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
                    Type::Float
                } else {
                    Type::Int
                }
            }
            BinOp::Sub | BinOp::Mul | BinOp::Pow => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if let Some(ty) = self.binary_operator_overload_result(op, &lt, &rt) {
                    return ty;
                }
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
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
            BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                self.binary_operator_overload_result(op, &lt, &rt)
                    .unwrap_or(Type::Int)
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
    for (key, ty) in narrowed {
        if is_real_variable_key(key) && !locals.contains_key(key) {
            locals.insert(key.clone(), ty.clone());
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

/// A defensive cap on how many distinct members `union_types` will ever
/// materialize into a real `Type::Union`, mirroring the `MAX_GROUP_DEPTH`/
/// `MAX_EXPR_DEPTH` precedent (Phase 20). Collapsing to `Mixed` past this
/// point is a stable fixed point (`union_types(Mixed, _) == Mixed` is this
/// function's own first special case below), so a variable that blows past
/// the cap once stays collapsed rather than being recomputed on every
/// subsequent merge. Found necessary by a real hang, not guessed: `gdb`
/// caught a `check-project` run spinning inside `Type::eq`, called from
/// `union_types`, called from `merge_scopes` (`checker/control_flow.rs`,
/// once per `if`/`else`) and from `write_index_into`'s own `Union` arm
/// (`checker/assignment.rs`, Phase 12 Finding E). Once Finding E made
/// `Type::Union` "stick" across reads/writes instead of immediately
/// collapsing to `Mixed`, a real corpus function with many sequential and
/// nested branches assigning genuinely distinct array shapes to the same
/// variable grew that variable's own union member count combinatorially -
/// each subsequent `contains`-based dedup scan below (and each member's own
/// recursive `PartialEq`) growing right along with it.
const MAX_UNION_MEMBERS: usize = 12;

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

/// Union of two types for a scope-merge join point - flattens nested
/// unions and keeps `Mixed` "unknown" rather than let it compose into a
/// falsely-precise-looking Union (same principle as the ternary inference).
pub(crate) fn union_types(a: Type, b: Type) -> Type {
    if types::types_equal_for_union(&a, &b) {
        return a;
    }
    if a == Type::Mixed || b == Type::Mixed {
        return Type::Mixed;
    }
    let mut members = Vec::new();
    match a {
        Type::Union(m) => members.extend(m),
        other => members.push(other),
    }
    match b {
        Type::Union(m) => {
            for x in m {
                if !members.iter().any(|m| types::types_equal_for_union(m, &x)) {
                    members.push(x);
                }
            }
        }
        other => {
            if !members
                .iter()
                .any(|m| types::types_equal_for_union(m, &other))
            {
                members.push(other);
            }
        }
    }
    // Phase 32 Finding L: drop a literal member whenever its own exact
    // base type is *also* present in the same union - a ternary's two
    // branches (`is_string($x) ? $x : ''`, `check_expr`'s own `Ternary`
    // arm calling this) build exactly this shape,
    // `union_types(String, StringLiteral(""))`, and keeping both was
    // this project's actual root cause for `SectionInitializer::parse()`'s
    // `str_replace()` subject never matching `== Type::String` again
    // after the ternary. Snapshotting which base types are present
    // first, rather than checking `members` from inside its own
    // `retain` closure (which Rust's borrow checker won't allow).
    // Phase 14: extended to `Array`/`Float` alongside the original
    // `Int`/`String`/`Bool` - `literal_base_type` can now also produce
    // an `Array(_)` base (an accessory-refined `list<T>`/`non-empty-
    // array<T>`) or `Float` (a `FloatLiteral`), and this filter has to
    // actually recognize those as "a base type present in this union"
    // or the new absorption arms in `literal_base_type` would never
    // find a match to absorb into.
    let present_bases: Vec<Type> = members
        .iter()
        .filter(|m| {
            matches!(
                m,
                Type::Int | Type::String | Type::Bool | Type::Array(_, _) | Type::Float
            )
        })
        .cloned()
        .collect();
    members.retain(|m| match literal_base_type(m) {
        Some(base) => !present_bases.contains(&base),
        None => true,
    });
    if members.len() > MAX_UNION_MEMBERS {
        members = generalize_by_key_set(members);
    }
    if members.len() > MAX_UNION_MEMBERS {
        return Type::Mixed;
    }
    if members.len() == 1 {
        members.into_iter().next().unwrap()
    } else {
        Type::Union(members)
    }
}

/// A follow-up to the `MAX_UNION_MEMBERS` cap above: before giving up to
/// `Mixed`, try to merge same-key-set `ArrayShape`s into one shape with
/// unioned field types - only tried once the cap would otherwise fire,
/// never applied to a union already under it, so a small union's
/// behavior is completely unaffected. Two `ArrayShape`s merge only when
/// their field-name *sets* are identical (order-independent) -
/// deliberately not a fuzzier "mostly-the-same-shape" heuristic. This
/// directly targets the exact failure pattern that caused the real hang
/// `MAX_UNION_MEMBERS` itself was added to fix (many sequential/nested
/// branches repeatedly writing the *same* known keys with different
/// value types onto one variable), without touching the cap itself,
/// which stays as the safety net for whatever's left ungeneralizable
/// (e.g. a variable whose branches genuinely have *different* key sets -
/// real, inherent branching precision, correctly left untouched here,
/// still protected by the hard cap if it ever grows past it).
pub(crate) fn generalize_by_key_set(members: Vec<Type>) -> Vec<Type> {
    type ShapeFields = Vec<(String, Type, bool)>;
    let mut groups: Vec<(Vec<String>, Vec<ShapeFields>)> = Vec::new();
    let mut passthrough: Vec<Type> = Vec::new();
    for m in members {
        match m {
            Type::ArrayShape(fields) => {
                let mut key_set: Vec<String> = fields.iter().map(|(k, _, _)| k.clone()).collect();
                key_set.sort();
                match groups.iter_mut().find(|(k, _)| *k == key_set) {
                    Some((_, variants)) => variants.push(fields),
                    None => groups.push((key_set, vec![fields])),
                }
            }
            other => passthrough.push(other),
        }
    }
    for (_, variants) in groups {
        if variants.len() == 1 {
            passthrough.push(Type::ArrayShape(variants.into_iter().next().unwrap()));
            continue;
        }
        let field_names: Vec<String> = variants[0].iter().map(|(k, _, _)| k.clone()).collect();
        let merged = field_names
            .into_iter()
            .map(|name| {
                let mut ty: Option<Type> = None;
                let mut optional = false;
                for fields in &variants {
                    let (_, field_ty, field_opt) =
                        fields.iter().find(|(k, _, _)| *k == name).unwrap();
                    ty = Some(match ty {
                        None => field_ty.clone(),
                        Some(acc) => union_types(acc, field_ty.clone()),
                    });
                    optional |= *field_opt;
                }
                (name, ty.unwrap(), optional)
            })
            .collect();
        passthrough.push(Type::ArrayShape(merged));
    }
    passthrough
}

/// The "lossless absorption" half of `union_types`' own literal-dedup
/// fix - an `IntLiteral`/`StringLiteral`/`True`/`False` member is
/// redundant once its own *exact* base type is already present in the
/// same union (an `Int` member already includes every possible
/// `IntLiteral`'s value, etc.), mirroring `is_subtype`'s own first four
/// match arms specifically (`types.rs` lines 498-500) rather than
/// calling `is_subtype` itself - `is_subtype`'s int-to-float widening
/// rule is a real, load-bearing *assignability* fact (an int argument is
/// accepted wherever a float is declared), not a "this value's
/// representation is a subset of that value's representation" one, so
/// reusing it here would also silently drop an `IntLiteral` sitting next
/// to an unrelated `Float` member, discarding real information for a
/// relation that only holds for compatibility, not equivalence.
fn literal_base_type(t: &Type) -> Option<Type> {
    match t {
        Type::IntLiteral(_) => Some(Type::Int),
        Type::StringLiteral(_) => Some(Type::String),
        Type::True | Type::False => Some(Type::Bool),
        Type::FloatLiteral(_) => Some(Type::Float),
        Type::IntRange(_, _) => Some(Type::Int),
        // Phase 14: scoped to intersections that actually carry an
        // `Accessory` member, so a genuine, unrelated multi-class
        // intersection (`Countable&Iterator`) is never affected - only
        // this plan's own accessory-refined intersections should ever
        // be treated as "more specific than their own base type."
        Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Accessory(_))) => {
            members
                .iter()
                .find(|m| !matches!(m, Type::Accessory(_)))
                .cloned()
        }
        _ => None,
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
fn invalid_array_key_description(ty: &Type) -> Option<String> {
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
fn is_canonical_int_key(key: &str) -> bool {
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
fn is_doctrine_query_get_result(checker: &Checker<'_>, class: &str, method: &str) -> bool {
    method == "getResult"
        && checker
            .registry
            .is_subclass_of(class, "Doctrine\\ORM\\AbstractQuery")
}

fn doctrine_query_result_list_type() -> Type {
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
