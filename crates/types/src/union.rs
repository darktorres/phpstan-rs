use crate::{AccessoryKind, Repr, Shape, ShapeField, ShapeKey, Type};

/// A defensive cap on how many distinct members `Type::union` will ever
/// materialize into a real `Repr::Union`. Collapsing to `Mixed` past this
/// point is a stable fixed point (a union with `mixed` is `mixed`), so a
/// variable that blows
/// past the cap once stays collapsed rather than being recomputed on
/// every subsequent merge. Needed because `Repr::Union` sticks across
/// reads/writes instead of immediately collapsing to `Mixed`: a function
/// with many sequential and nested branches assigning distinct array
/// shapes to the same variable can grow that variable's union member
/// count combinatorially, and each subsequent dedup scan below (plus
/// each member's own recursive `PartialEq`) grows right along with it.
const MAX_UNION_MEMBERS: usize = 12;

impl Type {
    /// The union of this type and `other`, normalized: nested unions
    /// flattened, subsumed members absorbed, `mixed` kept unknown.
    pub fn union(self, other: Type) -> Type {
        union_types_impl(self, other)
    }
}

fn union_types_impl(a: Type, b: Type) -> Type {
    syntax::stack::guard(|| {
        // `never` has no values to add.
        if a == Type::never() {
            return normalize_generic_args(b);
        }
        if b == Type::never() {
            return normalize_generic_args(a);
        }
        let (a, b) = (normalize_generic_args(a), normalize_generic_args(b));
        if a == b {
            return a;
        }
        // An error absorbs everything, as a `mixed` does.
        if matches!(a, Type(Repr::Error)) || matches!(b, Type(Repr::Error)) {
            return Type::error();
        }
        // A `mixed` absorbs everything, and stays explicit when either side
        // wrote it.
        if let Some(mixed) = union_with_mixed(&[&a, &b]) {
            return mixed;
        }
        if let Some(merged) = merge_has_offset_value_pair(&a, &b) {
            return merged;
        }
        let mut members = Vec::new();
        match a.into_repr() {
            Repr::Union(m) => members.extend(m),
            other => members.push(Type(other)),
        }
        match b.into_repr() {
            Repr::Union(m) => {
                for x in m {
                    if !members.contains(&x) {
                        members.push(x);
                    }
                }
            }
            other => {
                let other = Type(other);
                if !members.contains(&other) {
                    members.push(other);
                }
            }
        }
        // Same `Mixed`-absorption the fast-path check above already gives a
        // *bare* `Mixed` operand - here for a `Mixed` that only surfaces
        // once flattened out of a multi-member `Union` operand (real shape:
        // `?mixed`'s own union-with-`Null` construction, `types/parse.rs`'s
        // `parse_type_string`, before its own analogous fix), which the
        // fast-path's `a == Repr::Mixed(false, None)`/`b == Repr::Mixed(false, None)`
        // checks never see since neither whole operand is bare `Mixed` -
        // only one of its now-flattened members is. Without this, a
        // `Mixed`-containing union merged with anything else keeps
        // accumulating distinct members (`null|string|mixed`) instead of
        // collapsing to the `mixed` it's vacuously equal to, both visibly
        // (an unsimplified union in a diagnostic message) and functionally
        // (`nullsafe.neverNull` false-firing on a `T|mixed` receiver, since
        // `mixed` already includes `null` but a raw member scan for `Null`
        // specifically wouldn't find it).
        if members.contains(&Type::error()) {
            return Type::error();
        }
        if let Some(mixed) = union_with_mixed(&members.iter().collect::<Vec<_>>()) {
            return mixed;
        }
        members.retain(|m| *m != Type::never());
        // `true|false` is `bool`, and `bool` holds both.
        if members.contains(&Type::bool_literal(true))
            && members.contains(&Type::bool_literal(false))
        {
            let at = members
                .iter()
                .position(|m| matches!(m, Type(Repr::True) | Type(Repr::False)))
                .expect("checked above");
            members[at] = Type::bool();
        }
        if members.contains(&Type::bool()) {
            members.retain(|m| !matches!(m, Type(Repr::True) | Type(Repr::False)));
        }
        // An extension payload whose widened form is already a member adds
        // nothing - the union cap's widening stays put once it happened.
        let widened: Vec<Type> = members
            .iter()
            .filter_map(|m| match m {
                Type(Repr::Extension(data)) => data.widen(),
                _ => None,
            })
            .filter(|w| members.contains(w))
            .collect();
        if !widened.is_empty() {
            members.retain(|m| match m {
                Type(Repr::Extension(data)) => !data.widen().is_some_and(|w| widened.contains(&w)),
                _ => true,
            });
        }
        // `void` joined with `null` is just `null`, as in real PHPStan's
        // `TypeCombinator::union` (a nullsafe call to a `void` method reads
        // as `null`).
        if members.contains(&Type::null()) {
            members.retain(|m| *m != Type::void());
        }
        // A member a benevolent union co-member already holds, as it is or
        // as a literal of it, adds nothing: `int|(int|string)` is
        // `(int|string)`.
        let benevolent: Vec<Vec<Type>> = members
            .iter()
            .filter_map(|m| match m {
                Type(Repr::BenevolentUnion(inner)) => Some(inner.clone()),
                _ => None,
            })
            .collect();
        if !benevolent.is_empty() {
            members.retain(|m| {
                matches!(m, Type(Repr::BenevolentUnion(_)))
                    || !benevolent
                        .iter()
                        .flatten()
                        .any(|b| b == m || literal_base_type(m).is_some_and(|base| base == *b))
            });
        }
        // Drop a literal member whenever its exact base type is also present
        // in the same union - a ternary's two branches
        // (`is_string($x) ? $x : ''`) build exactly this shape,
        // `Type::union(String, StringLiteral(""))`, and keeping both would
        // stop later code from matching `== Repr::String`. Snapshotting
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
                    Type(Repr::Int)
                        | Type(Repr::String)
                        | Type(Repr::Bool)
                        | Type(Repr::Array(_, _))
                        | Type(Repr::Float)
                )
            })
            .cloned()
            .collect();
        members.retain(|m| match literal_base_type(m) {
            Some(base) if matches!(m, Type(Repr::Intersection(_))) => {
                !present_bases.iter().any(|b| {
                    crate::Relation::structural()
                        .is_super_type_of(b, &base)
                        .yes()
                })
            }
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
        // - `Repr::Null` is not a subtype of `Repr::String`, so `string|null`
        // is correctly still kept apart.
        merge_int_ranges(&mut members);
        reduce_array_members(&mut members);
        if members.len() > MAX_UNION_MEMBERS {
            members = generalize_by_key_set(members);
        }
        if members.len() > MAX_UNION_MEMBERS {
            members = widen_extension_types(members);
        }
        if members.len() > MAX_UNION_MEMBERS {
            members = generalize_members(members);
        }
        if members.len() > MAX_UNION_MEMBERS {
            return Type::mixed();
        }
        if members.len() == 1 {
            members.into_iter().next().unwrap()
        } else {
            Type(Repr::Union(members))
        }
    })
}

/// PHPStan's `IntegerRangeType::tryUnion`: a range and every integer
/// constant or range overlapping or touching it become one range
/// (`0|int<1, max>` is `int<0, max>`). Constants with no range among them
/// stay constants (`1|2`). The merged range takes the place of its first
/// member.
pub(crate) fn merge_int_ranges(members: &mut Vec<Type>) {
    if !members
        .iter()
        .any(|m| matches!(m, Type(Repr::IntRange(..))))
    {
        return;
    }
    let mut intervals: Vec<(usize, Option<i64>, Option<i64>, bool)> = members
        .iter()
        .enumerate()
        .filter_map(|(i, m)| match m {
            Type(Repr::IntRange(min, max)) => Some((i, *min, *max, true)),
            Type(Repr::IntLiteral(n)) => Some((i, Some(*n), Some(*n), false)),
            _ => None,
        })
        .collect();
    // Unbounded below sorts first.
    intervals.sort_by_key(|&(_, min, _, _)| min.map_or((0, 0), |n| (1, n)));
    let touches = |max: Option<i64>, min: Option<i64>| match (max, min) {
        (None, _) | (_, None) => true,
        (Some(max), Some(min)) => max.checked_add(1).is_none_or(|next| next >= min),
    };
    let mut replaced: Vec<(usize, Type)> = Vec::new();
    let mut removed: Vec<usize> = Vec::new();
    let mut cluster: Vec<(usize, Option<i64>, Option<i64>, bool)> = Vec::new();
    let mut flush = |cluster: &mut Vec<(usize, Option<i64>, Option<i64>, bool)>| {
        if cluster.len() > 1 && cluster.iter().any(|&(.., is_range)| is_range) {
            let min = cluster[0].1;
            let max = cluster
                .iter()
                .map(|&(_, _, max, _)| max)
                .reduce(|a, b| a.zip(b).map(|(a, b)| a.max(b)))
                .flatten();
            let first = cluster.iter().map(|&(i, ..)| i).min().expect("non-empty");
            let merged = match (min, max) {
                (None, None) => Type::int(),
                (min, max) => Type::int_range(min, max),
            };
            replaced.push((first, merged));
            removed.extend(cluster.iter().map(|&(i, ..)| i).filter(|&i| i != first));
        }
        cluster.clear();
    };
    for interval in intervals {
        let reaches = cluster
            .iter()
            .map(|&(_, _, max, _)| max)
            .reduce(|a, b| a.zip(b).map(|(a, b)| a.max(b)))
            .flatten();
        if !cluster.is_empty() && !touches(reaches, interval.1) {
            flush(&mut cluster);
        }
        cluster.push(interval);
    }
    flush(&mut cluster);
    for (i, merged) in replaced {
        members[i] = merged;
    }
    let mut index = 0;
    members.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
}

/// The union cap's third fallback: each member generalized (a literal to
/// its base type, a range to `int`, a shape to a general array), then
/// merged again - thirteen string literals are `string`, not `mixed`.
fn generalize_members(members: Vec<Type>) -> Vec<Type> {
    let mut generalized: Vec<Type> = Vec::new();
    for m in members {
        let m = m.generalize();
        if !generalized.contains(&m) {
            generalized.push(m);
        }
    }
    if generalized.contains(&Type::bool()) {
        generalized.retain(|m| !matches!(m, Type(Repr::True) | Type(Repr::False)));
    }
    reduce_array_members(&mut generalized);
    generalized
}

/// The union cap's second fallback: each extension payload widened the
/// way its extension allows (distinct payloads stay distinct below the
/// cap), duplicates dropped.
fn widen_extension_types(members: Vec<Type>) -> Vec<Type> {
    let mut out: Vec<Type> = Vec::new();
    for m in members {
        let m = match &m {
            Type(Repr::Extension(data)) => data.widen().unwrap_or(m),
            _ => m,
        };
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
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
pub fn generalize_by_key_set(members: Vec<Type>) -> Vec<Type> {
    // A shape's keys, sorted, and whether it's sealed.
    type KeySet = (Vec<ShapeKey>, bool);
    let mut groups: Vec<(KeySet, Vec<Shape>)> = Vec::new();
    let mut passthrough: Vec<Type> = Vec::new();
    for m in members {
        match m.into_repr() {
            Repr::ArrayShape(shape) => {
                let mut keys: Vec<ShapeKey> = shape.fields.iter().map(|f| f.key.clone()).collect();
                keys.sort();
                let key_set = (keys, shape.is_sealed());
                match groups.iter_mut().find(|(k, _)| *k == key_set) {
                    Some((_, variants)) => variants.push(shape),
                    None => groups.push((key_set, vec![shape])),
                }
            }
            other => passthrough.push(Type(other)),
        }
    }
    for (_, variants) in groups {
        if variants.len() == 1 {
            passthrough.push(Type::array_shape(variants.into_iter().next().unwrap()));
            continue;
        }
        let fields = variants[0]
            .fields
            .iter()
            .map(|first| {
                let mut ty: Option<Type> = None;
                let mut optional = false;
                for shape in &variants {
                    let field = shape.field(&first.key).unwrap();
                    ty = Some(match ty {
                        None => field.ty.clone(),
                        Some(acc) => Type::union(acc, field.ty.clone()),
                    });
                    optional |= field.optional;
                }
                ShapeField::new(first.key.clone(), ty.unwrap(), optional)
            })
            .collect();
        let unsealed = variants
            .iter()
            .filter_map(|shape| shape.unsealed.as_deref().cloned())
            .reduce(|(ka, va), (kb, vb)| (Type::union(ka, kb), Type::union(va, vb)))
            .map(Box::new);
        passthrough.push(Type::array_shape(Shape {
            fields,
            unsealed,
            next_index: None,
        }));
    }
    passthrough
}

/// The base type a literal, range or accessory-refined member widens to
/// (`1` to `int`, `'a'` to `string`, `non-empty-string` to `string`): a
/// union holding that base type already holds the member, so the member
/// goes.
fn literal_base_type(t: &Type) -> Option<Type> {
    match t {
        Type(Repr::IntLiteral(_)) => Some(Type::int()),
        Type(Repr::StringLiteral(_)) => Some(Type::string()),
        Type(Repr::True) | Type(Repr::False) => Some(Type::bool()),
        Type(Repr::FloatLiteral(_)) => Some(Type::float()),
        Type(Repr::IntRange(_, _)) => Some(Type::int()),
        // Scoped to intersections that carry an `Accessory` member, so a
        // genuine, unrelated multi-class intersection
        // (`Countable&Iterator`) is never affected - only an
        // accessory-refined intersection is "more specific than its own
        // base type."
        Type(Repr::Intersection(members))
            if members
                .iter()
                .any(|m| matches!(m, Type(Repr::Accessory(_)))) =>
        {
            members
                .iter()
                .find(|m| !matches!(m, Type(Repr::Accessory(_))))
                .cloned()
        }
        _ => None,
    }
}

impl Type {
    /// Splits a plain `Repr::Array` or a "pure" `hasOffsetValue(...)`-carrying
    /// `Repr::Intersection` (`write_literal_key_aware`'s own construction
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
    pub fn has_offset_value_parts(&self) -> Option<(Type, Vec<(ShapeKey, Type)>)> {
        match self {
            Type(Repr::Array(..)) => Some((self.clone(), Vec::new())),
            Type(Repr::Intersection(members))
                if members.iter().all(|m| {
                    matches!(m, Type(Repr::Array(..)) | Type(Repr::HasOffsetValue(..)))
                }) =>
            {
                let base = members
                    .iter()
                    .find(|m| matches!(m, Type(Repr::Array(..))))?
                    .clone();
                let pairs = members
                    .iter()
                    .filter_map(|m| match m {
                        Type(Repr::HasOffsetValue(k, v)) => Some((k.clone(), (**v).clone())),
                        _ => None,
                    })
                    .collect();
                Some((base, pairs))
            }
            _ => None,
        }
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
/// extra `Repr::Union` member per branch instead of converging back to a
/// single type the way real PHPStan's own merge does - a real corpus
/// regression this fixes (`ActivityService::record()`'s own
/// `$details['method'] = ...`/`$details['script'] = ...`/... sequence:
/// each independent `if` on its own doubled `$details`'s own union
/// member count, hitting `MAX_UNION_MEMBERS` and collapsing the *whole*
/// variable to `Mixed` a few branches in).
///
/// Returns `None` (falling back to `Type::union`' own generic handling)
/// unless *both* operands are one of the two eligible shapes - a
/// genuinely unrelated type on either side (an object, a shape, ...)
/// never reaches this merge at all.
fn merge_has_offset_value_pair(a: &Type, b: &Type) -> Option<Type> {
    let (base_a, pairs_a) = a.has_offset_value_parts()?;
    let (base_b, pairs_b) = b.has_offset_value_parts()?;
    // Neither side tracked anything - the plain `Array`/`Array` case
    // `Type::union`' own generic "merge multiple Array members" step
    // already handles just as well; leave it to that instead of
    // duplicating it here.
    if pairs_a.is_empty() && pairs_b.is_empty() {
        return None;
    }
    let merged_base = match (base_a.into_repr(), base_b.into_repr()) {
        (Repr::Array(ka, va), Repr::Array(kb, vb)) => {
            Type::array(Type::union(*ka, *kb), Type::union(*va, *vb))
        }
        // `has_offset_value_parts`' base is always an array.
        (base_a, _) => Type(base_a),
    };
    let mut merged_pairs: Vec<(ShapeKey, Type)> = Vec::new();
    for (k, va) in &pairs_a {
        if let Some((_, vb)) = pairs_b.iter().find(|(kb, _)| kb == k) {
            merged_pairs.push((k.clone(), Type::union(va.clone(), vb.clone())));
        }
    }
    if merged_pairs.is_empty() {
        return Some(merged_base);
    }
    let mut members = vec![merged_base];
    members.extend(
        merged_pairs
            .into_iter()
            .map(|(k, v)| Type::has_offset_value(k, v)),
    );
    Some(Type(Repr::Intersection(members)))
}

/// The array members of a union, as PHPStan's `processArrayTypes` and
/// `reduceArrays` combine them. With a general `array<K, V>` among them,
/// every shape folds into one general array. Otherwise shapes merge only
/// where nothing is lost: two single-key shapes whose values union into one
/// type, and a shape whose keys another's cover (`is_keys_superset`), so a
/// tagged union (`array{t: 'a', v: int}|array{t: 'b', v: string}`) stays
/// two members and `array{}` stays its own unless an all-optional shape
/// absorbs it.
fn reduce_array_members(members: &mut Vec<Type>) {
    if members.iter().any(|m| general_array_parts(m).is_some()) {
        merge_general_arrays(members);
        return;
    }
    let shape_count = members
        .iter()
        .filter(|m| matches!(m, Type(Repr::ArrayShape(_))))
        .count();
    if shape_count == 0 {
        return;
    }
    if shape_count == 1 {
        return;
    }
    let mut shapes: Vec<Option<Shape>> = Vec::new();
    members.retain(|m| match m {
        Type(Repr::ArrayShape(shape)) => {
            shapes.push(Some(shape.clone()));
            false
        }
        _ => true,
    });
    let relation = crate::Relation::structural();
    // Pairs sharing a key, as `reduceArrays`' first pass takes them.
    for i in 0..shapes.len() {
        for j in (i + 1)..shapes.len() {
            let (Some(a), Some(b)) = (&shapes[i], &shapes[j]) else {
                continue;
            };
            let overlapping = a
                .fields
                .iter()
                .filter(|f| b.field(&f.key).is_some())
                .count();
            if overlapping == 0 {
                continue;
            }
            let single_key_same_type = overlapping == 1
                && a.fields.len() == 1
                && b.fields.len() == 1
                && !matches!(
                    Type::union(a.fields[0].ty.clone(), b.fields[0].ty.clone()),
                    Type(Repr::Union(_))
                );
            if single_key_same_type
                || (overlapping == a.fields.len() && is_keys_superset(&relation, b, a))
            {
                shapes[j] = Some(merge_shapes(b, a, true));
                shapes[i] = None;
                break;
            }
            if overlapping == b.fields.len() && is_keys_superset(&relation, a, b) {
                shapes[i] = Some(merge_shapes(a, b, false));
                shapes[j] = None;
            }
        }
    }
    // Pairs the first pass can't reach: an empty shape, or one with an
    // unsealed tail or optional keys, absorbed into a shape covering it.
    let candidate =
        |s: &Shape| !s.is_sealed() || s.fields.is_empty() || s.fields.iter().any(|f| f.optional);
    for i in 0..shapes.len() {
        for j in (i + 1)..shapes.len() {
            let (Some(a), Some(b)) = (&shapes[i], &shapes[j]) else {
                continue;
            };
            if !candidate(a) && !candidate(b) {
                continue;
            }
            if is_keys_superset(&relation, b, a) {
                shapes[j] = Some(merge_shapes(b, a, true));
                shapes[i] = None;
                break;
            }
            if is_keys_superset(&relation, a, b) {
                shapes[i] = Some(merge_shapes(a, b, false));
                shapes[j] = None;
            }
        }
    }
    members.extend(shapes.into_iter().flatten().map(Type::array_shape));
}

/// A shape as the general array it is: its keys' literal types and its
/// values unioned, an unsealed tail's included. `never` for `array{}`.
/// A general array's parts, as `processArrayTypes` reads them: its key
/// and value types, and whether it is a list and non-empty. `None` for
/// anything but a plain or `list`/`non-empty-array`-refined `array<K, V>`.
fn general_array_parts(ty: &Type) -> Option<(Type, Type, bool, bool)> {
    match ty {
        Type(Repr::Array(key, value)) => Some(((**key).clone(), (**value).clone(), false, false)),
        Type(Repr::Intersection(members)) => {
            let mut array = None;
            let (mut list, mut non_empty) = (false, false);
            for m in members {
                match m {
                    Type(Repr::Array(key, value)) if array.is_none() => {
                        array = Some(((**key).clone(), (**value).clone()));
                    }
                    Type(Repr::Accessory(AccessoryKind::ArrayList)) => list = true,
                    Type(Repr::Accessory(AccessoryKind::NonEmptyArray)) => non_empty = true,
                    _ => return None,
                }
            }
            let (key, value) = array?;
            Some((key, value, list, non_empty))
        }
        _ => None,
    }
}

/// With a general array among `members`, every array member - general,
/// refined or a shape - becomes one general array of their keys and
/// values, a list or non-empty only when every one of them is
/// (`processArrayTypes`): `array{}|non-empty-array<int, V>` is
/// `array<int, V>`.
fn merge_general_arrays(members: &mut Vec<Type>) {
    let mut keys = Vec::new();
    let mut values = Vec::new();
    let (mut list, mut non_empty) = (true, true);
    let mut first = None;
    let mut index = 0;
    members.retain(|m| {
        let parts = match m {
            Type(Repr::ArrayShape(shape)) => {
                let (key, value) = match &shape_as_general_array(shape) {
                    Type(Repr::Array(key, value)) => {
                        (Some((**key).clone()), Some((**value).clone()))
                    }
                    _ => (None, None),
                };
                let shape_non_empty = shape.fields.iter().any(|f| !f.optional);
                Some((key, value, shape.is_list(), shape_non_empty))
            }
            other => {
                general_array_parts(other).map(|(key, value, l, n)| (Some(key), Some(value), l, n))
            }
        };
        index += 1;
        let Some((key, value, l, n)) = parts else {
            return true;
        };
        first.get_or_insert(index - 1);
        keys.extend(key);
        values.extend(value);
        list &= l;
        non_empty &= n;
        false
    });
    let key = keys
        .into_iter()
        .reduce(Type::union)
        .unwrap_or_else(Type::unknown_array_key);
    let value = values
        .into_iter()
        .reduce(Type::union)
        .unwrap_or_else(Type::mixed);
    let mut parts = vec![Type::array(key, value)];
    if list {
        parts.push(Type::accessory(AccessoryKind::ArrayList));
    }
    if non_empty {
        parts.push(Type::accessory(AccessoryKind::NonEmptyArray));
    }
    let merged = crate::intersect_types(parts);
    let at = first.unwrap_or(0).min(members.len());
    members.insert(at, merged);
}

fn shape_as_general_array(shape: &Shape) -> Type {
    let mut keys: Vec<Type> = shape.fields.iter().map(|f| f.key.to_type()).collect();
    let mut values: Vec<Type> = shape.fields.iter().map(|f| f.ty.clone()).collect();
    if let Some(tail) = shape.unsealed.as_deref() {
        keys.push(tail.0.clone());
        values.push(tail.1.clone());
    }
    match (
        keys.into_iter().reduce(Type::union),
        values.into_iter().reduce(Type::union),
    ) {
        (Some(key), Some(value)) => Type::array(key, value),
        _ => Type::never(),
    }
}

/// PHPStan's `ConstantArrayType::isKeysSupersetOf`: whether merging
/// `other` into `this` loses nothing. `this` must have every key `other`
/// has, at most one of them with a value disjoint from `other`'s (and only
/// when both have the same two or more keys - the tag of a tagged union),
/// and at most one required key `other` lacks. An empty `other` fits a
/// shape whose keys are all optional; an unsealed tail absorbs the other
/// side's required keys when it can hold them.
fn is_keys_superset(relation: &crate::Relation, this: &Shape, other: &Shape) -> bool {
    if other.fields.is_empty() && other.is_sealed() {
        return this.fields.iter().all(|f| f.optional);
    }
    let tail_absorbs = |tail_owner: &Shape, fields: &Shape| {
        let Some(tail) = tail_owner.unsealed.as_deref() else {
            return false;
        };
        fields.fields.iter().filter(|f| !f.optional).all(|f| {
            !relation.is_super_type_of(&tail.0, &f.key.to_type()).no()
                && !relation.is_super_type_of(&tail.1, &f.ty).no()
        })
    };
    if !this.is_sealed() && !other.is_sealed() {
        return legacy_is_keys_superset(relation, this, other)
            || tail_absorbs(this, other)
            || tail_absorbs(other, this);
    }
    if !this.is_sealed() {
        return legacy_is_keys_superset(relation, this, other) || tail_absorbs(this, other);
    }
    if !other.is_sealed() {
        return legacy_is_keys_superset(relation, this, other) || tail_absorbs(other, this);
    }
    legacy_is_keys_superset(relation, this, other)
}

fn legacy_is_keys_superset(relation: &crate::Relation, this: &Shape, other: &Shape) -> bool {
    if this.fields.len() < other.fields.len() {
        return false;
    }
    if other.fields.is_empty() {
        return this.fields.is_empty();
    }
    let mut fail_on_different_value =
        this.fields.len() != other.fields.len() || this.fields.len() < 2;
    for other_field in &other.fields {
        let Some(this_field) = this.field(&other_field.key) else {
            return false;
        };
        if !relation
            .is_super_type_of(&other_field.ty, &this_field.ty)
            .no()
        {
            continue;
        }
        if fail_on_different_value {
            return false;
        }
        fail_on_different_value = true;
    }
    this.fields
        .iter()
        .filter(|f| !f.optional && other.field(&f.key).is_none())
        .count()
        <= 1
}

/// `sa` and `sb` as one shape, in `sa`'s key order: a key both have takes
/// both values (`sb`'s first when `sb_first`, as when `sb` came earlier in
/// the source), a key only one has is optional.
fn merge_shapes(sa: &Shape, sb: &Shape, sb_first: bool) -> Shape {
    // An unsealed side's tail can hold any key it doesn't name, so a key
    // named only by the other side may exist there too, with the tail's
    // value type.
    let rest_a = sa.unsealed.as_deref().map(|tail| tail.1.clone());
    let rest_b = sb.unsealed.as_deref().map(|tail| tail.1.clone());
    let mut keys: Vec<&ShapeKey> = sa.fields.iter().map(|f| &f.key).collect();
    for f in &sb.fields {
        if !keys.contains(&&f.key) {
            keys.push(&f.key);
        }
    }
    let mut merged = Shape::sealed(Vec::new());
    for key in keys {
        let field = match (sa.field(key), sb.field(key)) {
            (Some(fa), Some(fb)) => {
                let ty = if sb_first {
                    Type::union(fb.ty.clone(), fa.ty.clone())
                } else {
                    Type::union(fa.ty.clone(), fb.ty.clone())
                };
                ShapeField::new(key.clone(), ty, fa.optional || fb.optional)
            }
            (Some(fa), None) => {
                let ty = match &rest_b {
                    Some(rest) => Type::union(fa.ty.clone(), rest.clone()),
                    None => fa.ty.clone(),
                };
                ShapeField::new(key.clone(), ty, true)
            }
            (None, Some(fb)) => {
                let ty = match &rest_a {
                    Some(rest) => Type::union(fb.ty.clone(), rest.clone()),
                    None => fb.ty.clone(),
                };
                ShapeField::new(key.clone(), ty, true)
            }
            (None, None) => unreachable!("key came from one of the two shapes"),
        };
        merged.fields.push(field);
    }
    merged.unsealed = match (sa.unsealed.as_deref(), sb.unsealed.as_deref()) {
        (Some((ka, va)), Some((kb, vb))) => Some(Box::new((
            Type::union(ka.clone(), kb.clone()),
            Type::union(va.clone(), vb.clone()),
        ))),
        (Some(tail), None) | (None, Some(tail)) => Some(Box::new(tail.clone())),
        (None, None) => None,
    };
    merged
}

/// `ty` with each generic argument's union members deduplicated
/// (`Collection<Dog|Dog>` is `Collection<Dog>`).
fn normalize_generic_args(ty: Type) -> Type {
    match ty.into_repr() {
        Repr::Object(name, args) if args.iter().any(|a| matches!(a, Type(Repr::Union(_)))) => {
            let args = args
                .into_iter()
                .map(|arg| match arg.into_repr() {
                    Repr::Union(members) => members
                        .into_iter()
                        .reduce(Type::union)
                        .unwrap_or(Type::never()),
                    other => Type(other),
                })
                .collect();
            Type::object(name, args)
        }
        other => Type(other),
    }
}

/// The union of `types` when one is `mixed`: `mixed`, explicit when one
/// is written out, still without what every `mixed` among them lacks and
/// no other member adds back (`mixed~null|int` is `mixed~null`,
/// `mixed~null|null` is `mixed`). `None` when none is `mixed`.
fn union_with_mixed(types: &[&Type]) -> Option<Type> {
    let mut explicit = None;
    let mut subtracted: Option<Option<Type>> = None;
    for ty in types {
        if let Type(Repr::Mixed(written, lacks)) = ty {
            explicit = Some(explicit.unwrap_or(false) || *written);
            subtracted = Some(match (subtracted, lacks.as_deref()) {
                (None, lacks) => lacks.cloned(),
                (Some(Some(before)), Some(lacks)) if before == *lacks => Some(before),
                _ => None,
            });
        }
    }
    let explicit = explicit?;
    let mut subtracted = subtracted.flatten();
    for ty in types {
        if !matches!(ty, Type(Repr::Mixed(..))) {
            subtracted = subtracted.map(|s| s.without(ty)).filter(|s| !s.is_never());
        }
    }
    Some(Type(Repr::Mixed(explicit, subtracted.map(Box::new))))
}
