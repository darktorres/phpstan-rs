use crate::ast::*;
use crate::checker::{
    Checker, Locals, build_matched_shape, int_range_bounds, literal_string_value,
    normalize_int_range, unwrap_grouping,
};
use crate::regex_groups;
use crate::registry::{AssertTarget, FunctionSig};
use crate::token::Span;
use crate::types::{AccessoryKind, Type};
mod accessory_range;
mod array_shape;
mod assertions;
mod predicates;
mod type_ops;

use accessory_range::{
    comparison_narrowed_bounds, int_comparison_operands, numeric_bounds_for_narrowing,
    upgrade_array_to_list, upgrade_array_to_non_empty, upgrade_string_to_numeric,
    upgrade_string_with_accessory,
};
pub(crate) use array_shape::{
    array_value_type_for_key, is_unsealed_shape, narrow_array_shape_by_key_presence,
    narrow_type_excluding_falsy,
};
use predicates::preg_match_condition;
pub(crate) use predicates::{
    is_real_variable_key, narrowing_key, narrowing_type_for_predicate, purge_stale_narrowing_for,
};
use type_ops::narrow_instanceof_true;
pub(crate) use type_ops::{narrow_type_excluding, narrow_type_to};

/// `count($x)`'s own operand-order detection for the count-comparison
/// narrowing below - same shape as `int_comparison_operands`, but keyed
/// on "is this side a single-argument `count(...)` call" rather than
/// `narrowing_key` (a bare `count(...)` call isn't itself a narrowable
/// key). Returns the call's own argument and whether `count(...)` was
/// written on the left (`count($x) > 0`, the already-handled form) or
/// the right (`0 < count($x)`, previously unhandled).
fn count_call_operand<'e>(l: &'e Expr, r: &'e Expr) -> Option<(&'e Expr, bool)> {
    fn as_count_arg(e: &Expr) -> Option<&Expr> {
        if let ExprKind::Call(callee, args) = &e.kind
            && let ExprKind::Name(name) = &callee.kind
            && name.trim_start_matches('\\') == "count"
            && let [single_arg] = args.as_slice()
        {
            Some(&single_arg.value)
        } else {
            None
        }
    }
    as_count_arg(l)
        .map(|a| (a, true))
        .or_else(|| as_count_arg(r).map(|a| (a, false)))
}

/// Flips a comparison operator to the operand-swapped equivalent - `0 <
/// count($x)` narrows the same way `count($x) > 0` already does, once
/// the operator itself is mirrored to match.
fn mirror_comparison(op: &BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Gt => BinOp::Lt,
        BinOp::LtEq => BinOp::GtEq,
        BinOp::GtEq => BinOp::LtEq,
        other => other.clone(),
    }
}

impl<'a> Checker<'a> {
    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    pub(crate) fn apply_narrowing(&self, cond: &Expr, locals: &mut Locals, branch_true: bool) {
        // Phase 12 Finding E: `preg_match(...)` (bare, or compared
        // against a literal 0/1) doesn't otherwise match any existing
        // narrowing shape below, so this doesn't shadow anything -
        // verified directly, not assumed: the `Binary(op, l, r)` arm
        // further down *does* also run afterward for a comparison form,
        // dispatching to `apply_equality_narrowing`, but that function's
        // own `narrowing_key(l)`/`narrowing_key(r)` calls have no arm for
        // `ExprKind::Call` at all, so it's a genuine no-op for this
        // shape, not a silent double-narrowing.
        if let Some((pattern_arg, matches_arg, true_means_matched)) = preg_match_condition(cond)
            && let ExprKind::Variable(matches_name) = &matches_arg.kind
            && let Some(pattern_str) = literal_string_value(pattern_arg)
            && let Some(parsed) = regex_groups::parse_capture_groups(&pattern_str)
        {
            let narrowed = if branch_true == true_means_matched {
                build_matched_shape(
                    &parsed.groups,
                    &parsed.alternation_branch_counts,
                    &parsed.alternation_wrapped_in_optional_quantifier,
                    &Type::String,
                    false,
                )
            } else {
                Type::ArrayShape(vec![])
            };
            purge_stale_narrowing_for(locals, matches_name);
            locals.insert(matches_name.clone(), narrowed);
        }
        // Phase 71 Item 4: `function_exists('foo')`'s own true branch
        // proves `foo` is callable for the rest of that branch - a
        // disclosed, project-specific adaptation (this project's own
        // registry, unlike real PHPStan's live reflection, can genuinely
        // lack a symbol that still exists at runtime) mirroring the
        // already-shipped `InnerFunctionRule` declaration-side adaptation.
        // Only the exact literal-string-argument shape is recognized - a
        // computed argument never populates the guard, matching this
        // project's usual "don't guess" leniency. The `&&` arm below
        // already recurses into this for free (`if (function_exists('foo')
        // && ...)`).
        if branch_true
            && let ExprKind::Call(callee, args) = &unwrap_grouping(cond).kind
            && let ExprKind::Name(fn_name) = &unwrap_grouping(callee).kind
            && fn_name
                .rsplit('\\')
                .next()
                .unwrap_or(fn_name)
                .eq_ignore_ascii_case("function_exists")
            && let [guarded_arg] = args.as_slice()
            && let Some(guarded_name) = literal_string_value(&guarded_arg.value)
        {
            locals
                .guarded_functions
                .insert(guarded_name.trim_start_matches('\\').to_ascii_lowercase());
        }
        match &cond.kind {
            // `&&` narrows the "both true" branch by composing each side
            // in order (the right operand's own narrowing can rely on the
            // left's, same as check_expr's short-circuit-aware handling)
            // - the false branch would need "at least one is false, but
            // not sure which", which isn't a narrowing this can express,
            // so it's left alone, same conservative choice `||` mirrors
            // for its own unrepresentable branch below.
            ExprKind::Binary(BinOp::And, l, r) if branch_true => {
                self.apply_narrowing(l, locals, true);
                self.apply_narrowing(r, locals, true);
            }
            ExprKind::Binary(BinOp::Or, l, r) if !branch_true => {
                self.apply_narrowing(l, locals, false);
                self.apply_narrowing(r, locals, false);
            }
            // Phase 34 Finding B: "at least one is true, but not sure
            // which" isn't representable *in general* - but when every
            // operand is a direct predicate-table check on the identical
            // expression, it narrows precisely to the union of what each
            // operand would give alone (`is_int($x) || is_string($x)`
            // being true means `$x` is exactly `int|string`, not "give
            // up"). `or_chain_narrowing_target` is the one doing the
            // actual restriction-checking; this arm only acts when it
            // succeeds.
            ExprKind::Binary(BinOp::Or, ..) if branch_true => {
                if let Some((key, target_ty, repr_expr)) = self.or_chain_narrowing_target(cond) {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, narrow_type_to(self.registry, &current, &target_ty));
                }
            }
            // A round of Piwigo-corpus bucketing found a real, common
            // idiom this project's own "Known limitations" already
            // named as needing "its own sizable narrowing-engine
            // feature": `if ((! is_int($a) && ! is_string($a)) || (!
            // is_int($b) && ! is_string($b)) || ...) { continue; }`
            // (real shape, `CommentRepository::findAllWithConditions`/
            // `findList`, 7 sites) - narrowing several *different*
            // variables at once through one combined condition. Turned
            // out not to need that: the `Or ... if !branch_true` arm
            // just above already decomposes an arbitrarily-long `||`
            // chain into independent per-leaf `apply_narrowing` calls
            // (De Morgan on the *outer* `||`) - each landing here, on
            // exactly one `&&`-of-negated-predicates leaf, one variable
            // at a time. The only missing piece was teaching `&&`'s own
            // false branch to finish the same De Morgan step one level
            // down: `!(!is_int($v) && !is_string($v))` is `is_int($v)
            // || is_string($v)`, which `and_chain_narrowing_target`
            // computes by reusing `or_chain_narrowing_target` on each
            // un-negated leaf directly (double negation), then this
            // arm narrows exactly the way the sibling `Or ... if
            // branch_true` arm above already does for a literal `||`.
            // Every leaf here is independent (a different key each),
            // so no "different keys in one condition" support was
            // needed at this level at all - each And-term simply lands
            // on its own call to this same arm, mutating the one
            // shared `locals` for its own key only.
            ExprKind::Binary(BinOp::And, ..) if !branch_true => {
                if let Some((key, target_ty, repr_expr)) = self.and_chain_narrowing_target(cond) {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, narrow_type_to(self.registry, &current, &target_ty));
                }
            }
            // Deliberately `===`/`!==` only, not `==`/`!=` - a loose
            // comparison matches PHP's own type-juggling table (`0 ==
            // false`, `0 == null`, `"0" == 0`, ...), a much wider set of
            // values than the literal compared-to one, so narrowing a
            // variable down to exactly that value on a loose match would
            // discard every other value `==` could also have matched.
            ExprKind::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical);
                let is_neq = matches!(op, BinOp::NotIdentical);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                } else if let Some((count_arg, count_on_left)) = count_call_operand(l, r)
                    && let Some(key) = narrowing_key(count_arg)
                    && let bound_expr = if count_on_left { r } else { l }
                    && let effective_op = if count_on_left {
                        op.clone()
                    } else {
                        mirror_comparison(op)
                    }
                    && let ExprKind::Literal(Literal::Int(n)) = &bound_expr.kind
                    && ((branch_true
                        && ((matches!(effective_op, BinOp::Gt) && *n == 0)
                            || (matches!(effective_op, BinOp::GtEq) && *n == 1)))
                        // Real corpus find (Piwigo's own `ImageDerivativeController::
                        // parseCustomParams`): `if (count($x) < 1) { $this
                        // ->ierror(...); }` (a real, `never`-typed early-
                        // exit idiom) proves the exact same "`$x` is non-
                        // empty" fact on the surviving path, just phrased
                        // as the *false* branch of the mirrored comparison
                        // (`count($x) < 1` false, or `count($x) <= 0`
                        // false, both mean `count($x) >= 1`) - the
                        // reversed-operand form (`1 > count($x)`) is now
                        // handled too, via `count_call_operand`/
                        // `mirror_comparison` above.
                        || (!branch_true
                            && ((matches!(effective_op, BinOp::Lt) && *n == 1)
                                || (matches!(effective_op, BinOp::LtEq) && *n == 0))))
                {
                    // Phase 14 (+ follow-up): `count($x) > 0`/`count($x) >=
                    // 1` proves `$x` non-empty - now handles both operand
                    // orders (`count($x) > 0` and `0 < count($x)`), via
                    // `count_call_operand`'s own left/right detection and
                    // `mirror_comparison` canonicalizing the operator back
                    // to the `count(...)`-on-the-left shape below.
                    let current = self.current_narrowed_type(count_arg, &key, locals);
                    locals.insert(key, upgrade_array_to_non_empty(&current));
                } else if matches!(op, BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq)
                    && let Some((keyed_expr, bound_expr, keyed_on_left)) =
                        int_comparison_operands(l, r)
                    && let Some(key) = narrowing_key(keyed_expr)
                {
                    // Phase 55 follow-up: `$x < N`/`$x >= N`/... (either
                    // operand order) narrows `$x`'s own int range - scoped
                    // directly against real PHPStan's own `IntegerRangeType
                    // ::getSmallerType`/`getGreaterType` family
                    // (`vendor/phpstan-src`), deliberately simplified: real
                    // PHPStan's version also has to subtract `true`/`null`/
                    // `false` per PHP's own loose-comparison rules for `<`/
                    // `>`, which this project doesn't model (same "no loose-
                    // comparison modeling" precedent the `===`/`!==`-only
                    // equality arm above already holds) - so this only acts
                    // when both the narrowed side's *current* type and the
                    // other side's *inferred* type are already unambiguously
                    // numeric (a bare `Type::Int` counts as an unbounded
                    // range here, unlike `int_range_bounds` itself), and the
                    // other side is a single literal value, not a genuine
                    // range (matching the `count() > 0` arm's own "exactly
                    // this shape" scoping just above, not general range-vs-
                    // range subtraction).
                    let current = self.current_narrowed_type(keyed_expr, &key, locals);
                    if let Some((cur_min, cur_max)) = numeric_bounds_for_narrowing(&current) {
                        let bound_ty = self.infer_expr(bound_expr, locals);
                        if let Some((Some(bound), Some(bound_max))) = int_range_bounds(&bound_ty)
                            && bound == bound_max
                            && let Some((new_min, new_max)) = comparison_narrowed_bounds(
                                op,
                                keyed_on_left,
                                branch_true,
                                cur_min,
                                cur_max,
                                bound,
                            )
                        {
                            locals.insert(key, normalize_int_range(new_min, new_max));
                        }
                    }
                }
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            ExprKind::Grouping(inner) => self.apply_narrowing(inner, locals, branch_true),
            // Phase 26: `(bool) $x`'s own truthiness is exactly `$x`'s -
            // PHP's cast doesn't change which values are truthy/falsy, it
            // just makes the check explicit (a common, idiomatic style:
            // `if (! (bool) $fp) { throw ...; }` after `fopen()`) - so
            // this narrows precisely as if the cast weren't there at all.
            // Any other cast (`(int)`, `(string)`, ...) genuinely changes
            // the value and isn't narrowable this way, so it's left alone.
            ExprKind::Cast(kind, inner) if kind == "bool" => {
                self.apply_narrowing(inner, locals, branch_true);
            }
            ExprKind::Call(callee, args) => {
                // Phase 22: `narrowing_key`, not a bare `ExprKind::
                // Variable` match - without this, `is_string($arr['key'])`
                // (or `empty($arr['key'])`) never narrowed `$arr['key']`
                // at all, the array-index counterpart of the exact gap
                // the `instanceof` arm below already covers for property
                // chains (`$arg->value instanceof Variable`).
                if let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice())
                    && let Some(key) = narrowing_key(&single_arg.value)
                {
                    // Phase 55: a fully-qualified call (`\is_array($x)`,
                    // `\count($x)`, ...) keeps its leading backslash in
                    // `ExprKind::Name` - unlike ordinary call resolution
                    // (`name_ctx::resolve_function_candidates`, used for
                    // every real argument/arity check), every name
                    // comparison in this whole match arm was a bare
                    // string equality with no normalization, so a real,
                    // common defensive-global-namespace style (pervasive
                    // in Rector-prefixed vendor code, real shape:
                    // `\is_array($value)` nested inside an
                    // `is_iterable($value)` guard) silently skipped
                    // narrowing entirely.
                    let name = name.trim_start_matches('\\');
                    if name == "empty" {
                        // `empty($x)` is PHP's own falsy check - only the
                        // branch where it evaluated `false` (`$x` proven
                        // truthy) is safely narrowable, the same asymmetry
                        // as a bare truthiness check.
                        if !branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding_falsy(&current));
                        }
                    } else if name == "isset" {
                        // Phase 23: `isset($x)`'s truthy branch means
                        // specifically "not null" - it's true for `0`,
                        // `''`, `false`, an empty array, every falsy-but-
                        // non-null value - so this narrows only `Null`
                        // away, unlike `empty()`'s full falsy exclusion
                        // above. The false branch isn't safely narrowable
                        // to anything more specific, the same asymmetry
                        // `empty`/the truthy fallback below already accept.
                        if branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding(&current, &Type::Null));
                        }
                    } else if name == "count" && branch_true {
                        // Phase 55: a bare `if (count($x))`/`while
                        // (count($x))` truthiness check, not the `> 0`/
                        // `>= 1` comparison form the `Binary` arm above
                        // already handles - `count()` is never negative,
                        // so a truthy result means exactly the same
                        // "provably non-empty" fact. The false branch
                        // (`count($x) === 0`) isn't narrowed here, the
                        // same asymmetry the comparison form's own arm
                        // already accepts (`upgrade_array_to_non_empty`
                        // has no "prove empty" counterpart).
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        locals.insert(key, upgrade_array_to_non_empty(&current));
                    } else if name == "array_is_list" && branch_true {
                        // Phase 55's own still-open list: a real, named
                        // gap - webmozart/assert's own `Assert::isList()`
                        // (a real, common assertion-library idiom) proves
                        // list-ness via `is_array($x) && array_is_list
                        // ($x)`, the same shape `count($x)`'s own
                        // truthiness upgrade just above already covers
                        // for non-emptiness. Only the true branch is
                        // narrowable - failing `array_is_list()` proves
                        // nothing more specific than "not proven a list",
                        // the same asymmetry `count`/`empty` already
                        // accept above.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        locals.insert(key, upgrade_array_to_list(&current));
                    } else if name == "is_numeric" {
                        // Phase 32 Finding G: deliberately its own arm,
                        // not a `narrowing_type_for_predicate` table entry
                        // - that table's single target type feeds *both*
                        // directions through the same dispatch below,
                        // which is sound for `is_scalar`/`is_string`/etc.
                        // (a string is unconditionally scalar/a-string
                        // either way) but not for `is_numeric`, where a
                        // string only *sometimes* qualifies: excluding a
                        // shared "int|float|string" target on the false
                        // branch would wrongly drop `String` entirely,
                        // even though a genuinely non-numeric string
                        // (`"hello"`) is exactly the ordinary case that
                        // makes `!is_numeric` true. The two directions
                        // need different exclusion sets instead.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            // Bool/True/False/Null can never satisfy
                            // is_numeric - unconditionally safe to drop.
                            // A kept String (bare, or inside the
                            // resulting Union) is upgraded to
                            // numeric-string (Phase 14 - verified
                            // against real PHPStan's own dumpType():
                            // `string $x` under is_numeric($x) narrows
                            // to plain numeric-string, `mixed $x`
                            // narrows to int|float|numeric-string).
                            let excluded = [Type::Bool, Type::True, Type::False, Type::Null]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                });
                            upgrade_string_to_numeric(&excluded)
                        } else {
                            // Int/Float are unconditionally numeric, so
                            // failing is_numeric proves the value wasn't
                            // one - nothing else can be dropped this way.
                            [Type::Int, Type::Float]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                })
                        };
                        locals.insert(key, narrowed);
                    } else if let Some(target_ty) = narrowing_type_for_predicate(name) {
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            narrow_type_to(self.registry, &current, &target_ty)
                        } else {
                            narrow_type_excluding(&current, &target_ty)
                        };
                        locals.insert(key, narrowed);
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "in_array"
                    && let [needle, haystack, strict] = args.as_slice()
                    // Phase 33 Finding 2: only the strict, literal-array
                    // form real PHPStan's own `InArrayFunctionTypeSpecifyingExtension`
                    // narrows - a loose comparison's type-juggling would
                    // make the real safe target wider than the literal
                    // values listed here.
                    && matches!(&strict.value.kind, ExprKind::Literal(Literal::Bool(true)))
                    && let ExprKind::ArrayLiteral(items) = &haystack.value.kind
                    && let Some(target_ty) = self.in_array_target_type(items, locals)
                    && let Some(key) = narrowing_key(&needle.value)
                {
                    let current = self.current_narrowed_type(&needle.value, &key, locals);
                    let narrowed = if branch_true {
                        narrow_type_to(self.registry, &current, &target_ty)
                    } else {
                        narrow_type_excluding(&current, &target_ty)
                    };
                    locals.insert(key, narrowed);
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "isset"
                    && branch_true
                    && args.len() > 1
                {
                    // Real PHP's `isset()` is genuinely variadic
                    // (`isset($a, $b, ...)`, equivalent to `isset($a) &&
                    // isset($b) && ...` - *every* argument must be set) -
                    // the single-arg dispatch above's own `[single_arg]`
                    // slice pattern never matches here at all, silently
                    // narrowing *none* of them, not just the ones past
                    // the first. Only the true branch is narrowable, the
                    // same asymmetry the single-arg case above already
                    // has (a multi-arg `isset()` being false doesn't say
                    // *which* argument was unset). Real corpus false
                    // positive: Monolog's own `MongoDBHandler::write()`'s
                    // `isset($this->manager, $this->namespace)` leaving
                    // `$this->namespace` (a real `string|null` property)
                    // un-narrowed inside the guard, wrongly flagging the
                    // subsequent `$this->manager->executeBulkWrite(
                    // $this->namespace, ...)` call.
                    for arg in args {
                        if let Some(key) = narrowing_key(&arg.value) {
                            let current = self.current_narrowed_type(&arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding(&current, &Type::Null));
                        }
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "array_key_exists"
                    && let [key_arg, arr_arg] = args.as_slice()
                    && let ExprKind::Literal(Literal::String(lit_key)) = &key_arg.value.kind
                    && let Some(arr_key) = narrowing_key(&arr_arg.value)
                {
                    // Phase 33 Finding 4: the checked expression here is
                    // the array argument, not the key - proving/disproving
                    // whether a specific offset is present lets a `Union`
                    // member that could never make the call come out this
                    // way be dropped, the same "keep/drop a Union member
                    // based on a provable fact" shape `instanceof`'s own
                    // member resolution already uses.
                    let current = self.current_narrowed_type(&arr_arg.value, &arr_key, locals);
                    let narrowed =
                        narrow_array_shape_by_key_presence(&current, lit_key, branch_true);
                    // Phase 50/73-74: also narrow the specific element
                    // access itself (`$arr['key']`), not just `$arr`'s
                    // own overall type - see `array_value_type_for_key`'s
                    // own doc comment for why this project's own
                    // composite `narrowing_key` mechanism is the right,
                    // proportionate stand-in for real PHPStan's
                    // `HasOffsetValueType`. Only the true branch: a
                    // *missing* key has no value type to narrow to at
                    // all (real PHP itself throws a warning and yields
                    // `null` on read, a fact this project doesn't model
                    // here - `isset`'s own narrowing already owns the
                    // "exclude `Null`" half of that for the false
                    // branch, a different, already-covered concern).
                    if branch_true
                        && let Some(value_ty) = array_value_type_for_key(&narrowed, lit_key)
                    {
                        locals.insert(format!("{arr_key}[{lit_key}]"), value_ty);
                    }
                    locals.insert(arr_key, narrowed);
                } else if let ExprKind::Name(name) = &callee.kind
                    && matches!(
                        name.trim_start_matches('\\'),
                        "str_contains" | "str_starts_with" | "str_ends_with"
                    )
                    && branch_true
                    && let [haystack, needle] = args.as_slice()
                    && let Some(key) = narrowing_key(&haystack.value)
                {
                    // Phase 55's own still-open list: real PHPStan's own
                    // `StrContainingTypeSpecifyingExtension` - a haystack
                    // that provably contains/starts/ends with a non-empty
                    // literal needle can't itself be empty. Only fires for
                    // a needle this checker can prove is a non-empty
                    // string literal (matching real PHPStan's own
                    // `isNonEmptyString()` gate) - a variable/computed
                    // needle proves nothing. Only the true branch is
                    // narrowable (mirrors `isFunctionSupported`'s own
                    // `$context->true()` restriction) - failing any of
                    // these three proves nothing about the haystack's own
                    // emptiness. `strpos`/`stripos`/`fnmatch`/`mb_*`
                    // variants (also in real PHPStan's own table) are
                    // deliberately left out - their `int|false` (or
                    // `false`-on-no-match) return makes a bare truthy
                    // `if (strpos(...))` a real, separate PHP footgun
                    // (position `0` is falsy despite being a match) this
                    // checker doesn't otherwise model, unlike these three
                    // genuinely bool-returning functions.
                    if let Type::StringLiteral(needle_lit) = self.infer_expr(&needle.value, locals)
                        && !needle_lit.is_empty()
                    {
                        let kind = if needle_lit == "0" {
                            AccessoryKind::NonEmptyString
                        } else {
                            AccessoryKind::NonFalsyString
                        };
                        let current = self.current_narrowed_type(&haystack.value, &key, locals);
                        locals.insert(key, upgrade_string_with_accessory(&current, kind));
                    }
                } else if let ExprKind::Name(name) = &callee.kind {
                    // A plain function call tagged `@phpstan-assert-if-true`/
                    // `-if-false` on its own docblock - see `FunctionSig::
                    // assert_if_true`'s own doc comment for the full
                    // picture. No `$this`/receiver here, so only a
                    // `Param`-targeted assertion can ever resolve
                    // (`apply_assert_infos` silently skips the rest).
                    let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                    if let Some(sig) = self.registry.lookup_function(&primary).or_else(|| {
                        fallback
                            .as_deref()
                            .and_then(|g| self.registry.lookup_function(g))
                    }) {
                        let infos = if branch_true {
                            &sig.assert_if_true
                        } else {
                            &sig.assert_if_false
                        };
                        if !infos.is_empty() {
                            self.apply_assert_infos(infos, sig, None, args, locals);
                        }
                    }
                }
            }
            // `@phpstan-assert-if-true`/`-if-false` on a method's own
            // docblock (real corpus shape: PHPUnit's own accessor pair
            // `hasTestFilesFile(): bool` tagged `@phpstan-assert-if-true
            // !null $this->testFilesFile`) - see `FunctionSig::
            // assert_if_true`'s own doc comment for the full picture.
            // `recv_class` picks a *representative* member out of a
            // `Union`/`Intersection` receiver the exact same way
            // `checker/expr.rs`'s own `MethodCall` arm already does for
            // ordinary method-call checking - real corpus shape this
            // fixes: Doctrine ORM's own `AssociationMapping::
            // isToOneOwningSide(): bool` (`@phpstan-assert-if-true
            // OneToOneOwningSideMapping|ManyToOneAssociationMapping
            // $this`), called on a variable typed as the *union* of all
            // six concrete mapping subclasses - every one of which
            // inherits the identical method, so any single member
            // resolves the same `AssertInfo` the others would. Once
            // resolved, `apply_assert_infos`/`narrow_type_to` already
            // narrow the receiver's own *full* current type generically,
            // Union or not - only the lookup itself needed to stop
            // requiring a bare `Type::Object`.
            ExprKind::MethodCall(recv, method, args, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let class = match &recv_ty {
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
                if let Some(class) = class
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        &sig.assert_if_true
                    } else {
                        &sig.assert_if_false
                    };
                    if !infos.is_empty() {
                        self.apply_assert_infos(infos, sig, Some(recv), args, locals);
                    }
                }
            }
            // Same idea as `MethodCall` just above, for a static method
            // call - no receiver expression exists here at all, so only
            // a `Param`-targeted assertion can ever resolve.
            ExprKind::StaticCall(class_name, method, args) => {
                if let Some(class) = self.resolve_static_class_name(class_name)
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        &sig.assert_if_true
                    } else {
                        &sig.assert_if_false
                    };
                    if !infos.is_empty() {
                        self.apply_assert_infos(infos, sig, None, args, locals);
                    }
                }
            }
            ExprKind::Instanceof(inner, target) => {
                // Phase 33 Finding 1: the false branch doesn't need a
                // "not this class" type to be representable - it only
                // needs to *drop* the one member just proven impossible
                // from an already-enumerated `Union`, exactly what
                // `narrow_type_excluding` already does soundly elsewhere.
                // A dynamic target (`$x instanceof $className`) still
                // doesn't name a concrete class to narrow to either way.
                if let Some(key) = narrowing_key(inner)
                    && let InstanceofTarget::Name(class_name, _) = target
                {
                    let resolved = self.resolve_name_expr(class_name);
                    if branch_true {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, narrow_instanceof_true(&current, &resolved));
                    } else {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, narrow_type_excluding(&current, &resolved));
                    }
                }
            }
            // An assignment used as a boolean sub-condition (`$a &&
            // ($x = f()) && $x->m()`, `!$a || !$x = f()`) evaluates
            // unconditionally whenever control actually reaches it -
            // the assigned value becomes `$x`'s real type regardless of
            // which way the *whole* enclosing condition ultimately goes,
            // so this must run before (and instead of) the generic
            // bare-truthiness catch-all just below, which `narrowing_
            // key`'s own `Assign` case would otherwise match too:
            // without this dedicated arm, that catch-all narrowed
            // falsiness out of the *pre*-assignment type it read via
            // `current_narrowed_type` (a no-op for most non-falsy
            // reassignments, silently discarding the assignment's real
            // effect) rather than binding the assigned type at all.
            // `check_expr`'s own `Binary(And/Or, ...)` handling re-derives
            // each operand's narrowing through this same function (see
            // its own doc comment) instead of reusing whatever it
            // already computed while checking each operand for
            // diagnostics, so a later &&/|| operand referencing the
            // freshly-assigned variable needs the assignment replayed
            // here too. Corpus-verified real shape (`Symfony\Component\
            // DependencyInjection\Compiler\AbstractRecursivePass::
            // getReflectionMethod()`): `$r->hasMethod('__call') && ($r =
            // $r->getMethod('__call')) && $r->isPublic()` wrongly flagged
            // `$r->isPublic()` as calling a nonexistent method on the
            // *pre*-reassignment `ReflectionClass`, never seeing that
            // `$r` had become a `ReflectionMethod` two conjuncts earlier
            // in the very same condition - a real, previously-invisible
            // gap unmasked only once `parse_not`'s own `!$var = expr`
            // fix stopped leaving reflection variables like this one
            // typed as `Mixed` (silencing every check through them)
            // whenever they were first bound via that idiom.
            //
            // The `branch_true` truthy case still needs its own extra
            // step on top of the plain bind: `if ($position = strpos($x,
            // "#")) { substr($x, 0, $position); }` (amphp's own
            // `Http2ConnectionProcessor`) relies on the *assignment's
            // own result* being truthy to reach the `if` body at all -
            // without also excluding falsy members from the
            // freshly-bound type here, `$position` kept `strpos()`'s raw
            // `int<0, max>|false` inside the truthy branch instead of a
            // real non-zero `int`, a corpus-verified false positive this
            // arm's own first version introduced (`substr()`'s 3rd
            // argument expects `int|null`, not `int<0, max>|false`).
            ExprKind::Assign(lhs, rhs) => {
                let rhs_ty = self.infer_expr(rhs, locals);
                self.bind_assignment_target(lhs, rhs_ty, locals);
                if branch_true
                    && let Some(key) = narrowing_key(lhs)
                    && let Some(current) = locals.get(&key).cloned()
                {
                    locals.insert(key, narrow_type_excluding_falsy(&current));
                }
            }
            // A bare truthiness check (`if ($x)` / `if (!$x)`, the latter
            // reaching here via the `Not` arm's recursion above) - only
            // the truthy branch is narrowable, by excluding the falsy
            // members this project can represent (`Null`/`False`); the
            // falsy branch isn't safely narrowable to anything more
            // specific than the current type.
            _ if branch_true && narrowing_key(cond).is_some() => {
                let key = narrowing_key(cond).expect("checked by guard above");
                let current = self.current_narrowed_type(cond, &key, locals);
                locals.insert(key, narrow_type_excluding_falsy(&current));
            }
            _ => {}
        }
    }

    /// Phase 48.7/48.C: `AssertRuleHelper`'s equivalent - a `@phpstan-
    /// assert`-family tag's own type is already resolved into a real
    /// `AssertInfo` (`FunctionSig::assert`/`assert_if_true`/
    /// `assert_if_false`) and consumed for narrowing (`apply_narrowing`'s
    /// own `check_assert_call` dispatch above); this is its own
    /// declaration-validity half - `class.notFound`/`assert.trait` reuse
    /// this file's sibling existence-checking helpers exactly the way
    /// `check_method_tag_types`/`check_mixin_types`/`check_type_alias_types`
    /// (`oop.rs`) already do; `parameter.notFound`/`assert.unknownExpr`
    /// surface `AssertTarget::NotFound`/`Unknown` (Phase 48.C's own
    /// registry-side change - see that enum's doc comment);
    /// `assert.impossibleType`/`.alreadyNarrowedType` are new (see
    /// `check_one_assert_tag`'s own doc comment for scope). Deliberately
    /// NOT built: `assert.unresolvableType` (real PHPStan's own pure-
    /// intersection-type unsatisfiability check - a genuinely separate,
    /// larger gap spanning multiple unrelated rule families per ROADMAP's
    /// own audit, not specific to asserts, and not attempted here) and
    /// the `missingType.iterableValue`/`.generics` sub-checks on an
    /// asserted type (real value, but needs its own message-template
    /// plumbing change to `report_missing_typehint_findings` - left as a
    /// disclosed, narrower cut for this pass rather than risking that
    /// already-shipped, separately-tested check family). Called once per
    /// declaration, alongside `check_declared_purity_tag`/`check_declared_
    /// param_callable_tags` (`function_decl.rs`).
    pub(crate) fn check_assert_tag_types(
        &mut self,
        sig: Option<&FunctionSig>,
        span: Span,
        label: &str,
    ) {
        let Some(sig) = sig else {
            return;
        };
        for (tag_name, infos) in [
            ("@phpstan-assert", &sig.assert),
            ("@phpstan-assert-if-true", &sig.assert_if_true),
            ("@phpstan-assert-if-false", &sig.assert_if_false),
        ] {
            for info in infos {
                self.check_one_assert_tag(info, tag_name, sig, span, label);
            }
        }
    }

    /// One `@phpstan-assert`-family tag's own full declaration-validity
    /// check - see `check_assert_tag_types`'s own doc comment for the
    /// full identifier list and what's deliberately not attempted.
    /// `assert.impossibleType`/`.alreadyNarrowedType` are only ever
    /// attempted for an `AssertTarget::Param` target compared against
    /// that exact parameter's own declared type - a disclosed, narrower
    /// cut versus real PHPStan's own `$this`/property-target coverage
    /// too, since those need a class-level property/receiver-type lookup
    /// this function doesn't otherwise need at all, and `Param` is by far
    /// the dominant real-corpus shape for this tag family.
    fn check_one_assert_tag(
        &mut self,
        info: &crate::registry::AssertInfo,
        tag_name: &str,
        sig: &FunctionSig,
        span: Span,
        label: &str,
    ) {
        let target_desc = match &info.target {
            AssertTarget::Param(name) => format!("${name}"),
            AssertTarget::ThisSelf => "$this".to_string(),
            AssertTarget::ThisProp(prop) => format!("$this->{prop}"),
            AssertTarget::NotFound(name) => {
                self.report(
                    label.to_string(),
                    format!("Assert references unknown parameter ${name}."),
                    span,
                    0,
                    "parameter.notFound",
                );
                return;
            }
            AssertTarget::Unknown(raw) => {
                self.report(
                    label.to_string(),
                    format!("Assert references unknown {raw}."),
                    span,
                    0,
                    "assert.unknownExpr",
                );
                return;
            }
        };
        let subject = format!("PHPDoc tag {tag_name} for {target_desc}");
        self.check_type_class_names_exist(&info.ty, &subject, span, label);
        self.check_type_trait_refs(&info.ty, &subject, span, label, "assert.trait");

        let AssertTarget::Param(name) = &info.target else {
            return;
        };
        let Some(param) = sig.params.iter().find(|p| &p.name == name) else {
            return;
        };
        let negated_prefix = if info.negated { "negated " } else { "" };
        let is_super = self.registry.is_assignable(&param.ty, &info.ty);
        let is_disjoint = types_definitely_disjoint(&info.ty, &param.ty);
        let (impossible, already_narrowed) = if info.negated {
            (is_super, is_disjoint)
        } else {
            (is_disjoint, is_super)
        };
        if impossible {
            self.report(
                label.to_string(),
                format!(
                    "Asserted {negated_prefix}type {} for {target_desc} with type {} can never happen.",
                    info.ty, param.ty
                ),
                span,
                0,
                "assert.impossibleType",
            );
        } else if already_narrowed {
            self.report(
                label.to_string(),
                format!(
                    "Asserted {negated_prefix}type {} for {target_desc} with type {} does not narrow down the type.",
                    info.ty, param.ty
                ),
                span,
                0,
                "assert.alreadyNarrowedType",
            );
        }
    }
}

/// Phase 48.C: `assert.impossibleType`'s own narrow, conservative type-
/// disjointness primitive - deliberately NOT Phase 84's XL tri-state
/// rewrite, despite surface similarity (confirmed against the design doc
/// this phase's own plan drew that distinction from). Only ever declares
/// two types disjoint when each resolves to one of a handful of coarse,
/// mutually-exclusive "scalar families" (`scalar_family` below) and those
/// families differ - e.g. `string` vs `int`, or `array` vs `object`.
/// Recurses into `Union` on either side (every member of a union side
/// must be disjoint from the other side for the whole comparison to
/// count - the same "stay on the lenient side" direction this project's
/// own union-legality check already established, see ROADMAP.md).
/// Returns `false` (not confidently disjoint - "don't guess") for
/// anything not covered: `Mixed`/`Never`/`Void`, two same-family types
/// (e.g. two unrelated object types - real PHPStan's own object-vs-object
/// disjointness needs a `final`-class/no-subtype-relation check this
/// primitive deliberately doesn't attempt), `Intersection`, `Callable`,
/// `Accessory`, templates, and every pseudo-type.
fn types_definitely_disjoint(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Union(members), _) => members.iter().all(|m| types_definitely_disjoint(m, b)),
        (_, Type::Union(members)) => members.iter().all(|m| types_definitely_disjoint(a, m)),
        _ => match (scalar_family(a), scalar_family(b)) {
            (Some(fa), Some(fb)) => fa != fb,
            _ => false,
        },
    }
}

#[derive(PartialEq)]
enum ScalarFamily {
    Int,
    Float,
    String,
    Bool,
    Null,
    Array,
    Object,
}

fn scalar_family(ty: &Type) -> Option<ScalarFamily> {
    match ty {
        Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => Some(ScalarFamily::Int),
        Type::Float | Type::FloatLiteral(_) => Some(ScalarFamily::Float),
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) => Some(ScalarFamily::String),
        Type::Bool | Type::True | Type::False => Some(ScalarFamily::Bool),
        Type::Null => Some(ScalarFamily::Null),
        Type::Array(_, _) | Type::ArrayShape(_) => Some(ScalarFamily::Array),
        Type::Object(_, _) | Type::EnumCase(_, _) | Type::ObjectShape(_) => {
            Some(ScalarFamily::Object)
        }
        _ => None,
    }
}
