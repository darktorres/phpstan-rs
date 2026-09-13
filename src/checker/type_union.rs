use crate::types::{self, Type};
/// A defensive cap on how many distinct members `union_types` will ever
/// materialize into a real `Type::Union`, mirroring the `MAX_GROUP_DEPTH`/
/// `MAX_EXPR_DEPTH` precedent (Phase 20). Collapsing to `Mixed` past this
/// point is a stable fixed point (`union_types(Mixed, _) == Mixed` is this
/// function's own first special case below), so a variable that blows past
/// the cap once stays collapsed rather than being recomputed on every
/// subsequent merge. Found necessary by a real hang, not guessed: `gdb`
/// caught a `check-project` run spinning inside `Type::eq`, called from
/// `union_types`, called from `merge_scopes` (`checker/control_flow.rs`,
/// once per `if`/`else`) and from `write_index_into`'s own `Union` arm
/// (`checker/assignment.rs`, Phase 12 Finding E). Once Finding E made
/// `Type::Union` "stick" across reads/writes instead of immediately
/// collapsing to `Mixed`, a real corpus function with many sequential and
/// nested branches assigning genuinely distinct array shapes to the same
/// variable grew that variable's own union member count combinatorially -
/// each subsequent `contains`-based dedup scan below (and each member's own
/// recursive `PartialEq`) growing right along with it.
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
    // Phase 32 Finding L: drop a literal member whenever its own exact
    // base type is *also* present in the same union - a ternary's two
    // branches (`is_string($x) ? $x : ''`, `check_expr`'s own `Ternary`
    // arm calling this) build exactly this shape,
    // `union_types(String, StringLiteral(""))`, and keeping both was
    // this project's actual root cause for `SectionInitializer::parse()`'s
    // `str_replace()` subject never matching `== Type::String` again
    // after the ternary. Snapshotting which base types are present
    // first, rather than checking `members` from inside its own
    // `retain` closure (which Rust's borrow checker won't allow).
    // Phase 14: extended to `Array`/`Float` alongside the original
    // `Int`/`String`/`Bool` - `literal_base_type` can now also produce
    // an `Array(_)` base (an accessory-refined `list<T>`/`non-empty-
    // array<T>`) or `Float` (a `FloatLiteral`), and this filter has to
    // actually recognize those as "a base type present in this union"
    // or the new absorption arms in `literal_base_type` would never
    // find a match to absorb into.
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
    // Real bug found and fixed, corpus-diff-driven (Piwigo, verified
    // against the real `phpstan/phpstan` phar via `\PHPStan\dumpType()`):
    // a genuinely-empty `ArrayShape` (`array{}`) is a subtype of *any*
    // `Array(key, val)` - vacuously, since an empty array satisfies "every
    // key is `key`-typed, every value is `val`-typed" for every possible
    // `key`/`val` - so keeping both as separate union members is exactly
    // the same kind of redundant-literal-not-absorbed-into-its-own-base
    // shape `literal_base_type` above already fixes for `Int`/`String`/
    // `Bool`/`Float`, just for a variant `literal_base_type` can't express
    // (its own absorbing base depends on *which* `Array(_, _)` member is
    // already present, not a fixed, context-free mapping). Concretely:
    // `$counter = []; foreach ($keys as $k) { $counter[$k] = 1; }` merges
    // the loop's own zero-iteration entry state (`array{}`, from
    // `LoopZeroIterations::Possible`'s own symmetric `merge_scopes`) with
    // the body's own accumulated `array<string, 1>` - real PHPStan's own
    // post-loop type (confirmed via `dumpType()`) is the *unqualified*
    // `array<string, 1>`, no `array{}` alternative surviving at all, since
    // it's already redundant once unioned with the general map. Without
    // this, `index_result_type`'s own separately-correct "an empty shape
    // plus a *dynamic* key reads `null`" rule (a real, deliberate fix for
    // a *different* bug - mid-loop auto-vivification during
    // `++$counts[$c]`) fired on the still-surviving empty-shape member,
    // unioning a bogus `null` into an otherwise-precise `$counter[$key]`
    // read - the single largest false-positive source found in a
    // real-PHPStan-vs-phpstan-rs corpus diff (33 of 70 `argument.type`/
    // `return.type` findings in one scoped Piwigo run shared this exact
    // signature). Deliberately scoped to an *empty* shape only - a
    // non-empty `ArrayShape` needs real field-by-field subtype checking
    // against the specific `Array(key, val)` co-member to be absorbed
    // safely, a real but separate, narrower feature this fix doesn't
    // attempt. Does not affect a whole-variable reassignment merge like
    // `$file_path = null; foreach (...) { $file_path = $row; }` -
    // `Type::Null` is not a subtype of `Type::String`, so `string|null`
    // is correctly still kept apart (confirmed against real PHPStan's own
    // `dumpType()` output for that exact shape too).
    let has_general_array = members.iter().any(|m| matches!(m, Type::Array(_, _)));
    if has_general_array {
        members.retain(|m| !matches!(m, Type::ArrayShape(fields) if fields.is_empty()));
    }
    // Phase 68.3: two structurally different `Array(key, val)` members
    // never merged into one on their own before this - each stayed its
    // own separate union member instead of the single, precise
    // `Array(key1|key2, val1|val2)` real PHPStan would infer (`array_
    // element_type`'s own `Union` arm, and every other consumer that
    // wants "the" element type of a plain array, silently only ever saw
    // the *first* matching member). Real corpus false positive this
    // fixed: `vendor/symfony/var-exporter/ProxyHelper.php`'s dynamic-key
    // index-assignment inside a loop's `if`-branch (assigning an object,
    // then `continue`) and its own fall-through path (assigning `false`)
    // each built their own separate `Array(string, Foo)`/`Array(string,
    // false)` member instead of merging into one `Array(string,
    // Foo|false)` - confirmed via an isolated repro producing four
    // separate, redundant `array<...>` union members instead of one.
    // Deliberately scoped to a bare `Type::Array` only, not an
    // accessory-refined `list<T>`/`non-empty-array<T>` (a real
    // `Intersection`) - a mix of the two is a rarer shape not needed to
    // fix this repro, left as a real, if narrower, residual.
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
/// unioned field types - only tried once the cap would otherwise fire,
/// never applied to a union already under it, so a small union's
/// behavior is completely unaffected. Two `ArrayShape`s merge only when
/// their field-name *sets* are identical (order-independent) -
/// deliberately not a fuzzier "mostly-the-same-shape" heuristic. This
/// directly targets the exact failure pattern that caused the real hang
/// `MAX_UNION_MEMBERS` itself was added to fix (many sequential/nested
/// branches repeatedly writing the *same* known keys with different
/// value types onto one variable), without touching the cap itself,
/// which stays as the safety net for whatever's left ungeneralizable
/// (e.g. a variable whose branches genuinely have *different* key sets -
/// real, inherent branching precision, correctly left untouched here,
/// still protected by the hard cap if it ever grows past it).
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
        // Phase 14: scoped to intersections that actually carry an
        // `Accessory` member, so a genuine, unrelated multi-class
        // intersection (`Countable&Iterator`) is never affected - only
        // this plan's own accessory-refined intersections should ever
        // be treated as "more specific than their own base type."
        Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Accessory(_))) => {
            members
                .iter()
                .find(|m| !matches!(m, Type::Accessory(_)))
                .cloned()
        }
        _ => None,
    }
}
