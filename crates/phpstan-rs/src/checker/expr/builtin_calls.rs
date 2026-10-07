use crate::checker::{
    Checker, Locals, literal_shape_key, narrowing_type_for_predicate, resolve_call_args,
};
use crate::registry::{FunctionReflection, Registry};
use syntax::ast::*;
use syntax::span::Span;
use types::PhpString;
use types::UnionAll as _;
use types::{self, AccessoryKind, Relation, Shape, ShapeField, ShapeKey, Type};

use super::{is_false_literal, is_true_literal};

impl<'a> Checker<'a> {
    /// Bespoke per-function return-type extensions, mirroring real
    /// PHPStan's own `src/Type/Php/` (190 files of exactly this kind of
    /// special-casing, not generic machinery). Falls through to the
    /// ordinary generic-template path (`infer_call_return_type`) for
    /// every other function, and for any shape one of these arms doesn't
    /// specially handle.
    ///
    /// Deliberately stays on the raw, positional `args`, not a resolved
    /// per-parameter slice - these are a small, fixed set of well-known
    /// standard-library functions nobody realistically calls with named
    /// arguments, and a resolved slice isn't sized the same way a raw
    /// argument count is (see `check_args`'s note on that distinction).
    pub(crate) fn infer_builtin_call_return_type(
        &self,
        name: &str,
        sig: &FunctionReflection,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        // Every special case below is a builtin's. A global function by a
        // builtin's name that isn't its stub is a polyfill, declared only
        // where the builtin is missing, so it behaves as the builtin does.
        if !sig.is_stub() && crate::registry::stubs::builtin_function_extension(name).is_none() {
            let resolved = resolve_call_args(args, sig.params());
            return self.infer_call_return_type(sig, &resolved, locals);
        }
        if let Some(outcome) = self.type_check_call_outcome(name, args, locals) {
            return Type::bool_literal(outcome);
        }
        match name {
            // Mirrors real PHPStan's `AbsFunctionDynamicReturnTypeExtension` -
            // delegates entirely to `Type::toAbsoluteNumber()`.
            "abs" if args.len() == 1 => {
                return self.infer_expr(&args[0].value, locals).to_absolute_number();
            }
            // Narrowed to the one part that's always true regardless of
            // the argument: `strlen()` can never be negative. Real
            // PHPStan additionally refines a literal or known-non-empty-
            // string argument's exact/minimum length; not modeled here.
            // A hex digest longer than any decimal int: lowercase, never
            // falsy, a string as an array key. `binary: true` gives raw
            // bytes instead, which may all be digits.
            "md5" | "sha1" | "hash" | "hash_hmac" | "spl_object_hash" => {
                let resolved = resolve_call_args(args, sig.params());
                let arg = |i: usize| resolved.get(i).copied().flatten();
                let (algorithm_long, binary) = match name {
                    "md5" | "sha1" => (true, arg(1)),
                    "hash" => (arg(0).is_some_and(long_hex_algorithm), arg(2)),
                    "hash_hmac" => (arg(0).is_some_and(long_hex_algorithm), arg(3)),
                    _ => (true, None),
                };
                let hex = binary.is_none_or(|b| {
                    matches!(&b.peeled().kind, ExprKind::Literal(Literal::Bool(false)))
                });
                if algorithm_long && hex {
                    return Type::intersection_of(vec![
                        Type::string(),
                        Type::accessory(AccessoryKind::NonFalsyString),
                        Type::accessory(AccessoryKind::LowercaseString),
                        Type::accessory(AccessoryKind::NonDecimalIntegerString),
                    ]);
                }
            }
            // Glue with a byte that is neither a digit nor a `-`, joining at
            // least two pieces, is in the result where no decimal int has it.
            "implode" | "join" if !args.is_empty() => {
                let resolved = resolve_call_args(args, sig.params());
                let (glue, pieces) = match (
                    resolved.first().copied().flatten(),
                    resolved.get(1).copied().flatten(),
                ) {
                    (Some(pieces), None) => (None, Some(pieces)),
                    (Some(glue), Some(pieces)) => (Some(glue), Some(pieces)),
                    _ => (None, None),
                };
                if let (Some(glue), Some(pieces)) = (glue, pieces)
                    && let Some(glue) = literal_string_value(glue)
                    && glue.bytes().any(|b| !b.is_ascii_digit() && b != b'-')
                    && self
                        .infer_expr(pieces, locals)
                        .count_range()
                        .is_some_and(|(lo, _)| lo >= 2)
                {
                    return Type::intersection_of(vec![
                        Type::string(),
                        Type::accessory(AccessoryKind::NonFalsyString),
                        Type::accessory(AccessoryKind::NonDecimalIntegerString),
                    ]);
                }
            }
            // A match count, or `false` when the call fails.
            "preg_match" | "preg_match_all" => {
                return crate::checker::preg_result_type(name == "preg_match_all");
            }
            "strlen" if args.len() == 1 => {
                return Type::int_range(Some(0), None);
            }
            // Narrows to `non-falsy-string` whenever the format string is
            // a literal whose non-specifier literal text (with every real
            // `%[argnum$][flags][width][.precision]specifier` sequence
            // removed) is itself provably non-falsy - the same reasoning `BinOp::Concat`/`ExprKind::
            // Interpolated` use: a guaranteed-present, non-empty, non-`'0'`
            // literal chunk means the result can never be exactly `""`/`"0"`,
            // regardless of what any specifier expands to.
            // `sprintf_literal_remainder` returns `None` on anything it
            // can't confidently parse, never wrongly claiming non-falsy.
            // A byte no decimal int has where it stands keeps the result a
            // string as an array key.
            "sprintf" | "vsprintf" if !args.is_empty() => {
                if let Some(fmt) = literal_string_value(&args[0].value)
                    && let Some(pieces) = sprintf_pieces(&fmt)
                    && let Some(remainder) = sprintf_literal_remainder(&fmt)
                {
                    let mut members = vec![Type::string()];
                    if Type::string_literal(remainder.as_str().into())
                        .is_refined_string(AccessoryKind::NonFalsyString)
                        .yes()
                    {
                        members.push(Type::accessory(AccessoryKind::NonFalsyString));
                    }
                    // `sprintf`'s arguments follow the format; `vsprintf`'s
                    // are one array.
                    let non_empty_arg = |arg: usize| {
                        name == "sprintf"
                            && args.get(arg + 1).is_some_and(|a| {
                                !a.spread
                                    && self
                                        .infer_expr(&a.value, locals)
                                        .to_string_type(self.registry)
                                        .is_some_and(|t| {
                                            t.is_refined_string(AccessoryKind::NonEmptyString).yes()
                                        })
                            })
                    };
                    if sprintf_is_non_decimal_int(&pieces, &non_empty_arg) {
                        members.push(Type::accessory(AccessoryKind::NonDecimalIntegerString));
                    }
                    if members.len() > 1 {
                        return Type::intersection_of(members);
                    }
                }
            }
            // Mirrors real PHPStan's own `StrtotimeFunctionReturnTypeExtension`
            // (confirmed by direct extraction from the vendored phpstan.phar):
            // it evaluates a constant-string first argument against real
            // PHP's own relative-time grammar and narrows away the stub's
            // blanket `int|false` when that parse succeeds - ignoring a
            // second `$now` argument even when one is passed. Real PHP's
            // full grammar (absolute dates, "next monday", ...) is large;
            // matching this project's own "don't guess, narrow to the
            // verified need" precedent (`strlen`'s doc comment above), only
            // the bounded "N unit(s) [ago]" relative-offset shape is
            // recognized (`php_date_parser::validate_relative_delta` - the
            // single source of truth for this one grammar fragment,
            // shared with `DateTime`'s own constructor-string check) -
            // every real corpus call site (`'-1 hour'`/`'-30 days'`/
            // `'-90 days'`) is exactly this shape. Anything else keeps
            // today's `int|false`.
            "strtotime" if !args.is_empty() => {
                if let Some(literal) = literal_string_value(&args[0].value)
                    && crate::php_date_parser::validate_relative_delta(&literal) == Some(true)
                {
                    return Type::int();
                }
            }
            // The stub declares `?string` unconditionally (`$return`
            // defaults to `false`, in which case `var_export` prints to
            // stdout and returns `null`), but a literal `true` second
            // argument means it always returns the representation string
            // instead. Scoped to that exact 2-argument, literal-`true`
            // form - anything else keeps the stub's nullable return type.
            "var_export" if args.len() == 2 && is_true_literal(&args[1].value) => {
                return Type::string();
            }
            // The count an array's type allows (`Type::count_range`), or a
            // `Countable` object's `count()` (PHP throws on a negative
            // one); `COUNT_RECURSIVE` counts at least as many, and a mode
            // that isn't a literal anything from 0.
            "count" | "sizeof" if !args.is_empty() => {
                let counted = self.infer_expr(&args[0].value, locals);
                let recursive = match args.get(1) {
                    None => false,
                    Some(mode) => match self.infer_expr(&mode.value, locals).int_literal_value() {
                        Some(0) => false,
                        Some(1) => true,
                        _ => return Type::int_range(Some(0), None),
                    },
                };
                let non_negative = Type::int_range(Some(0), None);
                let counts: Vec<Type> = counted
                    .union_members()
                    .iter()
                    .map(|member| match member.count_range() {
                        Some((least, most)) => {
                            Type::int_range(Some(least), if recursive { None } else { most })
                        }
                        None => match self.find_method_receiver(member, "count") {
                            Some((_, _, _, sig)) if member.is_object().yes() => self
                                .relation()
                                .intersect(vec![sig.return_type().clone(), non_negative.clone()]),
                            _ => non_negative.clone(),
                        },
                    })
                    .collect();
                return self.relation().union_of(counts);
            }
            // `random_int`/`rand`/`mt_rand($min, $max)` always produce a
            // value within `[$min, $max]` inclusive; a non-literal,
            // unbounded argument on either side degrades that side of the
            // range to unbounded too, rather than guessing. The
            // 0-argument form is left as the stub's plain `int` -
            // `getrandmax()`'s exact value isn't modeled, and guessing a
            // wrong upper bound would be worse than the stub's honest
            // `int`.
            "random_int" | "rand" | "mt_rand" if args.len() == 2 => {
                let min = self
                    .infer_expr(&args[0].value, locals)
                    .int_bounds()
                    .and_then(|(min, _)| min);
                let max = self
                    .infer_expr(&args[1].value, locals)
                    .int_bounds()
                    .and_then(|(_, max)| max);
                return Type::int_range(min, max);
            }
            // `hrtime()`/`hrtime(false)` return a 2-element list
            // `[seconds, nanoseconds]`; `hrtime(true)` instead collapses
            // both into one `int|float` nanosecond count. A non-literal-
            // bool argument can't rule either shape out, so both union
            // together.
            "hrtime" if args.is_empty() => {
                return hrtime_array_shape();
            }
            "hrtime" if args.len() == 1 => {
                return match self.infer_expr(&args[0].value, locals).bool_literal_value() {
                    Some(true) => hrtime_number_type(),
                    Some(false) => hrtime_array_shape(),
                    None => self
                        .relation()
                        .union(hrtime_array_shape(), hrtime_number_type()),
                };
            }
            // `array_pop`/`array_shift`'s element-or-null result type is
            // deliberately NOT special-cased: `predefine_byref_or_unknown_
            // args` widens this same by-ref argument's own type
            // immediately, so a separate, later re-inference of the same
            // call expression (e.g. when nested inside another call's
            // argument) would read an already-widened `locals` entry,
            // making a "provably non-empty -> non-nullable" narrowing
            // unsound. Even the safe, always-nullable fallback
            // (`elem_ty | null`) produced real noise for a common idiom -
            // an `explode()`-then-`array_shift()` or an explicit
            // `count() === 0` guard immediately before the pop/shift -
            // that this project's narrowing doesn't track as
            // non-emptiness.
            //
            // `range()`'s stub (`@return array`) gives no list-ness/
            // element-type information, but a real `range($low, $high)`
            // call always walks `$low` to `$high` inclusive with
            // sequential integer keys starting at 0 - genuinely a list,
            // and never empty (even `range(5, 5)` produces `[5]`) -
            // regardless of whether the bounds are literals. Scoped to
            // int and single-character-string bounds with no 3rd `$step`
            // argument (a non-default step doesn't change list-ness/
            // non-emptiness but isn't needed by any real call site yet) -
            // anything else falls through to the generic path below.
            "range" if args.len() == 2 => {
                let (from, to) = (
                    self.infer_expr(&args[0].value, locals),
                    self.infer_expr(&args[1].value, locals),
                );
                let one_char = |t: &Type| t.string_literal_value().is_some_and(|s| s.len() == 1);
                let elem = if from.is_int().yes() && to.is_int().yes() {
                    Some(Type::int())
                } else if one_char(&from) && one_char(&to) {
                    Some(Type::string())
                } else {
                    None
                };
                if let Some(elem) = elem {
                    return Type::intersection_of(vec![
                        Type::array(Type::unknown_array_key(), elem),
                        Type::accessory(AccessoryKind::ArrayList),
                        Type::accessory(AccessoryKind::NonEmptyArray),
                    ]);
                }
            }
            // `preg_split()`'s stub (`@return string[]|false`) has no
            // list-ness, but splitting a string always produces
            // sequential integer keys starting at 0, for any pattern/
            // subject: `list<string>|false` (`list<non-empty-string>|false`
            // when `PREG_SPLIT_NO_EMPTY` is one of the flags - checked the
            // same lenient bitwise-or-chain-of-names way `parse_preg_flags`
            // does for `preg_match`, without needing that function's
            // stricter "every flag must be recognized" requirement, since
            // an unrecognized flag doesn't change list-ness/non-emptiness
            // here).
            "preg_split" if (2..=4).contains(&args.len()) => {
                let no_empty = args
                    .get(3)
                    .is_some_and(|a| contains_flag_name(&a.value, "PREG_SPLIT_NO_EMPTY"));
                let elem = if no_empty {
                    Type::intersection_of(vec![
                        Type::string(),
                        Type::accessory(AccessoryKind::NonEmptyString),
                    ])
                } else {
                    Type::string()
                };
                let list_ty = Type::intersection_of(vec![
                    Type::array(Type::unknown_array_key(), elem),
                    Type::accessory(AccessoryKind::ArrayList),
                ]);
                return self.relation().union(list_ty, Type::bool_literal(false));
            }
            // Mirrors real PHPStan's `FilterFunctionReturnTypeHelper`,
            // scoped to what's statically provable - see
            // `filter_validate_result`'s doc comment for exactly what's
            // modeled and what's deliberately not (a computed filter
            // value, sanitize filters, `FILTER_DEFAULT`).
            "filter_var" if (2..=3).contains(&args.len()) => {
                let flags_arg = args.get(2).map(|a| &a.value);
                let flags_ty = flags_arg.map(|e| self.infer_expr(e, locals));
                if let ExprKind::Name(filter_name) = &args[1].value.kind
                    && let Some((element_ty, fallback, throw_on_failure)) = filter_validate_result(
                        filter_name.trim_start_matches('\\'),
                        flags_arg,
                        flags_ty.as_ref(),
                    )
                {
                    let require_array = flags_arg.is_some_and(|e| {
                        contains_flag_name_with_options(e, "FILTER_REQUIRE_ARRAY")
                    });
                    let force_array = flags_arg
                        .is_some_and(|e| contains_flag_name_with_options(e, "FILTER_FORCE_ARRAY"));
                    let value_ty = self.infer_expr(&args[0].value, locals);
                    // `FILTER_REQUIRE_ARRAY` wins when both are literally
                    // present, matching real PHPStan's precedence. Only
                    // the two provable shapes are modeled (favor false
                    // negatives over replicating PHPStan's full trinary-
                    // logic state machine) - a `FILTER_REQUIRE_ARRAY` call
                    // on a not-provably-array input falls through to the
                    // generic stub below, unchanged.
                    if require_array && !value_ty.is_array().yes() {
                        return Type::mixed();
                    }
                    // The fallback is unioned into the *element* type
                    // first, and only that gets wrapped in the array -
                    // never a bare, un-widened element type (real PHP
                    // gives `array<int|false>`, not `array<int>`).
                    let unioned = if throw_on_failure {
                        element_ty.clone()
                    } else {
                        self.relation().union(element_ty.clone(), fallback.clone())
                    };
                    if require_array || force_array {
                        let key_ty = if value_ty.is_array().yes() {
                            value_ty
                                .array_key_type()
                                .unwrap_or_else(Type::unknown_array_key)
                        } else {
                            Type::unknown_array_key()
                        };
                        return Type::array(key_ty, unioned);
                    }
                    return unioned;
                }
            }
            // `filter_input($type, $var_name, $filter, $flags)` is
            // modeled as reading an unknown offset out of a synthetic
            // `array<string, mixed>` (standing in for `$_GET`/`$_POST`/
            // etc.), then delegating to the same `filter_validate_result`
            // `filter_var` uses, shifted one argument position over. The
            // always-`maybe`-present synthetic array means both
            // `null` and `false` are unconditionally unioned
            // in regardless of `FILTER_NULL_ON_FAILURE` - the flag changes
            // which fallback attaches to which underlying cause, not
            // whether both appear in the final union - and this holds
            // even under `FILTER_THROW_ON_FAILURE`, which only strips the
            // validation-failure fallback, never the "variable doesn't
            // exist" one (a genuinely missing variable just returns
            // `null`, it doesn't throw). `$type` is never inspected - this
            // project has no "internal function throws TypeError"
            // tracking to model an invalid one with.
            "filter_input" if (2..=4).contains(&args.len()) => {
                let flags_arg = args.get(3).map(|a| &a.value);
                let flags_ty = flags_arg.map(|e| self.infer_expr(e, locals));
                // `FILTER_REQUIRE_ARRAY`/`FILTER_FORCE_ARRAY` aren't
                // modeled for `filter_input`: its subject is always the
                // synthetic array, so only the unconditional-wrap case
                // could ever apply, not worth the extra branch without
                // corpus evidence - falls through to the generic stub
                // instead of silently ignoring the flag.
                if flags_arg.is_some_and(|e| {
                    contains_flag_name_with_options(e, "FILTER_REQUIRE_ARRAY")
                        || contains_flag_name_with_options(e, "FILTER_FORCE_ARRAY")
                }) {
                    return Type::mixed();
                }
                if let Some(filter) = args.get(2)
                    && let ExprKind::Name(filter_name) = &filter.value.kind
                    && let Some((element_ty, fallback, throw_on_failure)) = filter_validate_result(
                        filter_name.trim_start_matches('\\'),
                        flags_arg,
                        flags_ty.as_ref(),
                    )
                {
                    // `Type::union` preserves first-seen insertion order
                    // in its `Display`, so `Null`-then-fallback-then-
                    // `False` matches this project's established
                    // convention for this three-way union.
                    let mut result = element_ty;
                    result = self.relation().union(result, Type::null());
                    if !throw_on_failure {
                        result = self.relation().union(result, fallback);
                    }
                    return self.relation().union(result, Type::bool_literal(false));
                }
            }
            // Real PHPStan's `MinMaxFunctionReturnTypeExtension`
            // constant-folds when every argument is a known literal,
            // falling back to a union of every argument's type otherwise
            // - only that fallback is ported here, since this project has
            // no elaborate constant-value tracking. Excludes the single-
            // array-argument overload (`min(array $value)`), which has
            // entirely different, unmodeled comparison semantics -
            // `args.len() == 1` always means that form.
            "min" | "max" if args.len() >= 2 && !args.iter().any(|a| a.spread) => {
                let arg_types: Vec<Type> = args
                    .iter()
                    .map(|a| self.infer_expr(&a.value, locals))
                    .collect();
                if !arg_types.iter().any(|t| t.is_array().yes()) {
                    return arg_types
                        .into_iter()
                        .union_all_with(&self.relation())
                        .unwrap_or(Type::mixed());
                }
            }
            // PHP 8.4's `array_find`/`array_find_key`, mirroring real
            // PHPStan's `ArrayFind{,Key}FunctionReturnTypeExtension`:
            // resolves to the array's value/key type unioned with `null`,
            // since a "not found" outcome is always real.
            "array_find" if args.len() == 2 => {
                let input_ty = self.infer_expr(&args[0].value, locals);
                if let Some(elem_ty) = input_ty.array_value_type() {
                    return self.relation().union(elem_ty, Type::null());
                }
            }
            "array_find_key" if args.len() == 2 => {
                let input_ty = self.infer_expr(&args[0].value, locals);
                if let Some(key_ty) = input_ty.array_key_type() {
                    return self.relation().union(key_ty, Type::null());
                }
            }
            // Scoped to the common 2-arg single-array form - the real
            // function is variadic (multiple arrays); a 3+-arg call falls
            // through to the generic path below, unchanged.
            "array_map" if args.len() == 2 => {
                let input_ty = self.infer_expr(&args[1].value, locals);
                if let Some(elem_ty) = input_ty.array_value_type()
                    && let Some(mapped) = self.infer_closure_return_with_param_overrides(
                        &args[0].value,
                        &[elem_ty],
                        locals,
                    )
                {
                    // Mapping never changes key structure or element
                    // count, so any Accessory marker (list-ness/non-
                    // emptiness) the input had survives onto the result.
                    return input_ty.with_array_value(mapped);
                }
            }
            // Filtering never transforms elements, so the result is
            // simply whatever the input array's own type already is - no
            // closure introspection needed. Real PHP requires at least 1
            // argument, but a syntactically-valid zero-argument call
            // (invalid at runtime, but real code in the corpus this
            // project verifies against) still parses fine and must not be
            // indexed into blindly.
            "array_filter" if !args.is_empty() => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if arr_ty.is_array().yes() {
                    // Filtering can both leave gaps in a list's sequential
                    // keys and empty out a non-empty array, so neither
                    // marker survives.
                    let stripped = arr_ty.without_accessories();
                    if args.len() >= 2 {
                        if is_null_filter_callback(&args[1].value) {
                            return strip_null_from_array_element(stripped);
                        }
                        // An `instanceof`-shaped filter callback (`fn ($v):
                        // bool => $v instanceof Target`) narrows the
                        // retained element type to `Target` specifically -
                        // not just strips `Null`: an `A|B|null` element
                        // type filtered by `instanceof B` keeps only `B`,
                        // not `A` too.
                        if let Some(class_name) = is_instanceof_filter_callback(&args[1].value) {
                            let target = self.resolve_name_expr(class_name);
                            return narrow_array_element_type_to(
                                &self.relation(),
                                stripped,
                                &target,
                            );
                        }
                        // A native `is_string`/`is_int`/`is_float`/
                        // `is_bool`/`is_null` type-check predicate narrows
                        // the same way.
                        if let Some(target) = is_type_check_filter_callback(&args[1].value) {
                            return narrow_array_element_type_to(
                                &self.relation(),
                                stripped,
                                &target,
                            );
                        }
                    } else {
                        // No callback at all - real PHP's default
                        // behavior, dropping every falsy element - is
                        // exactly `Type::without_falsy`'s job
                        // (`list<int|null>` -> `list<non-zero-int>`,
                        // stripping both `0` and `null`, not just `null`).
                        return stripped.filter_array_values(&Type::without_falsy);
                    }
                    return stripped;
                }
            }
            "array_reduce" => {
                let resolved = resolve_call_args(args, sig.params());
                if let [Some(array), Some(callback), initial, ..] = resolved[..] {
                    return self.reduced_array_type(array, callback, initial, locals);
                }
            }
            // `$keys`' values, converted to strings and cast as keys, key
            // `$values`' values; two constant lists zip into a shape. What
            // else the stub returns for the configured versions (`false`
            // on PHP 7 for different sizes) stays.
            "array_combine" => {
                let resolved = resolve_call_args(args, sig.params());
                if let (Some(keys), Some(values)) = (
                    resolved.first().copied().flatten(),
                    resolved.get(1).copied().flatten(),
                ) {
                    let keys_ty = self.infer_expr(keys, locals);
                    let values_ty = self.infer_expr(values, locals);
                    if keys_ty.is_array().yes() && values_ty.is_array().yes() {
                        let other = self
                            .infer_call_return_type(sig, &resolved, locals)
                            .union_members()
                            .iter()
                            .filter(|m| m.is_array().no())
                            .cloned()
                            .collect::<Vec<_>>();
                        let combined = self.combined_arrays(&keys_ty, &values_ty);
                        return self.relation().union_of([vec![combined], other].concat());
                    }
                }
            }
            // Both preserve list-ness/non-emptiness through to their
            // result when called with default arguments, but not with
            // `preserve_keys: true` (original indices survive, no longer
            // sequential from zero, matching real PHPStan's behavior) -
            // deliberately not handled here.
            "array_slice"
                if matches!(args.len(), 2..=3)
                    || (args.len() == 4 && is_false_literal(&args[3].value)) =>
            {
                let input_ty = self.infer_expr(&args[0].value, locals);
                if let Some(elem_ty) = input_ty.array_value_type() {
                    return input_ty.with_array_value(elem_ty);
                }
            }
            "array_reverse"
                if args.len() == 1 || (args.len() == 2 && is_false_literal(&args[1].value)) =>
            {
                let input_ty = self.infer_expr(&args[0].value, locals);
                if let Some(elem_ty) = input_ty.array_value_type() {
                    return input_ty.with_array_value(elem_ty);
                }
            }
            // Always returns list<V> regardless of whether the input was
            // already list-shaped - re-sequencing every key from zero is
            // exactly what array_values does.
            "array_values" if args.len() == 1 => {
                if let Some(elem_ty) = (self.infer_expr(&args[0].value, locals)).array_value_type()
                {
                    return Type::intersection_of(vec![
                        Type::array(Type::unknown_array_key(), elem_ty),
                        Type::accessory(AccessoryKind::ArrayList),
                    ]);
                }
            }
            "array_unique" if !args.is_empty() => {
                // Removing a duplicate can leave a gap in the surviving
                // sequential keys, so list-ness doesn't survive. Non-
                // emptiness is different: `array_unique` only ever
                // removes *duplicates* (real PHP always keeps at least
                // the first occurrence of each distinct value), so a
                // non-empty input can never come out empty - confirmed
                // via `dumpType()` against real PHPStan
                // (`non-empty-list<int>` stays `non-empty-array<...>`,
                // unlike `array_diff`/`array_intersect`, which really
                // can empty a non-empty input and correctly don't get
                // this same treatment).
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if arr_ty.is_array().yes() {
                    let stripped = arr_ty.clone().without_accessories();
                    return if arr_ty.is_non_empty_array().yes() {
                        Type::intersection_of(vec![
                            stripped,
                            Type::accessory(AccessoryKind::NonEmptyArray),
                        ])
                    } else {
                        stripped
                    };
                }
            }
            // Real PHP always takes every kept key/value pair's type
            // straight from `$array` (the first argument) - every other
            // argument only narrows which keys survive, never what type a
            // surviving one holds. List-ness doesn't survive (a matching
            // key can be anywhere in `$array`, leaving a gap in whatever
            // sequential keys remain), the same leniency `array_unique`
            // above applies. Generalizes to every `array_diff*`/
            // `array_udiff*`/`array_intersect*`/`array_uintersect*`
            // sibling - none of them ever transforms a surviving
            // element's value (only which key/value pairs survive
            // differs), so every one shares this "arg0's type, minus
            // accessory markers, verbatim" result.
            //
            // A single spread argument (`array_intersect(...$arrays)`)
            // has exactly one AST-level `Arg`, failing the plain
            // `args.len() >= 2` gate outright - unlike `array_merge`'s
            // sibling arm, which already unwraps a spread source one
            // level to reach the arrays actually being merged. Mirrors
            // that unwrap here: the spread source's element type stands
            // in for "the first array argument".
            "array_intersect_key"
            | "array_intersect"
            | "array_intersect_assoc"
            | "array_intersect_ukey"
            | "array_intersect_uassoc"
            | "array_uintersect"
            | "array_uintersect_assoc"
            | "array_uintersect_uassoc"
            | "array_diff"
            | "array_diff_key"
            | "array_diff_assoc"
            | "array_diff_ukey"
            | "array_diff_uassoc"
            | "array_udiff"
            | "array_udiff_assoc"
            | "array_udiff_uassoc"
                if args.len() >= 2 || matches!(args, [a] if a.spread) =>
            {
                let arr_ty = if let [a] = args
                    && a.spread
                {
                    let spread_ty = self.infer_expr(&a.value, locals);
                    spread_ty.array_value_type().unwrap_or(Type::mixed())
                } else {
                    self.infer_expr(&args[0].value, locals)
                };
                if arr_ty.is_array().yes() {
                    return arr_ty.without_accessories();
                }
            }
            // Swaps keys and values - the result's key type is `$array`'s
            // value type, and vice versa. List-ness never survives (the
            // new keys come from the old values, essentially never
            // already `0, 1, 2, ...`), but non-emptiness does (flipping
            // can't change whether there's at least one element).
            "array_flip" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                let keyable = self.relation().union(Type::int(), Type::string());
                // A real array shape gets a precise, field-by-field
                // flip: a field whose own value is an int/string literal
                // becomes a real named field in the result (keyed by that
                // literal, valued at the original field's own key). A
                // field whose value isn't a literal can't name a real
                // result field at all (real PHP requires an int|string
                // value here, or throws - either way the exact resulting
                // key isn't knowable), so it only unseals the result:
                // more keys exist, just not statically known which.
                if let Some(shape) = arr_ty.as_array_shape() {
                    let mut flipped = Shape::sealed(Vec::new());
                    if !shape.is_sealed() {
                        unseal(&mut flipped);
                    }
                    for field in shape.fields() {
                        match ShapeKey::from_type(&field.ty) {
                            Some(key) => flipped.set(ShapeField::new(
                                key,
                                field.key.to_type(),
                                field.optional,
                            )),
                            None => unseal(&mut flipped),
                        }
                    }
                    return Type::array_shape(flipped);
                }
                if arr_ty.is_array().yes() {
                    let key_ty = arr_ty
                        .array_key_type()
                        .unwrap_or_else(Type::unknown_array_key);
                    // Only int and string values flip; others are skipped.
                    let elem_ty = arr_ty.array_value_type().unwrap_or(Type::mixed());
                    let all_keyable = self.relation().is_super_type_of(&keyable, &elem_ty).yes();
                    let elem_ty = self.relation().narrow(&elem_ty, &keyable).to_array_key();
                    let flipped = Type::array(elem_ty, key_ty);
                    return if all_keyable && arr_ty.is_non_empty_array().yes() {
                        Type::intersection_of(vec![
                            flipped,
                            Type::accessory(AccessoryKind::NonEmptyArray),
                        ])
                    } else {
                        flipped
                    };
                }
            }
            // Each string-literal argument (recursively, for a nested
            // array-literal argument - real PHP accepts `compact(['a',
            // ['b', 'c']])` since 7.3) names a variable to read directly
            // out of `locals`, the exact whole-scope variable-type map
            // this project already threads through every call site here -
            // no new tracking mechanism, just a new read over existing
            // state. A name with no corresponding entry in `locals` is
            // silently omitted, matching real PHP's own "skip anything
            // undefined" `compact()` semantics (never an error). A
            // spread argument, or any argument that isn't itself a
            // string literal or a literal-nested array of names,
            // degrades the *whole* call to today's plain stub-declared
            // return type - `compact()` combines every argument into one
            // result, so one unresolvable argument means the real key
            // set genuinely isn't knowable, not just that one argument's
            // own names.
            "compact" if !args.is_empty() && !args.iter().any(|a| a.spread) => {
                let mut shape = Shape::sealed(Vec::new());
                let mut resolvable = true;
                for arg in args {
                    match compact_argument_names(&arg.value) {
                        Some(names) => {
                            for name in names {
                                let key = ShapeKey::from_name(&name);
                                if shape.field(&key).is_some() {
                                    continue;
                                }
                                if let Some(ty) = locals.get_var(&name) {
                                    shape.push(ShapeField::new(key, ty.clone(), false));
                                }
                            }
                        }
                        None => {
                            resolvable = false;
                            break;
                        }
                    }
                }
                if resolvable {
                    return Type::array_shape(shape);
                }
            }
            // Every currently-tracked entry in `locals`, the identical
            // whole-scope map every other purpose here already reads
            // (narrowing, unused-variable detection) - a new read path,
            // not new tracking. `is_real_variable_key` filters out
            // `key_of`'s own synthetic property-chain/array-index
            // keys, which share the same `Locals` map but were never a
            // real PHP variable `get_defined_vars()` could report.
            "mysqli_query" if args.len() >= 2 => {
                return self.mysqli_query_type(Some(&args[1].value), locals);
            }
            "mysqli_fetch_assoc"
            | "mysqli_fetch_row"
            | "mysqli_fetch_array"
            | "mysqli_fetch_all"
            | "mysqli_fetch_column"
                if !args.is_empty() =>
            {
                let result_ty = self.infer_expr(&args[0].value, locals);
                if let Some((class, type_args)) = result_ty.as_object()
                    && class.eq_ignore_ascii_case("mysqli_result")
                    && let Some(ty) = self.mysqli_fetch_type(
                        name.trim_start_matches("mysqli_"),
                        type_args.first(),
                        args.get(1).map(|a| &a.value),
                    )
                {
                    return ty;
                }
            }
            "get_defined_vars" if args.is_empty() => {
                if locals.any_variable_may_exist() {
                    return Type::array(Type::string(), Type::mixed());
                }
                let mut variables = locals.variables();
                variables.sort_by(|a, b| a.0.cmp(&b.0));
                let fields: Vec<ShapeField> = variables
                    .into_iter()
                    .map(|(name, ty)| {
                        ShapeField::new(ShapeKey::from_name(&name), ty.clone(), false)
                    })
                    .collect();
                return Type::array_shape(Shape::sealed(fields));
            }
            // `array_pop`/`array_shift` remove and return the last/first
            // element (by reference - the caller's argument shrinks by
            // one, not modeled here; only this expression's result type
            // is), or `null` on an already-empty array - the same
            // three-way "known empty / known non-empty / ambiguous" split
            // `array_pointer_result` implements for `reset`/`current`/
            // `end`, just with `null` as the empty-marker instead of
            // `false`, and neither additionally unions with the marker
            // once non-emptiness is already provable (`force_union: false`).
            "array_pop" | "array_shift" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if let Some(elem_ty) = arr_ty.array_value_type() {
                    return Self::array_pointer_result(&arr_ty, elem_ty, Type::null(), false);
                }
            }
            // `array_splice` returns an array of the removed elements (or
            // `[]` if nothing was removed) - always the input's value
            // type, re-sequenced from zero (the removed slice's original
            // keys are never preserved); non-emptiness never guaranteed
            // (an explicit zero-length `$length` legitimately removes
            // nothing).
            "array_splice" if !args.is_empty() => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if let Some(elem_ty) = arr_ty.array_value_type() {
                    return Type::intersection_of(vec![
                        Type::array(Type::unknown_array_key(), elem_ty),
                        Type::accessory(AccessoryKind::ArrayList),
                    ]);
                }
            }
            // `array_replace`/`array_replace_recursive` overlay every
            // argument's key/value pairs onto the first, later arguments
            // winning any key collision - the result's key type is the
            // union of every argument's key type, same for values
            // (mirrors `array_merge`'s key/value-union shape, without its
            // int-key renumbering quirk - `array_replace` always
            // preserves int keys exactly as given). List-ness/non-
            // emptiness aren't tracked (a later argument overwriting
            // every key of an earlier, non-empty one can leave fewer
            // surviving keys than any single argument had).
            "array_replace" | "array_replace_recursive" if !args.is_empty() => {
                let mut key_ty: Option<Type> = None;
                let mut val_ty: Option<Type> = None;
                for a in args {
                    let arg_ty = self.infer_expr(&a.value, locals);
                    if let Some(k) = arg_ty.array_key_type() {
                        key_ty = Some(
                            key_ty.map_or(k.clone(), |existing| self.relation().union(existing, k)),
                        );
                    }
                    if let Some(v) = arg_ty.array_value_type() {
                        val_ty = Some(
                            val_ty.map_or(v.clone(), |existing| self.relation().union(existing, v)),
                        );
                    }
                }
                if let (Some(k), Some(v)) = (key_ty, val_ty) {
                    return Type::array(k, v);
                }
            }
            // `array_pad` pads (or truncates, negative `$size`) `$array`
            // up to `abs($size)` elements with copies of `$value` - the
            // result's value type is the union of the input's element
            // type and the pad value's type (an already-long-enough
            // array is returned unchanged, so the pad value's type never
            // replaces the original, only widens it). Key type/list-ness
            // aren't modeled precisely, since padding can truncate from
            // either end depending on `$size`'s sign, re-sequencing keys.
            "array_pad" if args.len() == 3 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                let pad_ty = self.infer_expr(&args[2].value, locals);
                if let Some(elem_ty) = arr_ty.array_value_type() {
                    return Type::array(
                        Type::unknown_array_key(),
                        self.relation().union(elem_ty, pad_ty),
                    );
                }
            }
            // The result's keys are the values of `$array` (only
            // `int|string` ones are legal at runtime - any other value
            // type throws a `TypeError`, so this doesn't narrow further
            // based on input value type precision), each mapped to its
            // occurrence count - real PHP always returns a plain `int`
            // count, non-emptiness matching the input's.
            "array_count_values" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if arr_ty.is_array().yes() {
                    // Only int and string values count; others are skipped.
                    let keyable = self.relation().union(Type::int(), Type::string());
                    let elem_ty = arr_ty.array_value_type().unwrap_or(Type::mixed());
                    let all_keyable = self.relation().is_super_type_of(&keyable, &elem_ty).yes();
                    let counted = Type::array(
                        self.relation().narrow(&elem_ty, &keyable).to_array_key(),
                        Type::int_range(Some(1), None),
                    );
                    return if all_keyable && arr_ty.is_non_empty_array().yes() {
                        Type::intersection_of(vec![
                            counted,
                            Type::accessory(AccessoryKind::NonEmptyArray),
                        ])
                    } else {
                        counted
                    };
                }
            }
            // Keys are re-cased (`CASE_UPPER`/`CASE_LOWER`) but never
            // added, removed, or retyped - an `int` key is entirely
            // unaffected (only a string key is ever re-cased), so unlike
            // every other function in this match, the input's own type
            // survives completely unchanged.
            "array_change_key_case" if matches!(args.len(), 1..=2) => {
                return self.infer_expr(&args[0].value, locals);
            }
            // With `$num` omitted or exactly the literal `1`, `array_rand`
            // returns a single random key (never an array) - the input's
            // key type, not the bare `array|string|int` union its plain
            // stub signature gives with no way to know which. A literal
            // `$num` above `1` returns an array of that many keys instead.
            "array_rand" if matches!(args.len(), 1..=2) => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                let num_literal = args
                    .get(1)
                    .map(|a| self.infer_expr(&a.value, locals))
                    .and_then(|t| t.int_literal_value());
                if let Some(key_ty) = arr_ty.array_key_type() {
                    match num_literal.unwrap_or(1) {
                        1 => return key_ty,
                        n if n > 1 => {
                            return Type::array(Type::unknown_array_key(), key_ty);
                        }
                        _ => {}
                    }
                }
            }
            // `reset`/`current`/`end` all report the array's own element
            // type (bare when it's provably non-empty, unioned with
            // `false` otherwise, `false` alone when it's provably empty),
            // the same three-way split `array_pointer_result` below
            // shares with `key`. `next`/`prev` always union with `false`
            // even when the array is provably non-empty: moving the
            // pointer can run off either end regardless of element count.
            "reset" | "current" | "end" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if arr_ty.is_array().yes() {
                    let elem_ty = arr_ty.array_value_type().unwrap_or(Type::mixed());
                    return Self::array_pointer_result(
                        &arr_ty,
                        elem_ty,
                        Type::bool_literal(false),
                        false,
                    );
                }
            }
            "next" | "prev" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if arr_ty.is_array().yes() {
                    let elem_ty = arr_ty.array_value_type().unwrap_or(Type::mixed());
                    return Self::array_pointer_result(
                        &arr_ty,
                        elem_ty,
                        Type::bool_literal(false),
                        true,
                    );
                }
            }
            // `key()` (the current pointer's own key) and
            // `array_key_first`/`array_key_last` (pure, pointer-
            // independent) all share the same shape as `reset`/
            // `current`/`end` above, but report `null` instead of
            // `false` for "no such element" - real PHP's own
            // `key(): int|string|null` (never `false`, unlike its
            // siblings) and `array_key_first`/`array_key_last`'s
            // identical `int|string|null` signature.
            "key" | "array_key_first" | "array_key_last" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if name != "key"
                    && let Some(key) = shape_end_key(&arr_ty, name == "array_key_first")
                {
                    return key;
                }
                if arr_ty.is_array().yes() {
                    let key_ty = arr_ty
                        .array_key_type()
                        .unwrap_or_else(Type::unknown_array_key);
                    return Self::array_pointer_result(&arr_ty, key_ty, Type::null(), false);
                }
            }
            // Merging only list-safe arguments stays a list (a literal,
            // non-empty argument makes it a non-empty-list); merging in
            // even one argument whose key type could genuinely include a
            // `String` degrades list-ness for the whole result, same as a
            // spread array literal's degrade rule (real PHP's own
            // `array_merge` always renumbers every argument's *integer*
            // keys sequentially and discards their original values,
            // regardless of whether an argument was already list-shaped
            // itself - only a possible *string* key actually survives
            // into the output unrenumbered, confirmed directly against
            // the real vendored `phpstan.phar` via `assertType()`) - but
            // the element-type union is still tracked precisely either
            // way. General key-aware shape merging (`array_merge($shape,
            // ...)`) is real PHPStan's much richer behavior and
            // deliberately not attempted here.
            "array_merge" if !args.is_empty() => {
                let arg_types: Vec<Type> = args
                    .iter()
                    .map(|a| self.infer_expr(&a.value, locals))
                    .collect();
                // When every argument's exact fields are known
                // (`array{...}`), merge them field-by-field via the same
                // rule a spread array literal uses (canonical int keys
                // renumbered, other keys overwritten by the later
                // argument).
                // A spread argument (`array_merge(...$listOfLists)`)
                // expands
                // to however many arrays that value itself holds at
                // runtime, not one merge argument shaped like the
                // array-of-arrays itself - never eligible for the
                // fixed-arity precise shape merge below.
                let has_spread = args.iter().any(|a| a.spread);
                // Covers the common config/options-array merge idiom
                // (`array_merge($defaults, $options)`) by staying
                // eligible only when no argument could contribute a
                // canonical int key, sidestepping real PHP's int-key-
                // renumbering semantics entirely (a genuinely int-keyed
                // argument still falls through to the coarse path
                // below). Eligible either when a real array shape
                // argument exists, or when a plain array already carries
                // its own `hasOffsetValue(...)` precision
                // (`has_offset_value_parts` - `checker::assignment::
                // write_literal_key_aware`'s own construction, e.g. a
                // `@var array<string, mixed>` local built up via several
                // literal-key writes) - either kind of argument has real,
                // per-key precision worth preserving via the merge loop
                // below, which now promotes a tracked plain argument's
                // own keys into real fields too, not just a bare
                // `ArrayShape`'s.
                let all_string_keyed_or_shape = !has_spread
                    && arg_types.iter().any(|t| {
                        t.as_array_shape().is_some()
                            || t.has_offset_value_parts()
                                .is_some_and(|(_, pairs)| !pairs.is_empty())
                    })
                    && arg_types.iter().all(|t| match t.as_array_shape() {
                        Some(shape) => shape
                            .fields()
                            .iter()
                            .all(|f| matches!(f.key, ShapeKey::Str(_))),
                        // A plain array (`hasOffsetValue`-tracked or not)
                        // is only eligible when its own key type is
                        // confidently *not* int - one that could
                        // genuinely include an int key would need real
                        // PHP's renumbering semantics, which this
                        // restricted path deliberately doesn't attempt.
                        // Loosened from an exact `string` match: any
                        // key type that provably excludes `Int` is just
                        // as safe here, not only the bare `string`
                        // variant.
                        None => t.array_key_type().is_some_and(|k| k.is_int().no()),
                    });
                if all_string_keyed_or_shape {
                    let mut merged = Shape::sealed(Vec::new());
                    // `array_merge` renumbers from 0 and never reads a negative key,
                    // so the PHP versions agree on every index.
                    let mut next_index = types::NextArrayIndex::new(types::TrinaryLogic::Yes);
                    for arg_ty in &arg_types {
                        match arg_ty.as_array_shape() {
                            Some(inner) => {
                                for f in inner.fields() {
                                    if Self::merge_shape_field(
                                        &mut merged,
                                        &mut next_index,
                                        &f.key,
                                        f.ty.clone(),
                                        f.optional,
                                    )
                                    .is_err()
                                    {
                                        return Type::never();
                                    }
                                }
                                // An unsealed argument's own untracked
                                // keys could still be present in the
                                // merge too.
                                if let Some(tail) = &inner.unsealed {
                                    unseal_with(&mut merged, &tail.0, &tail.1);
                                }
                            }
                            _ => {
                                // A plain string-keyed array: every
                                // already-known field might be overwritten
                                // by one of its (unknown) keys, or might
                                // not - broaden the type, keep it present
                                // (the earlier shape already guaranteed
                                // presence; the plain array can only
                                // change the *value*, never remove a key
                                // array_merge never touches).
                                let elem_ty = arg_ty.array_value_type().unwrap_or(Type::mixed());
                                merged.update_fields(|_, ty, _| {
                                    *ty = self.relation().union(ty.clone(), elem_ty.clone());
                                });
                                // Mark unsealed - the plain array may also
                                // contribute keys this shape never had, of
                                // its own key and value types.
                                let key_ty =
                                    arg_ty.array_key_type().unwrap_or(Type::unknown_array_key());
                                unseal_with(&mut merged, &key_ty, &elem_ty);
                                // On top of the coarse widening above, a
                                // `hasOffsetValue`-tracked key gets
                                // promoted to a real, precise field via
                                // the same real-PHP-overwrite rule an
                                // `ArrayShape` argument's own fields use
                                // above - closing the gap this arm used to
                                // leave (a tracked plain argument's own
                                // known keys were previously discarded
                                // into the generic unsealed widening just
                                // like any other unknown key). A bare
                                // array (no tracked keys at all)
                                // gets an empty `pairs`, so this is a
                                // strict no-op for that already-covered
                                // case.
                                if let Some((_, pairs)) = arg_ty.has_offset_value_parts() {
                                    for (k, v) in pairs {
                                        if Self::merge_shape_field(
                                            &mut merged,
                                            &mut next_index,
                                            &k,
                                            v,
                                            false,
                                        )
                                        .is_err()
                                        {
                                            return Type::never();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return Type::array_shape(merged);
                }
                if !has_spread && arg_types.iter().all(|t| t.as_array_shape().is_some()) {
                    let mut merged = Shape::sealed(Vec::new());
                    // `array_merge` renumbers from 0 and never reads a negative key,
                    // so the PHP versions agree on every index.
                    let mut next_index = types::NextArrayIndex::new(types::TrinaryLogic::Yes);
                    for arg_ty in &arg_types {
                        if let Some(inner) = arg_ty.as_array_shape() {
                            for f in inner.fields() {
                                if Self::merge_shape_field(
                                    &mut merged,
                                    &mut next_index,
                                    &f.key,
                                    f.ty.clone(),
                                    f.optional,
                                )
                                .is_err()
                                {
                                    return Type::never();
                                }
                            }
                            if let Some(tail) = &inner.unsealed {
                                unseal_with(&mut merged, &tail.0, &tail.1);
                            }
                        }
                    }
                    return Type::array_shape(merged);
                }
                let mut elem_types: Vec<Type> = Vec::new();
                let mut all_list = true;
                let mut any_non_empty = false;
                let mut all_known_array_like = true;
                // Real PHP's own `array_merge` key semantics: an
                // argument that could contribute an int key has that key
                // *renumbered* (never preserving the argument's own int
                // values), while a string key is always preserved
                // verbatim - so the merged key type is `Int` and/or
                // `String` depending on which kinds are actually
                // *possible* across every argument, not an unconditional
                // `unknown_array_key()`. Both start `false` and only ever
                // turn `true` (`||=`), matching `all_list`/`any_non_empty`'s
                // own accumulate-across-arguments shape just below.
                let mut key_could_be_int = false;
                let mut key_could_be_string = false;
                // An argument whose own key type can't be determined at
                // all keeps today's existing safe default (both kinds
                // possible) rather than guessing narrower than known.
                let mut note_key_type = |k: Option<Type>| match k {
                    Some(k) => {
                        key_could_be_int |= !k.is_int().no();
                        key_could_be_string |= !k.is_string().no();
                    }
                    None => {
                        key_could_be_int = true;
                        key_could_be_string = true;
                    }
                };
                for (arg, arg_ty) in args.iter().zip(&arg_types) {
                    if arg.spread {
                        // Each element of the spread source is one of the
                        // arrays actually being merged - its own element
                        // (and key) type is one level further down than
                        // the spread source's own. The result is non-empty
                        // when at least one array is merged and every one
                        // of them is non-empty.
                        match arg_ty.array_value_type() {
                            Some(inner_ty) => {
                                if !Self::is_list_safe_array_merge_argument(&inner_ty) {
                                    all_list = false;
                                }
                                if arg_ty.is_non_empty_array().yes()
                                    && inner_ty.is_non_empty_array().yes()
                                {
                                    any_non_empty = true;
                                }
                                note_key_type(inner_ty.array_key_type());
                                match inner_ty.array_value_type() {
                                    Some(t) => elem_types.push(t),
                                    None => all_known_array_like = false,
                                }
                            }
                            None => all_known_array_like = false,
                        }
                    } else if arg_ty.is_array().yes() {
                        if !Self::is_list_safe_array_merge_argument(arg_ty) {
                            all_list = false;
                        }
                        if arg_ty.is_non_empty_array().yes() {
                            any_non_empty = true;
                        }
                        note_key_type(arg_ty.array_key_type());
                        if let Some(t) = arg_ty.array_value_type() {
                            elem_types.push(t);
                        }
                    } else {
                        all_known_array_like = false;
                    }
                }
                if all_known_array_like {
                    let elem_ty = elem_types
                        .into_iter()
                        .union_all_with(&self.relation())
                        .unwrap_or(Type::mixed());
                    let key_ty = match (key_could_be_int, key_could_be_string) {
                        (true, false) => Type::int(),
                        (false, true) => Type::string(),
                        // Both possible, or (shouldn't happen given the
                        // `None` fallback above) neither resolved -
                        // today's existing safe default.
                        _ => Type::unknown_array_key(),
                    };
                    let mut members = vec![Type::array(key_ty, elem_ty)];
                    if all_list {
                        members.push(Type::accessory(AccessoryKind::ArrayList));
                    }
                    if any_non_empty {
                        members.push(Type::accessory(AccessoryKind::NonEmptyArray));
                    }
                    return if members.len() == 1 {
                        members.pop().unwrap()
                    } else {
                        Type::intersection_of(members)
                    };
                }
            }
            // Fills every slot with the same, already-known $value - no
            // closure or element-type binding needed. Marks the result
            // `list`-accessory whenever the start index is provably `0`,
            // and `non-empty`-accessory whenever the count is provably
            // `>= 1`, the same accessory markers every other array-shaped
            // builtin in this match carries when it can (otherwise a
            // caller merging this result back with a loop's pre-loop `[]`
            // sees a needlessly bare `array<V>`).
            "array_fill" if args.len() == 3 => {
                let base = Type::array(
                    Type::unknown_array_key(),
                    self.infer_expr(&args[2].value, locals),
                );
                let start_is_zero =
                    self.infer_expr(&args[0].value, locals).int_literal_value() == Some(0);
                let count_is_non_empty = self
                    .infer_expr(&args[1].value, locals)
                    .int_bounds()
                    .is_some_and(|(min, _)| min.is_some_and(|m| m >= 1));
                let mut members = vec![base];
                if start_is_zero {
                    members.push(Type::accessory(AccessoryKind::ArrayList));
                }
                if count_is_non_empty {
                    members.push(Type::accessory(AccessoryKind::NonEmptyArray));
                }
                return if members.len() == 1 {
                    members.pop().unwrap()
                } else {
                    Type::intersection_of(members)
                };
            }
            // `$keys`' own elements become the result's real keys -
            // `array_fill_keys(['a', 'b'], 0)` really is `array<string,
            // int>`, not a bare `unknown_array_key()`.
            "array_fill_keys" if args.len() == 2 => {
                let keys_ty = self.infer_expr(&args[0].value, locals);
                let key_ty = keys_ty
                    .array_value_type()
                    .map_or_else(Type::unknown_array_key, |v| {
                        v.to_string_array_key(self.registry)
                    });
                let filled = Type::array(key_ty, self.infer_expr(&args[1].value, locals));
                return if keys_ty.is_non_empty_array().yes() {
                    filled.to_non_empty_array()
                } else {
                    filled
                };
            }
            // Splits one array into several same-shaped ones - the
            // result is an array *of* arrays, each holding the original
            // element type.
            "array_chunk" if args.len() == 2 || args.len() == 3 => {
                if let Some(elem_ty) = (self.infer_expr(&args[0].value, locals)).array_value_type()
                {
                    return Type::array(
                        Type::unknown_array_key(),
                        Type::array(Type::unknown_array_key(), elem_ty),
                    );
                }
            }
            // A list of the array's keys; the search arguments only choose
            // which.
            "array_keys" if (1..=3).contains(&args.len()) => {
                let arg_ty = self.infer_expr(&args[0].value, locals);
                if arg_ty.is_array().yes()
                    && let Some(key_ty) = arg_ty.array_key_type()
                {
                    // Keys nothing is known of are benevolently int or string.
                    let key_ty = if key_ty.is_unknown_array_key() {
                        Type::benevolent_union_of(key_ty.union_members().to_vec())
                    } else {
                        key_ty
                    };
                    return Type::intersection_of(vec![
                        Type::array(Type::unknown_array_key(), key_ty),
                        Type::accessory(AccessoryKind::ArrayList),
                    ]);
                }
            }
            // One named column of each row: a shaped row's field, an
            // unshaped row's value type.
            "array_column" if args.len() == 2 || args.len() == 3 => {
                if let Some(row_ty) = (self.infer_expr(&args[0].value, locals)).array_value_type() {
                    // A shaped row looks up the one named field; an
                    // unshaped row (`array<string, mixed>`, the common
                    // shape for a raw DB-fetch row) has no per-key
                    // precision to offer, so every column read is simply
                    // the row's own uniform element type.
                    let field_ty = match row_ty.as_array_shape() {
                        Some(shape) => literal_shape_key(&args[1].value)
                            .and_then(|key| shape.field(&key))
                            .map(|f| f.ty.clone()),
                        None if row_ty.is_array().yes() => row_ty.array_value_type(),
                        None => None,
                    };
                    if let Some(field_ty) = field_ty {
                        // With an index column, its values cast as keys; a
                        // row without it is appended under the next int.
                        let index_key = match args.get(2) {
                            Some(index) if !is_null_literal(&index.value) => {
                                match (row_ty.as_array_shape(), literal_shape_key(&index.value)) {
                                    (Some(shape), Some(key)) => match shape.field(&key) {
                                        Some(f) if f.optional => {
                                            self.relation().union(f.ty.to_array_key(), Type::int())
                                        }
                                        Some(f) => f.ty.to_array_key(),
                                        None if shape.is_sealed() => Type::int(),
                                        None => Type::unknown_array_key(),
                                    },
                                    (None, _) if row_ty.is_array().yes() => row_ty
                                        .array_value_type()
                                        .map_or_else(Type::unknown_array_key, |v| {
                                            self.relation().union(v.to_array_key(), Type::int())
                                        }),
                                    _ => Type::unknown_array_key(),
                                }
                            }
                            _ => Type::unknown_array_key(),
                        };
                        let arr_ty = Type::array(index_key, field_ty);
                        // The 2-argument form (no `$index_key`) always
                        // builds a brand-new array with fresh sequential
                        // integer keys starting at 0, regardless of the
                        // input array's own keys - a genuine list. The
                        // 3-argument form uses `$index_key`'s column
                        // values as the result's keys instead, which
                        // generally aren't sequential integers, so it
                        // keeps a plain, unmarked `Array`.
                        return if args
                            .get(2)
                            .is_none_or(|index| is_null_literal(&index.value))
                        {
                            Type::intersection_of(vec![
                                arr_ty,
                                Type::accessory(AccessoryKind::ArrayList),
                            ])
                        } else {
                            arr_ty
                        };
                    }
                }
            }
            // The real stub declares `array_search(mixed, array, bool):
            // string|int|false` unconditionally - this project's
            // array has no key-type parameter, so it can't
            // generally narrow the key-type half the way real PHPStan
            // does. But a list-typed haystack is a narrower special case
            // it already tracks precisely without any key-type machinery:
            // a list's keys are always sequential integers, so searching
            // one can only ever find an `int` key (or `false`), never
            // `string`.
            "array_search" if args.len() >= 2 => {
                let haystack_ty = self.infer_expr(&args[1].value, locals);
                // A shape's keys found by each value's comparison with the
                // needle: loose without `strict`, identical with it.
                if let Some(shape) = haystack_ty.as_array_shape()
                    && shape.is_sealed()
                {
                    let needle = self.infer_expr(&args[0].value, locals);
                    let strict = args.get(2).is_some_and(|a| {
                        matches!(&a.value.kind, ExprKind::Literal(Literal::Bool(true)))
                    });
                    let version = self.comparison_php_version();
                    let equals = |value: &Type| {
                        if strict {
                            if needle.is_single_value() && value.is_single_value() {
                                types::TrinaryLogic::from_bool(needle == *value)
                            } else if self.relation().disjoint(&needle, value) {
                                types::TrinaryLogic::No
                            } else {
                                types::TrinaryLogic::Maybe
                            }
                        } else {
                            self.relation().loose_equal(&needle, value, version)
                        }
                    };
                    let mut keys = Vec::new();
                    let mut surely_found = false;
                    for field in shape.fields() {
                        let verdict = equals(&field.ty);
                        if !verdict.no() {
                            keys.push(field.key.to_type());
                        }
                        surely_found |= verdict.yes() && !field.optional;
                    }
                    if !surely_found {
                        keys.push(Type::bool_literal(false));
                    }
                    return self.relation().union_of(keys);
                }
                if haystack_ty.is_list().yes() {
                    return self
                        .relation()
                        .union(Type::int(), Type::bool_literal(false));
                }
                // Same idea, one level less precise - a haystack whose
                // key type is confidently known (not just "definitely a
                // list") narrows the found-key half the same way, e.g. an
                // `array<string, V>` haystack can only ever find a
                // `string` key.
                if let Some(key_ty) = haystack_ty.array_key_type()
                    && !key_ty.is_unknown_array_key()
                {
                    return self.relation().union(key_ty, Type::bool_literal(false));
                }
            }
            // The real stub declares `version_compare(string, string,
            // ?string $operator): int|bool` unconditionally, but whether
            // the 3rd `$operator` argument is syntactically present (not
            // its value) determines the real return: present means
            // `bool`, absent means always exactly one of `-1`/`0`/`1`.
            // Scoped to the 2-argument form; the 3-argument form falls
            // through to the generic `int|bool` stub type unchanged.
            "version_compare" if args.len() == 2 => return Type::int(),
            // The real stub declares `microtime(bool $as_float = false):
            // string|float` unconditionally; a literal `true` argument
            // narrows to plain `float`, a literal `false` (or no
            // argument) to a string. `is_super_type_of`, not `==`, since a
            // `Union` that happens to be all-`True` or all-`False` (e.g.
            // narrowed from a wider type) should narrow the same way a
            // bare `True`/`False` would.
            "microtime" => match args.first().map(|a| self.infer_expr(&a.value, locals)) {
                None => return Type::string(),
                Some(ty)
                    if types::Relation::structural()
                        .is_super_type_of(&Type::bool_literal(true), &ty)
                        .yes() =>
                {
                    return Type::float();
                }
                Some(ty)
                    if types::Relation::structural()
                        .is_super_type_of(&Type::bool_literal(false), &ty)
                        .yes() =>
                {
                    return Type::string();
                }
                _ => {}
            },
            // The real stubs declare these unconditionally unioned with
            // `array` (`str_replace(...): array|string`), even though the
            // function only ever returns an array when `$subject` itself
            // is one. Scoped to "subject is string-like"
            // (`Relation::is_super_type_of` against `string`, not a bare
            // `==`) - an accessory-refined subject (`non-empty-string`
            // parses to `Intersection([String, Accessory(_)])`) would
            // silently fail to equal plain `string`.
            "str_replace" | "str_ireplace" if args.len() == 3 || args.len() == 4 => {
                if types::Relation::structural()
                    .is_super_type_of(&Type::string(), &self.infer_expr(&args[2].value, locals))
                    .yes()
                {
                    return Type::string();
                }
            }
            // Same subject-dependent shape as above, but these can also
            // return `null` on a regex error (already correctly present
            // in the real stub's own declared type) - computed from the
            // ordinary generic path first and then stripped of any
            // `array` member, rather than hardcoding the nullability this
            // project doesn't otherwise know for certain.
            "preg_replace" | "preg_replace_callback" | "preg_filter"
                if args.len() >= 3
                    && types::Relation::structural()
                        .is_super_type_of(&Type::string(), &self.infer_expr(&args[2].value, locals))
                        .yes() =>
            {
                let resolved = resolve_call_args(args, sig.params());
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return generic.without(&Type::array(Type::mixed(), Type::mixed()));
            }
            // Same subject-dependent-return-type family as the two arms
            // above - the real stub is `substr_replace(array|string
            // $string, ...): array|string`, and unlike `str_replace`'s
            // subject at index 2, `substr_replace`'s subject is index 0.
            // Never returns `null`, so a plain early return suffices.
            "substr_replace" if args.len() == 3 || args.len() == 4 => {
                if types::Relation::structural()
                    .is_super_type_of(&Type::string(), &self.infer_expr(&args[0].value, locals))
                    .yes()
                {
                    return Type::string();
                }
            }
            // The real stub declares `getenv(?string $name = null, bool
            // $local_only = false): array|string|false` unconditionally,
            // but real PHP only returns `array` for a truly zero-argument
            // call, and only `string|false` when `$name` is a real,
            // non-null string. `args.is_empty()` must be checked on the
            // raw call syntax, before any named/positional resolution -
            // resolving `name` to `None` also happens for
            // `getenv(local_only: true)`, which must NOT get the
            // plain-`array` answer.
            "getenv" => {
                if args.is_empty() {
                    return Type::array(Type::unknown_array_key(), Type::string());
                }
                let resolved = resolve_call_args(args, sig.params());
                if let Some(name_ty) = resolved
                    .first()
                    .copied()
                    .flatten()
                    .map(|e| self.infer_expr(e, locals))
                    && types::Relation::structural()
                        .is_super_type_of(&Type::string(), &name_ty)
                        .yes()
                {
                    return self
                        .relation()
                        .union_of(vec![Type::string(), Type::bool_literal(false)]);
                }
            }
            // The native stub declares a plain `string|false`, but a
            // successful call always returns a real non-falsy-string,
            // never `''`/`'0'`.
            "tempnam" if args.len() == 2 => {
                return self.relation().union_of(vec![
                    Type::intersection_of(vec![
                        Type::string(),
                        Type::accessory(AccessoryKind::NonFalsyString),
                    ]),
                    Type::bool_literal(false),
                ]);
            }
            // Constant-folds a small literal-input/literal-count call
            // outright, and otherwise marks the result `non-falsy-string`
            // when the input is itself non-falsy and the count is
            // provably `>= 1`. Doesn't attempt real PHPStan's further
            // accessory tracking (literal-string/numeric-string/
            // lowercase/uppercase propagation), which rests on machinery
            // this project doesn't otherwise track for plain strings.
            "str_repeat" if args.len() == 2 => {
                let input_ty = self.infer_expr(&args[0].value, locals);
                let count_ty = self.infer_expr(&args[1].value, locals);
                if count_ty.int_literal_value().is_some_and(|n| n <= 0) {
                    return Type::string_literal(PhpString::default());
                }
                if let (Some(s), Some(n)) = (
                    input_ty.string_literal_value(),
                    count_ty.int_literal_value(),
                ) && n >= 0
                    && s.len().saturating_mul(n as usize) < 100
                {
                    return Type::string_literal(s.repeat(n as usize));
                }
                let count_is_at_least_one = count_ty
                    .int_bounds()
                    .is_some_and(|(min, _)| min.is_some_and(|m| m >= 1));
                if count_is_at_least_one
                    && input_ty
                        .is_refined_string(AccessoryKind::NonFalsyString)
                        .yes()
                {
                    return Type::intersection_of(vec![
                        Type::string(),
                        Type::accessory(AccessoryKind::NonFalsyString),
                    ]);
                }
                return Type::string();
            }
            // Mirrors real PHPStan's `StrCaseFunctionsReturnTypeExtension` -
            // a literal argument is constant-folded outright (ASCII-only,
            // matching PHP 8.2+'s locale-independent semantics; this
            // project's case-tracking is ASCII-only project-wide, see
            // `AccessoryKind::LowercaseString`/`UppercaseString`'s doc
            // comment), otherwise the matching case accessory is composed
            // with whatever numeric/non-empty/non-falsy accessory the
            // argument already carried.
            "strtolower" | "strtoupper" | "lcfirst" | "ucfirst" | "mb_strtolower"
            | "mb_strtoupper" | "mb_lcfirst" | "mb_ucfirst"
                if !args.is_empty() =>
            {
                let op = match name.trim_start_matches("mb_") {
                    "strtolower" => CaseOp::Lower,
                    "strtoupper" => CaseOp::Upper,
                    "lcfirst" => CaseOp::LowerFirst,
                    _ => CaseOp::UpperFirst,
                };
                return self.infer_case_function_call(
                    &args[0].value,
                    locals,
                    &[op],
                    name.starts_with("mb_"),
                );
            }
            // A mode that isn't known literally could be any of them.
            "mb_convert_case" if args.len() >= 2 => {
                let mode_ty = self.infer_expr(&args[1].value, locals);
                let ops: Vec<CaseOp> = mode_ty
                    .constant_ints()
                    .iter()
                    .map(|&m| CaseOp::from_mb_mode(m))
                    .collect::<Option<Vec<CaseOp>>>()
                    .unwrap_or_default();
                return self.infer_case_function_call(&args[0].value, locals, &ops, true);
            }
            // The vendored stub's `@return string[]|false` reflects PHP
            // 7's behavior for an empty `$separator` - PHP 8+ throws a
            // `ValueError` there instead of returning `false`, so this
            // drops `|false` entirely and always answers
            // `non-empty-list<string>`, even for a non-literal
            // `$separator` (there's no other way `explode()` can return
            // fewer than 1 element).
            "explode" if matches!(args.len(), 2..=3) => {
                return Type::intersection_of(vec![
                    Type::array(Type::unknown_array_key(), Type::string()),
                    Type::accessory(AccessoryKind::ArrayList),
                    Type::accessory(AccessoryKind::NonEmptyArray),
                ]);
            }
            // Same family as tempnam() above - the native stub declares a
            // bare `array|false`, but a successful call always returns a
            // real `list<string>`.
            "glob" if matches!(args.len(), 1..=2) => {
                return self.relation().union_of(vec![
                    Type::intersection_of(vec![
                        Type::array(Type::unknown_array_key(), Type::string()),
                        Type::accessory(AccessoryKind::ArrayList),
                    ]),
                    Type::bool_literal(false),
                ]);
            }
            // Same family, subject at index 1 (`preg_replace_callback_array
            // (array $pattern, array|string $subject, ...): array|string|
            // null`) - strips the `array` member when `$subject` is
            // provably a string.
            "preg_replace_callback_array"
                if args.len() >= 2
                    && types::Relation::structural()
                        .is_super_type_of(&Type::string(), &self.infer_expr(&args[1].value, locals))
                        .yes() =>
            {
                let resolved = resolve_call_args(args, sig.params());
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return generic.without(&Type::array(Type::mixed(), Type::mixed()));
            }
            // `mb_convert_encoding()`'s real dynamic extension is
            // considerably more involved (it also reasons about
            // `$from_encoding` to decide whether `false` stays possible) -
            // only the array-stripping half is implemented here.
            "mb_convert_encoding"
                if args.len() >= 2
                    && types::Relation::structural()
                        .is_super_type_of(&Type::string(), &self.infer_expr(&args[0].value, locals))
                        .yes() =>
            {
                let resolved = resolve_call_args(args, sig.params());
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return generic.without(&Type::array(Type::mixed(), Type::mixed()));
            }
            // `json_encode(...): string|false` - passing
            // `JSON_THROW_ON_ERROR` makes a real failure throw instead of
            // returning false. `contains_json_throw_on_error` walks an
            // arbitrary-depth `|`-chain, not just a bare constant, since
            // the flag is commonly OR'd with others.
            // An array, or an object that can't serialize itself as
            // something else, encodes to text starting with `[` or `{`.
            "json_encode" if !args.is_empty() => {
                let resolved = resolve_call_args(args, sig.params());
                let throws = resolved
                    .get(1)
                    .copied()
                    .flatten()
                    .is_some_and(contains_json_throw_on_error);
                let structured = resolved.first().copied().flatten().is_some_and(|value| {
                    let ty = self.infer_expr(value, locals);
                    let serializable = Type::object("JsonSerializable".into(), Vec::new());
                    ty.union_members().iter().all(|m| {
                        m.is_array().yes()
                            || (m.is_object().yes()
                                && self.relation().is_super_type_of(&serializable, m).no())
                    })
                });
                if throws || structured {
                    let generic = self.infer_call_return_type(sig, &resolved, locals);
                    let generic = if throws {
                        generic.without(&Type::bool_literal(false))
                    } else {
                        generic
                    };
                    return if structured {
                        generic
                            .with_string_accessory(AccessoryKind::NonFalsyString)
                            .with_string_accessory(AccessoryKind::NonDecimalIntegerString)
                    } else {
                        generic
                    };
                }
            }
            // The real stub declares `curl_getinfo` unconditionally
            // `mixed` - maps a literal `CURLINFO_*` option constant to a
            // concrete type instead. Scoped to the 2-arg form; a bare
            // 1-arg `curl_getinfo($ch)` call returns a much bigger "all
            // components" array shape this project doesn't model, and
            // falls through to `mixed` unchanged.
            "curl_getinfo" if args.len() == 2 => {
                if let ExprKind::Name(option_name) = &args[1].value.kind
                    && let Some(ty) =
                        curl_getinfo_component_type(option_name.trim_start_matches('\\'))
                {
                    return ty;
                }
            }
            // The real stub declares `get_class` unconditionally
            // `string` - resolves the return type from the argument's
            // own class instead. Scoped to the 1-arg form; the 0-arg form
            // ("the calling object's own class") was removed in PHP 8. An
            // argument whose type isn't a concrete `Object` falls through
            // to plain `string`, unchanged.
            "get_class" if args.len() == 1 => {
                let object_ty = self.infer_expr(&args[0].value, locals);
                if let Some((class, _)) = object_ty.as_object() {
                    return Type::class_string(Type::object_ref(class, &[]));
                }
            }
            // `get_called_class()`'s stub is unconditionally
            // `string|false` - resolves to `static::class`'s type instead
            // (`false` only when called outside any class). This project
            // doesn't track true late static binding (`resolve_pseudo`
            // already collapses `self`/`static` to the same
            // `current_class`) - `current_class` is exactly that
            // resolution, the same source `self::class`/`static::class`
            // already use.
            "get_called_class" if args.is_empty() => {
                return match &self.current_class {
                    Some(class) => {
                        Type::class_string(Type::object(class.clone().into(), Vec::new()))
                    }
                    None => Type::bool_literal(false),
                };
            }
            // `get_parent_class($object_or_class)`'s stub is
            // unconditionally `string|false` - resolves it from the
            // argument's own class instead, walking its `extends` chain.
            // The vendored stub declares `$object_or_class` with no
            // default, so a 0-arg call already gets its own "expects at
            // least 1 argument" diagnostic from `check_args`, independent
            // of this refinement. Falls through to plain `string|false`
            // for anything `find_declared_parent_class` can't answer
            // confidently.
            "get_parent_class" if args.len() == 1 => {
                let object_ty = self.infer_expr(&args[0].value, locals);
                if let Some((class, _)) = object_ty.as_object()
                    && let Some(ty) = self.find_declared_parent_class(class)
                {
                    return ty;
                }
            }
            // `gettype()`'s stub is unconditionally `string` - maps a
            // concretely-known argument type to its exact runtime
            // type-name string instead. `gettype_result` returns `None`
            // for a genuinely unknown argument, leaving plain `string`
            // unchanged.
            "gettype" if args.len() == 1 => {
                if let Some(ty) = gettype_result(&self.infer_expr(&args[0].value, locals)) {
                    return ty;
                }
            }
            // `get_debug_type()`'s stub is unconditionally `string` - maps
            // a concretely-known argument type to its exact
            // `get_debug_type()`-flavored name instead: an object's own
            // class name only when that class is `final`, since a
            // non-final class's runtime value could be any subclass and
            // can't be pinned down tighter than `string`.
            "get_debug_type" if args.len() == 1 => {
                if let Some(ty) =
                    self.get_debug_type_result(&self.infer_expr(&args[0].value, locals))
                {
                    return ty;
                }
            }
            // `date()`'s stub is unconditionally `string`. Real PHPStan
            // observes each format character's real output by actually
            // calling PHP's `date()`; this instead uses a purely static
            // per-character classification (`date_format_return_type`
            // below), refining only a literal format string composed
            // entirely of recognized characters - anything else falls
            // through to plain `string`, unchanged.
            "date" if !args.is_empty() => {
                if let [format] = &self
                    .infer_expr(&args[0].value, locals)
                    .constant_strings_text()[..]
                    && let Some(ty) = date_format_return_type(format)
                {
                    return ty;
                }
            }
            // `idate()`'s stub is unconditionally `int|false` - maps each
            // single recognized format character to its known numeric
            // bound (`IntRange`). Scoped to a literal single-character
            // format string, `idate()`'s only real accepted shape -
            // anything else falls through to plain `int|false`, unchanged.
            "idate" if !args.is_empty() => {
                if let [format] = &self
                    .infer_expr(&args[0].value, locals)
                    .constant_strings_text()[..]
                    && let Some(c) = single_char(format)
                    && let Some((min, max)) = idate_format_char_range(c)
                {
                    return Type::int_range(Some(min), Some(max));
                }
            }
            // The real stub is unconditionally `array` (the 2-arg
            // `$flags` overload is `array|string`) - the 1-arg form gets
            // a real shape: `basename`/`filename` always present,
            // `extension` only when the path actually has one (simplified
            // to always-optional, since this project can't know a
            // literal path has a dot in it). `dirname` is only
            // unconditionally present when `$path`'s type is provably
            // `non-empty-string`; otherwise it's optional too, since an
            // empty `$path` makes `dirname` read back as `'.'` (a real,
            // present value, not an absent offset), which doesn't
            // contradict a plain `string`-typed argument's key being
            // genuinely unprovable either way. Narrows the 2-arg form
            // down to plain `string` for a literal single-component
            // `PATHINFO_*` flag (everything but `PATHINFO_ALL`, which
            // gets the same array shape as the 1-arg form). A non-literal
            // `$flags` keeps the `array|string` union, unchanged.
            "pathinfo" if args.len() == 1 => {
                let path_ty = self.infer_expr(&args[0].value, locals);
                return pathinfo_array_shape(&path_ty);
            }
            "pathinfo" if args.len() == 2 => {
                if let ExprKind::Name(flag_name) = &args[1].value.kind {
                    let path_ty = self.infer_expr(&args[0].value, locals);
                    return match flag_name.trim_start_matches('\\') {
                        "PATHINFO_DIRNAME" | "PATHINFO_BASENAME" | "PATHINFO_EXTENSION"
                        | "PATHINFO_FILENAME" => Type::string(),
                        "PATHINFO_ALL" => pathinfo_array_shape(&path_ty),
                        _ => self
                            .relation()
                            .union_of(vec![pathinfo_array_shape(&path_ty), Type::string()]),
                    };
                }
            }
            // The real stub is unconditionally `array|string|int|false|
            // null` - narrows the no-`$component`/non-literal-`$component`
            // form down to a real shape (every URL component optional,
            // since none are guaranteed present) unioned with `false`,
            // and the literal-`PHP_URL_*`-constant form down to that
            // one component's real type. Deliberately skips the literal-
            // `$url`-string special case: re-parsing a URL exactly the
            // way PHP's C implementation does isn't worth it for the
            // precision gained here.
            "parse_url" if args.len() == 1 => {
                return self.relation().union_of(vec![
                    parse_url_components_array_shape(),
                    Type::bool_literal(false),
                ]);
            }
            "parse_url" if args.len() == 2 => {
                if let ExprKind::Name(component_name) = &args[1].value.kind
                    && let Some(ty) =
                        parse_url_component_type(component_name.trim_start_matches('\\'))
                {
                    return ty;
                }
            }
            // `sscanf($string, $format)`'s stub is unconditionally
            // `array|null` - walks a literal `$format` string's
            // `%`-specifiers (`%d`, `%5s`, ...) and builds one array field
            // per specifier, each typed by its conversion character
            // (numeric conversions become `int`/`float`; anything else
            // stays `string`) - always nullable per element (a trailing
            // unmatched specifier leaves that slot `null`) and the whole
            // array nullable too (a total match failure). Scoped to the
            // 2-arg form; the 3+-arg by-reference-`$vars` form returns
            // `int|null` instead, a materially different shape not
            // modeled here.
            "sscanf" if args.len() == 2 => {
                if let [format] = &self
                    .infer_expr(&args[1].value, locals)
                    .constant_strings_text()[..]
                    && let Some(fields) = sscanf_result_fields(format)
                {
                    return self
                        .relation()
                        .union_of(vec![Type::array_shape(fields), Type::null()]);
                }
            }
            // `stat()`/`lstat()`/`fstat()`/`ssh2_sftp_stat()` share one
            // real per-key shape - their stubs are unconditionally
            // `array` (`fstat`) or `array|false`, discarding the fixed
            // 13-key shape (`dev`/`ino`/`mode`/..., every value an `int`)
            // every one of them actually returns. `dio_stat()` shares the
            // same shape under different key names, nullable rather than
            // `false`-able on failure.
            "stat" | "lstat" | "fstat" | "ssh2_sftp_stat" if args.len() == 1 => {
                return self
                    .relation()
                    .union_of(vec![stat_array_shape(), Type::bool_literal(false)]);
            }
            "dio_stat" if args.len() == 1 => {
                return self
                    .relation()
                    .union_of(vec![dio_stat_array_shape(), Type::null()]);
            }
            // `hash_algos()`/`openssl_get_cipher_methods()`/
            // `mb_list_encodings()`'s stubs are unconditionally a bare
            // `array`. Real PHPStan calls these for real at analysis time
            // to build a live allowlist against the host's installed
            // algorithms/ciphers/encodings - a heavier mechanism this
            // static binary can't replicate without shelling out to a
            // live PHP build. What's implemented here is only the
            // genuinely static half: each call's return value really is
            // always a non-empty list of non-empty strings, regardless of
            // host/version.
            "hash_algos" | "openssl_get_cipher_methods" | "mb_list_encodings"
                if args.is_empty() =>
            {
                return list_of_non_empty_strings();
            }
            _ => {}
        }
        let resolved = resolve_call_args(args, sig.params());
        self.infer_call_return_type(sig, &resolved, locals)
    }

    /// `get_parent_class`'s per-class answer: `class`'s registered
    /// `extends` chain gives an evidenced answer either way - a real
    /// parent (`class-string<Parent>`) or a real "no parent" (`false`,
    /// since a class with a known, empty `extends` genuinely has none).
    /// `None` only for a class this registry has no entry for at all,
    /// where the "don't guess" convention leaves plain `string|false`
    /// unchanged rather than picking a side.
    fn find_declared_parent_class(&self, class: &str) -> Option<Type> {
        let references = self.registry.class_references_by_name(class)?;
        Some(match references.extends.first() {
            Some(parent) => Type::class_string(Type::object(parent.into(), Vec::new())),
            None => Type::bool_literal(false),
        })
    }

    /// `get_debug_type()`'s object case: a `final` class's instances can
    /// only ever be that exact class, so its name is the exact answer; a
    /// non-`final` class's instance could be any subclass at runtime, so
    /// the tightest honest answer stays plain `string`. A class this
    /// registry has no entry for at all is treated the same as non-`final`.
    fn get_debug_type_result(&self, ty: &Type) -> Option<Type> {
        get_debug_type_for(ty, self.registry)
    }

    /// `array_reduce`'s value: the initial value when the array is empty,
    /// the callback's last return otherwise, both when either may be.
    fn reduced_array_type(
        &self,
        array: &ExprNode,
        callback: &ExprNode,
        initial: Option<&ExprNode>,
        locals: &Locals,
    ) -> Type {
        let array_ty = self.infer_expr(array, locals);
        let initial_ty = initial.map_or_else(Type::null, |e| self.infer_expr(e, locals));
        if array_ty.is_empty_array().yes() {
            return initial_ty;
        }
        let item_ty = array_ty.array_value_type().unwrap_or_else(Type::mixed);
        let returned = self.reduce_callback_return(callback, &initial_ty, &item_ty, locals);
        if array_ty.is_non_empty_array().yes() {
            returned
        } else {
            self.relation().union_of(vec![returned, initial_ty])
        }
    }

    /// What `array_reduce`'s callback returns. Its `$carry` holds the
    /// initial value on the first call and the callback's own return on
    /// every later one, so a closure is inferred with the two joined
    /// until that stops growing, widened as a loop's variables are; one
    /// that never settles is inferred with any `$carry`.
    fn reduce_callback_return(
        &self,
        callback: &ExprNode,
        initial: &Type,
        item: &Type,
        locals: &Locals,
    ) -> Type {
        const MAX_PASSES: usize = 6;
        const WIDEN_FROM_PASS: usize = 2;
        let relation = self.relation();
        let mut carry = initial.clone();
        for pass in 0..MAX_PASSES {
            let Some(returned) = self.infer_closure_return_with_param_overrides(
                callback,
                &[carry.clone(), item.clone()],
                locals,
            ) else {
                return self.callable_return_type(callback, locals);
            };
            let mut next = relation.union_of(vec![initial.clone(), returned.clone()]);
            if pass >= WIDEN_FROM_PASS && !relation.is_super_type_of(&carry, &next).yes() {
                next = carry.widen_towards(&next);
            }
            if relation.is_super_type_of(&carry, &next).yes() {
                return returned;
            }
            carry = next;
        }
        self.infer_closure_return_with_param_overrides(
            callback,
            &[Type::mixed(), item.clone()],
            locals,
        )
        .unwrap_or_else(|| self.callable_return_type(callback, locals))
    }

    /// The return type of the callable `callback` evaluates to: a
    /// closure's, a first-class callable's or a callable shape's, or that
    /// of the function or method a string or an array names; `mixed` when
    /// none is known.
    fn callable_return_type(&self, callback: &ExprNode, locals: &Locals) -> Type {
        let ty = self.infer_expr(callback, locals);
        if let Some(sig) = ty.callable_sig() {
            return (*sig.return_type).clone();
        }
        self.named_callable_signature(&ty)
            .map_or_else(Type::mixed, |sig| sig.return_type().clone())
    }

    /// The shared three-way split every internal-array-pointer function's
    /// return type follows - a provably empty `arr_ty` always reports
    /// `empty_marker` (`false` for `reset`/`current`/`end`/`next`/`prev`,
    /// `null` for `key`/`array_key_first`/`array_key_last`); a provably
    /// non-empty one reports the bare `result_ty` unless `force_union` -
    /// `next`/`prev` alone set it, since moving the pointer can run off
    /// either end regardless of element count; anything else unions the
    /// two.
    // Joins a few known types, far below the union cap.
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn array_pointer_result(
        arr_ty: &Type,
        result_ty: Type,
        empty_marker: Type,
        force_union: bool,
    ) -> Type {
        if arr_ty.is_empty_array().yes() {
            return empty_marker;
        }
        if arr_ty.is_non_empty_array().yes() && !force_union {
            return result_ty;
        }
        Type::union(result_ty, empty_marker)
    }

    /// `PrintfParametersRule`'s by-ref-output-variable-count half, for
    /// `sscanf`/`fscanf`'s 3+-arg form (`sscanf($s, $fmt, &$a, &$b, ...)`),
    /// distinct from the 2-arg return-array form (`sscanf_result_fields`,
    /// used by `infer_builtin_call_return_type`), which this never
    /// touches. Reuses `parse_sscanf_format` as a placeholder-count
    /// source, rather than reimplementing real PHPStan's live-`sscanf()`-
    /// shell-out counting, which this project has no PHP runtime for.
    pub(crate) fn check_scanf_argument_count(&mut self, name: &str, args: &[Arg], span: Span) {
        if args.len() < 3 || args.iter().any(|a| a.spread) {
            return;
        }
        let Some(fmt) = literal_string_value(&args[1].value) else {
            return;
        };
        let Some(specs) = parse_sscanf_format(&fmt) else {
            return;
        };
        let given = args.len() - 2;
        if specs.len() != given {
            self.report(
                format!(
                    "Call to {name}() contains {} placeholder(s), {given} value(s) given.",
                    specs.len()
                ),
                span,
                0,
                format!("argument.{}", name.to_ascii_lowercase()),
            );
        }
    }

    /// Mirrors real PHPStan's `FilterVarRule`'s
    /// `filterVar.nullOnFailureAndThrowOnFailure` check -
    /// `FILTER_NULL_ON_FAILURE` and `FILTER_THROW_ON_FAILURE` (PHP 8.5+)
    /// are mutually exclusive flags; real PHP doesn't error, but the
    /// combination is nonsensical. Gated on a configured `php_version` -
    /// with none configured, this stays silent rather than assume PHP 8.5
    /// is intended.
    pub(crate) fn check_filter_var_flag_conflict(&mut self, args: &[Arg], span: Span) {
        if self
            .registry
            .php_supports_somewhere(types::php_version::PhpFeature::FilterThrowOnFailureConstant)
            != Some(true)
        {
            return;
        }
        let Some(flags_arg) = args.get(2) else {
            return;
        };
        if contains_flag_name_with_options(&flags_arg.value, "FILTER_NULL_ON_FAILURE")
            && contains_flag_name_with_options(&flags_arg.value, "FILTER_THROW_ON_FAILURE")
        {
            self.report(
                "Cannot use both FILTER_NULL_ON_FAILURE and FILTER_THROW_ON_FAILURE.".to_string(),
                span,
                0,
                "filterVar.nullOnFailureAndThrowOnFailure",
            );
        }
    }

    /// A case function's result: a literal argument folded as the
    /// function would fold it (`multibyte`: by Unicode, else ASCII), each
    /// of `ops` when the mode is one of several, and otherwise a string
    /// carrying the case accessory the operations guarantee and the
    /// argument's numeric/non-falsy/non-empty one. `ops` empty is a mode
    /// that isn't known.
    fn infer_case_function_call(
        &self,
        arg: &ExprNode,
        locals: &Locals,
        ops: &[CaseOp],
        multibyte: bool,
    ) -> Type {
        let arg_ty = self.infer_expr(arg, locals);
        if let Some(s) = arg_ty.string_literal_value()
            && !ops.is_empty()
            && let Some(folded) = ops
                .iter()
                .map(|op| op.fold(s, multibyte).map(Type::string_literal))
                .collect::<Option<Vec<Type>>>()
        {
            return folded
                .into_iter()
                .union_all_with(&self.relation())
                .unwrap_or_else(Type::never);
        }

        let all = |f: fn(&CaseOp) -> bool| !ops.is_empty() && ops.iter().all(f);
        let mut accessories = Vec::new();
        if all(CaseOp::makes_lowercase)
            || (all(CaseOp::keeps_lowercase)
                && arg_ty
                    .is_refined_string(AccessoryKind::LowercaseString)
                    .yes())
        {
            accessories.push(AccessoryKind::LowercaseString);
        }
        // A multibyte titlecase can bring in ASCII lowercase (`ß` is `Ss`).
        let keeps_uppercase = !ops.is_empty()
            && ops
                .iter()
                .all(|op| op.keeps_uppercase() && !(multibyte && *op == CaseOp::UpperFirst));
        if all(CaseOp::makes_uppercase)
            || (keeps_uppercase
                && arg_ty
                    .is_refined_string(AccessoryKind::UppercaseString)
                    .yes())
        {
            accessories.push(AccessoryKind::UppercaseString);
        }
        if arg_ty.is_refined_string(AccessoryKind::NumericString).yes() {
            accessories.push(AccessoryKind::NumericString);
        } else if arg_ty
            .is_refined_string(AccessoryKind::NonFalsyString)
            .yes()
        {
            accessories.push(AccessoryKind::NonFalsyString);
        } else if arg_ty
            .is_refined_string(AccessoryKind::NonEmptyString)
            .yes()
        {
            accessories.push(AccessoryKind::NonEmptyString);
        }
        // Digits have no case, so whether the string is a decimal int stays.
        for kind in [
            AccessoryKind::DecimalIntegerString,
            AccessoryKind::NonDecimalIntegerString,
        ] {
            if arg_ty.is_refined_string(kind).yes() {
                accessories.push(kind);
            }
        }

        if accessories.is_empty() {
            Type::string()
        } else {
            let mut members: Vec<Type> = vec![Type::string()];
            members.extend(accessories.into_iter().map(Type::accessory));
            Type::intersection_of(members)
        }
    }
}

/// A string case operation: what `strtolower`/`mb_strtolower` and
/// friends, and each `mb_convert_case` mode, do.
#[derive(Clone, Copy, PartialEq)]
enum CaseOp {
    Lower,
    Upper,
    LowerFirst,
    UpperFirst,
    Title,
    Fold,
    /// A `*_SIMPLE` mode: one character to one.
    Simple(SimpleCaseOp),
}

#[derive(Clone, Copy, PartialEq)]
enum SimpleCaseOp {
    Lower,
    Upper,
    Title,
    Fold,
}

impl CaseOp {
    /// `mb_convert_case`'s `MB_CASE_*` mode.
    fn from_mb_mode(mode: i64) -> Option<CaseOp> {
        Some(match mode {
            0 => CaseOp::Upper,
            1 => CaseOp::Lower,
            2 => CaseOp::Title,
            3 => CaseOp::Fold,
            4 => CaseOp::Simple(SimpleCaseOp::Upper),
            5 => CaseOp::Simple(SimpleCaseOp::Lower),
            6 => CaseOp::Simple(SimpleCaseOp::Title),
            7 => CaseOp::Simple(SimpleCaseOp::Fold),
            _ => return None,
        })
    }

    /// Leaves no ASCII uppercase letter (the `lowercase-string` sense).
    fn makes_lowercase(&self) -> bool {
        matches!(
            self,
            CaseOp::Lower | CaseOp::Fold | CaseOp::Simple(SimpleCaseOp::Lower | SimpleCaseOp::Fold)
        )
    }

    fn keeps_lowercase(&self) -> bool {
        self.makes_lowercase() || *self == CaseOp::LowerFirst
    }

    fn makes_uppercase(&self) -> bool {
        matches!(self, CaseOp::Upper | CaseOp::Simple(SimpleCaseOp::Upper))
    }

    fn keeps_uppercase(&self) -> bool {
        self.makes_uppercase() || *self == CaseOp::UpperFirst
    }

    /// `s` after the operation, `None` where it isn't certain: a
    /// multibyte fold of a string that isn't UTF-8 (PHP substitutes `?`),
    /// or of characters whose titlecase, case folding or title-mode word
    /// boundary Rust's Unicode data doesn't give.
    fn fold(self, s: &PhpString, multibyte: bool) -> Option<PhpString> {
        if !multibyte {
            let mut bytes = s.as_bytes().to_vec();
            match self {
                CaseOp::Lower => bytes.make_ascii_lowercase(),
                CaseOp::Upper => bytes.make_ascii_uppercase(),
                CaseOp::LowerFirst => {
                    if let Some(first) = bytes.first_mut() {
                        first.make_ascii_lowercase();
                    }
                }
                CaseOp::UpperFirst => {
                    if let Some(first) = bytes.first_mut() {
                        first.make_ascii_uppercase();
                    }
                }
                _ => return None,
            }
            return Some(PhpString::from(bytes));
        }
        let text = s.as_str()?;
        let first_then = |first: Option<String>, rest: &str| -> Option<PhpString> {
            Some(PhpString::from(format!("{}{rest}", first?)))
        };
        let simple = |c: char, map: fn(char) -> String| -> Option<char> {
            let mapped = map(c);
            let mut mapped = mapped.chars();
            match (mapped.next(), mapped.next()) {
                (Some(one), None) => Some(one),
                _ => None,
            }
        };
        match self {
            CaseOp::Lower => Some(PhpString::from(text.to_lowercase())),
            CaseOp::Upper => Some(PhpString::from(text.to_uppercase())),
            CaseOp::LowerFirst | CaseOp::UpperFirst => {
                let mut chars = text.chars();
                let Some(first) = chars.next() else {
                    return Some(s.clone());
                };
                let mapped = if self == CaseOp::LowerFirst {
                    Some(first.to_lowercase().collect())
                } else {
                    titlecase(first)
                };
                first_then(mapped, chars.as_str())
            }
            CaseOp::Title => title_case(text, |c| c.to_lowercase().collect::<String>(), titlecase),
            CaseOp::Fold | CaseOp::Simple(SimpleCaseOp::Fold) => text
                .is_ascii()
                .then(|| PhpString::from(text.to_ascii_lowercase())),
            CaseOp::Simple(SimpleCaseOp::Lower) => text
                .chars()
                .map(|c| simple(c, |c| c.to_lowercase().collect()))
                .collect::<Option<String>>()
                .map(PhpString::from),
            CaseOp::Simple(SimpleCaseOp::Upper) => text
                .chars()
                .map(|c| simple(c, |c| c.to_uppercase().collect()))
                .collect::<Option<String>>()
                .map(PhpString::from),
            CaseOp::Simple(SimpleCaseOp::Title) => title_case(
                text,
                |c| {
                    let mut lower = c.to_lowercase();
                    match (lower.next(), lower.next()) {
                        (Some(one), None) => one.to_string(),
                        _ => c.to_string(),
                    }
                },
                |c| titlecase(c).filter(|t| t.chars().count() == 1),
            ),
        }
    }
}

/// A character's titlecase: the Unicode digraphs have their own
/// (`ǆ` is `ǅ`); any other with a one-character uppercase has that.
/// `None` for one whose titlecase isn't its uppercase and isn't known
/// here (`ß` is `Ss`, `ŉ` is `ʼN`).
fn titlecase(c: char) -> Option<String> {
    let digraph = match c {
        '\u{01C4}'..='\u{01C6}' => Some('\u{01C5}'),
        '\u{01C7}'..='\u{01C9}' => Some('\u{01C8}'),
        '\u{01CA}'..='\u{01CC}' => Some('\u{01CB}'),
        '\u{01F1}'..='\u{01F3}' => Some('\u{01F2}'),
        _ => None,
    };
    if let Some(t) = digraph {
        return Some(t.to_string());
    }
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(one), None) => Some(one.to_string()),
        _ => None,
    }
}

/// `MB_CASE_TITLE`: a cased letter is titlecased where a word starts and
/// lowercased elsewhere; a case-ignorable character (`'`, `.`, `:`, `^`,
/// `` ` ``) keeps the word going, any other character ends it (`3rd` is
/// `3Rd`, `o'neil` is `O'neil`). `None` when an uncased character isn't
/// ASCII, whose case-ignorability isn't known here.
fn title_case(
    text: &str,
    lower: impl Fn(char) -> String,
    title: impl Fn(char) -> Option<String>,
) -> Option<PhpString> {
    let mut out = String::with_capacity(text.len());
    let mut in_word = false;
    for c in text.chars() {
        // The titlecase digraphs are cased but neither lower nor upper.
        let cased = c.is_lowercase()
            || c.is_uppercase()
            || matches!(c, '\u{01C5}' | '\u{01C8}' | '\u{01CB}' | '\u{01F2}');
        if cased {
            out.push_str(&if in_word { lower(c) } else { title(c)? });
            in_word = true;
        } else if matches!(c, '\'' | '.' | ':' | '^' | '`') {
            out.push(c);
        } else if c.is_ascii() {
            out.push(c);
            in_word = false;
        } else {
            return None;
        }
    }
    Some(PhpString::from(out))
}

fn is_var_named(e: &ExprNode, name: &str) -> bool {
    matches!(&e.kind, ExprKind::Variable(n) if **n == *name)
}

/// `compact()`'s own name-list resolution for one argument - a bare
/// string literal is one name; a literal array (real PHP's own nested-
/// array-of-names form, accepted since 7.3) recurses into every item.
/// `None` the moment anything isn't one of those two shapes (a variable
/// holding a dynamically-built name, a spread item inside the nested
/// array, ...) - the caller can't partially trust the rest of the list
/// either, since real `compact()` combines every argument into one flat
/// result.
fn compact_argument_names(e: &ExprNode) -> Option<Vec<String>> {
    syntax::stack::guard(|| {
        if let Some(name) = literal_string_value(e) {
            return Some(vec![name.into_owned()]);
        }
        if let ExprKind::ArrayLiteral(items) = &e.kind {
            let mut names = Vec::new();
            for item in items {
                if item.spread {
                    return None;
                }
                names.extend(compact_argument_names(&item.value)?);
            }
            return Some(names);
        }
        None
    })
}

/// Marks `shape` unsealed (other, unknown entries may exist), unless it
/// already is.
fn unseal(shape: &mut Shape) {
    if shape.is_sealed() {
        *shape = Shape::unsealed(std::mem::replace(shape, Shape::sealed(Vec::new())).into_fields());
    }
}

/// `shape` unsealed with other entries of `key` and `value` types, joined
/// with the ones it may already have.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn unseal_with(shape: &mut Shape, key: &Type, value: &Type) {
    shape.unsealed = Some(Box::new(match shape.unsealed.take() {
        Some(tail) => (
            Type::union(tail.0, key.clone()),
            Type::union(tail.1, value.clone()),
        ),
        None => (key.clone(), value.clone()),
    }));
}

fn is_null_literal(e: &ExprNode) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Null))
}

/// Recognizes exactly the callback shape real PHPStan's `array_filter`
/// extension does for an explicit null check: `fn ($v) => $v !== null`
/// (or `null !== $v`), as an arrow function or a closure whose only
/// statement is the equivalent `return`. Deliberately narrow, not a
/// general boolean-expression evaluator.
fn is_null_filter_callback(e: &ExprNode) -> bool {
    fn is_not_null_check(body: &ExprNode, param: &str) -> bool {
        matches!(&body.kind, ExprKind::Binary(BinOp::NotIdentical, l, r)
            if (is_var_named(l, param) && is_null_literal(r))
                || (is_var_named(r, param) && is_null_literal(l)))
    }
    match &e.kind {
        ExprKind::ArrowFn(arrow) => {
            matches!(arrow.params.as_slice(), [p] if is_not_null_check(&arrow.body, &p.name))
        }
        ExprKind::Closure(closure) => match (closure.params.as_slice(), closure.body.as_slice()) {
            ([p], [stmt]) => {
                matches!(&stmt.kind, StmtKind::Return(Some(ret_expr)) if is_not_null_check(ret_expr, &p.name))
            }
            _ => false,
        },
        _ => false,
    }
}

/// `array_filter`'s explicit-null-filter callback (recognized by
/// `is_null_filter_callback` above) strips `Null` from the retained
/// element type, not just the accessory markers every `array_filter`
/// call already loses (`strip_accessory_markers`).
fn strip_null_from_array_element(ty: Type) -> Type {
    ty.filter_array_values(&|elem| elem.without(&Type::null()))
}

/// Recognizes real PHPStan's other `array_filter` callback shape:
/// `fn ($v): bool => $v instanceof Target` (or the equivalent
/// single-statement closure) - returns `Target`'s raw name string, for
/// the caller to resolve.
fn is_instanceof_filter_callback(e: &ExprNode) -> Option<&QualifiedName> {
    fn as_instanceof_class<'a>(body: &'a ExprNode, param: &str) -> Option<&'a QualifiedName> {
        match &body.kind {
            ExprKind::Instanceof(inner, InstanceofTarget::Name(class_name))
                if is_var_named(inner, param) =>
            {
                Some(class_name)
            }
            _ => None,
        }
    }
    match &e.kind {
        ExprKind::ArrowFn(arrow) => match arrow.params.as_slice() {
            [p] => as_instanceof_class(&arrow.body, &p.name),
            _ => None,
        },
        ExprKind::Closure(closure) => match (closure.params.as_slice(), closure.body.as_slice()) {
            ([p], [stmt]) => match &stmt.kind {
                StmtKind::Return(Some(ret_expr)) => as_instanceof_class(ret_expr, &p.name),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// A native `is_string`/`is_int`/`is_float`/`is_bool`/`is_null`
/// type-check predicate passed as `array_filter`'s own callback (e.g.
/// `array_filter($values, 'is_string')`) narrows the retained element
/// type to exactly that type, the same way `is_instanceof_filter_callback`
/// above narrows to a class. Deliberately scoped to the handful of PHP's
/// `is_*` predicates
/// with an exact, unambiguous 1:1 `Type` mapping already in this project
/// (`is_array`/`is_object`/`is_callable`/`is_numeric`/`is_scalar`/
/// `is_iterable` all narrow to something wider than a single `Type`
/// variant, not attempted here). Recognizes the first-class-callable form
/// (`is_string(...)`) alongside the same arrow-fn/closure wrapping shapes
/// `is_instanceof_filter_callback` handles, for consistency.
fn is_type_check_filter_callback(e: &ExprNode) -> Option<Type> {
    fn type_for_predicate(name: &str) -> Option<Type> {
        let name = name.trim_start_matches('\\').to_ascii_lowercase();
        match name.as_str() {
            "is_null" => Some(Type::null()),
            "is_real" => Some(Type::float()),
            _ => narrowing_type_for_predicate(&name),
        }
    }
    fn as_predicate_call<'a>(body: &'a ExprNode, param: &str) -> Option<&'a str> {
        match &body.kind {
            ExprKind::Call(callee, args) => match (&callee.kind, args.as_slice()) {
                (ExprKind::Name(name), [single_arg]) if is_var_named(&single_arg.value, param) => {
                    Some(name.written.as_ref())
                }
                _ => None,
            },
            _ => None,
        }
    }
    match &e.kind {
        // `'is_int'` - a callable string naming the predicate.
        ExprKind::Literal(Literal::String(name)) => type_for_predicate(name.as_str()?),
        ExprKind::FirstClassCallable(inner) => match &inner.kind {
            ExprKind::Call(callee, args) if args.is_empty() => match &callee.kind {
                ExprKind::Name(name) => type_for_predicate(name),
                _ => None,
            },
            _ => None,
        },
        ExprKind::ArrowFn(arrow) => match arrow.params.as_slice() {
            [p] => as_predicate_call(&arrow.body, &p.name).and_then(type_for_predicate),
            _ => None,
        },
        ExprKind::Closure(closure) => match (closure.params.as_slice(), closure.body.as_slice()) {
            ([p], [stmt]) => match &stmt.kind {
                StmtKind::Return(Some(ret_expr)) => {
                    as_predicate_call(ret_expr, &p.name).and_then(type_for_predicate)
                }
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// The shared core of `filter_var`'s/`filter_input`'s return-type
/// inference - everything computed before the final union with the
/// "validation failed" fallback, so each caller can apply the union/
/// array-wrap logic appropriate to its own shape without duplicating the
/// option-parsing. Returns `None` when `filter_name` isn't a literal
/// `FILTER_VALIDATE_*` constant this project models (a computed filter
/// value, a sanitize filter, `FILTER_DEFAULT` - real PHPStan's logic for
/// the latter two depends on the input value's own type, not just the
/// filter constant).
///
/// `(element_ty, fallback, throw_on_failure)`: `element_ty` is the
/// successfully-validated value's own type (already `min_range`/
/// `max_range`-narrowed for `FILTER_VALIDATE_INT` when a literal options
/// array supplies them); `fallback` is what a failed validation produces
/// (`FILTER_NULL_ON_FAILURE`'s own detection, both via a bare bitwise-OR
/// chain and via a literal `['flags' => ...]` options-array key -
/// `contains_flag_name_with_options` handles both - or a literal
/// `'default'` option, which wins over the implicit `null`/`false` when
/// present); `throw_on_failure` is `FILTER_THROW_ON_FAILURE`'s own
/// detection (PHP 8.5), for the caller to strip the fallback with
/// instead of unioning it in.
fn filter_validate_result(
    filter_name: &str,
    flags_expr: Option<&ExprNode>,
    flags_ty: Option<&Type>,
) -> Option<(Type, Type, bool)> {
    let base_ty = filter_validate_type_for_constant(filter_name)?;
    let null_on_failure =
        flags_expr.is_some_and(|e| contains_flag_name_with_options(e, "FILTER_NULL_ON_FAILURE"));
    let throw_on_failure =
        flags_expr.is_some_and(|e| contains_flag_name_with_options(e, "FILTER_THROW_ON_FAILURE"));
    let implicit_fallback = if null_on_failure {
        Type::null()
    } else {
        Type::bool_literal(false)
    };
    let flags_is_options_array = flags_ty.is_some_and(|t| t.is_array().yes());
    if !flags_is_options_array {
        return Some((base_ty, implicit_fallback, throw_on_failure));
    }
    // A literal `['options' => ...]` shape whose 'options' value isn't
    // itself a literal array is genuinely un-analyzable - callers want a
    // "too complicated, give up on the whole call" signal (`None`), not
    // `Some((base_ty, ...))`. Distinct from no 'options' key being
    // written at all (the ordinary flags-only call), which correctly
    // keeps using the plain per-filter base type below.
    let flags_ty_ref = flags_ty.expect("flags_is_options_array implies Some");
    let fields = filter_var_options_fields(flags_ty_ref);
    if fields.is_none()
        && let Some(flags_shape) = flags_ty_ref.as_array_shape()
        && flags_shape.field(&ShapeKey::from_name("options")).is_some()
    {
        return None;
    }
    let Some(fields) = fields else {
        return Some((base_ty, implicit_fallback, throw_on_failure));
    };
    let fallback = filter_var_option(fields, "default").unwrap_or(implicit_fallback);
    // Only `FILTER_VALIDATE_INT` gets `min_range`/`max_range` treatment -
    // `FILTER_VALIDATE_FLOAT` is excluded too, matching real PHPStan
    // ("PHPStan does not yet support FloatRangeType").
    let element_ty = if filter_name == "FILTER_VALIDATE_INT" {
        let min = filter_var_option(fields, "min_range")
            .and_then(|t| t.int_bounds())
            .and_then(|(min, _)| min);
        let max = filter_var_option(fields, "max_range")
            .and_then(|t| t.int_bounds())
            .and_then(|(_, max)| max);
        if min.is_some() || max.is_some() {
            Type::int_range(min, max)
        } else {
            base_ty
        }
    } else {
        base_ty
    };
    Some((element_ty, fallback, throw_on_failure))
}

fn filter_var_options_fields(flags_ty: &Type) -> Option<&Shape> {
    flags_ty
        .as_array_shape()?
        .field(&ShapeKey::from_name("options"))?
        .ty
        .as_array_shape()
}

/// One named option's value type (`'default'`/`'min_range'`/`'max_range'`)
/// out of an already-extracted `filter_var_options_fields` result.
fn filter_var_option(options: &Shape, name: &str) -> Option<Type> {
    options
        .field(&ShapeKey::from_name(name))
        .map(|f| f.ty.clone())
}

fn filter_validate_type_for_constant(name: &str) -> Option<Type> {
    match name {
        "FILTER_VALIDATE_BOOLEAN" | "FILTER_VALIDATE_BOOL" => Some(Type::bool()),
        "FILTER_VALIDATE_INT" => Some(Type::int()),
        "FILTER_VALIDATE_FLOAT" => Some(Type::float()),
        "FILTER_VALIDATE_DOMAIN" | "FILTER_VALIDATE_REGEXP" => Some(Type::string()),
        // These can never come back as the empty string on success (a
        // validated IP/email/MAC address is never `''`), so it's
        // `non-falsy-string`, not plain `string`.
        "FILTER_VALIDATE_IP"
        | "FILTER_VALIDATE_EMAIL"
        | "FILTER_VALIDATE_MAC"
        | "FILTER_VALIDATE_URL" => Some(Type::intersection_of(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonFalsyString),
        ])),
        _ => None,
    }
}

/// Whether `target` (a bare `PREG_*`-style constant name) appears anywhere
/// in `e`'s `BitOr`/`Grouping` chain - a lenient scan, unlike
/// `parse_preg_flags`'s stricter "every leaf must be a recognized name"
/// requirement: an unrecognized flag alongside `target` doesn't change
/// what this is used for, so it just answers "is `target` definitely one
/// of them" for whatever it does recognize.
pub(crate) fn contains_flag_name(e: &ExprNode, target: &str) -> bool {
    let e = e.peeled();
    syntax::stack::guard(|| match &e.kind {
        ExprKind::Name(name) => name.trim_start_matches('\\') == target,
        ExprKind::Binary(BinOp::BitOr, l, r) => {
            contains_flag_name(l, target) || contains_flag_name(r, target)
        }
        _ => false,
    })
}

/// `contains_flag_name`, additionally looking inside an options-array
/// literal's `'flags'` key (`filter_var($x, F, ['flags' => FILTER_X,
/// 'options' => [...]])`) - the spelling real PHPStan's PHP 8.5 test
/// fixture uses for `FILTER_THROW_ON_FAILURE`. Not folded into
/// `contains_flag_name` itself, which stays a narrow bitwise-or-chain
/// scan for its unrelated call sites.
fn contains_flag_name_with_options(e: &ExprNode, target: &str) -> bool {
    syntax::stack::guard(|| {
        if contains_flag_name(e, target) {
            return true;
        }
        if let ExprKind::ArrayLiteral(items) = &e.kind {
            for item in items {
                if item
                    .key
                    .as_ref()
                    .is_some_and(|k| literal_string_value(k).as_deref() == Some("flags"))
                    && contains_flag_name_with_options(&item.value, target)
                {
                    return true;
                }
            }
        }
        false
    })
}

/// `array_filter`'s `instanceof`-filter callback (recognized by
/// `is_instanceof_filter_callback` above) narrows the retained element
/// type to `target` specifically.
fn narrow_array_element_type_to(relation: &Relation, ty: Type, target: &Type) -> Type {
    ty.filter_array_values(&|elem| {
        if relation.disjoint(elem, target) {
            Type::never()
        } else {
            relation.narrow(elem, target)
        }
    })
}

/// Whether `e` is (or bitwise-ORs together, to any depth) a `Name`
/// resolving to `JSON_THROW_ON_ERROR` - real code commonly writes either
/// a bare `JSON_THROW_ON_ERROR` or a multi-flag chain
/// (`JSON_HEX_TAG | JSON_HEX_AMP | ... | JSON_THROW_ON_ERROR`). Tolerates
/// a leading-backslash global name (`\JSON_THROW_ON_ERROR`) too.
fn contains_json_throw_on_error(e: &ExprNode) -> bool {
    let e = e.peeled();
    syntax::stack::guard(|| match &e.kind {
        ExprKind::Name(name) => name.trim_start_matches('\\') == "JSON_THROW_ON_ERROR",
        ExprKind::Binary(BinOp::BitOr, l, r) => {
            contains_json_throw_on_error(l) || contains_json_throw_on_error(r)
        }
        _ => false,
    })
}

/// `curl_getinfo($ch, CONSTANT)`'s per-option return type, transcribed
/// from real PHPStan's `CurlGetinfoFunctionDynamicReturnTypeExtension`
/// table, including its RTSP/proxy/microsecond/bignum/`_T`-suffixed
/// variants. `None` falls through to the generic `mixed` behavior.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn curl_getinfo_component_type(name: &str) -> Option<Type> {
    let string_false = Type::union_of(vec![Type::string(), Type::bool_literal(false)]);
    let int_false = Type::union_of(vec![Type::int(), Type::bool_literal(false)]);
    // `list<string>` - `CURLINFO_SSL_ENGINES`/`CURLINFO_COOKIELIST`'s own
    // real type.
    let string_list = Type::intersection_of(vec![
        Type::array(Type::unknown_array_key(), Type::string()),
        Type::accessory(AccessoryKind::ArrayList),
    ]);
    // `list<array<string, string>>` - `CURLINFO_CERTINFO`'s own real type.
    let nested_array_in_list = Type::intersection_of(vec![
        Type::array(
            Type::unknown_array_key(),
            Type::array(Type::unknown_array_key(), Type::string()),
        ),
        Type::accessory(AccessoryKind::ArrayList),
    ]);
    Some(match name {
        "CURLINFO_EFFECTIVE_URL"
        | "CURLINFO_PRIMARY_IP"
        | "CURLINFO_LOCAL_IP"
        | "CURLINFO_SCHEME"
        | "CURLINFO_EFFECTIVE_METHOD" => Type::string(),
        "CURLINFO_REDIRECT_URL"
        | "CURLINFO_HEADER_OUT"
        | "CURLINFO_CONTENT_TYPE"
        | "CURLINFO_FTP_ENTRY_PATH"
        | "CURLINFO_RTSP_SESSION_ID"
        | "CURLINFO_REFERER"
        | "CURLINFO_CAINFO"
        | "CURLINFO_CAPATH" => string_false,
        "CURLINFO_TOTAL_TIME"
        | "CURLINFO_NAMELOOKUP_TIME"
        | "CURLINFO_CONNECT_TIME"
        | "CURLINFO_PRETRANSFER_TIME"
        | "CURLINFO_STARTTRANSFER_TIME"
        | "CURLINFO_REDIRECT_TIME"
        | "CURLINFO_SIZE_UPLOAD"
        | "CURLINFO_SIZE_DOWNLOAD"
        | "CURLINFO_SPEED_DOWNLOAD"
        | "CURLINFO_SPEED_UPLOAD"
        | "CURLINFO_CONTENT_LENGTH_DOWNLOAD"
        | "CURLINFO_CONTENT_LENGTH_UPLOAD"
        | "CURLINFO_APPCONNECT_TIME" => Type::float(),
        "CURLINFO_FILETIME"
        | "CURLINFO_REDIRECT_COUNT"
        | "CURLINFO_PRIMARY_PORT"
        | "CURLINFO_LOCAL_PORT"
        | "CURLINFO_HEADER_SIZE"
        | "CURLINFO_REQUEST_SIZE"
        | "CURLINFO_SSL_VERIFYRESULT"
        | "CURLINFO_RESPONSE_CODE"
        | "CURLINFO_HTTP_CODE"
        | "CURLINFO_HTTP_CONNECTCODE"
        | "CURLINFO_OS_ERRNO"
        | "CURLINFO_NUM_CONNECTS"
        | "CURLINFO_PROTOCOL"
        | "CURLINFO_HTTP_VERSION"
        | "CURLINFO_HTTPAUTH_AVAIL"
        | "CURLINFO_PROXYAUTH_AVAIL"
        | "CURLINFO_CONDITION_UNMET"
        | "CURLINFO_RTSP_CLIENT_CSEQ"
        | "CURLINFO_RTSP_CSEQ_RECV"
        | "CURLINFO_RTSP_SERVER_CSEQ"
        | "CURLINFO_PROXY_SSL_VERIFYRESULT"
        | "CURLINFO_CONTENT_LENGTH_DOWNLOAD_T"
        | "CURLINFO_CONTENT_LENGTH_UPLOAD_T"
        | "CURLINFO_SIZE_DOWNLOAD_T"
        | "CURLINFO_SIZE_UPLOAD_T"
        | "CURLINFO_SPEED_DOWNLOAD_T"
        | "CURLINFO_SPEED_UPLOAD_T"
        | "CURLINFO_APPCONNECT_TIME_T"
        | "CURLINFO_CONNECT_TIME_T"
        | "CURLINFO_FILETIME_T"
        | "CURLINFO_NAMELOOKUP_TIME_T"
        | "CURLINFO_PRETRANSFER_TIME_T"
        | "CURLINFO_REDIRECT_TIME_T"
        | "CURLINFO_STARTTRANSFER_TIME_T"
        | "CURLINFO_TOTAL_TIME_T"
        | "CURLINFO_PROXY_ERROR"
        | "CURLINFO_RETRY_AFTER" => Type::int(),
        "CURLINFO_SSL_ENGINES" | "CURLINFO_COOKIELIST" => string_list,
        "CURLINFO_CERTINFO" => nested_array_in_list,
        "CURLINFO_POSTTRANSFER_TIME_T"
        | "CURLINFO_QUEUE_TIME_T"
        | "CURLINFO_USED_PROXY"
        | "CURLINFO_HTTPAUTH_USED"
        | "CURLINFO_PROXYAUTH_USED"
        | "CURLINFO_CONN_ID" => int_false,
        "CURLINFO_PRIVATE" => Type::mixed(),
        _ => return None,
    })
}

/// `get_debug_type()`'s object case - a free function, not a `Checker`
/// method, so `Checker::get_debug_type_result`'s union handling can call
/// it once per member without fighting the borrow checker over `&self`.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn get_debug_type_for(ty: &Type, registry: &Registry) -> Option<Type> {
    let named = |name: &str| Type::string_literal(name.into());
    let mut result: Option<Type> = None;
    for member in ty.union_members() {
        let answer = if member.is_null().yes() {
            named("null")
        } else if member.is_bool().yes() {
            named("bool")
        } else if member.is_int().yes() {
            named("int")
        } else if member.is_float().yes() {
            named("float")
        } else if member.is_string().yes() {
            named("string")
        } else if member.is_array().yes() {
            named("array")
        } else if member.is_object().yes()
            && let Some(class) = member.single_object_class()
        {
            // An enum is final by definition; another class's own name
            // is the answer only when no subclass can stand in for it.
            let exact = member.is_enum_case()
                || registry
                    .class_by_name(class.as_str())
                    .is_some_and(|info| info.is_final());
            if exact {
                named(class.as_str())
            } else {
                Type::string()
            }
        } else {
            return None;
        };
        result = Some(match result {
            Some(acc) => Type::union(acc, answer),
            None => answer,
        });
    }
    result
}

/// `gettype()`'s per-kind runtime-string answer (real PHP's historically
/// quirky names: `"boolean"`, `"integer"`, `"double"`, `"NULL"` - not
/// `"bool"`/`"int"`/`"float"`/`"null"`, which is what `get_debug_type()`
/// returns instead). `None` for anything this project can't confidently
/// classify, leaving plain `string` unchanged instead of guessing.
fn gettype_string_for(ty: &Type) -> Option<&'static str> {
    if ty.is_string().yes() {
        Some("string")
    } else if ty.is_array().yes() {
        Some("array")
    } else if ty.is_bool().yes() {
        Some("boolean")
    } else if ty.is_int().yes() {
        Some("integer")
    } else if ty.is_float().yes() {
        Some("double")
    } else if ty.is_null().yes() {
        Some("NULL")
    } else if ty.is_object().yes() {
        Some("object")
    } else if ty.is_resource(false).yes() {
        Some("resource")
    } else if ty.is_resource(true).yes() {
        Some("resource (closed)")
    } else {
        None
    }
}

// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn gettype_result(ty: &Type) -> Option<Type> {
    ty.union_members()
        .iter()
        .map(|m| gettype_string_for(m).map(|s| Type::string_literal(s.into())))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .union_all()
}

/// Whether `s` is exactly one Unicode scalar value - `idate()`'s format
/// argument is only ever meaningfully a single character (real PHP only
/// reads the first byte).
fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(c)
}

/// `idate()`'s per-character numeric bound, transcribed from real
/// PHPStan's `IdateFunctionReturnTypeHelper`, using integer range in
/// place of `IntegerRangeType::fromInterval`. A handful of real `idate()`
/// format characters (`B`/`o`/`U`/`Y`/`Z`) are unbounded integers -
/// deliberately left out (returning `None`) rather than mapped to plain
/// `Int`, since that would narrow away the stub's `false` half for a
/// character this project can't back that claim for.
fn idate_format_char_range(c: char) -> Option<(i64, i64)> {
    match c {
        'd' | 'j' => Some((1, 31)),
        'h' | 'g' => Some((1, 12)),
        'H' | 'G' => Some((0, 23)),
        'i' | 's' => Some((0, 59)),
        'I' | 'L' => Some((0, 1)),
        'm' | 'n' => Some((1, 12)),
        'N' => Some((1, 7)),
        't' => Some((28, 31)),
        'w' => Some((0, 6)),
        'W' => Some((1, 53)),
        'y' => Some((0, 99)),
        'z' => Some((0, 365)),
        _ => None,
    }
}

/// `date()`'s per-character static classification: does this recognized
/// format character always produce digits-only text (`Some(true)`),
/// always produce text that isn't guaranteed numeric (`Some(false)`,
/// month/day names, `am`/`pm`, timezone abbreviations, the ordinal
/// suffix, ...), or is it not a character this table classifies at all
/// (`None` - an escape sequence or literal punctuation `date()` would
/// echo back verbatim, unevaluable without actually running `date()`)?
/// Transcribed from real PHPStan's `DateFunctionReturnTypeHelper`,
/// collapsed from "the exact literal/numeric-string type" down to this
/// coarser yes/no (a static table, not a runtime `date()` simulation).
pub(crate) fn date_format_char_is_always_numeric(c: char) -> Option<bool> {
    match c {
        // Digits-only output: day/week/month/year numbers, hour/minute/
        // second/sub-second, the leap-year and DST 0/1 flags, Swatch
        // Internet time (`B`, `000`-`999`), the signless-colon GMT
        // offset (`O`, e.g. `+0200` - PHP's own `is_numeric()` accepts a
        // leading `+`/`-` before the digits), the GMT offset in seconds
        // (`Z`, e.g. `-18000`), and the Unix timestamp (`U`) - all real,
        // digits-(optionally-signed-)only text.
        'd' | 'j' | 'N' | 'w' | 'm' | 'n' | 't' | 'L' | 'g' | 'G' | 'h' | 'H' | 'i' | 's' | 'y'
        | 'Y' | 'z' | 'W' | 'u' | 'v' | 'U' | 'B' | 'I' | 'O' | 'Z' | 'o' => Some(true),
        // Real letter/punctuation-bearing output: weekday/month names,
        // the `st`/`nd`/`rd`/`th` ordinal suffix, `am`/`pm`, timezone
        // identifiers/abbreviations, the colon-bearing GMT offset (`P`/
        // `p`, e.g. `+02:00` - the colon makes `is_numeric()` false), and
        // the full ISO-8601/RFC-2822 combined formats.
        'D' | 'l' | 'S' | 'F' | 'M' | 'a' | 'A' | 'e' | 'T' | 'P' | 'p' | 'c' | 'r' => Some(false),
        _ => None,
    }
}

/// `date($format)`'s return type for a literal `$format` - `None` (plain
/// `string`, unchanged) unless every character is one
/// `date_format_char_is_always_numeric` recognizes; a non-empty literal
/// format then always produces non-empty text (`NonEmptyString`),
/// additionally numeric text (`NumericString`) only when every character
/// does.
pub(crate) fn date_format_return_type(format: &str) -> Option<Type> {
    if format.is_empty() {
        // Real `date('')` returns `''`, already inferred precisely via
        // the generic stub path - not worth a special case.
        return None;
    }
    let mut all_numeric = true;
    for c in format.chars() {
        match date_format_char_is_always_numeric(c)? {
            true => {}
            false => all_numeric = false,
        }
    }
    let mut members = vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonEmptyString),
    ];
    if all_numeric {
        members.push(Type::accessory(AccessoryKind::NumericString));
    }
    Some(Type::intersection_of(members))
}

/// `DateInterval::format($format)`'s per-specifier substitution value for
/// a representative zero-valued interval (`new DateInterval('P0D')`),
/// used only to derive structural accessory facts about the real output
/// (never returned as a literal itself, since the real interval's
/// magnitude is unknown). Transcribed from PHP's documented
/// `DateInterval::format()` specifier table. `%a` (total days) uses the
/// conservative `"(unknown)"` representative, since this project doesn't
/// track a real `int` `->days` fact per-instance. `%R`/`%r` (sign) use
/// the positive/non-inverted representative (`"+"`/`""`).
fn date_interval_format_char_value(c: char) -> Option<&'static str> {
    match c {
        'Y' => Some("0000"),
        'y' => Some("0"),
        'M' => Some("00"),
        'm' => Some("0"),
        'D' => Some("00"),
        'd' => Some("0"),
        'H' => Some("00"),
        'h' => Some("0"),
        'I' => Some("00"),
        'i' => Some("0"),
        'S' => Some("00"),
        's' => Some("0"),
        'F' => Some("000000"),
        'f' => Some("0"),
        'R' => Some("+"),
        'r' => Some(""),
        'a' => Some("(unknown)"),
        '%' => Some("%"),
        _ => None,
    }
}

/// Substitutes every `%`-specifier in `format` with its own representative
/// value (`date_interval_format_char_value`), passing every other
/// character through verbatim (`DateInterval::format()`'s own literal-text
/// semantics, unlike `date()`'s own all-characters-are-specifiers
/// grammar) - `None` ("don't guess") on a truncated trailing `%` or an
/// unrecognized specifier character.
fn date_interval_format_literal(format: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let spec = chars.next()?;
        out.push_str(date_interval_format_char_value(spec)?);
    }
    Some(out)
}

/// `DateInterval::format($format)`'s return type for a literal `$format` -
/// `None` (plain `string`, unchanged) on anything
/// `date_interval_format_literal` doesn't confidently substitute. Because
/// every zero-width-varying specifier real PHP zero-pads to a fixed width
/// (`%Y`/`%M`/.../`%F`) or substitutes a fixed non-numeric literal (`%R`,
/// `%a`), the representative substitution's accessory properties
/// (non-empty, non-falsy, purely numeric) hold for every real interval
/// magnitude, not just the zero one - genuinely sound, not an
/// approximation, for those specifiers. `%r`/bare `%d`/`%m`/.../`%f` can
/// legitimately format to `""`/`"0"` for a real interval, so correctly
/// contribute no extra accessory guarantee there.
pub(crate) fn date_interval_format_return_type(format: &str) -> Option<Type> {
    let value = date_interval_format_literal(format)?;
    let mut members = vec![Type::string()];
    if types::is_numeric_string_literal(&value) {
        members.push(Type::accessory(AccessoryKind::NumericString));
    }
    if !value.is_empty() && value != "0" {
        members.push(Type::accessory(AccessoryKind::NonFalsyString));
    } else if !value.is_empty() {
        members.push(Type::accessory(AccessoryKind::NonEmptyString));
    }
    if members.len() == 1 {
        return None;
    }
    Some(Type::intersection_of(members))
}

/// `pathinfo($path)`'s 1-arg array shape - `basename`/`filename` always
/// present, `extension` only when the path actually has a dot in it
/// (simplified here to always-optional, since this project can't know
/// from `$path`'s `Type` alone whether a literal path contains one).
/// `dirname` mirrors real PHPStan's `isNonEmptyString()`-driven
/// optionality check exactly: unlike `extension`, `path_ty` being
/// provably `non-empty-string` is enough on its own to prove `dirname`
/// present.
fn pathinfo_array_shape(path_ty: &Type) -> Type {
    let dirname_optional = !path_ty
        .is_refined_string(AccessoryKind::NonEmptyString)
        .yes();
    Type::array_shape(Shape::from_named(vec![
        ("dirname".to_string(), Type::string(), dirname_optional),
        ("basename".to_string(), Type::string(), false),
        ("extension".to_string(), Type::string(), true),
        ("filename".to_string(), Type::string(), false),
    ]))
}

/// `parse_url($url)`'s "all components" array shape - every field
/// genuinely optional, `port` alone typed `IntRange(0, 65535)` rather
/// than plain `String` like the rest.
fn parse_url_components_array_shape() -> Type {
    Type::array_shape(Shape::from_named(vec![
        ("scheme".to_string(), Type::string(), true),
        ("host".to_string(), Type::string(), true),
        (
            "port".to_string(),
            Type::int_range(Some(0), Some(65535)),
            true,
        ),
        ("user".to_string(), Type::string(), true),
        ("pass".to_string(), Type::string(), true),
        ("path".to_string(), Type::string(), true),
        ("query".to_string(), Type::string(), true),
        ("fragment".to_string(), Type::string(), true),
    ]))
}

/// `parse_url($url, PHP_URL_*)`'s per-component return type, transcribed
/// from real PHPStan's `ParseUrlFunctionDynamicReturnTypeExtension`
/// table, minus its `AccessoryLowercaseStringType` refinement for a
/// lowercase `$url` (this project has no equivalent accessory kind).
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn parse_url_component_type(name: &str) -> Option<Type> {
    let string_false_null = Type::union_of(vec![
        Type::string(),
        Type::bool_literal(false),
        Type::null(),
    ]);
    let port_false_null = Type::union_of(vec![
        Type::int_range(Some(0), Some(65535)),
        Type::bool_literal(false),
        Type::null(),
    ]);
    Some(match name {
        "PHP_URL_SCHEME" | "PHP_URL_HOST" | "PHP_URL_USER" | "PHP_URL_PASS" | "PHP_URL_PATH"
        | "PHP_URL_QUERY" | "PHP_URL_FRAGMENT" => string_false_null,
        "PHP_URL_PORT" => port_false_null,
        _ => return None,
    })
}

/// One `sscanf()` conversion specifier - either a single `[cdeEfiosux]`
/// character, or a `[...]` bracket character-class (real `sscanf()`
/// treats every bracket class the same as `%s` for typing purposes: it
/// always captures a substring, never a number).
enum SscanfSpecifier {
    Char(char),
    Bracket,
}

/// Strips every real `sprintf()`/`vsprintf()` conversion specifier
/// (`%[argnum$][flags][width][.precision]specifier`) from a literal
/// format string, returning the concatenation of whatever literal text is
/// left (`%%` collapsing to one literal `%`). `None` on the first
/// specifier this can't confidently parse to the end - the point here is
/// proving the remaining literal text is non-falsy, so silently guessing
/// past an unparseable specifier would risk a claim this hasn't verified.
/// A hand-rolled scan since this project has no regex engine available
/// (see `parse_sscanf_format` below).
fn sprintf_literal_remainder(fmt: &str) -> Option<String> {
    Some(
        sprintf_pieces(fmt)?
            .into_iter()
            .filter_map(|piece| match piece {
                FormatPiece::Literal(c) => Some(c),
                FormatPiece::Conversion { .. } => None,
            })
            .collect(),
    )
}

/// A piece of a `sprintf` format: a literal character (`%%` as `%`), or
/// a conversion of the argument at `arg` (0-based), with whether it has a
/// width or a precision.
enum FormatPiece {
    Literal(char),
    Conversion {
        conversion: char,
        arg: usize,
        width: bool,
        precision: bool,
    },
}

/// `fmt`'s pieces; `None` for anything this parser doesn't read.
fn sprintf_pieces(fmt: &str) -> Option<Vec<FormatPiece>> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut next_arg = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(FormatPiece::Literal(chars[i]));
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            return None;
        }
        if chars[i] == '%' {
            out.push(FormatPiece::Literal('%'));
            i += 1;
            continue;
        }
        // Optional positional argument number (`1$`); digits not followed
        // by `$` are the width.
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let arg = if i > start && i < chars.len() && chars[i] == '$' {
            let n: usize = chars[start..i].iter().collect::<String>().parse().ok()?;
            i += 1;
            n.checked_sub(1)?
        } else {
            i = start;
            next_arg += 1;
            next_arg - 1
        };
        // Optional custom padding character (`'x`).
        if i < chars.len() && chars[i] == '\'' && i + 1 < chars.len() {
            i += 2;
        }
        // Optional flags.
        while i < chars.len() && matches!(chars[i], '-' | '+' | ' ' | '0') {
            i += 1;
        }
        let width_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let width = chars[width_start..i].iter().any(|c| *c != '0');
        let mut precision = false;
        if i < chars.len() && chars[i] == '.' {
            precision = true;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
        let conversion = *chars.get(i)?;
        if !matches!(
            conversion,
            'b' | 'c' | 'd' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'o' | 's' | 'u' | 'x' | 'X'
        ) {
            return None;
        }
        i += 1;
        out.push(FormatPiece::Conversion {
            conversion,
            arg,
            width,
            precision,
        });
    }
    Some(out)
}

/// Whether every result of formatting `pieces` holds a byte no
/// decimal-int string has there: a literal that is neither a digit nor a
/// `-`, or a `-` after something that surely writes a byte (a literal, a
/// numeric or `%c` conversion, a padded `%s`, or a `%s` whose argument is
/// surely non-empty, as `non_empty_arg` says).
fn sprintf_is_non_decimal_int(
    pieces: &[FormatPiece],
    non_empty_arg: &dyn Fn(usize) -> bool,
) -> bool {
    let writes = |piece: &FormatPiece| match piece {
        FormatPiece::Literal(_) => true,
        FormatPiece::Conversion {
            conversion: 's',
            arg,
            width,
            precision,
        } => *width || (!*precision && non_empty_arg(*arg)),
        FormatPiece::Conversion { .. } => true,
    };
    pieces.iter().enumerate().any(|(i, piece)| match piece {
        FormatPiece::Literal('-') => pieces[..i].iter().any(writes),
        FormatPiece::Literal(c) => !c.is_ascii_digit(),
        FormatPiece::Conversion { .. } => false,
    })
}

/// Walks a literal `sscanf()`/`fscanf()` format string for every real
/// conversion specifier (`%d`, `%5s`, `%[abc]`, ...) it contains, in
/// order - a hand-rolled scan standing in for real PHPStan's regex
/// (`/%(\d*)(\[[^\]]+\]|[cdeEfosux]{1})/`), since this project has no
/// regex engine available. `None` when the format contains no recognized
/// specifier at all. An unterminated `%[...` bracket at the end of the
/// string stops the scan there, matching the regex's own behavior.
/// Recognizes one specifier real PHPStan's regex doesn't - `%i`
/// (base-auto-detected integer, a real, working sscanf specifier) -
/// since `check_scanf_argument_count` treats `specs.len()` as an
/// authoritative count compared exactly against the number of by-ref
/// arguments given, so silently skipping a real `%i` specifier there
/// would produce an active, wrong "count mismatch" report, not just an
/// imprecise shape.
fn parse_sscanf_format(format: &str) -> Option<Vec<(Option<u32>, SscanfSpecifier)>> {
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;
    let mut specs = Vec::new();
    while i < chars.len() {
        if chars[i] != '%' {
            i += 1;
            continue;
        }
        i += 1;
        let digits_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let length: Option<u32> = if i > digits_start {
            chars[digits_start..i]
                .iter()
                .collect::<String>()
                .parse()
                .ok()
        } else {
            None
        };
        if i >= chars.len() {
            break;
        }
        if chars[i] == '[' {
            let bracket_body_start = i + 1;
            let mut j = bracket_body_start;
            while j < chars.len() && chars[j] != ']' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            specs.push((length, SscanfSpecifier::Bracket));
            i = j + 1;
        } else if "cdeEfiosux".contains(chars[i]) {
            specs.push((length, SscanfSpecifier::Char(chars[i])));
            i += 1;
        }
        // Any other character right after `%`/its digits (including a
        // literal `%%`) isn't a real conversion specifier - just keep
        // scanning from here, same as the real regex simply not matching
        // at this position.
    }
    if specs.is_empty() { None } else { Some(specs) }
}

/// One `sscanf()` field's type, given its specifier and any explicit
/// field-width digits - every field is nullable, since a trailing
/// unmatched specifier leaves that slot `null`.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn sscanf_field_type(length: Option<u32>, specifier: &SscanfSpecifier) -> Type {
    let base = match specifier {
        SscanfSpecifier::Char('d' | 'o' | 'u' | 'x' | 'i') => Type::int(),
        SscanfSpecifier::Char('e' | 'E' | 'f') => Type::float(),
        SscanfSpecifier::Char(_) | SscanfSpecifier::Bracket => match length {
            Some(n) if n > 1 => Type::intersection_of(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NonFalsyString),
            ]),
            Some(_) => Type::intersection_of(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NonEmptyString),
            ]),
            None => Type::string(),
        },
    };
    Type::union_of(vec![base, Type::null()])
}

/// `sscanf($string, $format)`'s numeric-keyed result-array fields, one
/// per conversion specifier `parse_sscanf_format` finds, in order - keyed
/// by each field's canonical string form of its integer index (`"0"`,
/// `"1"`, ...), the same way array shape represents an int-keyed
/// array elsewhere in this project (see `literal_string_key`).
fn sscanf_result_fields(format: &str) -> Option<Shape> {
    let specs = parse_sscanf_format(format)?;
    Some(Shape::sealed(
        specs
            .into_iter()
            .enumerate()
            .map(|(i, (length, specifier))| {
                ShapeField::new(
                    ShapeKey::Int(i as i64),
                    sscanf_field_type(length, &specifier),
                    false,
                )
            })
            .collect(),
    ))
}

/// `hrtime(false)`/`hrtime()`'s 2-element `[seconds, nanoseconds]` list
/// shape. `seconds` is always at least 1 (real PHP's monotonic clock
/// never returns `0` in practice); `nanoseconds` is always a sub-second
/// remainder, `0` to `999_999_999` inclusive.
fn hrtime_array_shape() -> Type {
    Type::array_shape(Shape::from_named(vec![
        ("0".to_string(), Type::int_range(Some(1), None), false),
        (
            "1".to_string(),
            Type::int_range(Some(0), Some(999_999_999)),
            false,
        ),
    ]))
}

/// `hrtime(true)`'s single-number nanosecond-count form.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn hrtime_number_type() -> Type {
    Type::union_of(vec![Type::int_range(Some(1), None), Type::float()])
}

/// `stat()`/`lstat()`/`fstat()`/`ssh2_sftp_stat()`'s shared 13-key shape,
/// every value a real `int`. Real `stat()`'s array is actually
/// double-keyed, each value reachable by both its numeric position and
/// its string name; only the string-keyed half is modeled here, the way
/// real code actually reads it (`$stat['size']`, not `$stat[7]`).
fn stat_array_shape() -> Type {
    Type::array_shape(Shape::from_named(
        [
            "dev", "ino", "mode", "nlink", "uid", "gid", "rdev", "size", "atime", "mtime", "ctime",
            "blksize", "blocks",
        ]
        .into_iter()
        .map(|key| (key, Type::int(), false)),
    ))
}

/// `dio_stat()`'s 13-key shape - the same values-are-all-`int` shape as
/// `stat_array_shape` above, under `dio_stat`'s own distinct key names.
fn dio_stat_array_shape() -> Type {
    Type::array_shape(Shape::from_named(
        [
            "device",
            "inode",
            "mode",
            "nlink",
            "uid",
            "gid",
            "device_type",
            "size",
            "blocksize",
            "blocks",
            "atime",
            "mtime",
            "ctime",
        ]
        .into_iter()
        .map(|key| (key, Type::int(), false)),
    ))
}

/// `list<non-empty-string>` - `hash_algos()`/`openssl_get_cipher_methods()`/
/// `mb_list_encodings()`'s evidenced return shape regardless of host/PHP
/// version/installed extensions.
fn list_of_non_empty_strings() -> Type {
    Type::intersection_of(vec![
        Type::array(
            Type::unknown_array_key(),
            Type::intersection_of(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NonEmptyString),
            ]),
        ),
        Type::accessory(AccessoryKind::ArrayList),
        Type::accessory(AccessoryKind::NonEmptyArray),
    ])
}

/// The first (`from_start`) or last key of a sealed array shape, or of
/// each member of a union of them: the keys up to and including the first
/// one that is surely there, `null` too when none is.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn shape_end_key(arr_ty: &Type, from_start: bool) -> Option<Type> {
    let mut keys = Vec::new();
    for member in arr_ty.union_members() {
        let shape = member.as_array_shape()?;
        if shape.unsealed.is_some() {
            return None;
        }
        let mut fields: Box<dyn Iterator<Item = &types::ShapeField>> = if from_start {
            Box::new(shape.fields().iter())
        } else {
            Box::new(shape.fields().iter().rev())
        };
        let mut surely_non_empty = false;
        for field in &mut fields {
            keys.push(field.key.to_type());
            if !field.optional {
                surely_non_empty = true;
                break;
            }
        }
        if !surely_non_empty {
            keys.push(Type::null());
        }
    }
    Some(Type::union_of(keys))
}

/// Whether `e` names a hash algorithm whose hex digest is longer than any
/// decimal int.
fn long_hex_algorithm(e: &ExprNode) -> bool {
    const LONG: [&str; 47] = [
        "md2",
        "md4",
        "md5",
        "sha1",
        "sha224",
        "sha256",
        "sha384",
        "sha512/224",
        "sha512/256",
        "sha512",
        "sha3-224",
        "sha3-256",
        "sha3-384",
        "sha3-512",
        "ripemd128",
        "ripemd160",
        "ripemd256",
        "ripemd320",
        "whirlpool",
        "tiger128,3",
        "tiger160,3",
        "tiger192,3",
        "tiger128,4",
        "tiger160,4",
        "tiger192,4",
        "snefru",
        "snefru256",
        "gost",
        "gost-crypto",
        "murmur3c",
        "murmur3f",
        "xxh128",
        "haval128,3",
        "haval160,3",
        "haval192,3",
        "haval224,3",
        "haval256,3",
        "haval128,4",
        "haval160,4",
        "haval192,4",
        "haval224,4",
        "haval256,4",
        "haval128,5",
        "haval160,5",
        "haval192,5",
        "haval224,5",
        "haval256,5",
    ];
    literal_string_value(e).is_some_and(|name| LONG.contains(&name.to_ascii_lowercase().as_str()))
}

impl Checker<'_> {
    /// `array_combine(keys, values)` for two arrays: two constant lists of
    /// one length zip into a shape, otherwise `keys`' values converted to
    /// strings and cast key `values`' values.
    fn combined_arrays(&self, keys: &Type, values: &Type) -> Type {
        let (key_arrays, value_arrays) = (keys.constant_arrays(), values.constant_arrays());
        if let ([k], [v]) = (key_arrays.as_slice(), value_arrays.as_slice())
            && k.fields().len() == v.fields().len()
            && k.fields().iter().chain(v.fields()).all(|f| !f.optional)
        {
            let mut shape = Shape::sealed(Vec::new());
            let zipped = k.fields().iter().zip(v.fields()).map(|(kf, vf)| {
                ShapeKey::from_type(&kf.ty.to_string_array_key(self.registry))
                    .map(|key| ShapeField::new(key, vf.ty.clone(), false))
            });
            if let Some(fields) = zipped.collect::<Option<Vec<_>>>() {
                for field in fields {
                    shape.set(field);
                }
                return Type::array_shape(shape);
            }
        }
        let key = keys
            .array_value_type()
            .map_or_else(Type::unknown_array_key, |k| {
                k.to_string_array_key(self.registry)
            });
        let value = values.array_value_type().unwrap_or_else(Type::mixed);
        let combined = Type::array(key, value);
        if keys.is_non_empty_array().yes() && values.is_non_empty_array().yes() {
            combined.to_non_empty_array()
        } else {
            combined
        }
    }
}
