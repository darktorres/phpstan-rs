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
    if types::types_equal_for_union(&a, &b) {
        return a;
    }
    if a == Type::Mixed || b == Type::Mixed {
        return Type::Mixed;
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
