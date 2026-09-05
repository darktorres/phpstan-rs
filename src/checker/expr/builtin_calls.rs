use crate::ast::*;
use crate::checker::{
    Checker, Locals, literal_string_key, narrow_type_excluding, narrow_type_excluding_falsy,
    narrow_type_to, resolve_call_args, to_absolute_number,
};
use crate::registry::{FunctionSig, Registry};
use crate::types::{self, AccessoryKind, Type};

use super::{is_canonical_int_key, is_false_literal, union_types};

impl<'a> Checker<'a> {
    /// Phase 12's bespoke per-function return-type extensions (real
    /// PHPStan's own `src/Type/Php/` is 190 files of exactly this kind of
    /// special-casing, not generic machinery) - `array_map`/`array_filter`
    /// always return a bare, structureless `array` today (element type
    /// `Mixed`), discarding real information already available: the
    /// input array's own element type, and (for `array_map`) the
    /// callback's own return type. Falls through to the ordinary
    /// generic-template path (`infer_call_return_type`) for every other
    /// function, and for any shape of these two this doesn't specially
    /// handle.
    /// Deliberately stays on the raw, positional `args` (not Item 1's own
    /// per-parameter resolution) for every special case below - these are
    /// a small, fixed set of well-known standard-library functions
    /// nobody realistically calls with named arguments, so resolving
    /// them by name would add risk (a resolved slice isn't sized the same
    /// way a raw argument count is - see `check_args`'s own note on this
    /// exact distinction) for no real benefit. Only the generic fallback
    /// at the end needs the real resolution, computed here from the same
    /// raw `args` this function already has.
    pub(crate) fn infer_builtin_call_return_type(
        &self,
        name: &str,
        sig: &FunctionSig,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        match name {
            // Phase 51: real PHPStan's own `AbsFunctionDynamicReturnTypeExtension`
            // - delegates entirely to `Type::toAbsoluteNumber()`.
            "abs" if args.len() == 1 => {
                return to_absolute_number(&self.infer_expr(&args[0].value, locals));
            }
            // `range()`'s own real stub (phpstorm-stubs' `@return array` -
            // no refinement at all) gives no list-ness/element-type
            // information whatsoever, but real PHPStan special-cases this
            // function (its own `RangeFunctionReturnTypeExtension`): a
            // real `range($low, $high)` call always walks `$low` to
            // `$high` inclusive with sequential integer keys starting at
            // 0 - genuinely a list, and never empty (even `range(5, 5)`
            // produces `[5]`) - regardless of whether the bounds are
            // literals this project could evaluate exactly. Scoped to
            // int and single-character-string bounds (the two real
            // corpus shapes - `range(1000, 1999)`, character-range
            // idioms) with no 3rd `$step` argument (a non-default step
            // can skip past `$high` without reaching it exactly, which
            // doesn't change list-ness/non-emptiness but isn't needed by
            // any real call site yet) - anything else falls through to
            // the generic path below, unchanged.
            "range" if args.len() == 2 => {
                let elem = match (
                    &self.infer_expr(&args[0].value, locals),
                    &self.infer_expr(&args[1].value, locals),
                ) {
                    (Type::Int | Type::IntLiteral(_), Type::Int | Type::IntLiteral(_)) => {
                        Some(Type::Int)
                    }
                    (Type::StringLiteral(a), Type::StringLiteral(b))
                        if a.chars().count() == 1 && b.chars().count() == 1 =>
                    {
                        Some(Type::String)
                    }
                    _ => None,
                };
                if let Some(elem) = elem {
                    return Type::Intersection(vec![
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem)),
                        Type::Accessory(AccessoryKind::ArrayList),
                        Type::Accessory(AccessoryKind::NonEmptyArray),
                    ]);
                }
            }
            // `preg_split()`'s own real stub (phpstorm-stubs' `@return
            // string[]|false` - the bare shorthand-array syntax, no
            // list-ness) gives up exactly the same real information real
            // PHPStan's own dedicated `PregSplitDynamicReturnTypeExtension`
            // computes: splitting a string always produces sequential
            // integer keys starting at 0, a real list, for any pattern/
            // subject (literal or not) - real PHPStan only bothers
            // constant-folding the *literal* case for maximum precision,
            // but its own fallback for the general (non-literal) case,
            // the only one worth modeling here, is exactly `list<string>
            // |false>` (`list<non-empty-string>|false` when
            // `PREG_SPLIT_NO_EMPTY` is one of the flags - checked the
            // same lenient bitwise-or-chain-of-names way `parse_preg_
            // flags` already does for `preg_match`, just without needing
            // that function's own stricter "every flag must be
            // recognized" requirement, since an unrecognized flag
            // doesn't change list-ness/non-emptiness either way here).
            "preg_split" if (2..=4).contains(&args.len()) => {
                let no_empty = args
                    .get(3)
                    .is_some_and(|a| contains_flag_name(&a.value, "PREG_SPLIT_NO_EMPTY"));
                let elem = if no_empty {
                    Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::NonEmptyString),
                    ])
                } else {
                    Type::String
                };
                let list_ty = Type::Intersection(vec![
                    Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem)),
                    Type::Accessory(AccessoryKind::ArrayList),
                ]);
                return union_types(list_ty, Type::False);
            }
            // Phase 76: a deliberately narrow slice of real PHPStan's own
            // `FilterFunctionReturnTypeHelper` - only the single most
            // common real-world shape (a literal `FILTER_VALIDATE_*`
            // constant as the filter, a plain int flags argument with no
            // `options` array, no `FILTER_REQUIRE_ARRAY`/
            // `FILTER_FORCE_ARRAY`) is modeled; everything else (a
            // computed filter value, sanitize filters, `FILTER_DEFAULT`,
            // an `options` array - real corpus shape:
            // `filter_var($v, FILTER_VALIDATE_INT, ['options' =>
            // ['min_range' => 1]])`, which narrows the successful result
            // to a real range this doesn't attempt to compute - the
            // array-requiring/forcing flags, PHP 8.4's
            // `FILTER_THROW_ON_FAILURE`) falls through to the generic
            // stub type unchanged rather than guessing at real PHPStan's
            // own considerably more involved logic for them.
            // `FILTER_NULL_ON_FAILURE` is checked the same lenient
            // bitwise-or-chain-of-names way `preg_split`'s own flags
            // argument just above already is, since real code commonly
            // ORs it together with an unrelated flag (e.g.
            // `FILTER_VALIDATE_IP`'s own `FILTER_FLAG_NO_PRIV_RANGE`).
            "filter_var" if (2..=3).contains(&args.len()) => {
                let flags_is_options_array = args
                    .get(2)
                    .is_some_and(|a| Self::is_array_like(&self.infer_expr(&a.value, locals)));
                if let ExprKind::Name(filter_name) = &args[1].value.kind
                    && let Some(base_ty) =
                        filter_validate_type_for_constant(filter_name.trim_start_matches('\\'))
                    && !flags_is_options_array
                    && !args
                        .get(2)
                        .is_some_and(|a| contains_flag_name(&a.value, "FILTER_REQUIRE_ARRAY"))
                    && !args
                        .get(2)
                        .is_some_and(|a| contains_flag_name(&a.value, "FILTER_FORCE_ARRAY"))
                {
                    let null_on_failure = args
                        .get(2)
                        .is_some_and(|a| contains_flag_name(&a.value, "FILTER_NULL_ON_FAILURE"));
                    let fallback = if null_on_failure {
                        Type::Null
                    } else {
                        Type::False
                    };
                    return union_types(base_ty, fallback);
                }
            }
            // Scoped to the common 2-arg single-array form - the real
            // function is variadic (multiple arrays); a 3+-arg call falls
            // through to the generic path below, unchanged.
            "array_map" if args.len() == 2 => {
                let input_ty = self.infer_expr(&args[1].value, locals);
                if let Some(elem_ty) = Self::array_element_type(&input_ty)
                    && let Some(mapped) = self.infer_closure_return_with_param_overrides(
                        &args[0].value,
                        &[elem_ty],
                        locals,
                    )
                {
                    // Phase 14: mapping never changes key structure or
                    // element count, so any Accessory marker (list-ness/
                    // non-emptiness) the input had survives onto the
                    // result - verified via dumpType().
                    return rewrap_with_accessory_markers(mapped, &input_ty);
                }
            }
            // Filtering never transforms elements, so the result is
            // simply whatever the input array's own type already is - no
            // closure introspection needed at all. Real PHP requires at
            // least 1 argument, but a syntactically-valid call passing
            // zero (invalid at runtime, real code in the vendor/ corpus
            // this project verifies against - a genuine, previously
            // unguarded panic here, found via --check-project against
            // the *full* corpus, not just first-party src/) still parses
            // fine and must not be indexed into blindly.
            "array_filter" if !args.is_empty() => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if Self::is_array_like(&arr_ty) {
                    // Phase 14: filtering can both leave gaps in a
                    // list's own sequential keys and empty out a
                    // non-empty array - verified via dumpType() that
                    // neither marker survives, so they're stripped
                    // rather than passed through unchanged.
                    let stripped = strip_accessory_markers(arr_ty);
                    if args.len() >= 2 {
                        if is_null_filter_callback(&args[1].value) {
                            return strip_null_from_array_element(stripped);
                        }
                        // A round of Piwigo-corpus bucketing found the
                        // same real PHPStan extension for an
                        // `instanceof`-shaped filter callback (`fn ($v):
                        // bool => $v instanceof Target`) - narrows the
                        // retained element type to `Target` specifically
                        // (not just strips `Null`: an `A|B|null` element
                        // type filtered by `instanceof B` keeps only `B`,
                        // not `A` too), verified via dumpType(). Real
                        // Piwigo code
                        // (`BatchManagerGlobalPageRenderer::render`)
                        // filters a `list<TagId|null>` from
                        // `array_map(TagId::tryFrom(...), ...)` down to a
                        // real `list<TagId>` this way.
                        if let Some(class_name) = is_instanceof_filter_callback(&args[1].value) {
                            let target = self.resolve_name_expr(class_name);
                            return narrow_array_element_type_to(stripped, &target);
                        }
                    } else {
                        // No callback at all - real PHP's own default
                        // behavior, dropping every falsy element - is
                        // exactly `narrow_type_excluding_falsy`'s own
                        // job, verified via dumpType() (`list<TagId|null>`
                        // -> `list<TagId>`; `list<int|null>` ->
                        // `list<non-zero-int>`, stripping both `0` and
                        // `null`, not just `null`). Real Piwigo code
                        // (`TusUploadCompletionService::completePhoto`)
                        // relies on exactly the object|null case.
                        return narrow_array_element_type_excluding_falsy(stripped);
                    }
                    return stripped;
                }
            }
            // array_reduce(array $array, callable $callback, mixed
            // $initial = null): mixed - a fold, so the result is
            // ultimately whatever the callback's own return type is
            // (each call's return becomes the next call's $carry). Binds
            // the callback's two params: $carry to $initial's own type
            // (Null when omitted, matching real PHP's own default), $item
            // to the array's element type.
            "array_reduce" if args.len() == 2 || args.len() == 3 => {
                if let Some(item_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    let carry_ty = args
                        .get(2)
                        .map(|initial| self.infer_expr(&initial.value, locals))
                        .unwrap_or(Type::Null);
                    if let Some(reduced) = self.infer_closure_return_with_param_overrides(
                        &args[1].value,
                        &[carry_ty, item_ty],
                        locals,
                    ) {
                        return reduced;
                    }
                }
            }
            // Combining never changes the values' own type - $keys
            // becomes what our Type::Array can't represent at all (key
            // types aren't tracked), so the result is simply $values'
            // own type verbatim, the same "no transformation" shape as
            // array_filter above.
            "array_combine" if args.len() == 2 => {
                let values_ty = self.infer_expr(&args[1].value, locals);
                if Self::is_array_like(&values_ty) {
                    // Phase 14: the result is keyed by $keys, not
                    // $values's own keys - verified via dumpType() that
                    // any Accessory marker $values carried doesn't
                    // survive, so it's stripped here too.
                    return strip_accessory_markers(values_ty);
                }
            }
            // Phase 14: two new special cases, real precision rather
            // than regression-avoidance - both preserve list-ness/
            // non-emptiness through to their result when called with
            // their own default arguments (verified via dumpType()),
            // but not with `preserve_keys: true` (confirmed to
            // genuinely break list-ness - original indices survive, no
            // longer sequential from zero - so deliberately not handled
            // here, matching real PHPStan's own behavior).
            "array_slice"
                if matches!(args.len(), 2..=3)
                    || (args.len() == 4 && is_false_literal(&args[3].value)) =>
            {
                let input_ty = self.infer_expr(&args[0].value, locals);
                if let Some(elem_ty) = Self::array_element_type(&input_ty) {
                    return rewrap_with_accessory_markers(elem_ty, &input_ty);
                }
            }
            "array_reverse"
                if args.len() == 1 || (args.len() == 2 && is_false_literal(&args[1].value)) =>
            {
                let input_ty = self.infer_expr(&args[0].value, locals);
                if let Some(elem_ty) = Self::array_element_type(&input_ty) {
                    return rewrap_with_accessory_markers(elem_ty, &input_ty);
                }
            }
            // Phase 14: verified via dumpType() to *always* return
            // list<V>, regardless of whether the input was already
            // list-shaped (re-sequencing every key from zero is exactly
            // what array_values does) - no existing special case here
            // before this.
            "array_values" if args.len() == 1 => {
                if let Some(elem_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    return Type::Intersection(vec![
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem_ty)),
                        Type::Accessory(AccessoryKind::ArrayList),
                    ]);
                }
            }
            "array_unique" if !args.is_empty() => {
                // Phase 14: removing a duplicate can leave a gap in the
                // surviving sequential keys - verified via dumpType()
                // that list-ness doesn't survive.
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if Self::is_array_like(&arr_ty) {
                    return strip_accessory_markers(arr_ty);
                }
            }
            // Phase 73: real PHP always takes every kept key/value pair's
            // own *type* straight from `$array` (the first argument) -
            // every other argument only narrows *which* keys survive,
            // never what type a surviving one holds. List-ness doesn't
            // survive (a matching key can be anywhere in `$array`,
            // leaving a gap in whatever sequential keys remain), same
            // "don't guess past what's certain" leniency `array_unique`
            // just above already applies to its own similar shape.
            // Phase 74 follow-up: generalizes the identical shape to
            // every other `array_diff*`/`array_udiff*`/`array_intersect*`/
            // `array_uintersect*` sibling - none of them ever transforms
            // a surviving element's own value (only *which* key/value
            // pairs survive differs between them: by value, by key, by
            // both, or via a user comparison callback for the `u`-
            // prefixed variants), so every one shares `array_unique`/
            // `array_intersect_key`'s identical "arg0's own type, minus
            // whatever accessory markers, verbatim" result. Confirmed
            // directly via `php -r`: `array_diff([10, 20, 30], [20])`
            // returns `[0 => 10, 2 => 30]`, not re-sequenced - list-ness
            // never survives here either, the same "don't guess past
            // what's certain" leniency already established.
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
                if args.len() >= 2 =>
            {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if Self::is_array_like(&arr_ty) {
                    return strip_accessory_markers(arr_ty);
                }
            }
            // Phase 73: swaps keys and values - the result's own key
            // type is `$array`'s *value* type, and vice versa. Verified
            // directly via `php`: list-ness never survives (the new keys
            // come from the old values, essentially never already
            // `0, 1, 2, ...`), but non-emptiness does (flipping can't
            // change whether there's at least one element).
            "array_flip" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if Self::is_array_like(&arr_ty) {
                    let key_ty =
                        Self::array_key_type(&arr_ty).unwrap_or_else(Type::unknown_array_key);
                    let elem_ty = Self::array_element_type(&arr_ty).unwrap_or(Type::Mixed);
                    let flipped = Type::Array(Box::new(elem_ty), Box::new(key_ty));
                    return if Self::is_known_non_empty_array(&arr_ty) {
                        Type::Intersection(vec![
                            flipped,
                            Type::Accessory(AccessoryKind::NonEmptyArray),
                        ])
                    } else {
                        flipped
                    };
                }
            }
            // Phase 74: the internal-array-pointer family
            // (`ArrayPointerFunctionsDynamicReturnTypeExtension`/
            // `ArrayCurrentDynamicReturnTypeExtension`/
            // `ArrayNextDynamicReturnTypeExtension` real PHPStan side) -
            // `reset`/`current`/`end` all report the array's own element
            // type (bare when it's provably non-empty, unioned with
            // `false` otherwise, `false` alone when it's provably
            // empty), the same three-way split `array_pointer_result`
            // below shares with `key`. `next`/`prev` always union with
            // `false` even when the array is provably non-empty -
            // confirmed directly via `php`: moving the pointer can run
            // off either end regardless of how many elements exist.
            "reset" | "current" | "end" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if Self::is_array_like(&arr_ty) {
                    let elem_ty = Self::array_element_type(&arr_ty).unwrap_or(Type::Mixed);
                    return Self::array_pointer_result(&arr_ty, elem_ty, Type::False, false);
                }
            }
            "next" | "prev" if args.len() == 1 => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if Self::is_array_like(&arr_ty) {
                    let elem_ty = Self::array_element_type(&arr_ty).unwrap_or(Type::Mixed);
                    return Self::array_pointer_result(&arr_ty, elem_ty, Type::False, true);
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
                if Self::is_array_like(&arr_ty) {
                    let key_ty =
                        Self::array_key_type(&arr_ty).unwrap_or_else(Type::unknown_array_key);
                    return Self::array_pointer_result(&arr_ty, key_ty, Type::Null, false);
                }
            }
            // A round of Piwigo-corpus bucketing after Phase 14 found this
            // was the single largest remaining false-positive source:
            // `array_merge(['a', 'b'], someListReturningCall())`, real
            // code combining several list<T>-typed arrays into one, a
            // common idiom now that list<T> is checked strictly. Verified
            // via dumpType(): merging only list-like arguments stays a
            // list (a literal, non-empty argument makes it a
            // non-empty-list); merging in even one plain, unproven
            // `array<T>` degrades list-ness for the whole result, same as
            // a spread array literal's own degrade rule above - but the
            // element-type union is still tracked precisely either way.
            // General key-aware shape merging (`array_merge($shape, ...)`)
            // is real PHPStan's own much richer behavior and deliberately
            // not attempted here, matching Phase 14's own decision to
            // leave array_merge itself unaudited beyond this.
            "array_merge" if !args.is_empty() => {
                let arg_types: Vec<Type> = args
                    .iter()
                    .map(|a| self.infer_expr(&a.value, locals))
                    .collect();
                // When every argument's own exact fields are known
                // (`array{...}`, e.g. merging a property's own default
                // shape with a caller-supplied options shape), merge them
                // field-by-field via the same rule a spread array literal
                // uses - verified via dumpType() that array_merge follows
                // the identical key rule (canonical int keys renumbered,
                // other keys overwritten by the later argument).
                // A spread argument (`array_merge(...$listOfLists)`, a
                // real corpus pattern flattening a word=>list<int> map's
                // own values - `SearchService::searchAllwords`) expands
                // to however many arrays that value itself holds at
                // runtime, not one merge argument shaped like the
                // array-of-arrays itself - never eligible for the
                // fixed-arity precise shape merge below.
                let has_spread = args.iter().any(|a| a.spread);
                if !has_spread && arg_types.iter().all(|t| matches!(t, Type::ArrayShape(_))) {
                    let mut fields: Vec<(String, Type, bool)> = Vec::new();
                    let mut next_index: i64 = 0;
                    for arg_ty in &arg_types {
                        if let Type::ArrayShape(inner) = arg_ty {
                            for (k, t, opt) in inner {
                                Self::merge_shape_field(
                                    &mut fields,
                                    &mut next_index,
                                    k,
                                    t.clone(),
                                    *opt,
                                );
                            }
                        }
                    }
                    return Type::ArrayShape(fields);
                }
                let mut elem_types: Vec<Type> = Vec::new();
                let mut all_list = true;
                let mut any_non_empty = false;
                let mut all_known_array_like = true;
                for (arg, arg_ty) in args.iter().zip(&arg_types) {
                    if arg.spread {
                        // Each element of the spread source is one of the
                        // arrays actually being merged - its own element
                        // type is one level further down than the spread
                        // source's own element type. List-ness/non-
                        // emptiness aren't provable this way (an unknown
                        // number of arrays, of unknown emptiness).
                        match Checker::array_element_type(arg_ty) {
                            Some(inner_ty) => {
                                if !Self::is_list_like(&inner_ty) {
                                    all_list = false;
                                }
                                match Checker::array_element_type(&inner_ty) {
                                    Some(t) => elem_types.push(t),
                                    None => all_known_array_like = false,
                                }
                            }
                            None => all_known_array_like = false,
                        }
                    } else if Self::is_array_like(arg_ty) {
                        if !Self::is_list_like(arg_ty) {
                            all_list = false;
                        }
                        if Self::is_known_non_empty_array(arg_ty) {
                            any_non_empty = true;
                        }
                        if let Some(t) = Checker::array_element_type(arg_ty) {
                            elem_types.push(t);
                        }
                    } else {
                        all_known_array_like = false;
                    }
                }
                if all_known_array_like {
                    let elem_ty = elem_types
                        .into_iter()
                        .reduce(union_types)
                        .unwrap_or(Type::Mixed);
                    let mut members = vec![Type::Array(
                        Box::new(Type::unknown_array_key()),
                        Box::new(elem_ty),
                    )];
                    if all_list {
                        members.push(Type::Accessory(AccessoryKind::ArrayList));
                    }
                    if any_non_empty {
                        members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
                    }
                    return if members.len() == 1 {
                        members.pop().unwrap()
                    } else {
                        Type::Intersection(members)
                    };
                }
            }
            // Both fill every slot with the same, already-known $value -
            // no closure or element-type binding needed at all.
            "array_fill" if args.len() == 3 => {
                return Type::Array(
                    Box::new(Type::unknown_array_key()),
                    Box::new(self.infer_expr(&args[2].value, locals)),
                );
            }
            // Phase 73: `$keys`' own *elements* become the result's real
            // keys - `array_fill_keys(['a', 'b'], 0)` really is
            // `array<string, int>`, not the `array<int|string, int>`
            // `unknown_array_key()` used to settle for unconditionally.
            "array_fill_keys" if args.len() == 2 => {
                let keys_ty = self.infer_expr(&args[0].value, locals);
                let key_ty =
                    Self::array_element_type(&keys_ty).unwrap_or_else(Type::unknown_array_key);
                return Type::Array(
                    Box::new(key_ty),
                    Box::new(self.infer_expr(&args[1].value, locals)),
                );
            }
            // Splits one array into several same-shaped ones - the
            // result is an array *of* arrays, each holding the original
            // element type.
            "array_chunk" if args.len() == 2 || args.len() == 3 => {
                if let Some(elem_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    return Type::Array(
                        Box::new(Type::unknown_array_key()),
                        Box::new(Type::Array(
                            Box::new(Type::unknown_array_key()),
                            Box::new(elem_ty),
                        )),
                    );
                }
            }
            // Extracts one named field from an array of known shapes
            // (e.g. `array<array{id: int, name: string}>`) - only fires
            // when the element type is a real, known ArrayShape and the
            // column key is a compile-time-known literal, mirroring
            // check_shape_key_access's own "only a literal, never a
            // dynamic value" precedent.
            // Phase 31: the real stub's own `@return int[]|string[]` -
            // not a single homogeneous key type but a whole-array
            // either-or, since a real array's keys could be all-int,
            // all-string, or (rarer) a mix - printed as the confusing
            // `array<int>|array<string>` seen in this corpus's own
            // diagnostics. Scoped to the one-argument form (no
            // `$filter_value`, which changes which keys survive in a way
            // this doesn't try to predict) with a known `ArrayShape`
            // argument, where every key is knowable up front: PHP itself
            // casts a canonical non-negative-integer string key to a real
            // int at the point the array was built, so this mirrors that
            // per-key, folding into a real `int|string` union only when
            // the shape's own keys are genuinely mixed.
            // Round 13: verified via dumpType() - array_keys() always
            // returns a real list (sequentially re-keyed from zero),
            // regardless of whether the input array's own keys were
            // already sequential - the same "always list<V>" shape
            // array_values() already gets above.
            "array_keys" if args.len() == 1 => {
                let arg_ty = self.infer_expr(&args[0].value, locals);
                if let Type::ArrayShape(fields) = &arg_ty {
                    let key_ty = fields
                        .iter()
                        .map(|(k, _, _)| {
                            if is_canonical_int_key(k) {
                                Type::Int
                            } else {
                                Type::String
                            }
                        })
                        .reduce(union_types)
                        .unwrap_or(Type::String);
                    return Type::Intersection(vec![
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(key_ty)),
                        Type::Accessory(AccessoryKind::ArrayList),
                    ]);
                }
                // The native stub's own `@return int[]|string[]` is real
                // but useless without this project's own key-type
                // tracking (`Type::Array` has no key-type parameter at
                // all, a known limitation) - falling through to it
                // verbatim surfaced `int|string` for every reader of the
                // result, a real regression once `array_element_type`
                // gained its own `Union` arm (below) and started actually
                // resolving it instead of silently giving up at `Mixed`.
                // Matches this project's own usual "don't know, don't
                // guess beyond Mixed" default for an unprovable key type.
                if Self::is_array_like(&arg_ty) {
                    return Type::Intersection(vec![
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
                        Type::Accessory(AccessoryKind::ArrayList),
                    ]);
                }
            }
            "array_column" if args.len() == 2 || args.len() == 3 => {
                if let Some(row_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    // A shaped row (`array{version: string, ...}`) looks
                    // up the one named field; an unshaped row
                    // (`array<string, mixed>`, the common real shape for
                    // a raw DB-fetch row with no further refinement)
                    // has no per-key precision to offer at all, so every
                    // column read is simply the row's own uniform
                    // element type - still real, useful information
                    // (even when that's `Mixed`) that the row-shape-only
                    // arm below used to give up on entirely.
                    let field_ty = match &row_ty {
                        Type::ArrayShape(fields) => literal_string_key(&args[1].value)
                            .and_then(|key| fields.iter().find(|(n, _, _)| *n == key))
                            .map(|(_, ty, _)| ty.clone()),
                        _ if Self::is_array_like(&row_ty) => Self::array_element_type(&row_ty),
                        _ => None,
                    };
                    if let Some(field_ty) = field_ty {
                        let arr_ty =
                            Type::Array(Box::new(Type::unknown_array_key()), Box::new(field_ty));
                        // A real Piwigo-corpus round found the
                        // 2-argument form (no `$index_key`) missing its
                        // own real list-ness entirely: `array_column()`
                        // always builds a brand-new array with fresh
                        // sequential integer keys starting at 0,
                        // regardless of the input array's own keys - a
                        // genuine list. The 3-argument form uses
                        // `$index_key`'s own column values as the
                        // result's keys instead, which generally aren't
                        // sequential integers, so it deliberately keeps
                        // today's plain, unmarked `Array`.
                        return if args.len() == 2 {
                            Type::Intersection(vec![
                                arr_ty,
                                Type::Accessory(AccessoryKind::ArrayList),
                            ])
                        } else {
                            arr_ty
                        };
                    }
                }
            }
            // The real stub declares `array_search(mixed, array, bool):
            // string|int|false` unconditionally - this project's own
            // `Type::Array` has no key-type parameter at all (a known,
            // sizable open gap - see Known limitations), so this can't
            // generally narrow the key-type half the way real PHPStan's
            // own `ArraySearchFunctionDynamicReturnTypeExtension`
            // (delegating to the haystack array type's own `searchArray()`)
            // does - but a *list*-typed haystack is a real, narrower
            // special case this project already tracks precisely
            // without any key-type machinery at all: a list's own keys
            // are always sequential integers by definition, so
            // searching one can only ever find an `int` key (or
            // `false`), never `string`. Real corpus shape
            // (`array_search($needle, $haystack, true)` on a `list<string>`
            // haystack, narrowed to `int` after the caller's own
            // `$index === false` guard).
            "array_search" if args.len() >= 2 => {
                let haystack_ty = self.infer_expr(&args[1].value, locals);
                if Checker::is_list_like(&haystack_ty) {
                    return union_types(Type::Int, Type::False);
                }
                // Phase 73: the same idea, one level less precise - a
                // haystack whose own key type is confidently known (not
                // just "definitely a list") narrows the found-key half
                // of the result the same way, e.g. an `array<string,
                // V>` haystack can only ever find a `string` key.
                if let Some(key_ty) = Self::array_key_type(&haystack_ty)
                    && key_ty != Type::unknown_array_key()
                {
                    return union_types(key_ty, Type::False);
                }
            }
            // The real stub declares `version_compare(string, string,
            // ?string $operator): int|bool` unconditionally (both PHP-
            // version-gated overloads agree on this) - real PHPStan's
            // own `VersionCompareFunctionDynamicReturnTypeExtension`
            // narrows based purely on whether the 3rd `$operator`
            // argument is syntactically *present* at all, regardless of
            // its own value: present means the real return is `bool`
            // (or `bool|null` pre-8.0, not modeled here - no corpus
            // evidence for it), absent means it's always exactly one of
            // `-1`/`0`/`1`, never `bool`. Scoped to the 2-argument form
            // (real corpus shape: `static fn (string $a, string $b): int
            // => version_compare($a, $b)`) - the 3-argument form falls
            // through to the generic, already-correct-if-loose `int|bool`
            // stub type unchanged.
            "version_compare" if args.len() == 2 => return Type::Int,
            // Phase 32 Finding D: the real stub declares `microtime(bool
            // $as_float = false): string|float` unconditionally - real
            // PHPStan's own `MicrotimeFunctionReturnTypeExtension` narrows
            // a literal `true` argument to plain `float`, a literal
            // `false` (or no argument) to a string. `is_subtype`, not
            // `==`, since a `Union` that happens to be all-`True` or
            // all-`False` (e.g. narrowed from a wider type) should narrow
            // the same way a bare `True`/`False` would.
            "microtime" => match args.first().map(|a| self.infer_expr(&a.value, locals)) {
                None => return Type::String,
                Some(ty) if types::is_subtype(&ty, &Type::True) => return Type::Float,
                Some(ty) if types::is_subtype(&ty, &Type::False) => return Type::String,
                _ => {}
            },
            // Phase 23: the real stubs declare these unconditionally
            // unioned with `array` (`str_replace(...): array|string`),
            // even though the function only ever returns an array when
            // `$subject` itself is one - real PHPStan special-cases this
            // via its own hand-written dynamic return type extension, not
            // from the stub signature alone. Scoped to "subject is
            // string-like" (`types::is_subtype` against `Type::String`,
            // not a bare `==`) - a `Mixed`, a union, or a real array
            // correctly keeps today's existing behavior, but so does a
            // Phase 14 accessory-refined subject (`non-empty-string`
            // parses to `Intersection([String, Accessory(_)])`, which a
            // bare `== Type::String` silently stopped matching - a real
            // corpus regression caught in round 11, `Lang::load()`'s own
            // `$source_file !== ''`-narrowed subject).
            "str_replace" | "str_ireplace" if args.len() == 3 || args.len() == 4 => {
                if types::is_subtype(&self.infer_expr(&args[2].value, locals), &Type::String) {
                    return Type::String;
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
                    && types::is_subtype(
                        &self.infer_expr(&args[2].value, locals),
                        &Type::String,
                    ) =>
            {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_array_member(generic);
            }
            // Phase 32 Finding E: the same subject-dependent-return-type
            // family as the two arms above, missed by Phase 23/31 - the
            // real stub is `substr_replace(array|string $string, ...):
            // array|string`, and unlike `str_replace`'s subject at index
            // 2, `substr_replace`'s own subject is index 0. Never returns
            // `null`, so a plain early return suffices - no
            // `strip_array_member` dance needed.
            "substr_replace" if args.len() == 3 || args.len() == 4 => {
                if types::is_subtype(&self.infer_expr(&args[0].value, locals), &Type::String) {
                    return Type::String;
                }
            }
            // Phase 13 Finding A: the real stub declares `getenv(?string
            // $name = null, bool $local_only = false): array|string|false`
            // unconditionally, but real PHP only ever returns `array` for
            // a truly zero-argument call, and only ever returns
            // `string|false` when `$name` is a real, non-null string -
            // confirmed against real PHPStan's own `dumpType()` for every
            // shape that matters, including the two easy-to-get-wrong
            // ones: `getenv(null)` and `getenv(local_only: true)` (a named
            // arg that leaves `$name` at its own `null` default) both
            // still return the full, broad type, same as real PHPStan.
            // `args.is_empty()` must be checked on the raw call syntax,
            // before any named/positional resolution - resolving `name`
            // to `None` also happens for `getenv(local_only: true)`,
            // which must NOT get the plain-`array` answer.
            "getenv" => {
                if args.is_empty() {
                    return Type::Array(
                        Box::new(Type::unknown_array_key()),
                        Box::new(Type::String),
                    );
                }
                let resolved = resolve_call_args(args, &sig.params);
                if let Some(name_ty) = resolved
                    .first()
                    .copied()
                    .flatten()
                    .map(|e| self.infer_expr(e, locals))
                    && types::is_subtype(&name_ty, &Type::String)
                {
                    return Type::Union(vec![Type::String, Type::False]);
                }
            }
            // A round of Piwigo-corpus bucketing found this was the
            // single largest non-array-related false-positive source: the
            // native stub declares a plain `string|false`, but real
            // PHPStan carries its own dynamic-return-type extension here
            // - verified via dumpType() that a successful call always
            // returns a real non-falsy-string, never ''/'0' - a common
            // idiom being the exact site that broke without this: `$jar =
            // tempnam(...); if ($jar === false) { throw ...; }` then
            // passing `$jar` on to a `non-empty-string`-typed sink.
            "tempnam" if args.len() == 2 => {
                return Type::Union(vec![
                    Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::NonFalsyString),
                    ]),
                    Type::False,
                ]);
            }
            // The vendored stub's own `@return string[]|false` reflects
            // PHP 7's behavior for an empty `$separator` - PHP 8+ throws a
            // `ValueError` there instead of returning `false` at all, so
            // real PHPStan's own dynamic-return-type extension drops
            // `|false` entirely and always answers `non-empty-list<string>`
            // - verified via dumpType(), even for a non-literal
            // `$separator` (there's no other way `explode()` can return
            // fewer than 1 element). Real Piwigo code
            // (GeneratedFileErrorMapper::lines) relies on exactly this.
            "explode" if matches!(args.len(), 2..=3) => {
                return Type::Intersection(vec![
                    Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::String)),
                    Type::Accessory(AccessoryKind::ArrayList),
                    Type::Accessory(AccessoryKind::NonEmptyArray),
                ]);
            }
            // Same family as tempnam() above - the native stub declares a
            // bare `array|false`, but real PHPStan's own dynamic-return-
            // type extension always returns a real `list<string>` on
            // success, confirmed via dumpType(). Real Piwigo code
            // (StructuralTest's own findCallSitesInRootPhpFiles/
            // findCallSitesInBinFiles) narrows away `false` via `$paths
            // !== false ? $paths : []` and then relies on the result
            // being a real `list<string>` of file paths.
            "glob" if matches!(args.len(), 1..=2) => {
                return Type::Union(vec![
                    Type::Intersection(vec![
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::String)),
                        Type::Accessory(AccessoryKind::ArrayList),
                    ]),
                    Type::False,
                ]);
            }
            // Same family, subject at index 1 - not evidenced directly in
            // this corpus, but identical mechanism and real stub shape
            // (`preg_replace_callback_array(array $pattern, array|string
            // $subject, ...): array|string|null`), so kept alongside its
            // siblings rather than left out for want of a diagnostic.
            "preg_replace_callback_array"
                if args.len() >= 2
                    && types::is_subtype(
                        &self.infer_expr(&args[1].value, locals),
                        &Type::String,
                    ) =>
            {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_array_member(generic);
            }
            // `mb_convert_encoding()` has its own real dynamic extension,
            // considerably more involved (it also reasons about
            // `$from_encoding` to decide whether `false` stays possible) -
            // only the array-stripping half is evidenced here
            // (`CharsetHelper::convertCharset`'s `mb_convert_encoding($str,
            // ...)` with `$str: string`), so that's all this implements.
            "mb_convert_encoding"
                if args.len() >= 2
                    && types::is_subtype(
                        &self.infer_expr(&args[0].value, locals),
                        &Type::String,
                    ) =>
            {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_array_member(generic);
            }
            // Phase 32 Finding J: json_encode(...): string|false -
            // passing JSON_THROW_ON_ERROR makes a real failure throw
            // instead of returning false. Real PHPStan's own
            // JsonThrowOnErrorDynamicReturnTypeExtension recognizes this
            // via a bitwise-OR-aware constant check on the flags
            // argument - `contains_json_throw_on_error` walks an
            // arbitrary-depth `|`-chain the same way (real corpus shape:
            // `JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT
            // | JSON_THROW_ON_ERROR`), not just a bare constant.
            "json_encode" if args.len() >= 2 && contains_json_throw_on_error(&args[1].value) => {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_false_member(generic);
            }
            // Phase 34 Finding A: the real stub declares `curl_getinfo`
            // unconditionally `mixed` - real PHPStan's own
            // CurlGetinfoFunctionDynamicReturnTypeExtension maps a
            // literal CURLINFO_* option constant to a concrete type
            // instead. Scoped to the 2-arg form actually used anywhere
            // in real code (a bare 1-arg `curl_getinfo($ch)` call
            // returns a much bigger "all components" array shape this
            // project doesn't model - falls through to today's `mixed`
            // unchanged).
            "curl_getinfo" if args.len() == 2 => {
                if let ExprKind::Name(option_name) = &args[1].value.kind
                    && let Some(ty) =
                        curl_getinfo_component_type(option_name.trim_start_matches('\\'))
                {
                    return ty;
                }
            }
            // Phase 35 Finding E: the real stub declares `get_class`
            // unconditionally `string` - real PHPStan's own
            // GetClassDynamicReturnTypeExtension resolves the return type
            // from the argument's own class instead, the same
            // "the correct type lives in a dynamic extension, not the
            // stub" shape `curl_getinfo`/`json_encode` above already
            // establish. Scoped to the 1-arg form - the 0-arg form ("the
            // calling object's own class") was removed in PHP 8. An
            // argument whose own type isn't a concrete `Object` falls
            // through to today's plain `string`, unchanged - matching
            // real PHPStan's own fallback for the same case.
            "get_class" if args.len() == 1 => {
                if let Type::Object(class, _) = self.infer_expr(&args[0].value, locals) {
                    return Type::ClassString(Box::new(Type::Object(class, Vec::new())));
                }
            }
            // Phase 77: `get_called_class()`'s own real stub is
            // unconditionally `string|false` - real PHPStan's own
            // `GetCalledClassDynamicReturnTypeExtension` resolves it to
            // `static::class`'s own type instead (`false` only when
            // called outside any class). This project doesn't track true
            // late static binding (`resolve_pseudo` already collapses
            // `self`/`static` to the same `current_class`, documented on
            // that function) - `current_class` is exactly that
            // resolution, same source `self::class`/`static::class`
            // already use.
            "get_called_class" if args.is_empty() => {
                return match &self.current_class {
                    Some(class) => {
                        Type::ClassString(Box::new(Type::Object(class.clone(), Vec::new())))
                    }
                    None => Type::False,
                };
            }
            // Phase 77: `get_parent_class($object_or_class)`'s own real
            // stub is unconditionally `string|false` - real PHPStan's own
            // `GetParentClassDynamicFunctionReturnTypeExtension` resolves
            // it from the argument's own class instead, walking its real
            // `extends` chain (the roadmap's own text additionally cites a
            // 0-arg "current class context" form real PHPStan also
            // supports - this project's own vendored stub
            // (`vendor/phpstorm-stubs/Core/Core.php`) declares
            // `$object_or_class` with no default, making it a required
            // parameter here, so a 0-arg call already gets its own
            // "expects at least 1 argument" diagnostic from `check_args`
            // independently of this dynamic-return-type refinement; not
            // worth a redundant, effectively-dead extra match arm for a
            // shape this project's own stub already rejects). Falls
            // through to today's plain `string|false` for anything
            // `find_declared_parent_class` can't answer confidently (an
            // unknown class, or a class literally declared with no parent
            // gets a confident `false` instead - see that function's own
            // doc comment).
            "get_parent_class" if args.len() == 1 => {
                if let Type::Object(class, _) = self.infer_expr(&args[0].value, locals)
                    && let Some(ty) = self.find_declared_parent_class(&class)
                {
                    return ty;
                }
            }
            // Phase 77: `gettype()`'s own real stub is unconditionally
            // `string` - real PHPStan's own
            // `GettypeFunctionReturnTypeExtension` maps a concretely-known
            // argument type to its own exact runtime type-name string
            // instead (falling back to the full 10-name union only for a
            // genuinely unknown argument, not worth replicating here -
            // `gettype_result` below returns `None` for that case,
            // leaving today's plain `string` unchanged).
            "gettype" if args.len() == 1 => {
                if let Some(ty) = gettype_result(&self.infer_expr(&args[0].value, locals)) {
                    return ty;
                }
            }
            // Phase 77: `get_debug_type()`'s own real stub is
            // unconditionally `string` - real PHPStan's own
            // `GetDebugTypeFunctionReturnTypeExtension` maps a
            // concretely-known argument type to its own exact
            // `get_debug_type()`-flavored name instead (an object's own
            // class name only when that class is `final` - a non-final
            // class's actual runtime value could be any subclass, so its
            // own real string can't be pinned down any tighter than
            // `string`, exactly mirroring that real extension's own
            // `isFinal()` check).
            "get_debug_type" if args.len() == 1 => {
                if let Some(ty) =
                    self.get_debug_type_result(&self.infer_expr(&args[0].value, locals))
                {
                    return ty;
                }
            }
            // Phase 77: `date()`'s own real stub is unconditionally
            // `string` - real PHPStan's own `DateFunctionReturnTypeHelper`
            // observes each format character's own real output by
            // actually calling PHP's `date()`. Scoped down to a purely
            // static per-character classification instead (no runtime
            // date simulation, unlike the DateTime/DOM/SimpleXML cluster
            // Phase 78 covers) - `date_format_return_type` below only
            // refines a literal format string composed entirely of
            // recognized characters; anything else (a non-literal
            // `$format`, or one containing an escape/punctuation
            // character this table doesn't classify) falls through to
            // today's plain `string`, unchanged.
            "date" if !args.is_empty() => {
                if let Type::StringLiteral(format) = self.infer_expr(&args[0].value, locals)
                    && let Some(ty) = date_format_return_type(&format)
                {
                    return ty;
                }
            }
            // Phase 77: `idate()`'s own real stub is unconditionally
            // `int|false` - real PHPStan's own
            // `IdateFunctionReturnTypeHelper` maps each single recognized
            // format character to its own known numeric bound (`IntRange`
            // is this project's own exact match for that). Scoped to a
            // literal single-character format string, `idate()`'s only
            // real accepted shape - anything else falls through to
            // today's plain `int|false`, unchanged.
            "idate" if !args.is_empty() => {
                if let Type::StringLiteral(format) = self.infer_expr(&args[0].value, locals)
                    && let Some(c) = single_char(&format)
                    && let Some((min, max)) = idate_format_char_range(c)
                {
                    return Type::IntRange(Some(min), Some(max));
                }
            }
            // Phase 77: `pathinfo($path)`'s own real stub is
            // unconditionally `array` (the 2-arg `$flags` overload is
            // `array|string`) - real PHPStan's own
            // `PathinfoFunctionDynamicReturnTypeExtension` gives the
            // 1-arg form a real shape (`dirname`/`basename`/`filename`
            // always present, `extension` only when the path actually has
            // one - simplified here to always-optional, since this
            // project has no way to know a literal path has a dot in it)
            // and narrows the 2-arg form down to plain `string` for a
            // literal single-component `PATHINFO_*` flag (everything but
            // `PATHINFO_ALL`, which gets the same array shape as the 1-arg
            // form). A non-literal `$flags` keeps today's `array|string`
            // union, unchanged.
            "pathinfo" if args.len() == 1 => {
                return pathinfo_array_shape();
            }
            "pathinfo" if args.len() == 2 => {
                if let ExprKind::Name(flag_name) = &args[1].value.kind {
                    return match flag_name.trim_start_matches('\\') {
                        "PATHINFO_DIRNAME" | "PATHINFO_BASENAME" | "PATHINFO_EXTENSION"
                        | "PATHINFO_FILENAME" => Type::String,
                        "PATHINFO_ALL" => pathinfo_array_shape(),
                        _ => Type::Union(vec![pathinfo_array_shape(), Type::String]),
                    };
                }
            }
            // Phase 77: `parse_url()`'s own real stub is unconditionally
            // `array|string|int|false|null` - real PHPStan's own
            // `ParseUrlFunctionDynamicReturnTypeExtension` narrows the
            // no-`$component`/non-literal-`$component` form down to a
            // real shape (every URL component optional, since none of
            // them are guaranteed present) unioned with `false`, and the
            // literal-`PHP_URL_*`-constant form down to that one
            // component's own real type. Deliberately skips that real
            // extension's own literal-`$url`-string special case (letting
            // Rust's own `url`/regex-free code re-parse a URL exactly the
            // way PHP's own C implementation does is real, evidenced
            // complexity `curl_getinfo`/`json_encode` above don't need -
            // not worth it for one function nothing in this project's own
            // corpus needs that precision for).
            "parse_url" if args.len() == 1 => {
                return Type::Union(vec![parse_url_components_array_shape(), Type::False]);
            }
            "parse_url" if args.len() == 2 => {
                if let ExprKind::Name(component_name) = &args[1].value.kind
                    && let Some(ty) =
                        parse_url_component_type(component_name.trim_start_matches('\\'))
                {
                    return ty;
                }
            }
            // Phase 77: `sscanf($string, $format)`'s own real stub is
            // unconditionally `array|null` - real PHPStan's own
            // `SscanfFunctionDynamicReturnTypeExtension` walks a literal
            // `$format` string's own `%`-specifiers (`%d`, `%5s`, ...) and
            // builds one array field per specifier, each typed by its own
            // conversion character (numeric conversions become `int`/
            // `float`; anything else stays `string`, refined by any
            // explicit field-width digits the same way that real
            // extension's own length-based accessory choice does) -
            // always nullable per element (a real trailing unmatched
            // specifier leaves that slot `null`) and the whole array
            // nullable too (a total match failure). Scoped to the 2-arg
            // form (the 3+-arg by-reference-`$vars` form returns `int|
            // null` instead, a materially different shape not modeled
            // here) - a non-literal or entirely-unrecognized `$format`
            // falls through to today's plain `array|null`, unchanged.
            "sscanf" if args.len() == 2 => {
                if let Type::StringLiteral(format) = self.infer_expr(&args[1].value, locals)
                    && let Some(fields) = sscanf_result_fields(&format)
                {
                    return Type::Union(vec![Type::ArrayShape(fields), Type::Null]);
                }
            }
            // Phase 77: `stat()`/`lstat()`/`fstat()`/`ssh2_sftp_stat()`
            // share one real per-key shape (real PHPStan's own
            // `StatDynamicReturnTypeExtension` covers exactly this set) -
            // their own real stubs are unconditionally `array` (`fstat`)
            // or `array|false`, discarding the real fixed 13-key shape
            // (`dev`/`ino`/`mode`/... , every value an `int`) every one of
            // them actually returns. `dio_stat()` shares the same shape
            // under different key names (real PHPStan's own separate
            // `DioStatDynamicFunctionReturnTypeExtension`), nullable
            // rather than `false`-able on failure.
            "stat" | "lstat" | "fstat" | "ssh2_sftp_stat" if args.len() == 1 => {
                return Type::Union(vec![stat_array_shape(), Type::False]);
            }
            "dio_stat" if args.len() == 1 => {
                return Type::Union(vec![dio_stat_array_shape(), Type::Null]);
            }
            // Phase 77: `hash_algos()`/`openssl_get_cipher_methods()`/
            // `mb_list_encodings()`'s own real stubs are unconditionally
            // a bare `array` - real PHPStan doesn't type these calls
            // themselves any more precisely either; its own
            // `HashFunctionsReturnTypeExtension`/`OpenSslCipherMethods
            // Provider`/`MbFunctionsReturnTypeExtensionTrait` instead
            // call them for real at PHPStan's own analysis time to build
            // a live allowlist validating *other* functions' string
            // arguments against the actual host's installed algorithms/
            // ciphers/encodings (`hash($algo, ...)`,
            // `openssl_encrypt($data, $cipher, ...)`, `mb_convert_
            // encoding($str, $to, ...)`) - a fundamentally different,
            // heavier mechanism than a per-function return-type refinement,
            // and one this project has no way to replicate without a
            // hardcoded, platform/PHP-version-fragile literal snapshot
            // (this static binary can't shell out to a live PHP+OpenSSL+
            // mbstring build the way PHPStan's own PHP-hosted analysis
            // does). What's implemented here is only the roadmap's other,
            // genuinely static half: each of these three calls' own
            // return value really is always a non-empty list of non-empty
            // strings, real, evidenced information today's bare `array`
            // throws away regardless of host/version.
            "hash_algos" | "openssl_get_cipher_methods" | "mb_list_encodings"
                if args.is_empty() =>
            {
                return list_of_non_empty_strings();
            }
            _ => {}
        }
        let resolved = resolve_call_args(args, &sig.params);
        self.infer_call_return_type(sig, &resolved, locals)
    }

    /// `get_parent_class`'s own real per-class answer: `class`'s
    /// registered `extends` chain gives a real, evidenced answer either
    /// way - a real parent (`class-string<Parent>`) or a real "no parent"
    /// (`false`, since a class with a known, empty `extends` genuinely has
    /// none). `None` only for a class this registry has no entry for at
    /// all (an external/unstubbed class) - there real PHPStan itself
    /// stays uncertain (`class-string|false`), so this project's own
    /// "don't guess" convention leaves today's plain `string|false`
    /// unchanged rather than picking one side of that uncertainty.
    fn find_declared_parent_class(&self, class: &str) -> Option<Type> {
        let info = self.registry.classes.get(class)?;
        Some(match info.extends.first() {
            Some(parent) => Type::ClassString(Box::new(Type::Object(parent.clone(), Vec::new()))),
            None => Type::False,
        })
    }

    /// `get_debug_type()`'s own object case: a `final` class's own real
    /// instances can only ever be that exact class (never a subclass, by
    /// definition), so its own name is the one real, exact answer - a
    /// non-`final` class's own real instance could be any subclass at
    /// runtime, so the tightest honest answer stays plain `string` (real
    /// PHPStan's own `GetDebugTypeFunctionReturnTypeExtension` makes this
    /// exact same `isFinal()` distinction). A class this registry has no
    /// entry for at all (external/unstubbed) is treated the same as
    /// non-`final` - real PHPStan itself can't assume a class it hasn't
    /// reflected is sealed either.
    fn get_debug_type_result(&self, ty: &Type) -> Option<Type> {
        get_debug_type_for(ty, self.registry)
    }

    /// Phase 74: the shared three-way split every internal-array-pointer
    /// function's own return type follows - a provably empty `arr_ty`
    /// always just reports `empty_marker` (`false` for `reset`/
    /// `current`/`end`/`next`/`prev`, `null` for `key`/`array_key_first`/
    /// `array_key_last`); a provably non-empty one reports the bare
    /// `result_ty` (the element or key type) *unless* `force_union` -
    /// `next`/`prev` alone set it, since moving the pointer can run off
    /// either end regardless of how many elements exist; anything else
    /// (emptiness genuinely unknown) unions the two.
    fn array_pointer_result(
        arr_ty: &Type,
        result_ty: Type,
        empty_marker: Type,
        force_union: bool,
    ) -> Type {
        if Self::is_known_empty_array(arr_ty) {
            return empty_marker;
        }
        if Self::is_known_non_empty_array(arr_ty) && !force_union {
            return result_ty;
        }
        union_types(result_ty, empty_marker)
    }
}

fn is_var_named(e: &Expr, name: &str) -> bool {
    matches!(&e.kind, ExprKind::Variable(n) if n == name)
}

fn is_null_literal(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Literal(Literal::Null))
}

/// Recognizes exactly the callback shape real PHPStan's own
/// `array_filter` extension does for an explicit null check: `fn ($v) =>
/// $v !== null` (or `null !== $v`), as an arrow function or a closure
/// whose only statement is the equivalent `return` - verified via
/// dumpType(). Deliberately narrow, matching this project's own "verify
/// the exact shape, then implement exactly that" convention rather than
/// a general boolean-expression evaluator.
fn is_null_filter_callback(e: &Expr) -> bool {
    fn is_not_null_check(body: &Expr, param: &str) -> bool {
        matches!(&body.kind, ExprKind::Binary(BinOp::NotIdentical, l, r)
            if (is_var_named(l, param) && is_null_literal(r))
                || (is_var_named(r, param) && is_null_literal(l)))
    }
    match &e.kind {
        ExprKind::ArrowFn { params, body, .. } => {
            matches!(params.as_slice(), [p] if is_not_null_check(body, &p.name))
        }
        ExprKind::Closure { params, body, .. } => match (params.as_slice(), body.as_slice()) {
            ([p], [stmt]) => {
                matches!(&stmt.kind, StmtKind::Return(Some(ret_expr)) if is_not_null_check(ret_expr, &p.name))
            }
            _ => false,
        },
        _ => false,
    }
}

/// `array_filter`'s own explicit-null-filter callback (recognized by
/// `is_null_filter_callback` above) strips `Null` from the retained
/// element type, not just the accessory markers every `array_filter`
/// call already loses (`strip_accessory_markers`) - verified via
/// dumpType().
fn strip_null_from_array_element(ty: Type) -> Type {
    match ty {
        Type::Array(key, elem) => {
            Type::Array(key, Box::new(narrow_type_excluding(&elem, &Type::Null)))
        }
        other => other,
    }
}

/// Recognizes real PHPStan's own other recognized `array_filter` callback
/// shape: `fn ($v): bool => $v instanceof Target` (or the equivalent
/// single-statement closure) - returns `Target`'s own raw name string,
/// for the caller to resolve. Shares `is_not_null_check`'s own
/// arrow-fn/closure traversal shape via a local nested helper, same
/// narrow, "verify the exact shape" scope as `is_null_filter_callback`
/// above.
fn is_instanceof_filter_callback(e: &Expr) -> Option<&str> {
    fn as_instanceof_class<'a>(body: &'a Expr, param: &str) -> Option<&'a str> {
        match &body.kind {
            ExprKind::Instanceof(inner, InstanceofTarget::Name(class_name))
                if is_var_named(inner, param) =>
            {
                Some(class_name.as_str())
            }
            _ => None,
        }
    }
    match &e.kind {
        ExprKind::ArrowFn { params, body, .. } => match params.as_slice() {
            [p] => as_instanceof_class(body, &p.name),
            _ => None,
        },
        ExprKind::Closure { params, body, .. } => match (params.as_slice(), body.as_slice()) {
            ([p], [stmt]) => match &stmt.kind {
                StmtKind::Return(Some(ret_expr)) => as_instanceof_class(ret_expr, &p.name),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// Whether `target` (a bare `PREG_*`-style constant name) appears
/// anywhere in `e`'s own `BitOr`/`Grouping` chain - a lenient scan,
/// unlike `parse_preg_flags`'s own stricter "every leaf must be a
/// recognized name" requirement: an unrecognized flag alongside
/// `target` doesn't change what this is used for (`preg_split`'s own
/// list-ness/non-emptiness), so there's no need to reject the whole
/// expression over one, just answer "is `target` definitely one of
/// them" for whatever it does recognize.
/// Phase 76: `filter_var()`'s own `FILTER_VALIDATE_*` half of real
/// PHPStan's `FilterFunctionReturnTypeHelper::getFilterTypeMap()` -
/// deliberately excludes every `FILTER_SANITIZE_*` constant (they all
/// map to plain `string`, exactly what the generic stub already
/// declares, so there'd be nothing to gain by special-casing them) and
/// `FILTER_DEFAULT`/`FILTER_UNSAFE_RAW` (real PHPStan's own logic for
/// those depends on the *input* value's own type, not just the filter
/// constant - genuinely more than this narrow slice attempts).
fn filter_validate_type_for_constant(name: &str) -> Option<Type> {
    match name {
        "FILTER_VALIDATE_BOOLEAN" | "FILTER_VALIDATE_BOOL" => Some(Type::Bool),
        "FILTER_VALIDATE_INT" => Some(Type::Int),
        "FILTER_VALIDATE_FLOAT" => Some(Type::Float),
        "FILTER_VALIDATE_DOMAIN" | "FILTER_VALIDATE_REGEXP" => Some(Type::String),
        // Real PHPStan's own map: these three can never come back as the
        // empty string on success (a validated IP/email/MAC address is
        // never `''`), so it's `non-falsy-string`, not plain `string`.
        "FILTER_VALIDATE_IP"
        | "FILTER_VALIDATE_EMAIL"
        | "FILTER_VALIDATE_MAC"
        | "FILTER_VALIDATE_URL" => Some(Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonFalsyString),
        ])),
        _ => None,
    }
}

fn contains_flag_name(e: &Expr, target: &str) -> bool {
    match &e.kind {
        ExprKind::Name(name) => name.trim_start_matches('\\') == target,
        ExprKind::Binary(BinOp::BitOr, l, r) => {
            contains_flag_name(l, target) || contains_flag_name(r, target)
        }
        ExprKind::Grouping(inner) => contains_flag_name(inner, target),
        _ => false,
    }
}

/// `array_filter`'s own `instanceof`-filter callback (recognized by
/// `is_instanceof_filter_callback` above) narrows the retained element
/// type to `target` specifically - verified via dumpType().
fn narrow_array_element_type_to(ty: Type, target: &Type) -> Type {
    match ty {
        Type::Array(key, elem) => Type::Array(key, Box::new(narrow_type_to(&elem, target))),
        other => other,
    }
}

/// `array_filter($x)` with no callback at all - real PHP's own default,
/// dropping every falsy element - shares the exact same falsy-exclusion
/// rule `!empty($x)`'s own narrowing already uses, verified via
/// dumpType().
fn narrow_array_element_type_excluding_falsy(ty: Type) -> Type {
    match ty {
        Type::Array(key, elem) => Type::Array(key, Box::new(narrow_type_excluding_falsy(&elem))),
        other => other,
    }
}

/// Phase 14: drops any `Accessory` marker from `ty`, collapsing an
/// Intersection back to its own plain base - used by array functions
/// that can't guarantee they preserve list-ness/non-emptiness
/// (`array_filter`/`array_combine`/`array_unique`).
///
/// Phase 68 corpus find: a `Union` branch of `ty` (e.g. `non-empty-
/// list<T>|array{}` - the real shape a loop-conditionally-appended
/// array literal gets, one branch for "at least one append happened,"
/// one for "zero iterations") used to fall straight to the `other => ty`
/// case below untouched, silently leaving every member's own `Accessory`
/// marker in place - only a *bare* `Intersection` at the very top level
/// ever got stripped. Found via a real corpus false positive:
/// `array_values(array_filter($imageIds, is_string(...)))` flagged as a
/// no-op (`arrayValues.list`) because `array_filter`'s own 2-argument,
/// non-null/non-instanceof-callback case (which calls this expecting
/// list-ness genuinely gone) left the input's `non-empty-list<T>|
/// array{}` completely unstripped, so `array_values`'s own `is_list_like`
/// still saw it as list-shaped.
fn strip_accessory_markers(ty: Type) -> Type {
    match ty {
        Type::Intersection(members) => members
            .into_iter()
            .find(|m| !matches!(m, Type::Accessory(_)))
            .unwrap_or(Type::Mixed),
        Type::Union(members) => members
            .into_iter()
            .map(strip_accessory_markers)
            .reduce(union_types)
            .unwrap_or(Type::Mixed),
        other => other,
    }
}

/// Phase 14: rewraps `mapped_elem` in whatever `Accessory` markers
/// `input_ty` had - list-ness/non-emptiness are unaffected by mapping/
/// slicing/reversing with default arguments (none of which change key
/// structure or element count) - used by `array_map`/`array_slice`/
/// `array_reverse`'s own "preserve the input's own markers" cases.
///
/// A Piwigo-corpus round found `input_ty` arriving here as a `Union` just
/// as often as an `Intersection` - `$x = []; if (...) { foreach (...) {
/// $x[] = ...; } }` control-flow-merges to `array{}|list<T>`, which is
/// still every bit as list-like as a bare `list<T>` (`Checker::is_list_like`
/// already recognizes this exact shape), but the original `Intersection`-
/// only match silently dropped both markers the moment the input came from
/// a merge rather than a single literal/return - verified via a direct
/// repro (`array_map` over such a variable losing `list<>`-ness on its
/// result).
fn rewrap_with_accessory_markers(mapped_elem: Type, input_ty: &Type) -> Type {
    match input_ty {
        Type::Intersection(members) => {
            let mut result = vec![Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(mapped_elem),
            )];
            result.extend(
                members
                    .iter()
                    .filter(|m| matches!(m, Type::Accessory(_)))
                    .cloned(),
            );
            Type::Intersection(result)
        }
        Type::Union(_) => {
            let mut result = vec![Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(mapped_elem),
            )];
            if Checker::is_list_like(input_ty) {
                result.push(Type::Accessory(AccessoryKind::ArrayList));
            }
            if Checker::is_known_non_empty_array(input_ty) {
                result.push(Type::Accessory(AccessoryKind::NonEmptyArray));
            }
            if result.len() > 1 {
                Type::Intersection(result)
            } else {
                result.remove(0)
            }
        }
        _ => Type::Array(Box::new(Type::unknown_array_key()), Box::new(mapped_elem)),
    }
}

/// Removes any `Array`/`ArrayShape` member from `ty` - the array-ish
/// counterpart of `narrow_type_excluding`, which matches a specific value
/// rather than a whole type *kind*. Used for the `preg_replace` family,
/// where the real return type includes `array` only when `$subject` is
/// itself an array - once that's confirmed not the case, this drops the
/// always-possible-per-the-stub `array` member while leaving everything
/// else (a real declared `null`, say) untouched.
fn strip_array_member(ty: Type) -> Type {
    match ty {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .into_iter()
                .filter(|m| !matches!(m, Type::Array(_, _) | Type::ArrayShape(_)))
                .collect();
            match kept.len() {
                0 => Type::Mixed,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Array(_, _) | Type::ArrayShape(_) => Type::Mixed,
        other => other,
    }
}

/// The `json_encode`/Finding J counterpart of `strip_array_member` above -
/// identical shape, filtering `Type::False` instead of `Array`/`ArrayShape`.
fn strip_false_member(ty: Type) -> Type {
    match ty {
        Type::Union(members) => {
            let kept: Vec<Type> = members.into_iter().filter(|m| *m != Type::False).collect();
            match kept.len() {
                0 => Type::Mixed,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::False => Type::Mixed,
        other => other,
    }
}

/// Whether `e` is (or bitwise-ORs together, to any depth) a `Name`
/// resolving to `JSON_THROW_ON_ERROR` - real corpus shapes include both a
/// bare `JSON_THROW_ON_ERROR` and a multi-flag chain
/// (`JSON_HEX_TAG | JSON_HEX_AMP | ... | JSON_THROW_ON_ERROR`). Tolerates
/// a leading-backslash global name (`\JSON_THROW_ON_ERROR`), seen as-is
/// in the corpus.
fn contains_json_throw_on_error(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Name(name) => name.trim_start_matches('\\') == "JSON_THROW_ON_ERROR",
        ExprKind::Binary(BinOp::BitOr, l, r) => {
            contains_json_throw_on_error(l) || contains_json_throw_on_error(r)
        }
        ExprKind::Grouping(inner) => contains_json_throw_on_error(inner),
        _ => false,
    }
}

/// `curl_getinfo($ch, CONSTANT)`'s own real per-option return type - Phase
/// 34 Finding A originally curated ~20 of real PHPStan's own ~73-entry
/// `CurlGetinfoFunctionDynamicReturnTypeExtension` table (only the
/// constants this project's own verification corpus evidenced at the
/// time); Phase 77 transcribes the rest of that real table verbatim
/// (checked directly against
/// `vendor/phpstan-src/src/Type/Php/CurlGetinfoFunctionDynamicReturnTypeExtension.php`),
/// including its RTSP/proxy/microsecond/bignum/`_T`-suffixed variants.
/// `None` for anything else (there is no real `CURLINFO_*` constant left
/// uncovered) falls through to this project's existing generic (`mixed`)
/// behavior, unchanged.
fn curl_getinfo_component_type(name: &str) -> Option<Type> {
    let string_false = Type::Union(vec![Type::String, Type::False]);
    let int_false = Type::Union(vec![Type::Int, Type::False]);
    // `list<string>` - `CURLINFO_SSL_ENGINES`/`CURLINFO_COOKIELIST`'s own
    // real type.
    let string_list = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::String)),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    // `list<array<string, string>>` - `CURLINFO_CERTINFO`'s own real type.
    let nested_array_in_list = Type::Intersection(vec![
        Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(Type::String),
            )),
        ),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    Some(match name {
        "CURLINFO_EFFECTIVE_URL"
        | "CURLINFO_PRIMARY_IP"
        | "CURLINFO_LOCAL_IP"
        | "CURLINFO_SCHEME"
        | "CURLINFO_EFFECTIVE_METHOD" => Type::String,
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
        | "CURLINFO_APPCONNECT_TIME" => Type::Float,
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
        | "CURLINFO_RETRY_AFTER" => Type::Int,
        "CURLINFO_SSL_ENGINES" | "CURLINFO_COOKIELIST" => string_list,
        "CURLINFO_CERTINFO" => nested_array_in_list,
        "CURLINFO_POSTTRANSFER_TIME_T"
        | "CURLINFO_QUEUE_TIME_T"
        | "CURLINFO_USED_PROXY"
        | "CURLINFO_HTTPAUTH_USED"
        | "CURLINFO_PROXYAUTH_USED"
        | "CURLINFO_CONN_ID" => int_false,
        "CURLINFO_PRIVATE" => Type::Mixed,
        _ => return None,
    })
}

/// `get_debug_type()`'s object case - shared free function (not a
/// `Checker` method) so `Checker::get_debug_type_result`'s own union
/// handling below can call it once per member without fighting the
/// borrow checker over `&self`.
fn get_debug_type_for(ty: &Type, registry: &Registry) -> Option<Type> {
    match ty {
        Type::Null => Some(Type::StringLiteral("null".to_string())),
        Type::Bool | Type::True | Type::False => Some(Type::StringLiteral("bool".to_string())),
        Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => {
            Some(Type::StringLiteral("int".to_string()))
        }
        Type::Float | Type::FloatLiteral(_) => Some(Type::StringLiteral("float".to_string())),
        Type::String | Type::StringLiteral(_) => Some(Type::StringLiteral("string".to_string())),
        Type::Array(_, _) | Type::ArrayShape(_) => Some(Type::StringLiteral("array".to_string())),
        Type::Object(class, _) => Some(
            if registry
                .classes
                .get(class)
                .is_some_and(|info| info.is_final)
            {
                Type::StringLiteral(class.clone())
            } else {
                Type::String
            },
        ),
        // Phase 79: an enum is always implicitly final (real PHP forbids
        // `extends` on one entirely), so a specific case's own class name
        // is always the exact runtime answer - same rule as the ordinary
        // `Object` arm's own `is_final` branch above, just unconditional.
        Type::EnumCase(fqn, _) => Some(Type::StringLiteral(fqn.clone())),
        Type::Intersection(members) => members.iter().find_map(|m| get_debug_type_for(m, registry)),
        Type::Union(members) => {
            let mut iter = members.iter();
            let mut acc = get_debug_type_for(iter.next()?, registry)?;
            for m in iter {
                acc = union_types(acc, get_debug_type_for(m, registry)?);
            }
            Some(acc)
        }
        _ => None,
    }
}

/// `gettype()`'s own real per-kind runtime-string answer (real PHP's own
/// historically-quirky names: `"boolean"`, `"integer"`, `"double"`,
/// `"NULL"` - not `"bool"`/`"int"`/`"float"`/`"null"`, which is what
/// `get_debug_type()` returns instead). `None` for anything this project
/// can't confidently classify (`Mixed`, a bare unresolved `Callable`/
/// `TemplateParam`, ...) - real PHPStan itself falls back to the full
/// 10-name union for that case; this project's own "don't guess"
/// convention leaves today's plain `string` unchanged instead, matching
/// `curl_getinfo_component_type`'s own `None`-means-"unchanged" contract.
fn gettype_string_for(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::String | Type::StringLiteral(_) => Some("string"),
        Type::Array(_, _) | Type::ArrayShape(_) => Some("array"),
        Type::Bool | Type::True | Type::False => Some("boolean"),
        Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => Some("integer"),
        Type::Float | Type::FloatLiteral(_) => Some("double"),
        Type::Null => Some("NULL"),
        Type::Object(_, _) | Type::EnumCase(_, _) => Some("object"),
        Type::Intersection(members) => members.iter().find_map(gettype_string_for),
        _ => None,
    }
}

fn gettype_result(ty: &Type) -> Option<Type> {
    match ty {
        Type::Union(members) => {
            let mut iter = members.iter();
            let mut acc = Type::StringLiteral(gettype_string_for(iter.next()?)?.to_string());
            for m in iter {
                acc = union_types(acc, Type::StringLiteral(gettype_string_for(m)?.to_string()));
            }
            Some(acc)
        }
        other => gettype_string_for(other).map(|s| Type::StringLiteral(s.to_string())),
    }
}

/// Whether `s` is exactly one Unicode scalar value - `idate()`'s own
/// format argument is only ever meaningfully a single character (real
/// PHP itself only reads the first byte).
fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(c)
}

/// `idate()`'s own real per-character numeric bound - transcribed from
/// real PHPStan's own `IdateFunctionReturnTypeHelper`
/// (`vendor/phpstan-src/src/Type/Php/IdateFunctionReturnTypeHelper.php`),
/// using this project's own `Type::IntRange` in place of that file's
/// `IntegerRangeType::fromInterval`. A handful of real `idate()` format
/// characters (`B`/`o`/`U`/`Y`/`Z`) are real, unbounded integers there -
/// deliberately left out of this table (returning `None`, i.e. "no
/// refinement") rather than mapped to plain `Int`, since `idate()`'s own
/// stub already gives `int|false` and an unbounded `Int` here would only
/// ever narrow away the `false` half for a recognized-but-unbounded
/// character, a real claim this project can't back for every one of
/// those five (real PHP does raise a `ValueError`/return `false` for a
/// handful of other, truly unrecognized characters this table also
/// doesn't distinguish from the unbounded five) - kept conservative.
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

/// `date()`'s own per-character static classification: does this
/// recognized format character always produce digits-only text (`Some(
/// true)`), always produce text that isn't guaranteed numeric (`Some(
/// false)`, month/day names, `am`/`pm`, timezone abbreviations, the `st`/
/// `nd`/`rd`/`th` ordinal suffix, ...), or is it not a character this
/// table classifies at all (`None` - an escape sequence or literal
/// punctuation `date()` would echo back verbatim, which this project has
/// no way to evaluate without actually running `date()`)? Transcribed
/// from real PHPStan's own `DateFunctionReturnTypeHelper` (`vendor/
/// phpstan-src/src/Type/Php/DateFunctionReturnTypeHelper.php`)'s own
/// `switch` cases, collapsed from "the exact literal/numeric-string type"
/// down to this coarser yes/no (Phase 77's own scope: a static table, not
/// a runtime `date()` simulation).
fn date_format_char_is_always_numeric(c: char) -> Option<bool> {
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

/// `date($format)`'s own real return type for a literal `$format` -
/// `None` (today's plain `string`, unchanged) unless every character is
/// one `date_format_char_is_always_numeric` recognizes; a non-empty
/// literal format then always produces non-empty text (`Accessory::
/// NonEmptyString`), additionally numeric text
/// (`Accessory::NumericString`) only when every character does.
fn date_format_return_type(format: &str) -> Option<Type> {
    if format.is_empty() {
        // Real `date('')` returns `''` - a degenerate literal this
        // project already infers precisely via the generic stub path
        // (`ConstantStringType`-equivalent isn't modeled for `date()`
        // itself here), not worth a special case.
        return None;
    }
    let mut all_numeric = true;
    for c in format.chars() {
        match date_format_char_is_always_numeric(c)? {
            true => {}
            false => all_numeric = false,
        }
    }
    let mut members = vec![Type::String, Type::Accessory(AccessoryKind::NonEmptyString)];
    if all_numeric {
        members.push(Type::Accessory(AccessoryKind::NumericString));
    }
    Some(Type::Intersection(members))
}

/// `pathinfo($path)`'s own real 1-arg array shape - `dirname`/`basename`/
/// `filename` always present, `extension` only when the path actually has
/// a dot in it (simplified here to always-optional: this project has no
/// way to know from `$path`'s own `Type` alone whether a literal path
/// contains one, unlike real PHPStan's own `isNonEmptyString()`-driven
/// `dirname`-optionality check, which this project mirrors for the same
/// reason on that field too - see the 1-arg match arm above referencing
/// this function).
fn pathinfo_array_shape() -> Type {
    Type::ArrayShape(vec![
        ("dirname".to_string(), Type::String, false),
        ("basename".to_string(), Type::String, false),
        ("extension".to_string(), Type::String, true),
        ("filename".to_string(), Type::String, false),
    ])
}

/// `parse_url($url)`'s own real "all components" array shape - every
/// field genuinely optional (none of `scheme`/`host`/`user`/`pass`/
/// `path`/`query`/`fragment`/`port` are guaranteed present in a real
/// parsed URL), `port` alone typed `IntRange(0, 65535)` rather than plain
/// `String` like the rest.
fn parse_url_components_array_shape() -> Type {
    Type::ArrayShape(vec![
        ("scheme".to_string(), Type::String, true),
        ("host".to_string(), Type::String, true),
        (
            "port".to_string(),
            Type::IntRange(Some(0), Some(65535)),
            true,
        ),
        ("user".to_string(), Type::String, true),
        ("pass".to_string(), Type::String, true),
        ("path".to_string(), Type::String, true),
        ("query".to_string(), Type::String, true),
        ("fragment".to_string(), Type::String, true),
    ])
}

/// `parse_url($url, PHP_URL_*)`'s own real per-component return type -
/// transcribed from real PHPStan's own
/// `ParseUrlFunctionDynamicReturnTypeExtension` (`vendor/phpstan-src/src/
/// Type/Php/ParseUrlFunctionDynamicReturnTypeExtension.php`)'s own
/// `$componentTypesPairedConstants` table, minus that real extension's
/// own `AccessoryLowercaseStringType` refinement for a lowercase `$url`
/// (this project has no equivalent accessory kind to carry that
/// distinction - see `types::AccessoryKind`).
fn parse_url_component_type(name: &str) -> Option<Type> {
    let string_false_null = Type::Union(vec![Type::String, Type::False, Type::Null]);
    let port_false_null = Type::Union(vec![
        Type::IntRange(Some(0), Some(65535)),
        Type::False,
        Type::Null,
    ]);
    Some(match name {
        "PHP_URL_SCHEME" | "PHP_URL_HOST" | "PHP_URL_USER" | "PHP_URL_PASS" | "PHP_URL_PATH"
        | "PHP_URL_QUERY" | "PHP_URL_FRAGMENT" => string_false_null,
        "PHP_URL_PORT" => port_false_null,
        _ => return None,
    })
}

/// One `sscanf()` conversion specifier - either a single `[cdeEfosux]`
/// character, or a `[...]` bracket character-class (real `sscanf()`
/// treats every bracket class the same as `%s` for typing purposes: it
/// always captures a substring, never a number).
enum SscanfSpecifier {
    Char(char),
    Bracket,
}

/// Walks a literal `sscanf()`/`fscanf()` format string for every real
/// conversion specifier (`%d`, `%5s`, `%[abc]`, ...) it contains, in
/// order - a hand-rolled scan standing in for real PHPStan's own regex
/// (`/%(\d*)(\[[^\]]+\]|[cdeEfosux]{1})/`), since this project has no
/// regex engine available (see `Cargo.toml`). `None` when the format
/// contains no recognized specifier at all (today's plain `array|null`
/// stays unchanged) - an unterminated `%[...` bracket at the very end of
/// the string stops the scan there (matching the regex's own behavior:
/// it simply doesn't match past the point it can't close the bracket).
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
        } else if "cdeEfosux".contains(chars[i]) {
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

/// One `sscanf()` field's own real type, given its own specifier and any
/// explicit field-width digits - every field is nullable (a real trailing
/// unmatched specifier leaves that slot `null`), mirroring real PHPStan's
/// own `TypeCombinator::addNull` on each field.
fn sscanf_field_type(length: Option<u32>, specifier: &SscanfSpecifier) -> Type {
    let base = match specifier {
        SscanfSpecifier::Char('d' | 'o' | 'u' | 'x') => Type::Int,
        SscanfSpecifier::Char('e' | 'E' | 'f') => Type::Float,
        SscanfSpecifier::Char(_) | SscanfSpecifier::Bracket => match length {
            Some(n) if n > 1 => Type::Intersection(vec![
                Type::String,
                Type::Accessory(AccessoryKind::NonFalsyString),
            ]),
            Some(_) => Type::Intersection(vec![
                Type::String,
                Type::Accessory(AccessoryKind::NonEmptyString),
            ]),
            None => Type::String,
        },
    };
    Type::Union(vec![base, Type::Null])
}

/// `sscanf($string, $format)`'s own real numeric-keyed result-array
/// fields, one per conversion specifier `parse_sscanf_format` finds, in
/// order - keyed by each field's own canonical string form of its
/// integer index (`"0"`, `"1"`, ...), exactly how `Type::ArrayShape`
/// itself already represents an int-keyed array elsewhere in this project
/// (see `literal_string_key`).
fn sscanf_result_fields(format: &str) -> Option<Vec<(String, Type, bool)>> {
    let specs = parse_sscanf_format(format)?;
    Some(
        specs
            .into_iter()
            .enumerate()
            .map(|(i, (length, specifier))| {
                (i.to_string(), sscanf_field_type(length, &specifier), false)
            })
            .collect(),
    )
}

/// `stat()`/`lstat()`/`fstat()`/`ssh2_sftp_stat()`'s own real, shared
/// 13-key shape (transcribed from real PHPStan's own
/// `StatDynamicReturnTypeExtension` - `vendor/phpstan-src/src/Type/Php/
/// StatDynamicReturnTypeExtension.php`'s own `$keys`) - every value a
/// real `int`. Real `stat()`'s own array is actually double-keyed, each
/// value reachable by both its numeric position and its own string name;
/// only the string-keyed half is modeled here, the way real code
/// actually reads it (`$stat['size']`, not `$stat[7]`), matching this
/// project's own established "model the evidenced access pattern, not
/// the full PHP-internal shape" convention (`curl_getinfo`'s own 1-arg
/// "all components" form is skipped for the identical reason).
fn stat_array_shape() -> Type {
    Type::ArrayShape(
        [
            "dev", "ino", "mode", "nlink", "uid", "gid", "rdev", "size", "atime", "mtime", "ctime",
            "blksize", "blocks",
        ]
        .into_iter()
        .map(|key| (key.to_string(), Type::Int, false))
        .collect(),
    )
}

/// `dio_stat()`'s own real 13-key shape - transcribed from real PHPStan's
/// own `DioStatDynamicFunctionReturnTypeExtension` (`vendor/phpstan-src/
/// src/Type/Php/DioStatDynamicFunctionReturnTypeExtension.php`), the same
/// values-are-all-`int` shape as `stat_array_shape` above under `dio_stat`'s
/// own distinct key names.
fn dio_stat_array_shape() -> Type {
    Type::ArrayShape(
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
        .map(|key| (key.to_string(), Type::Int, false))
        .collect(),
    )
}

/// `list<non-empty-string>` - `hash_algos()`/`openssl_get_cipher_methods(
/// )`/`mb_list_encodings()`'s own real, evidenced return shape regardless
/// of host/PHP version/installed extensions (see the match arm above
/// referencing this function for why this project stops there, rather
/// than also replicating what real PHPStan additionally does with these
/// three functions' own real output).
fn list_of_non_empty_strings() -> Type {
    Type::Intersection(vec![
        Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Intersection(vec![
                Type::String,
                Type::Accessory(AccessoryKind::NonEmptyString),
            ])),
        ),
        Type::Accessory(AccessoryKind::ArrayList),
        Type::Accessory(AccessoryKind::NonEmptyArray),
    ])
}
