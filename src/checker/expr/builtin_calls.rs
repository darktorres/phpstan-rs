use crate::ast::*;
use crate::checker::{
    Checker, Locals, literal_string_key, narrow_type_excluding, narrow_type_excluding_falsy,
    narrow_type_to, resolve_call_args,
};
use crate::registry::FunctionSig;
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
                        Type::Array(Box::new(elem)),
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
                    Type::Array(Box::new(elem)),
                    Type::Accessory(AccessoryKind::ArrayList),
                ]);
                return union_types(list_ty, Type::False);
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
                        Type::Array(Box::new(elem_ty)),
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
                    let mut members = vec![Type::Array(Box::new(elem_ty))];
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
                return Type::Array(Box::new(self.infer_expr(&args[2].value, locals)));
            }
            "array_fill_keys" if args.len() == 2 => {
                return Type::Array(Box::new(self.infer_expr(&args[1].value, locals)));
            }
            // Splits one array into several same-shaped ones - the
            // result is an array *of* arrays, each holding the original
            // element type.
            "array_chunk" if args.len() == 2 || args.len() == 3 => {
                if let Some(elem_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    return Type::Array(Box::new(Type::Array(Box::new(elem_ty))));
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
                        Type::Array(Box::new(key_ty)),
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
                        Type::Array(Box::new(Type::Mixed)),
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
                        let arr_ty = Type::Array(Box::new(field_ty));
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
                    return Type::Array(Box::new(Type::String));
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
                    Type::Array(Box::new(Type::String)),
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
                        Type::Array(Box::new(Type::String)),
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
            _ => {}
        }
        let resolved = resolve_call_args(args, &sig.params);
        self.infer_call_return_type(sig, &resolved, locals)
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
        Type::Array(elem) => Type::Array(Box::new(narrow_type_excluding(&elem, &Type::Null))),
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
        Type::Array(elem) => Type::Array(Box::new(narrow_type_to(&elem, target))),
        other => other,
    }
}

/// `array_filter($x)` with no callback at all - real PHP's own default,
/// dropping every falsy element - shares the exact same falsy-exclusion
/// rule `!empty($x)`'s own narrowing already uses, verified via
/// dumpType().
fn narrow_array_element_type_excluding_falsy(ty: Type) -> Type {
    match ty {
        Type::Array(elem) => Type::Array(Box::new(narrow_type_excluding_falsy(&elem))),
        other => other,
    }
}

/// Phase 14: drops any `Accessory` marker from `ty`, collapsing an
/// Intersection back to its own plain base - used by array functions
/// that can't guarantee they preserve list-ness/non-emptiness
/// (`array_filter`/`array_combine`/`array_unique`).
fn strip_accessory_markers(ty: Type) -> Type {
    match ty {
        Type::Intersection(members) => members
            .into_iter()
            .find(|m| !matches!(m, Type::Accessory(_)))
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
            let mut result = vec![Type::Array(Box::new(mapped_elem))];
            result.extend(
                members
                    .iter()
                    .filter(|m| matches!(m, Type::Accessory(_)))
                    .cloned(),
            );
            Type::Intersection(result)
        }
        Type::Union(_) => {
            let mut result = vec![Type::Array(Box::new(mapped_elem))];
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
        _ => Type::Array(Box::new(mapped_elem)),
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
                .filter(|m| !matches!(m, Type::Array(_) | Type::ArrayShape(_)))
                .collect();
            match kept.len() {
                0 => Type::Mixed,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Array(_) | Type::ArrayShape(_) => Type::Mixed,
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

/// `curl_getinfo($ch, CONSTANT)`'s own real per-option return type - a
/// curated subset of real PHPStan's own ~70-entry
/// `CurlGetinfoFunctionDynamicReturnTypeExtension` table, covering the
/// commonly-used constants real code actually reaches for (every real
/// call site in this project's own verification corpus uses one of
/// these) rather than transcribing the full table, including its RTSP/
/// proxy/microsecond/bignum variants, for entries nothing here gives any
/// evidence about. `None` for anything else falls through to this
/// project's existing generic (`mixed`) behavior, unchanged.
fn curl_getinfo_component_type(name: &str) -> Option<Type> {
    let string_false = Type::Union(vec![Type::String, Type::False]);
    Some(match name {
        "CURLINFO_EFFECTIVE_URL"
        | "CURLINFO_PRIMARY_IP"
        | "CURLINFO_LOCAL_IP"
        | "CURLINFO_SCHEME"
        | "CURLINFO_EFFECTIVE_METHOD" => Type::String,
        "CURLINFO_REDIRECT_URL" | "CURLINFO_HEADER_OUT" | "CURLINFO_CONTENT_TYPE" => string_false,
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
        | "CURLINFO_CONTENT_LENGTH_UPLOAD" => Type::Float,
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
        | "CURLINFO_HTTP_VERSION" => Type::Int,
        "CURLINFO_PRIVATE" => Type::Mixed,
        _ => return None,
    })
}
