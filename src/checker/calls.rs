use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::ast::*;
use crate::checker::expr::is_canonical_int_key;
use crate::checker::{
    Checker, Locals, literal_string_value, purge_stale_narrowing_for, union_types, unwrap_grouping,
};
use crate::registry::{FunctionSig, ParamSig};
use crate::token::Span;
use crate::types::{self, Type};

mod closure_context;
mod format_string;
mod misc_rules;

/// `check_args`'s own call bundle. `callee_label`/`label` are two
/// adjacent, identically-typed `&str` parameters (the callee's display
/// name vs. the enclosing function/method being checked) - a real
/// mis-ordering risk a positional call site can't catch, which named
/// struct fields fix. `args` is the raw, as-written argument list
/// (needed for the count-based checks, which are about how many
/// arguments were actually written); `resolved` is `resolve_call_args`'s
/// per-parameter resolution of that same list, used for the
/// missing-required-parameter test (correct for a named call that fills
/// required parameters out of order, where a raw count alone can't
/// tell) and for the per-argument type-checking loop.
pub(crate) struct CheckArgsCall<'a> {
    pub(crate) callee_label: &'a str,
    pub(crate) sig: &'a FunctionSig,
    pub(crate) args: &'a [Arg],
    pub(crate) resolved: &'a [Option<&'a Expr>],
    pub(crate) locals: &'a Locals,
    pub(crate) label: &'a str,
    pub(crate) defining_class: Option<&'a str>,
    pub(crate) span: Span,
}

impl<'a> Checker<'a> {
    /// No-op unless `collect_inlay_hints` is set (the LSP's
    /// `textDocument/inlayHint` collection mode) - every ordinary check
    /// pays only this one boolean test. `sig` is `None` for an
    /// unresolvable callee (dynamic, unknown function/method, or an
    /// unresolvable `call_user_func(_array)` callable), in which case
    /// there's nothing to hint.
    pub(crate) fn maybe_record_param_name_hints(
        &mut self,
        sig: Option<&FunctionSig>,
        args: &[Arg],
    ) {
        if !self.collect_inlay_hints {
            return;
        }
        let Some(sig) = sig else {
            return;
        };
        self.inlay_param_name_hints
            .extend(call_arg_param_name_hints(args, &sig.params));
    }

    /// A bare-variable argument that isn't defined yet is either a real bug
    /// or PHP's by-reference "output parameter" idiom
    /// (`preg_match($pattern, $subject, $matches)` defines `$matches` as a
    /// side effect). Without tracking every stdlib function's exact
    /// by-ref positions, the safe default for a call to an *unknown*
    /// function is to assume it might be exactly that and define the
    /// variable (as Mixed) rather than flag it; a *known* function only
    /// gets this leniency for the specific parameter position actually
    /// marked by-ref.
    /// `sig: None` (an unresolvable call) conservatively treats any
    /// bare-variable argument, in its own written position, as a
    /// definition site. Once a real signature is known, `resolved`
    /// (the caller's already-computed per-parameter resolution) is used
    /// instead of the raw argument list - a named or reordered-named
    /// by-ref argument must be matched against its real declared
    /// parameter, not whichever parameter sits at the same index.
    pub(crate) fn predefine_byref_or_unknown_args(
        &self,
        call_name: Option<&str>,
        sig: Option<&FunctionSig>,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &mut Locals,
    ) {
        let Some(sig) = sig else {
            for arg in args {
                let ExprKind::Variable(name) = &arg.value.kind else {
                    continue;
                };
                // Real PHP by-ref auto-vivification: an unhinted
                // `function(&$x)` called with an as-yet-undefined `$x`
                // is this variable's genuine first appearance -
                // `declare`, not `insert`, matching
                // `bind_assignment_target`'s own index-write treatment.
                if !locals.contains_key(name) {
                    locals.declare(name.to_string(), Type::Mixed(false), arg.value.span);
                }
            }
            return;
        };
        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg_expr) = slot else {
                continue;
            };
            let ExprKind::Variable(name) = &arg_expr.kind else {
                continue;
            };
            // Both the by-ref flag and the declared parameter itself need
            // the identical primary-then-variadic-fallback lookup -
            // `resolved` can extend past `sig.params`'s own length for a
            // variadic tail argument.
            let param = sig
                .params
                .get(i)
                .or_else(|| sig.params.last().filter(|p| p.variadic));
            let Some(param) = param else {
                continue;
            };
            if !param.by_ref {
                continue;
            }
            if !locals.contains_key(name) {
                locals.declare(name.to_string(), Type::Mixed(false), arg_expr.span);
                continue;
            }
            // Real PHPStan models the sort() family's own dynamic
            // type-specifying extensions rather than the generic
            // declared-param-type widening below: sort/rsort/usort/
            // shuffle always reindex their by-ref argument to a real
            // list, preserving element type (and non-emptiness, when
            // known); asort/arsort/ksort/krsort/uasort/uksort/natsort/
            // natcasesort preserve the exact key structure unchanged.
            if let Some(fn_name) = call_name {
                let current = locals.get(name).cloned().unwrap_or(Type::Mixed(false));
                match fn_name {
                    "sort" | "rsort" | "usort" | "shuffle" => {
                        if let Some(elem_ty) = Checker::array_element_type(&current) {
                            let mut members = vec![
                                Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem_ty)),
                                Type::Accessory(types::AccessoryKind::ArrayList),
                            ];
                            if Checker::is_known_non_empty_array(&current) {
                                members.push(Type::Accessory(types::AccessoryKind::NonEmptyArray));
                            }
                            purge_stale_narrowing_for(locals, name);
                            locals.insert(name.to_string(), Type::Intersection(members));
                            continue;
                        }
                    }
                    "asort" | "arsort" | "ksort" | "krsort" | "uasort" | "uksort" | "natsort"
                    | "natcasesort" => continue,
                    // `reset`/`end`/`next`/`prev` (`current`/`key` are
                    // native by-value, never reach this by-ref-only loop
                    // at all) only ever move the array's own internal
                    // pointer - none of them ever change the array's
                    // *value* at all (confirmed via `dumpType()` against
                    // real PHPStan: a precise argument type, including
                    // one this project's own dynamic-return-type logic
                    // for these exact functions relies on -
                    // `infer_builtin_call_return_type`'s own `"reset" |
                    // "current" | "end"` arm - survives `reset()`
                    // completely unchanged). Without this, the by-ref
                    // stub's own declared `object|array &$array` param
                    // type (real shape: `phpstorm-stubs`' own `function
                    // reset(object|array &$array): mixed`) would widen a
                    // precise argument down to the generic declared-type
                    // fallback below, discarding real information the
                    // *same* call's own return-type computation still
                    // needs.
                    "reset" | "end" | "next" | "prev" => continue,
                    // `array_walk` never adds, removes, or reorders keys,
                    // and never changes a value's type unless the
                    // callback's `$value` parameter is itself declared
                    // by-ref and reassigns it - not tracked through a
                    // closure literal's body, so left unchanged rather
                    // than falling through to the generic widening below,
                    // which would otherwise discard `$array`'s real
                    // element/key types (the stub's declared
                    // `array|object &$array` param type fails
                    // `Checker::is_array_like`'s guard and widens to
                    // `Mixed`).
                    "array_walk" => continue,
                    // `array_push`/`array_unshift` always append/prepend
                    // at least one real value, so a list-like (or empty)
                    // `$array` stays a list afterward, now provably
                    // non-empty - element type widens to include whatever
                    // was pushed. A non-list-like input keeps its odd key
                    // structure and falls through to the generic
                    // declared-type widening below unchanged.
                    "array_push" | "array_unshift" if Checker::is_list_like(&current) => {
                        if let Some(elem_ty) = Checker::array_element_type(&current) {
                            let pushed_ty = args[1..]
                                .iter()
                                .map(|a| self.infer_expr(&a.value, locals))
                                .reduce(union_types);
                            let combined_elem = match pushed_ty {
                                Some(pushed_ty) => union_types(elem_ty, pushed_ty),
                                None => elem_ty,
                            };
                            let members = vec![
                                Type::Array(
                                    Box::new(Type::unknown_array_key()),
                                    Box::new(combined_elem),
                                ),
                                Type::Accessory(types::AccessoryKind::ArrayList),
                                Type::Accessory(types::AccessoryKind::NonEmptyArray),
                            ];
                            purge_stale_narrowing_for(locals, name);
                            locals.insert(name.to_string(), Type::Intersection(members));
                            continue;
                        }
                    }
                    // `array_shift`/`array_pop` remove an element rather
                    // than adding one - list-ness survives the same way
                    // (both reindex/leave sequential integer keys
                    // sequential), but non-emptiness doesn't: removing
                    // the one remaining element of a single-element list
                    // leaves `[]`, so any existing non-empty marker is
                    // deliberately dropped rather than carried through.
                    "array_shift" | "array_pop" if Checker::is_list_like(&current) => {
                        if let Some(elem_ty) = Checker::array_element_type(&current) {
                            let members = vec![
                                Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem_ty)),
                                Type::Accessory(types::AccessoryKind::ArrayList),
                            ];
                            purge_stale_narrowing_for(locals, name);
                            locals.insert(name.to_string(), Type::Intersection(members));
                            continue;
                        }
                    }
                    // `array_splice` always removes at most
                    // `count($array)` elements (real PHP clamps the
                    // removal count regardless of `$offset`/`$length`),
                    // so `count($after) = count(before) - removed +
                    // count($replacement) >= count($replacement)`
                    // unconditionally - a provably non-empty literal
                    // `$replacement` alone proves the result non-empty,
                    // independent of `$offset`/`$length`/the array's own
                    // prior state (confirmed against real PHPStan's own
                    // `ArrayType::spliceArray()`, which uses the
                    // identical rule). List-ness survives the same way
                    // `array_push`/`array_shift` already do; the element
                    // type widens to include the replacement's own.
                    "array_splice" if Checker::is_list_like(&current) => {
                        if let Some(elem_ty) = Checker::array_element_type(&current) {
                            let replacement_ty =
                                args.get(3).map(|a| self.infer_expr(&a.value, locals));
                            let (combined_elem, non_empty) = match &replacement_ty {
                                Some(r) => (
                                    union_types(
                                        elem_ty,
                                        Checker::array_element_type(r)
                                            .unwrap_or(Type::Mixed(false)),
                                    ),
                                    Checker::is_known_non_empty_array(r),
                                ),
                                None => (elem_ty, false),
                            };
                            let mut members = vec![
                                Type::Array(
                                    Box::new(Type::unknown_array_key()),
                                    Box::new(combined_elem),
                                ),
                                Type::Accessory(types::AccessoryKind::ArrayList),
                            ];
                            if non_empty {
                                members.push(Type::Accessory(types::AccessoryKind::NonEmptyArray));
                            }
                            purge_stale_narrowing_for(locals, name);
                            locals.insert(name.to_string(), Type::Intersection(members));
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            // An already-defined variable passed by-ref widens to the
            // declared parameter's own type, not a blanket `Type::Mixed(false)`
            // - `array &$errors` becomes `Type::Array(Mixed)`, preserving
            // the type-hint-guaranteed "still an array" information.
            // `substitute_template` with empty bindings replaces any
            // unresolved `TemplateParam` with `Mixed` too, since proper
            // substitution would need the receiver's own type arguments,
            // not available at this call site.
            //
            // Only trust the substituted declared type when it's itself
            // plainly array-shaped (`Type::Array`/`Type::ArrayShape`):
            // several stdlib by-ref parameters (`array_walk(array|object
            // &$array, ...)`) declare a union wider than what the
            // already-array-typed argument could actually become - the
            // parameter type says what the callee accepts, not what an
            // already-narrower-typed argument turns into. Anything else
            // falls back to `Mixed`, matching the undefined-variable case
            // above.
            let substituted = types::substitute_template(&param.ty, &HashMap::new());
            // `Checker::is_array_like` also trusts a `list<T>`/
            // `non-empty-array<T>`-declared by-ref parameter (an
            // `Intersection`), rather than falling into the "anything
            // else" `Mixed` catch-all a wider declared `Union`/`Object`
            // deliberately does.
            let widened = if Checker::is_array_like(&substituted) {
                substituted
            } else {
                Type::Mixed(false)
            };
            // A by-ref call argument is exactly as much a "this variable
            // now holds something else" event as a direct assignment -
            // without this, a stale composite key rooted at `name` would
            // keep answering for whatever the callee may have replaced
            // it with.
            purge_stale_narrowing_for(locals, name);
            locals.insert(name.to_string(), widened);
        }
    }

    #[hotpath::measure]
    pub(crate) fn check_args(&mut self, call: CheckArgsCall) {
        let CheckArgsCall {
            callee_label,
            sig,
            args,
            resolved,
            locals,
            label,
            defining_class,
            span,
        } = call;
        // Naming/ordering legality first - independent of, and prior to,
        // every count/type check below.
        self.check_argument_legality(callee_label, sig, args, label);

        if self.registry.check_boolean_trap {
            self.check_boolean_trap_arguments(callee_label, sig, args, defining_class, label);
        }
        if self.registry.check_swapped_arguments {
            self.check_swapped_argument_names(callee_label, sig, args, defining_class, label);
        }

        // A spread/unpack argument's own real contents can't be resolved
        // statically (see `resolve_call_args`'s own doc comment) - every
        // declared parameter at or past the positional index the spread
        // occupies might still be filled by it at runtime, so neither
        // the "missing required parameter" nor the "too many arguments"
        // check below can trust a `None` slot (or a raw `args.len()`) at
        // or past that point. `None` here still lets both checks run
        // precisely for everything before the spread.
        let spread_from = first_spread_positional_index(args);
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.has_default)
            .count();
        let required_scan_len = spread_from.map_or(required, |sp| sp.min(required));
        let missing_required = resolved
            .iter()
            .take(required_scan_len)
            .filter(|s| s.is_none())
            .count();
        if missing_required > 0 {
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() expects at least {required} argument(s), {} given",
                    args.len()
                ),
                span,
                0,
                "arguments.count",
            );
        }

        // Calling a non-variadic function/method with more positional
        // arguments than it can accept is a real PHP fatal error ("Too
        // many arguments... expected exactly N"); the per-argument loop
        // below wouldn't catch it, since it simply stops once
        // `sig.params.get(i)` runs out and the last param isn't
        // variadic. Uses the raw `args.len()` deliberately, not
        // `resolved`'s length - a non-variadic signature's resolved
        // slice is always exactly `params.len()` long by
        // `resolve_call_args`'s design, which would make this check
        // always read `false` if it read from that shape instead.
        let last_is_variadic = sig.params.last().is_some_and(|p| p.variadic);
        if spread_from.is_none() && !last_is_variadic && args.len() > sig.params.len() {
            let quantifier = if required == sig.params.len() {
                "exactly"
            } else {
                "at most"
            };
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() expects {quantifier} {} argument(s), {} given",
                    sig.params.len(),
                    args.len()
                ),
                span,
                0,
                "arguments.count",
            );
        }

        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg) = slot else {
                continue;
            };
            let param = match sig.params.get(i) {
                Some(p) => p,
                None => match sig.params.last() {
                    Some(p) if p.variadic => p,
                    _ => continue,
                },
            };
            // By-ref target legality: independent of - and checked
            // before - the type check below, since it's about whether
            // `arg` is even a real storage location at all, not what
            // its value's type is. A nullsafe-chain root is checked
            // first and reported with its own, more specific reason
            // (short-circuiting, not "not an lvalue") even though a
            // nullsafe chain is *also* not one of `is_valid_byref_
            // argument_expr`'s legal shapes.
            if param.by_ref {
                if contains_nullsafe_access(arg) {
                    self.report(
                        label.to_string(),
                        format!(
                            "{callee_label}() argument #{} (${}) is passed by reference, but its access chain contains a nullsafe operator (?->), which may short-circuit and never write back",
                            i + 1,
                            param.name
                        ),
                        arg.span,
                        0,
                        "argument.byRef",
                    );
                } else if !is_valid_byref_argument_expr(arg) {
                    self.report(
                        label.to_string(),
                        format!(
                            "{callee_label}() argument #{} (${}) is passed by reference, so it expects a variable, not an expression",
                            i + 1,
                            param.name
                        ),
                        arg.span,
                        0,
                        "argument.byRef",
                    );
                }
            }
            let arg_ty = self.infer_expr(arg, locals);
            // `accepts_at_configured_level` already accepts a `Mixed`
            // argument unconditionally below real PHPStan's own level
            // 9/10 `checkExplicitMixed`/`checkImplicitMixed` threshold -
            // no separate early bail-out needed, and one here would
            // wrongly skip this check entirely once either is enabled.
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(&param.ty, c),
                None => param.ty.clone(),
            };
            if !self
                .registry
                .accepts_at_configured_level(&arg_ty, &param_ty)
            {
                self.report(
                    label.to_string(),
                    format!(
                        "{callee_label}() argument #{} (${}) expects {}, {} given",
                        i + 1,
                        param.name,
                        param_ty,
                        arg_ty
                    ),
                    arg.span,
                    5,
                    "argument.type",
                );
            }
        }

        self.check_constant_array_spread_arguments(
            callee_label,
            sig,
            args,
            resolved,
            locals,
            label,
            defining_class,
        );
    }

    /// Argument-checking for a call through a value statically known to
    /// be a shaped `Closure(...)`-typed callable
    /// (`$fn(...)`, `$arr['make']()`, `($this->makeCallback())()`, etc) -
    /// a deliberately trimmed, standalone subset of `check_args`'s own
    /// count/type loop, not a reuse of it: `CallableSig`'s own
    /// `CallableParam` carries no parameter name at all (a PHPDoc
    /// callable shape never spells one out), so named-argument
    /// resolution isn't possible here. Positional-only: a named or
    /// spread argument silently skips this whole check rather than
    /// guessing. Reuses `check_args`'s `arguments.count`/`argument.type`
    /// identifiers and message shape, with `"Closure"` standing in for a
    /// real callee name (one doesn't exist for a dynamically-held
    /// callable value).
    pub(crate) fn check_dynamic_callable_args(
        &mut self,
        sig: &types::CallableSig,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: Span,
    ) {
        if args.iter().any(|a| a.name.is_some() || a.spread) {
            return;
        }
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.optional)
            .count();
        let last_is_variadic = sig.params.last().is_some_and(|p| p.variadic);
        if args.len() < required {
            self.report(
                label.to_string(),
                format!(
                    "Closure() expects at least {required} argument(s), {} given",
                    args.len()
                ),
                span,
                0,
                "arguments.count",
            );
        } else if !last_is_variadic && args.len() > sig.params.len() {
            let quantifier = if required == sig.params.len() {
                "exactly"
            } else {
                "at most"
            };
            self.report(
                label.to_string(),
                format!(
                    "Closure() expects {quantifier} {} argument(s), {} given",
                    sig.params.len(),
                    args.len()
                ),
                span,
                0,
                "arguments.count",
            );
        }
        for (i, a) in args.iter().enumerate() {
            let param = match sig.params.get(i) {
                Some(p) => p,
                None => match sig.params.last() {
                    Some(p) if p.variadic => p,
                    _ => continue,
                },
            };
            let arg_ty = self.infer_expr(&a.value, locals);
            // See the identical, earlier comment in this file's own
            // ordinary-call argument check for why no separate `Mixed`
            // bail-out is needed here.
            if !self
                .registry
                .accepts_at_configured_level(&arg_ty, &param.ty)
            {
                self.report(
                    label.to_string(),
                    format!(
                        "Closure() argument #{} expects {}, {} given",
                        i + 1,
                        param.ty,
                        arg_ty
                    ),
                    a.value.span,
                    5,
                    "argument.type",
                );
            }
        }
    }

    /// `f(...['a', 'b', 'named' => $c])`-style spread arguments whose
    /// inferred type is a literal `Type::ArrayShape` have every eventual
    /// argument slot known statically - real PHP resolves each int-keyed
    /// shape field positionally (in shape order, not by its literal key
    /// value) and each string-keyed field by name, like an ordinary
    /// named argument. Every other check in `check_args` gives up the
    /// moment it sees a spread (see `resolve_call_args`'s doc comment)
    /// because an arbitrary spread's contents can't be resolved
    /// statically - this recovers type-checking for the one common,
    /// fully static shape.
    #[allow(clippy::too_many_arguments)]
    fn check_constant_array_spread_arguments(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &Locals,
        label: &str,
        defining_class: Option<&str>,
    ) {
        if args.iter().filter(|a| a.spread).count() != 1 {
            return;
        }
        let spread_index = args.iter().position(|a| a.spread).unwrap();
        if !args[spread_index + 1..].iter().all(|a| a.name.is_some()) {
            return;
        }
        let spread_arg = &args[spread_index];
        let spread_ty = self.infer_expr(&spread_arg.value, locals);
        let Some(fields) = array_shape_fields(&spread_ty) else {
            return;
        };
        if fields.iter().any(|(k, _, _)| k == "...") {
            return;
        }
        let Some(mut positional_counter) = first_spread_positional_index(args) else {
            return;
        };
        for (key, field_ty, optional) in fields {
            if *optional {
                continue;
            }
            let param = if is_canonical_int_key(key) {
                let p = sig
                    .params
                    .get(positional_counter)
                    .or_else(|| sig.params.last().filter(|p| p.variadic));
                positional_counter += 1;
                p
            } else {
                sig.params.iter().find(|p| p.name.as_ref() == key.as_str())
            };
            let Some(param) = param else {
                continue;
            };
            let already_filled = sig
                .params
                .iter()
                .position(|p| std::ptr::eq(p, param))
                .and_then(|pos| resolved.get(pos))
                .is_some_and(|s| s.is_some());
            if already_filled {
                continue;
            }
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(&param.ty, c),
                None => param.ty.clone(),
            };
            if !self
                .registry
                .accepts_at_configured_level(field_ty, &param_ty)
            {
                self.report(
                    label.to_string(),
                    format!(
                        "{callee_label}() argument (${}) expects {}, {} given",
                        param.name, param_ty, field_ty
                    ),
                    spread_arg.value.span,
                    5,
                    "argument.type",
                );
            }
        }
    }

    /// A bare `true`/`false` literal passed *positionally* into a
    /// bare-`bool`-typed parameter is always type-correct, so no
    /// type-checking rule can ever see it. Named-argument and spread
    /// calls are exempt (the name/callee already documents the value);
    /// mirrors `resolve_call_args`'s own positional-index bookkeeping
    /// rather than its resolved output, which loses the named-vs-
    /// positional distinction this check depends on.
    fn check_boolean_trap_arguments(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Arg],
        defining_class: Option<&str>,
        label: &str,
    ) {
        let mut positional_index = 0usize;
        for arg in args {
            if arg.spread || arg.name.is_some() {
                continue;
            }
            let index = positional_index;
            positional_index += 1;
            let literal = match &unwrap_grouping(&arg.value).kind {
                ExprKind::Literal(Literal::Bool(v)) => *v,
                _ => continue,
            };
            let Some(param) = sig.params.get(index) else {
                continue;
            };
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(&param.ty, c),
                None => param.ty.clone(),
            };
            if param_ty != Type::Bool {
                continue;
            }
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() argument #{} (${}) is a bare `{literal}` literal passed positionally into a bool parameter - consider a named argument or a self-documenting constant instead",
                    index + 1,
                    param.name
                ),
                arg.value.span,
                0,
                "argument.booleanTrap",
            );
        }
    }

    /// Two adjacent, same-typed positional parameters are structurally
    /// invisible to a type-checker as "different roles" (`resize(int
    /// $width, int $height)` type-checks a swapped `resize($height,
    /// $width)` call just fine) - a disclosed heuristic. Fires only on
    /// the exact "crossed names" shape: both arguments are bare
    /// variables, and each one's name matches the *other* position's
    /// declared parameter name case-insensitively - never a fuzzy/
    /// partial match, to keep the false-positive rate low.
    fn check_swapped_argument_names(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Arg],
        defining_class: Option<&str>,
        label: &str,
    ) {
        let mut positional: Vec<(usize, &Expr)> = Vec::new();
        let mut positional_index = 0usize;
        for arg in args {
            if arg.spread || arg.name.is_some() {
                continue;
            }
            let index = positional_index;
            positional_index += 1;
            if let ExprKind::Variable(_) = &unwrap_grouping(&arg.value).kind {
                positional.push((index, &arg.value));
            }
        }
        for window in positional.windows(2) {
            let [(i, a), (j, b)] = window else { continue };
            if *j != *i + 1 {
                continue;
            }
            let ExprKind::Variable(a_name) = &unwrap_grouping(a).kind else {
                continue;
            };
            let ExprKind::Variable(b_name) = &unwrap_grouping(b).kind else {
                continue;
            };
            let (Some(param_a), Some(param_b)) = (sig.params.get(*i), sig.params.get(*j)) else {
                continue;
            };
            if param_a.name.eq_ignore_ascii_case(&param_b.name) {
                continue;
            }
            if !a_name.eq_ignore_ascii_case(&param_b.name)
                || !b_name.eq_ignore_ascii_case(&param_a.name)
            {
                continue;
            }
            let ty_a = match defining_class {
                Some(c) => self.resolve_pseudo(&param_a.ty, c),
                None => param_a.ty.clone(),
            };
            let ty_b = match defining_class {
                Some(c) => self.resolve_pseudo(&param_b.ty, c),
                None => param_b.ty.clone(),
            };
            if ty_a == Type::Mixed(false) || ty_a != ty_b {
                continue;
            }
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() arguments #{} (${a_name}) and #{} (${b_name}) may be swapped - their variable names match the reversed parameter order (${}, ${})",
                    i + 1,
                    j + 1,
                    param_a.name,
                    param_b.name
                ),
                a.span,
                5,
                "arguments.possiblySwapped",
            );
        }
    }

    /// Validates a call's argument list for the naming/ordering legality
    /// real PHP itself enforces, independent of which parameter each
    /// argument actually resolves to (`resolve_call_args`'s job).
    /// Mirrors real PHPStan's `FunctionCallParametersCheck::
    /// processArguments` plus its raw-args ordering pre-pass, reusing
    /// its identifiers verbatim (`argument.unknown`, `argument.duplicate`,
    /// `argument.positionalAfterNamed`, `argument.unpackAfterNamed`,
    /// `argument.nonUnpackAfterUnpacked`).
    ///
    /// The three ordering checks are deliberately `else if` against each
    /// other (real PHPStan instead runs an ordering pre-pass over the raw
    /// argument list and a separate, later pass over an *expanded*
    /// argument list - one that turns a spread over a known constant
    /// array into one synthetic argument per element - to get
    /// `positionalAfterNamed` from a genuinely different pass than
    /// `unpackAfterNamed`/`nonUnpackAfterUnpacked`; this project doesn't
    /// attempt that constant-array expansion, so the three collapse onto
    /// the same raw-args loop here) - a single malformed argument only
    /// ever gets one ordering complaint, not a pile of overlapping ones
    /// for what's really the same mistake.
    fn check_argument_legality(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Arg],
        label: &str,
    ) {
        let variadic = sig.params.last().is_some_and(|p| p.variadic);
        let mut named_seen = false;
        let mut spread_seen = false;
        let mut used_param_names: HashSet<&str> = HashSet::new();
        let mut positional_index = 0usize;

        for arg in args {
            if named_seen && arg.name.is_none() && !arg.spread {
                self.report(
                    label.to_string(),
                    format!(
                        "named argument cannot be followed by a positional argument in call to {callee_label}()"
                    ),
                    arg.value.span,
                    0,
                    "argument.positionalAfterNamed",
                );
            } else if named_seen && arg.spread {
                self.report(
                    label.to_string(),
                    format!(
                        "named argument cannot be followed by an unpacked (...) argument in call to {callee_label}()"
                    ),
                    arg.value.span,
                    0,
                    "argument.unpackAfterNamed",
                );
            } else if spread_seen && arg.name.is_none() && !arg.spread {
                self.report(
                    label.to_string(),
                    format!(
                        "unpacked (...) argument cannot be followed by a non-unpacked argument in call to {callee_label}()"
                    ),
                    arg.value.span,
                    0,
                    "argument.nonUnpackAfterUnpacked",
                );
            }

            if arg.spread {
                spread_seen = true;
                continue;
            }

            if let Some(name) = &arg.name {
                named_seen = true;
                match sig.params.iter().find(|p| p.name.as_ref() == name.as_str()) {
                    // A named argument targeting the real, declared
                    // variadic parameter's own name (rather than an
                    // unmatched name collected *into* it, the `None`
                    // arm below) is just an ordinary named argument.
                    Some(_) => {
                        if !used_param_names.insert(name.as_str()) {
                            self.report(
                                label.to_string(),
                                format!(
                                    "argument for parameter ${name} has already been passed to {callee_label}()"
                                ),
                                arg.value.span,
                                0,
                                "argument.duplicate",
                            );
                        }
                    }
                    // No declared parameter has this name - legal only
                    // when there's a trailing variadic parameter to
                    // collect it into (real PHP's own named-argument-
                    // into-variadic support), and then only once per
                    // name.
                    None if variadic => {
                        if !used_param_names.insert(name.as_str()) {
                            self.report(
                                label.to_string(),
                                format!(
                                    "named parameter ${name} overwrites a previous argument to {callee_label}()"
                                ),
                                arg.value.span,
                                0,
                                "argument.duplicate",
                            );
                        }
                    }
                    None => {
                        self.report(
                            label.to_string(),
                            format!("unknown named parameter ${name} in call to {callee_label}()"),
                            arg.value.span,
                            0,
                            "argument.unknown",
                        );
                    }
                }
                continue;
            }

            // A plain positional argument, still legally placeable
            // (nothing above flagged it): record which declared
            // parameter it claims, the same way a named argument does
            // above, so a *later* named argument re-targeting that same
            // parameter is caught as a duplicate too.
            if !spread_seen && let Some(p) = sig.params.get(positional_index) {
                used_param_names.insert(p.name.as_ref());
            }
            positional_index += 1;
        }
    }

    /// `RegularExpressionQuotingRule`'s equivalent - applies to the 7
    /// pattern-taking functions whose pattern argument sits at position 0
    /// and is written as a string concatenation. Finds the pattern's real
    /// delimiter (the leftmost concat operand's literal first character),
    /// then walks the whole concat tree for any `preg_quote(...)` call,
    /// flagging one with no delimiter argument at all, or one whose given
    /// delimiter doesn't match the pattern's real delimiter and isn't one
    /// of `preg_quote`'s own always-escaped characters (mirrors real
    /// PHPStan's `isDefaultEscaped`/`removeDefaultEscapedDelimiters`
    /// exactly - a mismatch against one of those is inconsequential,
    /// since `preg_quote` escapes it either way).
    pub(crate) fn check_preg_quote_delimiter(&mut self, name: &str, args: &[Arg], label: &str) {
        const PREG_PATTERN_FUNCTIONS: &[&str] = &[
            "preg_match",
            "preg_match_all",
            "preg_filter",
            "preg_grep",
            "preg_replace",
            "preg_replace_callback",
            "preg_split",
        ];
        if !PREG_PATTERN_FUNCTIONS
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name))
        {
            return;
        }
        let Some(pattern_arg) = args.first() else {
            return;
        };
        let pattern_expr = unwrap_grouping(&pattern_arg.value);
        let ExprKind::Binary(BinOp::Concat, _, _) = &pattern_expr.kind else {
            return;
        };
        let Some(delimiter) = concat_leftmost_delimiter(pattern_expr) else {
            return;
        };
        // `preg_quote($str)` always escapes this fixed character set
        // regardless of any `$delimiter` argument - when the pattern's
        // real delimiter already falls in it, it's covered for free, so
        // neither a missing `$delimiter` argument nor a mismatched one
        // actually leaves it unescaped. Tested against the pattern's own
        // delimiter, not the given one - testing the given one would let
        // `#` pass for a `/`-delimited pattern purely because `#` itself
        // is in this set, even though `/` stays unescaped.
        const ALWAYS_ESCAPED: &[char] = &[
            '.', '\\', '+', '*', '?', '[', '^', ']', '$', '(', ')', '{', '}', '=', '!', '<', '>',
            '|', ':', '-', '#',
        ];
        if ALWAYS_ESCAPED.contains(&delimiter) {
            return;
        }
        let mut quote_calls = Vec::new();
        find_preg_quote_calls(pattern_expr, &mut quote_calls);
        for call_args in quote_calls {
            if call_args.len() == 1 {
                self.report(
                    label.to_string(),
                    format!(
                        "Call to preg_quote() is missing delimiter {delimiter} to be effective."
                    ),
                    call_args[0].value.span,
                    0,
                    "argument.invalidPregQuote",
                );
                continue;
            }
            let Some(given) = call_args
                .get(1)
                .and_then(|a| literal_string_value(&a.value))
            else {
                continue;
            };
            if !given.starts_with(delimiter) {
                self.report(
                    label.to_string(),
                    format!(
                        "Call to preg_quote() uses invalid delimiter {given} while pattern uses {delimiter}."
                    ),
                    call_args[1].value.span,
                    0,
                    "argument.invalidPregQuote",
                );
            }
        }
    }

    /// Resolves the two common, statically-knowable
    /// `call_user_func(_array)` callable shapes to a real `FunctionSig` -
    /// a plain string-literal function name, or a two-element array
    /// literal naming a class/object plus a literal method name. Any
    /// other shape (a closure literal, a bare variable of unknown type,
    /// `Closure::fromCallable(...)`, an invokable object, ...) returns
    /// `None` rather than guessing.
    fn resolve_call_user_func_callable(
        &self,
        callable_expr: &Expr,
        locals: &Locals,
    ) -> Option<&FunctionSig> {
        let callable_expr = unwrap_grouping(callable_expr);
        if let Some(name) = literal_string_value(callable_expr) {
            return self.registry.lookup_function(name.trim_start_matches('\\'));
        }
        let ty = self.infer_expr(callable_expr, locals);
        let Type::ArrayShape(fields) = &ty else {
            return None;
        };
        let method_ty = fields
            .iter()
            .find(|(k, _, _)| k == "1")
            .map(|(_, t, _)| t)?;
        let Type::StringLiteral(method) = method_ty else {
            return None;
        };
        let class_ty = fields
            .iter()
            .find(|(k, _, _)| k == "0")
            .map(|(_, t, _)| t)?;
        let class = match class_ty {
            Type::StringLiteral(c) => c.as_str(),
            Type::ClassString(inner) => match inner.as_ref() {
                Type::Object(c, _) => c.as_str(),
                _ => return None,
            },
            Type::Object(c, _) => c.as_str(),
            _ => return None,
        };
        self.registry.lookup_method(class, method)
    }

    /// `call_user_func`/`call_user_func_array`'s argument re-checking -
    /// once the callable resolves (see `resolve_call_user_func_callable`'s
    /// doc comment for the two shapes this covers), the resolved
    /// arguments feed straight into the existing, call-shape-agnostic
    /// `check_args`, reusing its `arguments.count`/`argument.type`/
    /// `argument.byRef` diagnostics verbatim. `call_user_func_array`'s
    /// second argument is only trusted when written as a literal array
    /// expression at the call site (mirrors `vsprintf`/`vprintf`'s "trust
    /// only a literal/sequential shape" precedent) - real `Arg` nodes
    /// (needed for `check_args`'s by-ref/span handling) only exist for a
    /// literal array's elements, not for a value reached only through
    /// its inferred `Type::ArrayShape`.
    pub(crate) fn check_call_user_func(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &mut Locals,
        label: &str,
        span: Span,
    ) {
        if name.eq_ignore_ascii_case("call_user_func") {
            if args.is_empty() || args[1..].iter().any(|a| a.spread) {
                return;
            }
            let Some(sig) = self.resolve_call_user_func_callable(&args[0].value, locals) else {
                return;
            };
            let sig = sig.clone();
            let callee_args = &args[1..];
            let resolved = resolve_call_args(callee_args, &sig.params);
            self.maybe_record_param_name_hints(Some(&sig), callee_args);
            self.check_args(CheckArgsCall {
                callee_label: "callable passed to call_user_func()",
                sig: &sig,
                args: callee_args,
                resolved: &resolved,
                locals,
                label,
                defining_class: None,
                span,
            });
        } else if name.eq_ignore_ascii_case("call_user_func_array") {
            if args.len() != 2 {
                return;
            }
            let Some(sig) = self.resolve_call_user_func_callable(&args[0].value, locals) else {
                return;
            };
            let sig = sig.clone();
            let ExprKind::ArrayLiteral(items) = &unwrap_grouping(&args[1].value).kind else {
                return;
            };
            if items.iter().any(|item| item.spread || item.key.is_some()) {
                return;
            }
            let synthesized: Vec<Arg> = items
                .iter()
                .map(|item| Arg {
                    name: None,
                    spread: false,
                    value: item.value.clone(),
                })
                .collect();
            let resolved = resolve_call_args(&synthesized, &sig.params);
            self.maybe_record_param_name_hints(Some(&sig), &synthesized);
            self.check_args(CheckArgsCall {
                callee_label: "callable passed to call_user_func_array()",
                sig: &sig,
                args: &synthesized,
                resolved: &resolved,
                locals,
                label,
                defining_class: None,
                span,
            });
        }
    }
}

/// PHP superglobals are implicitly defined in every scope without any
/// assignment ever appearing in source - undefined-variable detection must
/// never flag these.
/// Real PHP language constructs that are syntactically indistinguishable
/// from an ordinary function call in this parser (none of `isset`,
/// `empty`, `unset`, `eval`, `exit`, `die` are lexed as keywords - see
/// `token.rs` - so `isset($x)` parses as `ExprKind::Call(Name("isset"), ...)`
/// exactly like a real call). None of them are declared in any stub
/// source (they're compiler intrinsics, not functions - a real PHP
/// function can be called through a variable, `$f = 'isset'; $f();` is a
/// fatal error), so without this list every real codebase's constant use
/// of `isset`/`empty`/`unset` would be flagged as calling an undefined
/// function. Matched case-insensitively since PHP itself treats function
/// (and construct) names case-insensitively.
pub(crate) fn is_language_construct(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "isset"
            | "empty"
            | "unset"
            | "eval"
            | "exit"
            | "die"
            // The parser deliberately models these four as a call to a
            // synthetic name (see parser.rs's own comment on that
            // decision) rather than a dedicated AST node - real language
            // constructs, not functions, so they belong on this list for
            // exactly the same reason as isset/empty/unset above.
            | "include"
            | "include_once"
            | "require"
            | "require_once"
            // Same synthetic-call modeling, for the same reason.
            | "print"
    )
}

/// Resolves a call's argument list against the declared parameter list
/// it's being checked against, producing one slot per declared parameter:
/// `Some(&Expr)` when some argument (positional or named) actually
/// fills that parameter, `None` when it doesn't (an omitted optional
/// parameter, or a parameter a spread/unpack argument might fill, whose
/// real contents can't be resolved statically). This lets a named or
/// reordered-named call resolve against the real parameter, not
/// whichever one happens to sit at the same index in the written
/// argument list, since `args[i]` only lines up with `params[i]` for
/// the all-positional case.
///
/// The returned `Vec` can be longer than `params.len()` when the last
/// parameter is variadic: real PHP has no way to fill a variadic
/// parameter by name, so once a variadic parameter is in play, any
/// further positional argument is necessarily extra, appended past the
/// initial `params.len()`-sized block - the same shape `check_args`'s
/// variadic fallback (`sig.params.last()` when `i` runs past
/// `params.len()`) already expects.
///
/// A positional argument fills the next open slot by declaration order,
/// tracked via a running count of positional arguments seen so far (not
/// "the next unfilled slot overall" - the two only diverge for a
/// positional argument written after a named one, which real PHP itself
/// forbids as a parse-time error; this project's parser doesn't enforce
/// that ordering, so such input could reach here anyway, but only needs
/// to avoid panicking on it).
///
/// A spread/unpack argument's real contents (how many elements, which
/// keys) aren't known statically, so no further positional argument can
/// be resolved to a slot once one is seen - but a named argument
/// following a spread (legal PHP: `f(...$a, name: $b)`) still resolves
/// exactly as it would anywhere else.
pub(crate) fn resolve_call_args<'a>(args: &'a [Arg], params: &[ParamSig]) -> Vec<Option<&'a Expr>> {
    let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut positional_index = 0usize;
    let mut spread_seen = false;
    for arg in args {
        if arg.spread {
            spread_seen = true;
            continue;
        }
        if let Some(name) = &arg.name {
            if let Some(pos) = params.iter().position(|p| p.name.as_ref() == name.as_str()) {
                resolved[pos] = Some(&arg.value);
            }
            continue;
        }
        if spread_seen {
            // A plain positional argument written after a spread is
            // invalid PHP in the first place (`argument.
            // nonUnpackAfterUnpacked`, flagged separately by
            // `check_argument_legality`) - nothing meaningful to resolve
            // it to.
            continue;
        }
        if positional_index < resolved.len() {
            resolved[positional_index] = Some(&arg.value);
        } else if params.last().is_some_and(|p| p.variadic) {
            resolved.push(Some(&arg.value));
        }
        positional_index += 1;
    }
    resolved
}

/// One `(span, param_name)` pair per positional call argument, for
/// `textDocument/inlayHint`'s parameter-name hints - a named argument
/// already spells the parameter name in the source, so it's skipped; a
/// spread's contents aren't statically known to line up with any one
/// parameter, so it's skipped too. A positional argument past
/// `params.len()` is only hinted when the last parameter is variadic
/// (mirroring `resolve_call_args`'s own variadic-overflow handling),
/// reusing that same declared name for every overflow argument.
pub(crate) fn call_arg_param_name_hints(
    args: &[Arg],
    params: &[ParamSig],
) -> Vec<(Span, Arc<str>)> {
    let mut hints = Vec::new();
    let mut positional_index = 0usize;
    let mut spread_seen = false;
    for arg in args {
        if arg.spread {
            spread_seen = true;
            continue;
        }
        if arg.name.is_some() {
            continue;
        }
        if spread_seen {
            continue;
        }
        if let Some(param) = params.get(positional_index) {
            hints.push((arg.value.span, param.name.clone()));
        } else if let Some(param) = params.last().filter(|p| p.variadic) {
            hints.push((arg.value.span, param.name.clone()));
        }
        positional_index += 1;
    }
    hints
}

/// The positional index `resolve_call_args` would have reached at the
/// moment it hits this call's first spread/unpack argument - `None` when
/// there isn't one. Every declared parameter at or past that index might
/// still be filled by the spread's own runtime contents, which can't be
/// resolved statically, so `check_args`'s "too few"/"too many" argument-
/// count checks must stop trusting a `None` slot (or a raw `args.len()`)
/// at or past that point to mean what it normally would.
/// The `Type::ArrayShape` fields underlying `ty`, unwrapping an
/// `Intersection` of exactly one shape plus any number of accessory
/// markers (`non-empty-array`, `list`, ...) the same way real PHPStan's
/// own accessory-typed array shapes are represented here - `None` for
/// anything else (a plain `Type::Array`, a union, two shapes intersected
/// together, ...), since none of those have statically-known fields.
fn array_shape_fields(ty: &Type) -> Option<&[(String, Type, bool)]> {
    match ty {
        Type::ArrayShape(fields) => Some(fields),
        Type::Intersection(members) => {
            let mut shape = None;
            for member in members {
                match member {
                    Type::ArrayShape(fields) if shape.is_none() => shape = Some(fields.as_slice()),
                    Type::Accessory(_) => {}
                    _ => return None,
                }
            }
            shape
        }
        _ => None,
    }
}

/// Descends a concat chain's own `.left` operand (unwrapping `Grouping`
/// at each step) until it reaches a non-`Concat` leaf, then reads that
/// leaf's own literal string value's first character - the pattern's
/// real delimiter, matching real PHP's own left-associative concat
/// evaluation order (`'/' . $x . '/'` starts with the leftmost `'/'`
/// literal). `None` when the leaf isn't a literal at all (a variable, a
/// function call) - don't guess.
fn concat_leftmost_delimiter(e: &Expr) -> Option<char> {
    let e = unwrap_grouping(e);
    if let ExprKind::Binary(BinOp::Concat, l, _) = &e.kind {
        return concat_leftmost_delimiter(l);
    }
    literal_string_value(e)?.trim_start().chars().next()
}

/// Recursive walk of every operand of a concat tree (both `.left` and
/// `.right` at every node, unlike `concat_leftmost_delimiter`'s own
/// left-only descent) collecting each `preg_quote(...)` call's own
/// argument list found anywhere in it.
fn find_preg_quote_calls<'a>(e: &'a Expr, out: &mut Vec<&'a [Arg]>) {
    let e = unwrap_grouping(e);
    match &e.kind {
        ExprKind::Binary(BinOp::Concat, l, r) => {
            find_preg_quote_calls(l, out);
            find_preg_quote_calls(r, out);
        }
        ExprKind::Call(callee, args) => {
            if let ExprKind::Name(n) = &unwrap_grouping(callee).kind
                && n.rsplit('\\')
                    .next()
                    .unwrap_or(n)
                    .eq_ignore_ascii_case("preg_quote")
            {
                out.push(args);
            }
        }
        _ => {}
    }
}

fn first_spread_positional_index(args: &[Arg]) -> Option<usize> {
    let mut positional_index = 0usize;
    for arg in args {
        if arg.spread {
            return Some(positional_index);
        }
        if arg.name.is_none() {
            positional_index += 1;
        }
    }
    None
}

/// Whether `expr`'s access chain passes through a nullsafe operator
/// anywhere along it (`$x?->y->z`, `$x->y?->z`, `$x?->y['k']`, ...).
/// Mirrors real PHPStan's `NullsafeCheck::containsNullSafe`. A nullsafe
/// access short-circuits its entire enclosing expression to `null` the
/// moment its base is `null`, so nothing built on top of one, however
/// deep, can be relied on to name a genuine, always-evaluated storage
/// location, which is exactly what passing an argument by reference
/// needs.
pub(crate) fn contains_nullsafe_access(expr: &Expr) -> bool {
    match &unwrap_grouping(expr).kind {
        ExprKind::PropertyAccess(base, _, nullsafe) => *nullsafe || contains_nullsafe_access(base),
        ExprKind::MethodCall(base, _, _, nullsafe) => *nullsafe || contains_nullsafe_access(base),
        ExprKind::DynamicPropertyAccess(base, _) => contains_nullsafe_access(base),
        ExprKind::DynamicMethodCall(base, _, _) => contains_nullsafe_access(base),
        ExprKind::Index(base, _) => contains_nullsafe_access(base),
        _ => false,
    }
}

/// Whether `expr` is one of the narrow expression shapes real PHP itself
/// allows as the target of a by-reference parameter - a plain variable,
/// or an array/property access rooted in one - independent of the
/// nullsafe-chain legality `contains_nullsafe_access` checks separately
/// (a nullsafe chain can be built from otherwise-legal shapes below and
/// still needs its own, more specific rejection reason). Mirrors the
/// shapes real PHPStan's own by-ref argument check treats as legal
/// (`Variable`, `ArrayDimFetch`, `PropertyFetch`, `StaticPropertyFetch`)
/// before falling back to `callReturnsByReference` for anything else -
/// not modeled here, since this project has no notion of a user-defined
/// function/method that itself returns by reference, so every remaining
/// shape (a call result, a literal, an operator expression, ...) is
/// conservatively treated as illegal.
pub(crate) fn is_valid_byref_argument_expr(expr: &Expr) -> bool {
    matches!(
        unwrap_grouping(expr).kind,
        ExprKind::Variable(_)
            | ExprKind::DynamicVariable(_)
            | ExprKind::Index(_, _)
            | ExprKind::PropertyAccess(_, _, _)
            | ExprKind::DynamicPropertyAccess(_, _)
            | ExprKind::StaticAccess(_, _)
    )
}
