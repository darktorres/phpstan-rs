use crate::regex_groups;
use syntax::ast::*;
use types::PhpString;

use types::{AccessoryKind, Shape, Type};

/// The type of one matched group's "leaf" value in `preg_match`'s own
/// `$matches` shape - plain `string` normally, or the real, positional
/// (not named-field) `[string, int]` tuple `PREG_OFFSET_CAPTURE`
/// actually produces at runtime (the match text and its byte offset).
pub(super) fn preg_leaf_type(offset_capture: bool) -> Type {
    if offset_capture {
        Type::array_shape(Shape::from_named(vec![
            ("0".to_string(), Type::string(), false),
            ("1".to_string(), Type::int(), false),
        ]))
    } else {
        Type::string()
    }
}

/// A per-group leaf type, refined by that group's own literal-value
/// enumeration when available, else by its own `RegexGroupFallback`
/// tri-state - never applied to field `"0"` (the whole match), matching
/// real PHPStan's own scope: even a fully-constant pattern like
/// `/(a(?:x|y))/` gets its own overall match typed as a general
/// refinement (`non-falsy-string`), never a literal union, so this
/// project's own field-`"0"` handling (plain `leaf.clone()`, unchanged)
/// already matches that scope without needing this helper at all.
fn group_leaf_type(leaf: &Type, group: &regex_groups::RegexCaptureGroup) -> Type {
    if let Some(values) = &group.literal_values {
        let literal_ty = values
            .iter()
            .map(|v| Type::string_literal(v.as_str().into()))
            .reduce(Type::union)
            .expect("literal_values is never Some(empty) by construction");
        return refine_leaf(leaf, literal_ty);
    }
    match group.fallback {
        regex_groups::RegexGroupFallback::NumericString => {
            refine_leaf(leaf, accessory_string(AccessoryKind::NumericString))
        }
        regex_groups::RegexGroupFallback::NonEmptyString => {
            refine_leaf(leaf, accessory_string(AccessoryKind::NonEmptyString))
        }
        regex_groups::RegexGroupFallback::Unknown => leaf.clone(),
    }
}

/// `Intersection([String, Accessory(kind)])` - the same representation
/// `types::parse_type_string` builds for `numeric-string`/
/// `non-empty-string` docblock pseudo-types, reused here so a
/// regex-derived refinement and a docblock-declared one are
/// indistinguishable to the rest of the type system.
fn accessory_string(kind: AccessoryKind) -> Type {
    Type::intersection_of(vec![Type::string(), Type::accessory(kind)])
}

/// Substitutes a group's own refined text type (`refined`, either an
/// exact literal union or an accessory-refined string) into `leaf`:
/// directly, for a plain `string` leaf, or into just the text slot of
/// `PREG_OFFSET_CAPTURE`'s own `[string, int]` tuple, leaving the byte-
/// offset slot untouched.
fn refine_leaf(leaf: &Type, refined: Type) -> Type {
    if *leaf == Type::string() {
        return refined;
    }
    match leaf.as_array_shape() {
        Some(shape) if shape.fields.len() == 2 => {
            let mut shape = shape.clone();
            shape.fields[0].ty = refined;
            Type::array_shape(shape)
        }
        _ => leaf.clone(),
    }
}

/// Builds `preg_match`'s own `$matches` shape from a pattern's parsed
/// capture groups (`regex_groups::parse_capture_groups`): field `"0"` is
/// always present (the whole match); each group gets a numbered field
/// with `optional: group.optional`; a named group gets an *additional*
/// field under its own name with identical type/optionality (real PHP
/// places both the numeric and named key in `$matches`). Skips adding a
/// named field whose name is already present - real PCRE allows the same
/// group name on two different alternation branches without the `J`
/// modifier (`(?<x>a)|(?<x>b)` is ordinary, valid PCRE), which would
/// otherwise produce a shape with a duplicated field.
///
/// `unmatched_as_null`: `PREG_UNMATCHED_AS_NULL` changes a
/// non-participating optional group from "entirely absent from the
/// array" to "always present, as `null`". A group that's never optional at all is
/// unaffected either way, since it can never have a non-participating
/// slot to turn nullable.
pub(crate) fn preg_match_matches_shape(
    groups: &[regex_groups::RegexCaptureGroup],
    leaf: &Type,
    unmatched_as_null: bool,
) -> Type {
    let mut fields = vec![("0".to_string(), leaf.clone(), false)];
    // Only a group within the trailing run of all-optional groups (from
    // the pattern's own last group backward) can ever be entirely absent
    // from `$matches` - real PCRE only ever truncates the array from its
    // own tail, e.g. `preg_match('/(a)(b)?(c)/', 'ac', $m)`
    // gives `$m = [0=>'ac', 1=>'a', 2=>'', 3=>'c']`, never omitting
    // `$m[2]`. An interior optional group (something required still
    // follows it) is always present instead, as `''` when it didn't
    // itself capture.
    let trailing_optional_count = groups.iter().rev().take_while(|g| g.optional).count();
    for (i, g) in groups.iter().enumerate() {
        // Set only by `build_matched_shape`'s own alternation-variant
        // builder, for a group known to sit *before* the variant's own
        // matched branch - always present, forced to the empty string,
        // bypassing `literal_values` entirely even when the group has
        // its own - a forced group never unions in what it could have
        // been.
        if g.forced_empty {
            let ty = Type::string_literal(PhpString::default());
            fields.push((g.number.to_string(), ty.clone(), false));
            if let Some(name) = &g.name
                && !fields.iter().any(|(n, _, _)| n == name)
            {
                fields.push((name.clone(), ty, false));
            }
            continue;
        }
        let is_trailing_optional = i >= groups.len() - trailing_optional_count;
        let is_last_group = i == groups.len() - 1;
        let base = group_leaf_type(leaf, g);
        let (leaf_ty, optional) = if g.optional && unmatched_as_null {
            (Type::union(base, Type::null()), false)
        } else if is_trailing_optional {
            (base, g.optional) // today's existing, already-correct behavior
        } else {
            (base, false) // interior - always present
        };
        // `''` is possible for any optional group except the absolute
        // last, independent of the trailing/interior split above - a
        // later trailing-optional group capturing something still
        // back-fills an earlier one with `''` too, e.g.
        // `/(x)(?:(a)|(b))/` matching `"xb"`: `$m = [0=>'xb', 1=>'x',
        // 2=>'', 3=>'b']`. Scoped to a plain `string` leaf only: real
        // PCRE's own tuple shape for this under `PREG_OFFSET_CAPTURE` is
        // commonly documented as `['', -1]`, but that hasn't been
        // checked the way every other fact here has, so this
        // stays unapplied rather than guessed when `leaf` isn't exactly
        // `string`. A no-op in the overwhelmingly common no-flags
        // case anyway (`Type::union(String, StringLiteral(""))` already
        // collapses back to plain `String`) - only observable once
        // literal enumeration narrows `base` below plain `String`.
        let ty = if g.optional && !is_last_group && !unmatched_as_null && *leaf == Type::string() {
            Type::union(leaf_ty, Type::string_literal(PhpString::default()))
        } else {
            leaf_ty
        };
        fields.push((g.number.to_string(), ty.clone(), optional));
        if let Some(name) = &g.name
            && !fields.iter().any(|(n, _, _)| n == name)
        {
            fields.push((name.clone(), ty, optional));
        }
    }
    Type::array_shape(Shape::from_named(fields))
}

/// Dispatches `preg_match`'s own unconditional `$matches` shape to one of
/// real PHPStan's two tagged-union special cases when a pattern
/// qualifies, falling back to the ordinary flat `preg_match_matches_shape`
/// otherwise. `preg_match_all` never calls this - it needs none of this,
/// since both its own modes already produce a single, unconditional,
/// always-correct shape for every match count.
pub(super) fn build_matched_shape(
    groups: &[regex_groups::RegexCaptureGroup],
    alternation_branch_counts: &[(u32, usize)],
    alternation_wrapped_in_optional_quantifier: &[u32],
    leaf: &Type,
    unmatched_as_null: bool,
) -> Type {
    let top_level: Vec<&regex_groups::RegexCaptureGroup> =
        groups.iter().filter(|g| g.top_level).collect();

    // Case 1: exactly one optional top-level group - checked first,
    // matching real PHPStan's own `if`/`elseif` precedence (mutually
    // exclusive with case 2 below even when a pattern would technically
    // also qualify for it, e.g. `/(a)|x/`, which has exactly
    // one top-level group and so takes this path).
    if !unmatched_as_null && top_level.len() == 1 && top_level[0].optional {
        let target = top_level[0].number;
        // With the target matched, a group is optional only when it or an
        // enclosing group other than the target can go unmatched.
        let optional_with_target = |mut at: usize| loop {
            let g = &groups[at];
            if g.number != target && g.own_optional {
                break true;
            }
            match g.parent {
                Some(p) => at = p,
                None => break false,
            }
        };
        let with_group: Vec<regex_groups::RegexCaptureGroup> = groups
            .iter()
            .enumerate()
            .map(|(at, g)| regex_groups::RegexCaptureGroup {
                optional: optional_with_target(at),
                ..g.clone()
            })
            .collect();
        // The "without" variant is a bare `{0: subject}` shape (an
        // empty groups slice), not "every group except the target" -
        // e.g. `/(a(b))?/` -> `array{string, 'ab',
        // 'b'}|array{string}` (field 2, `(b)`'s own nested capture, is
        // entirely absent from the second variant, not present-but-
        // optional). Also structurally forced, not an arbitrary choice:
        // the qualifying gate requires *exactly one* top-level group, so
        // any other group in the pattern must be nested inside it - if
        // it didn't run, nothing nested inside it could have either.
        return Type::union(
            preg_match_matches_shape(&with_group, leaf, unmatched_as_null),
            preg_match_matches_shape(&[], leaf, unmatched_as_null),
        );
    }

    // Case 2: exactly one top-level alternation - every top-level group
    // shares one alternation id, and none is `in_optional_quantification`
    // (the pattern's own top-level structure *is* this one alternation,
    // not wrapped in a further `?`/`*` around the whole thing).
    if !unmatched_as_null
        && let Some(first) = top_level.first()
        && let Some((alt_id, _)) = first.alternation
        && !first.in_optional_quantification
        && top_level.iter().all(|g| {
            g.alternation.map(|(id, _)| id) == Some(alt_id) && !g.in_optional_quantification
        })
    {
        let alternations_count = alternation_branch_counts
            .iter()
            .find(|(id, _)| *id == alt_id)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        let mut branches: Vec<u32> = groups
            .iter()
            .filter_map(|g| {
                g.alternation
                    .and_then(|(id, b)| (id == alt_id).then_some(b))
            })
            .collect();
        branches.sort_unstable();
        branches.dedup();
        // Iterating groups in number order, tracking a `before_combo`
        // flag that flips false the moment a group belonging to *this*
        // branch is reached: any group encountered while still before
        // the branch's own group(s) gets forced to `''` (never removed),
        // e.g. `/(a)|(b)|(c)/`'s own branch-3 variant,
        // `array{string, '', '', 'c'}` (groups 1 and 2, from earlier
        // branches, both show as `''`). Only a sibling's group
        // encountered *after* is removed.
        let mut variants: Vec<Type> = branches
            .iter()
            .map(|&b| {
                // A `(?|...)` branch-reset construct reuses
                // the same public `.number` across branches (see
                // `regex_groups::parse_capture_groups`'s own doc comment
                // on the topic) - an earlier sibling sharing one of
                // this branch's own numbers is really the same slot,
                // not a distinct field to backfill with `''`
                // (`(?|(\d+)|([a-z]+))` never produces two same-
                // numbered fields, only ever one, holding whichever
                // branch actually ran), so it's excluded entirely here
                // instead of falling into the generic `before_combo`
                // forced-empty case below, which would otherwise push a
                // second, wrong field under the same key.
                let this_branch_numbers: Vec<usize> = groups
                    .iter()
                    .filter(|g| g.alternation == Some((alt_id, b)))
                    .map(|g| g.number)
                    .collect();
                let mut before_combo = true;
                let variant_groups: Vec<regex_groups::RegexCaptureGroup> = groups
                    .iter()
                    .filter_map(|g| {
                        let in_this_branch = g.alternation == Some((alt_id, b));
                        if in_this_branch {
                            before_combo = false;
                            Some(regex_groups::RegexCaptureGroup {
                                optional: false,
                                ..g.clone()
                            })
                        } else if before_combo {
                            if this_branch_numbers.contains(&g.number) {
                                None // a branch-reset sibling of one of this branch's own groups
                            } else {
                                Some(regex_groups::RegexCaptureGroup {
                                    optional: false,
                                    forced_empty: true,
                                    ..g.clone()
                                })
                            }
                        } else if g.alternation.map(|(id, _)| id) == Some(alt_id) {
                            None // a *later* sibling branch's own group - never participated
                        } else {
                            Some(g.clone()) // unrelated (e.g. nested under a *different* alternation) - untouched
                        }
                    })
                    .collect();
                preg_match_matches_shape(&variant_groups, leaf, unmatched_as_null)
            })
            .collect();
        // The bare `{0: subject}` variant is added whenever some branch
        // has zero groups or the whole alternation sits under an outer
        // `?`/`*` (`alternation_wrapped_in_optional_quantifier`, e.g.
        // `/(?:(a)|(b))?/`) - either condition alone is
        // sufficient, matching real PHPStan's own `||`.
        if branches.len() != alternations_count
            || alternation_wrapped_in_optional_quantifier.contains(&alt_id)
        {
            variants.push(preg_match_matches_shape(&[], leaf, unmatched_as_null));
        }
        return variants
            .into_iter()
            .reduce(Type::union)
            .expect("branches is non-empty: at least one top-level group exists");
    }

    preg_match_matches_shape(groups, leaf, unmatched_as_null)
}

/// Builds `preg_match_all`'s own `$matches` shape for its default
/// `PREG_PATTERN_ORDER` mode - structurally different from
/// `preg_match_matches_shape`'s single-match shape: every key (`0`, each
/// group number, each name) is always present, as an array of the leaf type,
/// regardless of match count (a genuine zero-match call still populates
/// every key with an empty array, not the whole thing collapsing to
/// `[]`), and a group that doesn't participate in one particular match
/// still gets a slot in that key's own array (`''` normally, or `null`
/// under `PREG_UNMATCHED_AS_NULL`) rather than an entirely missing
/// element. So no field is ever `optional` at the top level here, and a
/// group's own `optional` flag only ever affects the *element* type
/// nested inside its array (when combined with `PREG_UNMATCHED_AS_NULL`),
/// never whether the top-level key itself is present. `sized` is applied
/// to each group's list, as the match count bounds it.
pub(super) fn preg_match_all_pattern_order_shape(
    groups: &[regex_groups::RegexCaptureGroup],
    leaf: &Type,
    unmatched_as_null: bool,
    sized: impl Fn(Type) -> Type,
) -> Type {
    let element_ty = |base: Type, optional: bool| {
        if optional && unmatched_as_null {
            Type::union(base, Type::null())
        } else {
            base
        }
    };
    let list_of = |elem: Type| sized(list_of(elem));
    let mut fields = vec![(
        "0".to_string(),
        list_of(element_ty(leaf.clone(), false)),
        false,
    )];
    for g in groups {
        let base = group_leaf_type(leaf, g);
        let arr_ty = list_of(element_ty(base, g.optional));
        fields.push((g.number.to_string(), arr_ty.clone(), false));
        if let Some(name) = &g.name
            && !fields.iter().any(|(n, _, _)| n == name)
        {
            fields.push((name.clone(), arr_ty, false));
        }
    }
    Type::array_shape(Shape::from_named(fields))
}

/// A bitwise-OR-chain flag collector for `preg_match`/`preg_match_all`'s
/// own `$flags` argument, mirroring `contains_json_throw_on_error`'s own
/// recursive shape but collecting a small flag struct instead of a single
/// boolean.
#[derive(Default, Clone, Copy)]
pub(super) struct PregFlags {
    pub(super) offset_capture: bool,
    pub(super) unmatched_as_null: bool,
    // Only meaningful for `preg_match_all`; `false` = `PREG_PATTERN_ORDER`
    // (the real default).
    pub(super) set_order: bool,
}

/// Walks `e` as a `Name` or a `BitOr`/`Grouping` chain of `Name`s,
/// recognizing exactly `PREG_OFFSET_CAPTURE`/`PREG_UNMATCHED_AS_NULL`
/// (and, when `allow_order` is set, `PREG_PATTERN_ORDER`/
/// `PREG_SET_ORDER`). `None` if any leaf name isn't one of these, or if
/// both `PATTERN_ORDER` and `SET_ORDER` appear together - never guessed.
pub(super) fn parse_preg_flags(e: &Expr, allow_order: bool) -> Option<PregFlags> {
    fn collect<'a>(e: &'a Expr, out: &mut Vec<&'a str>) -> bool {
        let e = e.peeled();
        match &e.kind {
            ExprKind::Name(name) => {
                out.push(name.trim_start_matches('\\'));
                true
            }
            ExprKind::Binary(BinOp::BitOr, l, r) => collect(l, out) && collect(r, out),
            _ => false,
        }
    }
    let mut names = Vec::new();
    if !collect(e, &mut names) {
        return None;
    }
    let mut flags = PregFlags::default();
    let mut order_seen = 0;
    for name in names {
        match name {
            "PREG_OFFSET_CAPTURE" => flags.offset_capture = true,
            "PREG_UNMATCHED_AS_NULL" => flags.unmatched_as_null = true,
            "PREG_PATTERN_ORDER" if allow_order => order_seen += 1,
            "PREG_SET_ORDER" if allow_order => {
                flags.set_order = true;
                order_seen += 1;
            }
            _ => return None,
        }
    }
    if order_seen > 1 {
        return None;
    }
    Some(flags)
}

/// A `preg_match` or `preg_match_all` call, its arguments resolved by
/// parameter name.
pub(crate) struct PregCall<'e> {
    pub(crate) all: bool,
    pub(crate) pattern: &'e Expr,
    pub(crate) subject: Option<&'e Expr>,
    pub(crate) matches: Option<&'e Expr>,
    pub(crate) flags: Option<&'e Expr>,
    pub(crate) offset: Option<&'e Expr>,
}

/// What a preg call's result, as narrowed, says happened: the match
/// counts it allows (`None` when it can't be a count), and whether it
/// may be `false`, a failed call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PregOutcome {
    pub(crate) count: Option<(i64, Option<i64>)>,
    pub(crate) may_fail: bool,
}

impl PregOutcome {
    /// Whether the call surely found a match.
    pub(crate) fn matched(self) -> bool {
        !self.may_fail && self.count.is_some_and(|(lo, _)| lo >= 1)
    }
}

/// `e` as a `preg_match` / `preg_match_all` call. The parameters are
/// PHP's own (`pattern`, `subject`, `matches`, `flags`, `offset`), so
/// named arguments resolve without the registry; a spread gives `None`.
pub(crate) fn preg_call(e: &Expr) -> Option<PregCall<'_>> {
    const PARAMS: [&str; 5] = ["pattern", "subject", "matches", "flags", "offset"];
    let ExprKind::Call(callee, args) = &e.peeled().kind else {
        return None;
    };
    let ExprKind::Name(name) = &callee.kind else {
        return None;
    };
    let name = name.trim_start_matches('\\');
    let all = if name.eq_ignore_ascii_case("preg_match") {
        false
    } else if name.eq_ignore_ascii_case("preg_match_all") {
        true
    } else {
        return None;
    };
    let mut resolved: [Option<&Expr>; 5] = [None; 5];
    let mut position = 0;
    for arg in args {
        if arg.spread {
            return None;
        }
        let index = match &arg.name {
            Some(n) => PARAMS.iter().position(|p| *p == &*n.name)?,
            None => {
                position += 1;
                position - 1
            }
        };
        *resolved.get_mut(index)? = Some(&arg.value);
    }
    Some(PregCall {
        all,
        pattern: resolved[0]?,
        subject: resolved[1],
        matches: resolved[2],
        flags: resolved[3],
        offset: resolved[4],
    })
}

/// The result type of a preg call: `0|1|false` for `preg_match`, the
/// match count or `false` for `preg_match_all`.
pub(crate) fn preg_result_type(all: bool) -> Type {
    let count = if all {
        Type::int_range(Some(0), None)
    } else {
        Type::union(Type::int_literal(0), Type::int_literal(1))
    };
    Type::union(count, Type::bool_literal(false))
}

/// The outcome `result`, a preg call's result type as narrowed, allows.
/// `None` when it holds anything but ints and `false`, or nothing.
pub(crate) fn preg_result_outcome(result: &Type) -> Option<PregOutcome> {
    let mut count: Option<(i64, Option<i64>)> = None;
    let mut may_fail = false;
    for member in result.union_members().iter() {
        if *member == Type::bool_literal(false) {
            may_fail = true;
            continue;
        }
        if !member.is_int().yes() {
            return None;
        }
        let (lo, hi) = member.int_bounds()?;
        let lo = lo.unwrap_or(0).max(0);
        count = Some(match count {
            None => (lo, hi),
            Some((l, h)) => (l.min(lo), h.zip(hi).map(|(a, b)| a.max(b))),
        });
    }
    (count.is_some() || may_fail).then_some(PregOutcome { count, may_fail })
}

impl crate::checker::Checker<'_> {
    /// The type `call` leaves in `$matches` when its result is within
    /// `outcome`. A compiling pattern (one `parse_capture_groups`
    /// reads) always fills `$matches`: `preg_match` with the match or
    /// `[]`, `preg_match_all` with one list per group (or one per match
    /// under `PREG_SET_ORDER`) of the count's size, or of any size when
    /// it failed part way, and `[]` when an offset argument is out of
    /// range. A pattern that doesn't compile leaves `$matches` unchanged,
    /// and constant flags are needed, so either gives `None`.
    pub(crate) fn preg_matches_type(
        &self,
        call: &PregCall<'_>,
        outcome: PregOutcome,
        locals: &crate::checker::Locals,
    ) -> Option<Type> {
        let pattern = self.constant_string_value(call.pattern, locals)?;
        let parsed = regex_groups::parse_capture_groups(&pattern)?;
        let flags = match call.flags {
            Some(e) => parse_preg_flags(e, call.all)?,
            None => PregFlags::default(),
        };
        let leaf = preg_leaf_type(flags.offset_capture);
        let empty = || Type::array_shape(Shape::sealed(Vec::new()));
        let mut parts = Vec::new();
        if !call.all {
            if outcome
                .count
                .is_some_and(|(_, hi)| hi.is_none_or(|h| h >= 1))
            {
                parts.push(build_matched_shape(
                    &parsed.groups,
                    &parsed.alternation_branch_counts,
                    &parsed.alternation_wrapped_in_optional_quantifier,
                    &leaf,
                    flags.unmatched_as_null,
                ));
            }
            if outcome.may_fail || outcome.count.is_some_and(|(lo, _)| lo <= 0) {
                parts.push(empty());
            }
            return Some(Type::union_of(parts));
        }
        let lists = |count: Option<(i64, Option<i64>)>| {
            let sized = |list: Type| match count {
                Some((lo, hi)) => self.relation().narrow_count(&list, lo, hi),
                None => list,
            };
            if flags.set_order {
                sized(list_of(preg_match_matches_shape(
                    &parsed.groups,
                    &leaf,
                    flags.unmatched_as_null,
                )))
            } else {
                preg_match_all_pattern_order_shape(
                    &parsed.groups,
                    &leaf,
                    flags.unmatched_as_null,
                    sized,
                )
            }
        };
        if let Some(count) = outcome.count {
            parts.push(lists(Some(count)));
        }
        if outcome.may_fail {
            parts.push(lists(None));
            if call.offset.is_some() {
                parts.push(empty());
            }
        }
        Some(Type::union_of(parts))
    }
}

/// `list<elem>`: one slot per match, keyed `0..count`.
fn list_of(elem: Type) -> Type {
    Type::intersection_of(vec![
        Type::array(Type::unknown_array_key(), elem),
        Type::accessory(AccessoryKind::ArrayList),
    ])
}
