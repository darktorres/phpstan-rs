use crate::{AccessoryKind, ClassHierarchy, Repr, Shape, ShapeField, ShapeKey, Type};

/// How many members a union keeps: past it, its members are generalized
/// (shapes by key set, extension payloads widened, literals to their base
/// types), and only a union still too wide is `mixed`, a fixed point, so a
/// variable past it stays settled across merges. Branches assigning
/// distinct shapes can grow a union combinatorially; 64 members cost next
/// to nothing over 12 on a large project, where 256 cost half again the
/// whole run.
pub const MAX_UNION_MEMBERS: usize = 64;

impl Type {
    /// The union of this type and `other`, normalized: nested unions
    /// flattened, subsumed members absorbed, `mixed` kept unknown.
    pub fn union(self, other: Type) -> Type {
        let arguments = [&self.clone(), &other.clone()];
        crate::intern::memoized(crate::intern::Operation::Union, &arguments, || {
            union_types_impl(self, other, None)
        })
    }
}

impl crate::Relation<'_> {
    /// The union of `a` and `b`, as `Type::union` builds it, but past the
    /// member cap objects fold to the classes they share, which only the
    /// hierarchy knows. Below the cap the two agree, so only operands that
    /// together could reach it are joined again.
    pub fn union(&self, a: Type, b: Type) -> Type {
        if a.union_members().len() + b.union_members().len() <= MAX_UNION_MEMBERS {
            return Type::union(a, b);
        }
        union_types_impl(a, b, Some(self.hierarchy))
    }

    /// The union of `members`, as `union` joins them.
    pub fn union_of(&self, members: Vec<Type>) -> Type {
        members
            .into_iter()
            .reduce(|a, b| self.union(a, b))
            .unwrap_or_else(Type::never)
    }
}

fn union_types_impl(a: Type, b: Type, hierarchy: Option<&dyn ClassHierarchy>) -> Type {
    syntax::stack::guard(|| {
        // `never` has no values to add.
        if a == Type::never() {
            return b;
        }
        if b == Type::never() {
            return a;
        }
        if a == b {
            return a;
        }
        // An error absorbs everything, as a `mixed` does.
        if matches!(a.repr(), Repr::Error) || matches!(b.repr(), Repr::Error) {
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
            other => members.push(Type::new(other)),
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
                let other = Type::new(other);
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
                .position(|m| matches!(m.repr(), Repr::True | Repr::False))
                .expect("checked above");
            members[at] = Type::bool();
        }
        if members.contains(&Type::bool()) {
            members.retain(|m| !matches!(m.repr(), Repr::True | Repr::False));
        }
        // An extension payload whose widened form is already a member adds
        // nothing - the union cap's widening stays put once it happened.
        let widened: Vec<Type> = members
            .iter()
            .filter_map(|m| match m.repr() {
                Repr::Extension(data) => data.widen(),
                _ => None,
            })
            .filter(|w| members.contains(w))
            .collect();
        if !widened.is_empty() {
            members.retain(|m| match m.repr() {
                Repr::Extension(data) => !data.widen().is_some_and(|w| widened.contains(&w)),
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
            .filter_map(|m| match m.repr() {
                Repr::BenevolentUnion(inner) => Some(inner.clone()),
                _ => None,
            })
            .collect();
        if !benevolent.is_empty() {
            members.retain(|m| {
                matches!(m.repr(), Repr::BenevolentUnion(_))
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
                    m.repr(),
                    Repr::Int | Repr::String | Repr::Bool | Repr::Array(_, _) | Repr::Float
                )
            })
            .cloned()
            .collect();
        members.retain(|m| match literal_base_type(m) {
            Some(base) if matches!(m.repr(), Repr::Intersection(_)) => {
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
        absorb_contained_members(&mut members);
        if members.len() > MAX_UNION_MEMBERS {
            members = generalize_by_key_set(members);
        }
        if members.len() > MAX_UNION_MEMBERS {
            members = widen_extension_types(members);
        }
        if members.len() > MAX_UNION_MEMBERS {
            members = fold_value_families(members);
        }
        if members.len() > MAX_UNION_MEMBERS {
            members = fold_objects(members, hierarchy);
        }
        if members.len() > MAX_UNION_MEMBERS {
            members = generalize_members(members);
        }
        if members.len() > MAX_UNION_MEMBERS {
            debug_assert!(
                members
                    .iter()
                    .any(|m| matches!(m.repr(), Repr::Extension(_))),
                "only extension types that refuse to widen outlast the folds"
            );
            return Type::mixed();
        }
        if members.len() == 1 {
            members.into_iter().next().unwrap()
        } else {
            Type::new(Repr::Union(members))
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
        .any(|m| matches!(m.repr(), Repr::IntRange(..)))
    {
        return;
    }
    let mut intervals: Vec<(usize, Option<i64>, Option<i64>, bool)> = members
        .iter()
        .enumerate()
        .filter_map(|(i, m)| match m.repr() {
            Repr::IntRange(min, max) => Some((i, *min, *max, true)),
            Repr::IntLiteral(n) => Some((i, Some(*n), Some(*n), false)),
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

/// PHPStan's `TypeCombinator::union` absorption: a member another member
/// contains adds nothing (`Suit::Hearts|Suit` is `Suit`, `object|Foo` is
/// `object`); of two that contain each other the first stays. Judged
/// without the class hierarchy, so `Base|Child` stays; a member naming a
/// template or not yet resolved (`value-of<T>`, an offset access, a class
/// constant, a conditional) isn't compared, what it contains being open.
fn absorb_contained_members(members: &mut Vec<Type>) {
    if members.len() < 2 {
        return;
    }
    let relation = crate::Relation::structural();
    let settled = |m: &Type| {
        !m.references_template()
            && !m.contains_late()
            && !matches!(
                m.repr(),
                Repr::ValueOf(_)
                    | Repr::ClassConstant { .. }
                    | Repr::Conditional { .. }
                    | Repr::Extension(_)
            )
    };
    let settled: Vec<bool> = members.iter().map(settled).collect();
    // A single value contains only itself, and equal members are already
    // one, so only a member with more than one value can contain another:
    // a union of literals costs nothing to normalize.
    let containers: Vec<usize> = (0..members.len())
        .filter(|&j| settled[j] && !members[j].is_single_value())
        .collect();
    if containers.is_empty() {
        return;
    }
    let dropped: Vec<bool> = (0..members.len())
        .map(|i| {
            let m = &members[i];
            settled[i]
                && containers.iter().any(|&j| {
                    let n = &members[j];
                    j != i
                        && relation.is_super_type_of(n, m).yes()
                        && (j < i || !relation.is_super_type_of(m, n).yes())
                })
        })
        .collect();
    let mut index = 0;
    members.retain(|_| {
        index += 1;
        !dropped[index - 1]
    });
}

/// Past the cap, each family of values as one member: ints and ranges
/// as their hull, floats as `float`, strings as `string` with every
/// refinement all of them have, and array shapes as one shape where a
/// key some lack is optional (a general array past `MAX_SHAPE_FIELDS`).
fn fold_value_families(members: Vec<Type>) -> Vec<Type> {
    let is_int = |m: &Type| {
        matches!(
            m.repr(),
            Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(..)
        )
    };
    let is_float = |m: &Type| matches!(m.repr(), Repr::Float | Repr::FloatLiteral(_));
    let is_string = |m: &Type| {
        m.is_string().yes() && !matches!(m.repr(), Repr::ClassString(_) | Repr::ClassKindString(_))
    };
    let is_shape = |m: &Type| shape_parts(m).is_some();
    let mut out: Vec<Type> = Vec::new();
    let mut folded = |family: &dyn Fn(&Type) -> bool,
                      fold: &dyn Fn(Vec<Type>) -> Type,
                      members: &mut Vec<Type>| {
        let (family_members, rest): (Vec<Type>, Vec<Type>) =
            std::mem::take(members).into_iter().partition(|m| family(m));
        *members = rest;
        if family_members.len() > 1 {
            out.push(fold(family_members));
        } else {
            out.extend(family_members);
        }
    };
    let mut members = members;
    folded(&is_int, &int_hull, &mut members);
    folded(&is_float, &|_| Type::float(), &mut members);
    folded(&is_string, &common_string, &mut members);
    folded(&is_shape, &one_shape, &mut members);
    out.extend(members);
    out
}

/// `int<min, max>` over every member's bounds.
fn int_hull(members: Vec<Type>) -> Type {
    let bounds: Vec<(Option<i64>, Option<i64>)> =
        members.iter().filter_map(Type::int_bounds).collect();
    let min = bounds
        .iter()
        .map(|b| b.0)
        .reduce(|a, b| a.zip(b).map(|(a, b)| a.min(b)))
        .flatten();
    let max = bounds
        .iter()
        .map(|b| b.1)
        .reduce(|a, b| a.zip(b).map(|(a, b)| a.max(b)))
        .flatten();
    match (min, max) {
        (None, None) => Type::int(),
        (min, max) => Type::int_range(min, max),
    }
}

/// `string` with every refinement all of `members` have.
fn common_string(members: Vec<Type>) -> Type {
    [
        AccessoryKind::NonFalsyString,
        AccessoryKind::NonEmptyString,
        AccessoryKind::LowercaseString,
        AccessoryKind::UppercaseString,
        AccessoryKind::LiteralString,
        AccessoryKind::DecimalIntegerString,
        AccessoryKind::NumericString,
        AccessoryKind::NonDecimalIntegerString,
    ]
    .into_iter()
    .filter(|kind| members.iter().all(|m| m.is_refined_string(*kind).yes()))
    .fold(Type::string(), |acc, kind| acc.with_string_accessory(kind))
}

/// The shapes as one: a key some lack optional, values joined.
fn one_shape(members: Vec<Type>) -> Type {
    match Type::new(Repr::Union(members.clone())).combined_shape() {
        Some((shape, non_empty)) if shape.fields.len() <= crate::offsets::MAX_SHAPE_FIELDS => {
            shape_type(shape, non_empty)
        }
        _ => Type::new(Repr::Union(members)),
    }
}

/// Past the cap, objects as the classes they share: an enum's cases as
/// the enum, one generic class's instantiations as that class with each
/// argument joined, and then, with the hierarchy, every class as the
/// most specific ancestors all of them have (their intersection, or
/// `object` when none is common). Without it, `object`.
fn fold_objects(members: Vec<Type>, hierarchy: Option<&dyn ClassHierarchy>) -> Vec<Type> {
    let (objects, mut out): (Vec<Type>, Vec<Type>) =
        members.into_iter().partition(|m| m.is_object().yes());
    if objects.len() < 2 {
        out.extend(objects);
        return out;
    }
    // Each class once, its arguments joined across instantiations.
    let mut classes: Vec<(crate::Name, Vec<Type>)> = Vec::new();
    let mut others: Vec<Type> = Vec::new();
    for object in objects {
        let (class, args) = match object.repr() {
            Repr::EnumCase(class, _) => (class.clone(), Vec::new()),
            Repr::Object(class, args) => (class.clone(), args.clone()),
            _ => {
                others.push(object);
                continue;
            }
        };
        match classes.iter_mut().find(|(c, _)| c.eq_ignore_case(&class)) {
            Some((_, joined)) if joined.len() == args.len() => {
                for (j, a) in joined.iter_mut().zip(args) {
                    *j = Type::union(j.clone(), a);
                }
            }
            // Instantiations that disagree on arity are the raw class.
            Some((_, joined)) => joined.clear(),
            None => classes.push((class, args)),
        }
    }
    let as_objects = |classes: Vec<(crate::Name, Vec<Type>)>| -> Vec<Type> {
        classes
            .into_iter()
            .map(|(class, args)| Type::object(class, args))
            .collect()
    };
    if classes.len() + others.len() + out.len() <= MAX_UNION_MEMBERS {
        out.extend(as_objects(classes));
        out.extend(others);
        return out;
    }
    // An object that isn't one class's (`object`, an intersection, an
    // object shape) shares no class with the rest to fold to.
    out.push(match hierarchy {
        Some(hierarchy) if others.is_empty() => common_supertype(&classes, hierarchy),
        _ => Type::object_without_class(),
    });
    out
}

/// The most specific classes and interfaces every one of `classes` is or
/// extends, as their intersection, each generic one instantiated with the
/// arguments every class binds joined; `object` when there are none.
fn common_supertype(classes: &[(crate::Name, Vec<Type>)], hierarchy: &dyn ClassHierarchy) -> Type {
    let lineage = |class: &crate::Name| -> Vec<crate::Name> {
        let mut names = hierarchy.ancestor_names(class);
        names.push(class.clone());
        names.retain(|n| {
            hierarchy
                .find_class(n)
                .is_some_and(|facts| facts.kind != syntax::ast::ClassKind::Trait)
        });
        names
    };
    let mut common = lineage(&classes[0].0);
    for (class, _) in &classes[1..] {
        let names = lineage(class);
        common.retain(|n| names.iter().any(|m| m.eq_ignore_case(n)));
    }
    // Only the most specific: none another common one extends.
    let minimal: Vec<crate::Name> = common
        .iter()
        .filter(|n| {
            !common
                .iter()
                .any(|m| !m.eq_ignore_case(n) && hierarchy.is_subclass_of(m, n))
        })
        .cloned()
        .collect();
    if minimal.is_empty() {
        return Type::object_without_class();
    }
    let parts: Vec<Type> = minimal
        .into_iter()
        .map(|ancestor| {
            let templates: Vec<crate::Name> = hierarchy
                .find_class(&ancestor)
                .map(|facts| facts.templates.to_vec())
                .unwrap_or_default();
            let args: Vec<Type> = templates
                .iter()
                .map(|template| {
                    classes
                        .iter()
                        .map(|(class, args)| {
                            hierarchy
                                .ancestor_template_bindings(class, args, &ancestor)
                                .get(template.as_ref())
                                .cloned()
                                .unwrap_or_else(Type::mixed)
                        })
                        .reduce(Type::union)
                        .unwrap_or_else(Type::mixed)
                })
                .collect();
            Type::object(ancestor, args)
        })
        .collect();
    crate::intersect_types(parts)
}

/// The union cap's third fallback: each member generalized (a literal to
/// its base type, a range to `int`, a shape to a general array), then
/// merged again - too many string literals are `string`, not `mixed`.
fn generalize_members(members: Vec<Type>) -> Vec<Type> {
    let mut generalized: Vec<Type> = Vec::new();
    for m in members {
        let m = m.generalize();
        if !generalized.contains(&m) {
            generalized.push(m);
        }
    }
    if generalized.contains(&Type::bool()) {
        generalized.retain(|m| !matches!(m.repr(), Repr::True | Repr::False));
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
        let m = match m.repr() {
            Repr::Extension(data) => data.widen().unwrap_or(m),
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
                let shape = std::sync::Arc::unwrap_or_clone(shape);
                let mut keys: Vec<ShapeKey> = shape.fields.iter().map(|f| f.key.clone()).collect();
                keys.sort();
                let key_set = (keys, shape.is_sealed());
                match groups.iter_mut().find(|(k, _)| *k == key_set) {
                    Some((_, variants)) => variants.push(shape),
                    None => groups.push((key_set, vec![shape])),
                }
            }
            other => passthrough.push(Type::new(other)),
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
    match t.repr() {
        Repr::IntLiteral(_) => Some(Type::int()),
        Repr::StringLiteral(_) => Some(Type::string()),
        Repr::True | Repr::False => Some(Type::bool()),
        Repr::FloatLiteral(_) => Some(Type::float()),
        Repr::IntRange(_, _) => Some(Type::int()),
        // Scoped to intersections that carry an `Accessory` member, so a
        // genuine, unrelated multi-class intersection
        // (`Countable&Iterator`) is never affected - only an
        // accessory-refined intersection is "more specific than its own
        // base type."
        Repr::Intersection(members)
            if members
                .iter()
                .any(|m| matches!(m.repr(), Repr::Accessory(_))) =>
        {
            members
                .iter()
                .find(|m| !matches!(m.repr(), Repr::Accessory(_)))
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
        match self.repr() {
            Repr::Array(..) => Some((self.clone(), Vec::new())),
            Repr::Intersection(members)
                if members
                    .iter()
                    .all(|m| matches!(m.repr(), Repr::Array(..) | Repr::HasOffsetValue(..))) =>
            {
                let base = members
                    .iter()
                    .find(|m| matches!(m.repr(), Repr::Array(..)))?
                    .clone();
                let pairs = members
                    .iter()
                    .filter_map(|m| match m.repr() {
                        Repr::HasOffsetValue(k, v) => Some((k.clone(), (*v).clone())),
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
        (Repr::Array(ka, va), Repr::Array(kb, vb)) => Type::array(
            Type::union(Type::clone(&ka), Type::clone(&kb)),
            Type::union(Type::clone(&va), Type::clone(&vb)),
        ),
        // `has_offset_value_parts`' base is always an array.
        (base_a, _) => Type::new(base_a),
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
    Some(Type::new(Repr::Intersection(members)))
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
    let shape_count = members.iter().filter(|m| shape_parts(m).is_some()).count();
    if shape_count <= 1 {
        return;
    }
    // Each shape with whether it's known non-empty; a merge is non-empty
    // when both sides are.
    let mut shapes: Vec<Option<(Shape, bool)>> = Vec::new();
    members.retain(|m| match shape_parts(m) {
        Some((shape, non_empty)) => {
            shapes.push(Some((Shape::clone(shape), non_empty)));
            false
        }
        None => true,
    });
    let relation = crate::Relation::structural();
    // Pairs sharing a key, as `reduceArrays`' first pass takes them.
    for i in 0..shapes.len() {
        for j in (i + 1)..shapes.len() {
            let (Some((a, a_non_empty)), Some((b, b_non_empty))) = (&shapes[i], &shapes[j]) else {
                continue;
            };
            let non_empty = *a_non_empty && *b_non_empty;
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
                    (Type::union(a.fields[0].ty.clone(), b.fields[0].ty.clone())).repr(),
                    Repr::Union(_)
                );
            if single_key_same_type
                || (overlapping == a.fields.len() && is_keys_superset(&relation, b, a))
            {
                shapes[j] = Some((merge_shapes(b, a, true), non_empty));
                shapes[i] = None;
                break;
            }
            if overlapping == b.fields.len() && is_keys_superset(&relation, a, b) {
                shapes[i] = Some((merge_shapes(a, b, false), non_empty));
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
            let (Some((a, a_non_empty)), Some((b, b_non_empty))) = (&shapes[i], &shapes[j]) else {
                continue;
            };
            if !candidate(a) && !candidate(b) {
                continue;
            }
            let non_empty = *a_non_empty && *b_non_empty;
            if is_keys_superset(&relation, b, a) {
                shapes[j] = Some((merge_shapes(b, a, true), non_empty));
                shapes[i] = None;
                break;
            }
            if is_keys_superset(&relation, a, b) {
                shapes[i] = Some((merge_shapes(a, b, false), non_empty));
                shapes[j] = None;
            }
        }
    }
    members.extend(
        shapes
            .into_iter()
            .flatten()
            .map(|(shape, non_empty)| shape_type(shape, non_empty)),
    );
}

/// An array shape, or one refined non-empty (`non-empty-array{a?: int}`),
/// with whether it's known non-empty: refined so, or holding a required
/// key.
pub(crate) fn shape_parts(ty: &Type) -> Option<(&Shape, bool)> {
    match ty.repr() {
        Repr::ArrayShape(shape) => Some((&**shape, shape.fields.iter().any(|f| !f.optional))),
        Repr::Intersection(members) if members.len() == 2 => {
            let shape = members.iter().find_map(|m| match m.repr() {
                Repr::ArrayShape(shape) => Some(&**shape),
                _ => None,
            })?;
            members
                .contains(&Type::accessory(AccessoryKind::NonEmptyArray))
                .then_some((shape, true))
        }
        _ => None,
    }
}

/// `shape` as a type, refined non-empty when it's known to be and no
/// required key already says so. A sealed one known non-empty with a
/// single key has it, and one with none is `never`.
pub(crate) fn shape_type(mut shape: Shape, non_empty: bool) -> Type {
    if non_empty && shape.is_sealed() {
        match shape.fields.as_mut_slice() {
            [] => return Type::never(),
            [only] => only.optional = false,
            _ => {}
        }
    }
    let says_non_empty = shape.fields.iter().any(|f| !f.optional);
    let ty = Type::array_shape(shape);
    if non_empty && !says_non_empty {
        crate::intersect_types(vec![ty, Type::accessory(AccessoryKind::NonEmptyArray)])
    } else {
        ty
    }
}

/// A shape as the general array it is: its keys' literal types and its
/// values unioned, an unsealed tail's included. `never` for `array{}`.
/// A general array's parts, as `processArrayTypes` reads them: its key
/// and value types, and whether it is a list and non-empty. `None` for
/// anything but a plain or `list`/`non-empty-array`-refined `array<K, V>`.
fn general_array_parts(ty: &Type) -> Option<(Type, Type, bool, bool)> {
    match ty.repr() {
        Repr::Array(key, value) => Some(((*key).clone(), (*value).clone(), false, false)),
        Repr::Intersection(members) => {
            let mut array = None;
            let (mut list, mut non_empty) = (false, false);
            for m in members {
                match m.repr() {
                    Repr::Array(key, value) if array.is_none() => {
                        array = Some(((*key).clone(), (*value).clone()));
                    }
                    Repr::Accessory(AccessoryKind::ArrayList) => list = true,
                    Repr::Accessory(AccessoryKind::NonEmptyArray) => non_empty = true,
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
        let parts = match shape_parts(m) {
            Some((shape, shape_non_empty)) => {
                let (key, value) = match shape_as_general_array(shape).repr() {
                    Repr::Array(key, value) => (Some((*key).clone()), Some((*value).clone())),
                    _ => (None, None),
                };
                Some((key, value, shape.is_list(), shape_non_empty))
            }
            None => general_array_parts(m).map(|(key, value, l, n)| (Some(key), Some(value), l, n)),
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

/// The union of `types` when one is `mixed`: `mixed`, explicit when one
/// is written out, still without what every `mixed` among them lacks and
/// no other member adds back (`mixed~null|int` is `mixed~null`,
/// `mixed~null|null` is `mixed`). `None` when none is `mixed`.
fn union_with_mixed(types: &[&Type]) -> Option<Type> {
    let mut explicit = None;
    let mut subtracted: Option<Option<Type>> = None;
    for ty in types {
        if let Repr::Mixed(written, lacks) = ty.repr() {
            explicit = Some(explicit.unwrap_or(false) || *written);
            subtracted = Some(match (subtracted, lacks.as_ref()) {
                (None, lacks) => lacks.cloned(),
                (Some(Some(before)), Some(lacks)) if before == *lacks => Some(before),
                _ => None,
            });
        }
    }
    let explicit = explicit?;
    let mut subtracted = subtracted.flatten();
    for ty in types {
        if !matches!(ty.repr(), Repr::Mixed(..)) {
            subtracted = subtracted.map(|s| s.without(ty)).filter(|s| !s.is_never());
        }
    }
    Some(Type::new(Repr::Mixed(explicit, subtracted)))
}
