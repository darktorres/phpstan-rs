use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::checker::{Checker, Locals, purge_stale_narrowing_for, union_types};
use crate::registry::{FunctionReflection, ParameterReflection};
use syntax::ast::*;
use syntax::token::Span;
use types::{self, Type};

mod closure_context;
mod format_string;
mod misc_rules;
pub(crate) mod parameters;

/// `check_args`'s call bundle. `callee` names the callee in PHPStan's
/// messages; `callee_label` in this project's own (boolean-trap and
/// swapped-argument) ones. `label` is the enclosing function or method.
pub(crate) struct CheckArgsCall<'a> {
    pub(crate) callee_label: &'a str,
    pub(crate) callee: parameters::Callee<'a>,
    pub(crate) sig: &'a FunctionReflection,
    pub(crate) args: &'a [Arg],
    pub(crate) locals: &'a Locals,
    pub(crate) defining_class: Option<&'a str>,
    /// A PHP builtin (whose variadic takes no unknown named argument).
    pub(crate) is_builtin: bool,
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
        sig: Option<&FunctionReflection>,
        args: &[Arg],
    ) {
        if !self.collect_inlay_hints {
            return;
        }
        let Some(sig) = sig else {
            return;
        };
        self.inlay_param_name_hints
            .extend(call_arg_param_name_hints(args, sig.params()));
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
        sig: Option<&FunctionReflection>,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &mut Locals,
    ) {
        {
            let mut empty_dims = self.by_ref_empty_dim_args.borrow_mut();
            for (i, arg) in args.iter().enumerate() {
                if !matches!(arg.value.kind, ExprKind::Index(_, None)) {
                    continue;
                }
                let by_ref = match sig {
                    None => true,
                    Some(sig) => resolved
                        .iter()
                        .position(|slot| slot.is_some_and(|e| std::ptr::eq(e, &arg.value)))
                        .or(Some(i))
                        .and_then(|p| {
                            sig.params()
                                .get(p)
                                .or_else(|| sig.params().last().filter(|param| param.variadic()))
                        })
                        .is_some_and(|param| param.by_ref()),
                };
                if by_ref {
                    empty_dims.insert(arg.value.span);
                }
            }
        }
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
                locals.clear_native(name);
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
                .params()
                .get(i)
                .or_else(|| sig.params().last().filter(|p| p.variadic()));
            let Some(param) = param else {
                continue;
            };
            if !param.by_ref() {
                continue;
            }
            if !locals.contains_key(name) {
                locals.declare(name.to_string(), Type::Mixed(false), arg_expr.span);
                continue;
            }
            // The callee may write anything its own native type allows.
            locals.clear_native(name);
            locals.set_native(name, param.ty_native().clone());
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
            // A declared `@param-out` type is written after the call's
            // arguments are checked (`apply_param_out_types`).
            if param.out_ty().is_some() {
                continue;
            }
            let substituted = types::substitute_template(param.ty(), &FxHashMap::default());
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

    /// After a call's arguments are checked: each by-reference variable
    /// argument whose parameter declares a `@param-out` type holds that
    /// type, and no native type (PHP checks parameter types on entry only).
    pub(crate) fn apply_param_out_types(
        &self,
        sig: &FunctionReflection,
        args: &[Arg],
        locals: &mut Locals,
    ) {
        let resolved = resolve_call_args(args, sig.params());
        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg_expr) = slot else {
                continue;
            };
            let ExprKind::Variable(name) = &arg_expr.kind else {
                continue;
            };
            let param = sig
                .params()
                .get(i)
                .or_else(|| sig.params().last().filter(|p| p.variadic()));
            let Some(out_ty) = param
                .filter(|p| p.by_ref())
                .and_then(|p| p.out_ty().as_ref())
            else {
                continue;
            };
            let out_ty = types::substitute_template(out_ty, &FxHashMap::default());
            locals.clear_native(name);
            purge_stale_narrowing_for(locals, name);
            locals.insert(name.to_string(), out_ty);
        }
    }

    pub(crate) fn check_args(&mut self, call: CheckArgsCall) {
        let CheckArgsCall {
            callee_label,
            callee,
            sig,
            args,
            locals,
            defining_class,
            is_builtin,
            span,
        } = call;
        if self.settings.extras.check_boolean_trap {
            self.check_boolean_trap_arguments(callee_label, sig, args, defining_class);
        }
        if self.settings.extras.check_swapped_arguments {
            self.check_swapped_argument_names(callee_label, sig, args, defining_class);
        }
        self.check_parameters(parameters::ParametersCheck {
            callee,
            sig,
            args,
            locals,
            defining_class,
            is_builtin,
            span,
        });
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
            if !self.rule_level_accepts(&param.ty, &arg_ty).yes() {
                self.report(
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
        sig: &FunctionReflection,
        args: &[Arg],
        defining_class: Option<&str>,
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
            let Some(param) = sig.params().get(index) else {
                continue;
            };
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(param.ty(), c),
                None => param.ty().clone(),
            };
            if param_ty != Type::Bool {
                continue;
            }
            self.report(format!(
                    "{callee_label}() argument #{} (${}) is a bare `{literal}` literal passed positionally into a bool parameter - consider a named argument or a self-documenting constant instead",
                    index + 1,
                    param.name()
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
        sig: &FunctionReflection,
        args: &[Arg],
        defining_class: Option<&str>,
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
            let (Some(param_a), Some(param_b)) = (sig.params().get(*i), sig.params().get(*j))
            else {
                continue;
            };
            if param_a.name().eq_ignore_ascii_case(param_b.name()) {
                continue;
            }
            if !a_name.eq_ignore_ascii_case(param_b.name())
                || !b_name.eq_ignore_ascii_case(param_a.name())
            {
                continue;
            }
            let ty_a = match defining_class {
                Some(c) => self.resolve_pseudo(param_a.ty(), c),
                None => param_a.ty().clone(),
            };
            let ty_b = match defining_class {
                Some(c) => self.resolve_pseudo(param_b.ty(), c),
                None => param_b.ty().clone(),
            };
            if ty_a == Type::Mixed(false) || ty_a != ty_b {
                continue;
            }
            self.report(format!(
                    "{callee_label}() arguments #{} (${a_name}) and #{} (${b_name}) may be swapped - their variable names match the reversed parameter order (${}, ${})",
                    i + 1,
                    j + 1,
                    param_a.name(),
                    param_b.name()
                ),
                a.span,
                5,
                "arguments.possiblySwapped",
            );
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
    pub(crate) fn check_preg_quote_delimiter(&mut self, name: &str, args: &[Arg]) {
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
                self.report(format!(
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
    /// `call_user_func(_array)` callable shapes to a real `FunctionReflection` -
    /// a plain string-literal function name, or a two-element array
    /// literal naming a class/object plus a literal method name. Any
    /// other shape (a closure literal, a bare variable of unknown type,
    /// `Closure::fromCallable(...)`, an invokable object, ...) returns
    /// `None` rather than guessing.
    fn resolve_call_user_func_callable(
        &self,
        callable_expr: &Expr,
        locals: &Locals,
    ) -> Option<&FunctionReflection> {
        let callable_expr = unwrap_grouping(callable_expr);
        if let Some(name) = literal_string_value(callable_expr) {
            return self.registry.lookup_function(name.trim_start_matches('\\'));
        }
        let ty = self.infer_expr(callable_expr, locals);
        let Type::ArrayShape(shape) = &ty else {
            return None;
        };
        let method_ty = &shape.field(&types::ShapeKey::Int(1))?.ty;
        let Type::StringLiteral(method) = method_ty else {
            return None;
        };
        let method = method.as_str()?;
        let class_ty = &shape.field(&types::ShapeKey::Int(0))?.ty;
        let class = match class_ty {
            Type::StringLiteral(c) => c.as_str()?,
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
            self.maybe_record_param_name_hints(Some(&sig), callee_args);
            self.check_args(CheckArgsCall {
                callee_label: "callable passed to call_user_func()",
                callee: parameters::Callee::Callable("callable passed to call_user_func()"),
                sig: &sig,
                args: callee_args,
                locals,
                defining_class: None,
                is_builtin: false,
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
                    id: syntax::ast::NodeId::SYNTHETIC,
                    name: None,
                    spread: false,
                    value: item.value.clone(),
                })
                .collect();
            self.maybe_record_param_name_hints(Some(&sig), &synthesized);
            self.check_args(CheckArgsCall {
                callee_label: "callable passed to call_user_func_array()",
                callee: parameters::Callee::Callable("callable passed to call_user_func_array()"),
                sig: &sig,
                args: &synthesized,
                locals,
                defining_class: None,
                is_builtin: false,
                span,
            });
        }
    }
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
pub(crate) fn resolve_call_args<'a>(
    args: &'a [Arg],
    params: &[ParameterReflection],
) -> Vec<Option<&'a Expr>> {
    let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut positional_index = 0usize;
    let mut spread_seen = false;
    for arg in args {
        if arg.spread {
            spread_seen = true;
            continue;
        }
        if let Some(name) = &arg.name {
            if let Some(pos) = params.iter().position(|p| p.name().as_ref() == &*name.name) {
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
        } else if params.last().is_some_and(|p| p.variadic()) {
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
    params: &[ParameterReflection],
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
            hints.push((arg.value.span, param.name().clone()));
        } else if let Some(param) = params.last().filter(|p| p.variadic()) {
            hints.push((arg.value.span, param.name().clone()));
        }
        positional_index += 1;
    }
    hints
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
        ExprKind::StaticPropertyFetch(StaticTarget::Expr(base), _)
        | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Expr(base), _) => {
            contains_nullsafe_access(base)
        }
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
            | ExprKind::ClassConstFetch(..)
            | ExprKind::ClassNameFetch(_)
            | ExprKind::StaticPropertyFetch(..)
            | ExprKind::DynamicStaticPropertyFetch(..)
    )
}
