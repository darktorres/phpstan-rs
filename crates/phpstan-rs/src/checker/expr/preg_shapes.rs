use crate::regex_groups;
use crate::types::{AccessoryKind, Shape, Type};
use crate::types::{union_types, union_types_keep_shapes_distinct};
use syntax::ast::*;

/// The type of one matched group's "leaf" value in `preg_match`'s own
/// `$matches` shape - plain `string` normally, or the real, positional
/// (not named-field) `[string, int]` tuple `PREG_OFFSET_CAPTURE`
/// actually produces at runtime (the match text and its byte offset).
pub(super) fn preg_leaf_type(offset_capture: bool) -> Type {
    if offset_capture {
        Type::ArrayShape(Shape::from_named(vec![
            ("0".to_string(), Type::String, false),
            ("1".to_string(), Type::Int, false),
        ]))
    } else {
        Type::String
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
            .cloned()
            .map(Type::StringLiteral)
            .reduce(union_types)
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
    Type::Intersection(vec![Type::String, Type::Accessory(kind)])
}

/// Substitutes a group's own refined text type (`refined`, either an
/// exact literal union or an accessory-refined string) into `leaf`:
/// directly, for a plain `string` leaf, or into just the text slot of
/// `PREG_OFFSET_CAPTURE`'s own `[string, int]` tuple, leaving the byte-
/// offset slot untouched.
fn refine_leaf(leaf: &Type, refined: Type) -> Type {
    match leaf {
        Type::String => refined,
        Type::ArrayShape(shape) if shape.fields.len() == 2 => {
            let mut shape = shape.clone();
            shape.fields[0].ty = refined;
            Type::ArrayShape(shape)
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
            let ty = Type::StringLiteral(String::new());
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
            (union_types(base, Type::Null), false)
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
        // `Type::String`. A no-op in the overwhelmingly common no-flags
        // case anyway (`union_types(String, StringLiteral(""))` already
        // collapses back to plain `String`) - only observable once
        // literal enumeration narrows `base` below plain `String`.
        let ty = if g.optional && !is_last_group && !unmatched_as_null && leaf == &Type::String {
            union_types(leaf_ty, Type::StringLiteral(String::new()))
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
    Type::ArrayShape(Shape::from_named(fields))
}

/// Dispatches `preg_match`'s own unconditional `$matches` shape to one of
/// real PHPStan's two tagged-union special cases when a pattern
/// qualifies, falling back to the ordinary flat `preg_match_matches_shape`
/// otherwise. `preg_match_all` never calls this - it needs none of this,
/// since both its own modes already produce a single, unconditional,
/// always-correct shape for every match count.
pub(crate) fn build_matched_shape(
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
        let with_group: Vec<regex_groups::RegexCaptureGroup> = groups
            .iter()
            .map(|g| regex_groups::RegexCaptureGroup {
                optional: g.optional && g.number != target,
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
        // `union_types_keep_shapes_distinct` - see its own doc comment
        // and the identical reasoning a few lines below for the
        // alternation-branches case: the "without" variant's own key
        // set is a subset of the "with" variant's, and `union_types`'s
        // own subset-collapse rule would otherwise merge them into one
        // shape with field 1/2 merely optional, destroying the
        // `array_key_exists`-based discriminant the doc comment above
        // relies on.
        return union_types_keep_shapes_distinct(
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
        // `union_types_keep_shapes_distinct` - each variant here is a
        // distinct, mutually exclusive alternation branch meant to be
        // told apart *later* by `array_key_exists`-based narrowing
        // (`checker::narrowing`), not a single value narrowed across an
        // ordinary conditional; letting `union_types`'s own subset-
        // collapse rule fire here would merge two variants whenever one
        // branch's own key set happens to be a subset of another's
        // (common - a zero-group or earlier-in-the-alternation branch
        // naturally has fewer fields), destroying the very discriminant
        // that narrowing depends on.
        return variants
            .into_iter()
            .reduce(union_types_keep_shapes_distinct)
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
/// never whether the top-level key itself is present.
pub(super) fn preg_match_all_pattern_order_shape(
    groups: &[regex_groups::RegexCaptureGroup],
    leaf: &Type,
    unmatched_as_null: bool,
) -> Type {
    let element_ty = |base: Type, optional: bool| {
        if optional && unmatched_as_null {
            union_types(base, Type::Null)
        } else {
            base
        }
    };
    // Every one of these per-key match arrays is itself always real,
    // sequentially-int-keyed (one slot per match, 0..match_count-1) -
    // verified via dumpType() that real PHPStan types each as a genuine
    // `list<T>`, not merely `array<T>`.
    let list_of = |elem: Type| {
        Type::Intersection(vec![
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem)),
            Type::Accessory(AccessoryKind::ArrayList),
        ])
    };
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
    Type::ArrayShape(Shape::from_named(fields))
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
        match &e.kind {
            ExprKind::Name(name) => {
                out.push(name.trim_start_matches('\\'));
                true
            }
            ExprKind::Binary(BinOp::BitOr, l, r) => collect(l, out) && collect(r, out),
            ExprKind::Grouping(inner) => collect(inner, out),
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
