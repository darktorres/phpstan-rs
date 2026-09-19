use crate::types::{self, Type};
/// A defensive cap on how many distinct members `union_types` will ever
/// materialize into a real `Type::Union`. Collapsing to `Mixed` past this
/// point is a stable fixed point (`union_types(Mixed, _) == Mixed` is
/// this function's first special case below), so a variable that blows
/// past the cap once stays collapsed rather than being recomputed on
/// every subsequent merge. Needed because `Type::Union` sticks across
/// reads/writes instead of immediately collapsing to `Mixed`: a function
/// with many sequential and nested branches assigning distinct array
/// shapes to the same variable can grow that variable's union member
/// count combinatorially, and each subsequent dedup scan below (plus
/// each member's own recursive `PartialEq`) grows right along with it.
const MAX_UNION_MEMBERS: usize = 12;

/// Union of two types for a scope-merge join point - flattens nested
/// unions and keeps `Mixed` "unknown" rather than let it compose into a
/// falsely-precise-looking Union (same principle as the ternary inference).
pub(crate) fn union_types(a: Type, b: Type) -> Type {
    union_types_impl(a, b, true)
}

/// `union_types`'s own escape hatch for a caller that must keep several
/// `Type::ArrayShape` variants discrete even when one's key set happens
/// to be a subset of another's, skipping only `merge_array_shape_pair`'s
/// own subset-collapse rule below (every other `union_types` behavior -
/// the `hasOffsetValue` merge, `Mixed` absorption, the union member cap,
/// ... - stays identical). `checker::expr::preg_shapes`'s own regex-
/// alternation `$matches` shape construction is the one real user: each
/// variant there represents a distinct, mutually exclusive branch meant
/// to be told apart *later* by `array_key_exists`-based narrowing, not
/// one variable's own value narrowed across a plain conditional (which
/// is what `merge_array_shape_pair`'s subset rule is actually verified
/// against - see its own doc comment) - collapsing them the same way
/// would destroy the very discriminant that narrowing depends on.
pub(crate) fn union_types_keep_shapes_distinct(a: Type, b: Type) -> Type {
    union_types_impl(a, b, false)
}

fn union_types_impl(a: Type, b: Type, merge_shapes: bool) -> Type {
    if types::types_equal_for_union(&a, &b) {
        return a;
    }
    if a == Type::Mixed || b == Type::Mixed {
        return Type::Mixed;
    }
    if let Some(merged) = merge_has_offset_value_pair(&a, &b) {
        return merged;
    }
    if merge_shapes && let Some(merged) = merge_array_shape_pair(&a, &b) {
        return merged;
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
    // Drop a literal member whenever its exact base type is also present
    // in the same union - a ternary's two branches
    // (`is_string($x) ? $x : ''`) build exactly this shape,
    // `union_types(String, StringLiteral(""))`, and keeping both would
    // stop later code from matching `== Type::String`. Snapshotting
    // which base types are present first, rather than checking `members`
    // from inside its own `retain` closure (which the borrow checker
    // won't allow). Covers `Array`/`Float` alongside `Int`/`String`/
    // `Bool` since `literal_base_type` can also produce an `Array(_)`
    // base (an accessory-refined `list<T>`/`non-empty-array<T>`) or
    // `Float` (a `FloatLiteral`).
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
    // A genuinely-empty `ArrayShape` (`array{}`) is a subtype of *any*
    // `Array(key, val)` - vacuously, since an empty array satisfies
    // "every key is `key`-typed, every value is `val`-typed" for every
    // possible `key`/`val` - so keeping both as separate union members is
    // the same kind of redundant-literal-not-absorbed-into-its-own-base
    // shape `literal_base_type` above fixes for `Int`/`String`/`Bool`/
    // `Float`, for a variant `literal_base_type` can't express (its
    // absorbing base depends on *which* `Array(_, _)` member is already
    // present, not a fixed mapping). Concretely:
    // `$counter = []; foreach ($keys as $k) { $counter[$k] = 1; }` merges
    // the loop's zero-iteration entry state (`array{}`) with the body's
    // accumulated `array<string, 1>`; the correct post-loop type is the
    // unqualified `array<string, 1>`, with no `array{}` alternative
    // surviving. Without this, `index_result_type`'s separately-correct
    // "an empty shape plus a dynamic key reads `null`" rule fires on the
    // still-surviving empty-shape member, unioning a bogus `null` into an
    // otherwise-precise `$counter[$key]` read. Scoped to an *empty* shape
    // only - a non-empty `ArrayShape` needs real field-by-field subtype
    // checking against the specific `Array(key, val)` co-member to be
    // absorbed safely. Does not affect a whole-variable reassignment
    // merge like `$file_path = null; foreach (...) { $file_path = $row; }`
    // - `Type::Null` is not a subtype of `Type::String`, so `string|null`
    // is correctly still kept apart.
    let has_general_array = members.iter().any(|m| matches!(m, Type::Array(_, _)));
    if has_general_array {
        members.retain(|m| !matches!(m, Type::ArrayShape(fields) if fields.is_empty()));
    }
    // Two structurally different `Array(key, val)` members merge into
    // one precise `Array(key1|key2, val1|val2)` instead of staying
    // separate union members - `array_element_type`'s `Union` arm, and
    // every other consumer wanting "the" element type of a plain array,
    // would otherwise silently only see the first matching member.
    // Scoped to a bare `Type::Array` only, not an accessory-refined
    // `list<T>`/`non-empty-array<T>` (a real `Intersection`) - a mix of
    // the two is left as a narrower residual.
    if members
        .iter()
        .filter(|m| matches!(m, Type::Array(_, _)))
        .count()
        > 1
    {
        let mut merged_key: Option<Type> = None;
        let mut merged_val: Option<Type> = None;
        members.retain(|m| {
            let Type::Array(k, v) = m else {
                return true;
            };
            merged_key = Some(match merged_key.take() {
                None => (**k).clone(),
                Some(acc) => union_types(acc, (**k).clone()),
            });
            merged_val = Some(match merged_val.take() {
                None => (**v).clone(),
                Some(acc) => union_types(acc, (**v).clone()),
            });
            false
        });
        members.push(Type::Array(
            Box::new(merged_key.unwrap()),
            Box::new(merged_val.unwrap()),
        ));
    }
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
/// unioned field types - only tried once the cap would otherwise fire, so
/// a small union's behavior is unaffected. Two `ArrayShape`s merge only
/// when their field-name *sets* are identical (order-independent), not a
/// fuzzier "mostly-the-same-shape" heuristic. Targets many sequential/
/// nested branches repeatedly writing the same known keys with different
/// value types onto one variable; the cap itself stays as the safety net
/// for whatever's left ungeneralizable (a variable whose branches
/// genuinely have different key sets).
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
        // Scoped to intersections that carry an `Accessory` member, so a
        // genuine, unrelated multi-class intersection
        // (`Countable&Iterator`) is never affected - only an
        // accessory-refined intersection is "more specific than its own
        // base type."
        Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Accessory(_))) => {
            members
                .iter()
                .find(|m| !matches!(m, Type::Accessory(_)))
                .cloned()
        }
        _ => None,
    }
}

/// Splits a plain `Type::Array` or a "pure" `hasOffsetValue(...)`-carrying
/// `Type::Intersection` (`write_literal_key_aware`'s own construction
/// shape - an `Array` member plus zero or more `HasOffsetValue` members,
/// nothing else) into its base `Array` and its own tracked
/// `(key, value)` pairs. `None` for anything else (a real class
/// intersection, a list/`non-empty-array`-marked one, ...) - this is
/// deliberately narrow, matching only the one shape
/// `write_literal_key_aware` ever produces, not "any intersection that
/// happens to contain an `Array` member." Also reused directly by
/// `array_merge`'s own shape-merge path (`checker::expr::builtin_calls`)
/// to promote a `hasOffsetValue`-carrying argument's own tracked keys
/// into real merged-shape fields, not just this module's own pairwise
/// union merge below.
pub(crate) fn has_offset_value_parts(t: &Type) -> Option<(Type, Vec<(String, Type)>)> {
    match t {
        Type::Array(..) => Some((t.clone(), Vec::new())),
        Type::Intersection(members)
            if members
                .iter()
                .all(|m| matches!(m, Type::Array(..) | Type::HasOffsetValue(..))) =>
        {
            let base = members
                .iter()
                .find(|m| matches!(m, Type::Array(..)))?
                .clone();
            let pairs = members
                .iter()
                .filter_map(|m| match m {
                    Type::HasOffsetValue(k, v) => Some((k.clone(), (**v).clone())),
                    _ => None,
                })
                .collect();
            Some((base, pairs))
        }
        _ => None,
    }
}

/// Merges two `write_literal_key_aware`-shaped operands (see
/// `has_offset_value_parts`) at a scope-merge join point, mirroring real
/// PHPStan's own observed behavior exactly (`assertType()`-confirmed): a
/// key tracked on *both* sides survives with its value types unioned; a
/// key tracked on only one side is dropped, generalizing back to the
/// shared base rather than staying precise for a path that might not
/// have taken this write at all. Without this, a variable written across
/// several independent, non-mutually-exclusive branches (`if ($a) {
/// $x['a'] = ...; } if ($b) { $x['b'] = ...; } ...`) would grow one
/// extra `Type::Union` member per branch instead of converging back to a
/// single type the way real PHPStan's own merge does - a real corpus
/// regression this fixes (`ActivityService::record()`'s own
/// `$details['method'] = ...`/`$details['script'] = ...`/... sequence:
/// each independent `if` on its own doubled `$details`'s own union
/// member count, hitting `MAX_UNION_MEMBERS` and collapsing the *whole*
/// variable to `Mixed` a few branches in).
///
/// Returns `None` (falling back to `union_types`' own generic handling)
/// unless *both* operands are one of the two eligible shapes - a
/// genuinely unrelated type on either side (an object, a shape, ...)
/// never reaches this merge at all.
fn merge_has_offset_value_pair(a: &Type, b: &Type) -> Option<Type> {
    let (base_a, pairs_a) = has_offset_value_parts(a)?;
    let (base_b, pairs_b) = has_offset_value_parts(b)?;
    // Neither side tracked anything - the plain `Array`/`Array` case
    // `union_types`' own generic "merge multiple Array members" step
    // already handles just as well; leave it to that instead of
    // duplicating it here.
    if pairs_a.is_empty() && pairs_b.is_empty() {
        return None;
    }
    let merged_base = match (base_a, base_b) {
        (Type::Array(ka, va), Type::Array(kb, vb)) => Type::Array(
            Box::new(union_types(*ka, *kb)),
            Box::new(union_types(*va, *vb)),
        ),
        // Unreachable given `has_offset_value_parts`'s own contract
        // (its `base` is always a `Type::Array`), kept as a safe
        // fallback rather than an `unreachable!()`.
        (base_a, _) => base_a,
    };
    let mut merged_pairs: Vec<(String, Type)> = Vec::new();
    for (k, va) in &pairs_a {
        if let Some((_, vb)) = pairs_b.iter().find(|(kb, _)| kb == k) {
            merged_pairs.push((k.clone(), union_types(va.clone(), vb.clone())));
        }
    }
    if merged_pairs.is_empty() {
        return Some(merged_base);
    }
    let mut members = vec![merged_base];
    members.extend(
        merged_pairs
            .into_iter()
            .map(|(k, v)| Type::HasOffsetValue(k, Box::new(v))),
    );
    Some(Type::Intersection(members))
}

/// A pairwise `Type::ArrayShape`-vs-`Type::ArrayShape` merge at a
/// scope-merge join point, mirroring real PHPStan's own observed
/// behavior exactly (confirmed via `assertType()`/`dumpType()` against
/// the vendored phar, not guessed): the two shapes only merge into one
/// when one side's own real (non-`"..."`) key set is a *subset* of the
/// other's - the common shape a branch join actually produces (an `if`
/// without an `else` merges the pre-branch shape with a strict extension
/// of itself; a ternary between a shape and one of its own supersets
/// does too). A key present on both sides keeps its required-ness only
/// if required on *both*, with its value types unioned; a key present on
/// only one side becomes *optional* (not dropped - the value type it had
/// on the one side that wrote it is still real, precise information)
/// with that side's own value type kept as-is.
///
/// Two shapes with genuinely incomparable key sets (neither a subset of
/// the other - e.g. two unrelated parameter shapes merged in a ternary)
/// are deliberately left unmerged (`None`, falling back to
/// `union_types`' own generic "stay a real `Union`" handling) - real
/// PHPStan keeps those as a literal `array{a: int}|array{b: string}`
/// too, confirmed the same way, rather than ever collapsing them into
/// one over-general `array{a?: int, b?: string}`.
///
/// Without this, promoting a `hasOffsetValue`-tracked argument's own
/// keys into real merged-shape fields (`array_merge`'s own shape-merge
/// path, `checker::expr::builtin_calls`) made a variable become a real
/// `ArrayShape` earlier and more often than before - and without this
/// merge rule, that shape then written across several independent,
/// non-mutually-exclusive branches reproduced the exact same
/// `MAX_UNION_MEMBERS`-overflow-to-`Mixed` collapse
/// `merge_has_offset_value_pair` above already fixed once for
/// `hasOffsetValue`, just one layer up on bare `ArrayShape` unions
/// instead - a real, measured regression (confirmed via a dedicated
/// probe test before this fix was written), not a hypothetical one.
fn merge_array_shape_pair(a: &Type, b: &Type) -> Option<Type> {
    let (Type::ArrayShape(fa), Type::ArrayShape(fb)) = (a, b) else {
        return None;
    };
    fn field<'a>(
        fields: &'a [(String, Type, bool)],
        name: &str,
    ) -> Option<&'a (String, Type, bool)> {
        fields.iter().find(|(k, _, _)| k == name)
    }
    fn real_names(fields: &[(String, Type, bool)]) -> Vec<&str> {
        fields
            .iter()
            .filter(|(k, _, _)| k != "...")
            .map(|(k, _, _)| k.as_str())
            .collect()
    }
    let (names_a, names_b) = (real_names(fa), real_names(fb));
    let is_subset = |xs: &[&str], ys: &[&str]| xs.iter().all(|x| ys.contains(x));
    if !(is_subset(&names_a, &names_b) || is_subset(&names_b, &names_a)) {
        return None;
    }
    let has_dotdotdot =
        fa.iter().any(|(k, _, _)| k == "...") || fb.iter().any(|(k, _, _)| k == "...");
    let mut all_names: Vec<&str> = names_a.clone();
    for n in &names_b {
        if !all_names.contains(n) {
            all_names.push(n);
        }
    }
    let mut merged: Vec<(String, Type, bool)> = Vec::new();
    for name in all_names {
        match (field(fa, name), field(fb, name)) {
            (Some((_, ta, oa)), Some((_, tb, ob))) => {
                merged.push((
                    name.to_string(),
                    union_types(ta.clone(), tb.clone()),
                    *oa || *ob,
                ));
            }
            (Some((_, ta, _)), None) | (None, Some((_, ta, _))) => {
                merged.push((name.to_string(), ta.clone(), true));
            }
            (None, None) => unreachable!("name came from one of the two lists"),
        }
    }
    if has_dotdotdot {
        merged.push(("...".to_string(), Type::Mixed, false));
    }
    Some(Type::ArrayShape(merged))
}
