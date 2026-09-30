//! Narrowing a type by what a condition proved: to a checked type, away
//! from an excluded one, by key presence, by truthiness, and the array
//! and integer refinements comparisons and counts prove.

use crate::OrderingOp;
use crate::{AccessoryKind, PhpString, Relation, Repr, Shape, ShapeField, ShapeKey, Type};

/// A comparison's own truth value, canonicalized to "the narrowed side
/// is smaller/smaller-or-equal/greater/greater-or-equal than the bound" -
/// `<`/`<=`/`>`/`>=` all four collapse to Smaller/SmallerOrEqual here (a
/// negated comparison mirrors real PHPStan's own identical `Greater(l,
/// r)` -> `Smaller(r, l)` rewrite, `BinaryOpHandler.php`), so
/// `comparison_narrowed_bounds` only ever has to reason
/// about two shapes, not four.
enum ComparisonOutcome {
    Smaller,
    SmallerOrEqual,
    Greater,
    GreaterOrEqual,
}

impl ComparisonOutcome {
    fn negate(self) -> Self {
        match self {
            ComparisonOutcome::Smaller => ComparisonOutcome::GreaterOrEqual,
            ComparisonOutcome::SmallerOrEqual => ComparisonOutcome::Greater,
            ComparisonOutcome::Greater => ComparisonOutcome::SmallerOrEqual,
            ComparisonOutcome::GreaterOrEqual => ComparisonOutcome::Smaller,
        }
    }
}

impl Relation<'_> {
    /// Narrows `original` to `target` along the branch where a check for
    /// `target` succeeded: if `original` is a union, keep only the member(s)
    /// that match; otherwise just trust the more specific `target`.
    ///
    /// A union member is kept when it's a subtype of
    /// `target`, not just when it's structurally identical to it - exact
    /// equality is the wrong tool for "is this the kind of thing being
    /// narrowed": `is_array()`'s registered target is generic
    /// `Repr::Array(Mixed)`, so a specific-element `Array(String)` member
    /// (from a docblock-refined param type) must survive an `is_array()`
    /// exclusion, even though `Array(String) != Array(Mixed)`. Deliberately
    /// scoped to this `Union(members)` branch only, not the `_ => target.
    /// clone()` fallback below it for a non-`Union` `original` - that
    /// fallback already unconditionally trusts `target` regardless of
    /// `original`'s own content, so there's nothing for `is_super_type_of` to
    /// usefully change there.
    /// `instanceof`'s own true-branch narrowing - deliberately not
    /// `Relation::narrow` above, which unconditionally trusts `target` for a
    /// non-`Union` `original`. That's correct for a scalar predicate
    /// (`is_int($x)` on `int|string` needs nothing more than plain `int` -
    /// mutually exclusive scalar types can't combine), but wrong for
    /// `instanceof` against an unrelated sibling interface/class: `$view:
    /// View` (an interface), `if ($view instanceof HasPageAssets)` (a
    /// second, unrelated interface neither extending nor implementing the
    /// other) needs `View&HasPageAssets`, not bare `HasPageAssets` -
    /// discarding `View` entirely would lose real information a caller
    /// elsewhere still needs (e.g. three independent, sequential
    /// `instanceof` checks on the same parameter). Only builds a real
    /// `Intersection` when neither
    /// side is already a sub/supertype of the other - the common down-cast
    /// (`Base` narrowed to a known `Child`) and nullable-narrowing
    /// (`?Foo` narrowed to `Foo`) cases both still resolve to a single
    /// class/interface exactly as before.
    ///
    /// A union narrows member by member: a member that can't be an object
    /// fails the check and goes, the rest narrow on their own
    /// (`array<int>|Traversable<mixed, int>` to `Traversable<mixed, int>`).
    /// A member already an instance of `target` stays as it is, its
    /// generic arguments included.
    pub fn narrow_to_instance(&self, current: &Type, target: &Type) -> Type {
        if let Repr::Union(members) | Repr::BenevolentUnion(members) = current.repr() {
            let narrowed: Vec<Type> = members
                .iter()
                .filter(|m| !self.is_super_type_of(&Type::object_without_class(), m).no())
                .map(|m| self.narrow_to_instance(m, target))
                .filter(|m| !m.is_never())
                .collect();
            return if narrowed.is_empty() {
                target.clone()
            } else {
                Type::union_of(narrowed)
            };
        }
        if self.is_super_type_of(target, current).yes() {
            return current.clone();
        }
        if self.is_super_type_of(current, target).yes() {
            return target.clone();
        }
        // A nullable `current` (`?Socket`, i.e. `Union([Object("Socket"),
        // Null])`) can never actually be `null` once a true `instanceof`
        // check has passed - `null instanceof AnyClass` is always `false` in
        // real PHP - so `null` is dropped before deciding whether what's
        // left is object-like enough to intersect with. Without this, a
        // nullable property/variable checked against an unrelated
        // interface (`?Socket` checked via `instanceof ResourceStream`)
        // would fall all the way through to the simpler "just trust the
        // checked class" branch below - discarding `Socket` entirely
        // instead of intersecting down to `Socket&ResourceStream` - so
        // every one of `Socket`'s own members would look confirmed-missing
        // on the resulting bare `ResourceStream`.
        let non_null = current.without(&Type::null());
        // Only combined into a real Intersection when every branch of
        // `current` (once stripped of any nullability, per above) is itself
        // provably object-like - anything else (`Mixed`, or a scalar/array
        // type that could never actually satisfy a true `instanceof` in the
        // first place, a sign this project's own upstream inference already
        // went wrong somewhere else) falls back to the simpler "just trust
        // the checked class" behavior instead of risking compounding an
        // unrelated bug into a nonsensical type like `null&ImageId`.
        // Recurses through `Union`/`Intersection` so a second, independent
        // `instanceof` check against the same variable still combines
        // correctly once the first check's own merge has already turned
        // `current` into a real union of object-like branches.
        if non_null.is_object().yes() {
            self.intersect(vec![non_null, target.clone()])
        } else {
            target.clone()
        }
    }
}

impl Relation<'_> {
    /// `original` where a check that its value is of `target` passed. A
    /// member that is a subtype of `target`, through this relation's class
    /// hierarchy, stays whole: a class implementing the target interface
    /// only through an ancestor keeps its own members. Subtyping, not
    /// acceptance: `is_float()` of `int|float` leaves `float`, though a
    /// `float` parameter accepts an `int`.
    pub fn narrow(&self, original: &Type, target: &Type) -> Type {
        match original.repr() {
            Repr::Union(members) | Repr::BenevolentUnion(members) => {
                // A member the target covers stays; one it doesn't gives way
                // to the target's members inside it (`?string` narrowed to
                // `'a'|null` is `'a'|null`).
                let target_members: &[Type] = match target.repr() {
                    Repr::Union(ms) | Repr::BenevolentUnion(ms) => ms,
                    _ => std::slice::from_ref(target),
                };
                let mut kept: Vec<Type> = Vec::new();
                for m in members {
                    if self.is_super_type_of(target, m).yes() {
                        if !kept.contains(m) {
                            kept.push(m.clone());
                        }
                        continue;
                    }
                    for t in target_members {
                        if self.is_super_type_of(m, t).yes() && !kept.contains(t) {
                            kept.push(t.clone());
                        }
                    }
                }
                match kept.len() {
                    0 => target.clone(),
                    1 => kept.into_iter().next().unwrap(),
                    _ => original.with_members(kept),
                }
            }
            // Blindly returning `target` would discard
            // `original` even when `original` was already narrower, wrongly
            // widening it: for `$image_id === $this->sessionService->
            // getRefererImageId()`, where `$image_id` is a definite `int`
            // and `getRefererImageId()` returns `int|null`, overwriting
            // `$image_id` with the broader `int|null` inside the `===` true
            // branch would corrupt every downstream use for the rest of the
            // method. If `original`
            // is already at least as narrow as `target`, real equality
            // proves nothing new about it - keep it unchanged; only fall
            // back to `target` when it's the narrower side (the literal-
            // narrowing case this fallback exists for, `$x === 'foo'`).
            // Same hierarchy-aware upgrade as the `Union` arm above: a single
            // concrete class that only transitively implements `target` (an
            // interface inherited from an ancestor, not declared directly)
            // must still keep its own, more specific type here rather than
            // needlessly widening to the bare interface.
            _ if self.is_super_type_of(target, original).yes() => original.clone(),
            Repr::Mixed(_, Some(lacks)) => target.without(lacks),
            _ => target.clone(),
        }
    }
}

impl Type {
    /// This type without `excluded`, along the branch where a check
    /// for it failed - the "not null" half of a `=== null` guard clause, the
    /// most common narrowing pattern in real code.
    ///
    /// Whatever is a subtype of `excluded` goes, not just what is
    /// identical to it: `is_array(...)`'s false branch excludes
    /// `array<mixed>`, which must drop an `array<string>` too.
    pub fn without(&self, excluded: &Type) -> Type {
        crate::intern::memoized(crate::intern::Operation::Without, &[self, excluded], || {
            self.without_uncached(excluded)
        })
    }

    fn without_uncached(&self, excluded: &Type) -> Type {
        syntax::stack::guard(|| {
            // `mixed` without a type is `mixed~type`: what it can't be is
            // remembered (`isset($x)` leaves `mixed~null`).
            if let Repr::Mixed(explicit, lacks) = self.repr()
                && !matches!(excluded.repr(), Repr::Mixed(..) | Repr::Never | Repr::Error)
            {
                let lacks = match lacks {
                    Some(lacks) => Type::union((*lacks).clone(), excluded.clone()),
                    None => excluded.clone(),
                };
                return Type::new(Repr::Mixed(*explicit, Some(lacks)));
            }
            match self.repr() {
                // Recurses into `Type::without` for each kept member,
                // rather than just cloning it, so a `String`/`Array` member
                // found inside a `Union` gets the same accessory upgrade a
                // bare one does below - a member that doesn't hit a new arm
                // still falls through to the same `other.clone()` it reached
                // directly before.
                Repr::Union(members) | Repr::BenevolentUnion(members) => {
                    let kept: Vec<Type> = members
                        .iter()
                        .filter(|m| {
                            !crate::Relation::structural()
                                .is_super_type_of(excluded, m)
                                .yes()
                        })
                        .map(|m| m.without(excluded))
                        .collect();
                    self.with_members(kept)
                }
                // `$x !== ''` on a plain `string`. Deliberately not extended to an
                // already-Intersection-refined value (e.g. upgrading an existing
                // `non-empty-string` to `non-falsy-string` inside a nested
                // `!empty($x)` check) - a real gap, but a rare, redundant
                // double-guard pattern this project doesn't attempt yet.
                Repr::String if *excluded == Type::string_literal(PhpString::default()) => {
                    Type::new(Repr::Intersection(vec![
                        Type::string(),
                        Type::accessory(AccessoryKind::NonEmptyString),
                    ]))
                }
                // `$x !== []` on a plain `array`/`list` - the same idiom as the
                // `String` arm above: both a plain `array<T>` and an already-
                // `list<T>` upgrade to their own non-empty form. Shares
                // `Type::to_non_empty_array` with `!empty($x)`/`count($x) > 0`
                // so the three
                // "prove this array is non-empty" paths can't drift apart.
                Repr::Array(..) | Repr::ArrayShape(_) | Repr::Intersection(_)
                    if excluded.is_empty_array().yes() =>
                {
                    self.to_non_empty_array()
                }
                // Real PHP's own `Traversable` is a marker interface userland
                // code can never implement directly (`class Foo implements
                // Traversable {}` is a real fatal error, confirmed via `php -r`)
                // - only through one of its exactly two direct subinterfaces,
                // `Iterator` and `IteratorAggregate`. Excluding one therefore
                // structurally proves the other, unlike the general "we can't
                // disprove anything more precise than the type itself" fallback
                // below - real PHPStan special-cases this identical relationship.
                // Without it, e.g. `while ($x instanceof IteratorAggregate) {
                // $x = $x->getIterator(); }`'s loop-exit type stays bare
                // `Traversable` (missing `rewind()`/`valid()`/`current()`)
                // instead of narrowing to the only other structurally possible
                // answer, `Iterator`.
                Repr::Object(name, args)
                    if name == "Traversable"
                        && matches!(excluded.repr(), Repr::Object(excl, _) if excl == "IteratorAggregate") =>
                {
                    Type::object("Iterator".into(), args.clone())
                }
                Repr::Object(name, args)
                    if name == "Traversable"
                        && matches!(excluded.repr(), Repr::Object(excl, _) if excl == "Iterator") =>
                {
                    Type::object("IteratorAggregate".into(), args.clone())
                }
                // Plain `bool` is exactly the two-element set `{true, false}` -
                // unlike the general "a type covering multiple distinct values
                // proves nothing when excluded" caution this function's own
                // callers already apply (`apply_equality_narrowing`'s doc
                // comment), excluding *from* `Bool` is always sound: `True`/
                // `False` are each single values, so knowing
                // one is ruled out leaves exactly the other, unconditionally -
                // no case this could ever get wrong. Without this, `match ($b)
                // { true => ..., false => ... }` (or a bare `if ($b !== true)`)
                // never narrows `$b` down to `false` at all, since `Repr::Bool`
                // isn't itself represented as `Union([True, False])`.
                Repr::Bool if *excluded == Type::bool_literal(true) => Type::bool_literal(false),
                Repr::Bool if *excluded == Type::bool_literal(false) => Type::bool_literal(true),
                _ if crate::Relation::structural()
                    .is_super_type_of(excluded, self)
                    .yes() =>
                {
                    Type::never()
                }
                _ => self.clone(),
            }
        })
    }
}

impl Type {
    pub fn to_non_empty_array(&self) -> Type {
        self.refine_arrays(&Type::accessory(AccessoryKind::NonEmptyArray))
    }

    /// Each member that is an array intersected with `refinement`
    /// (`non-empty-array`, `list`, `hasOffset(k)`), one it rules out
    /// dropped; the rest kept as they are.
    fn refine_arrays(&self, refinement: &Type) -> Type {
        syntax::stack::guard(|| {
            let kept: Vec<Type> = self
                .union_members()
                .iter()
                .map(|m| {
                    if m.is_array().yes() {
                        Relation::structural().intersect(vec![m.clone(), refinement.clone()])
                    } else {
                        m.clone()
                    }
                })
                .filter(|m| !m.is_never())
                .collect();
            self.with_members(kept)
        })
    }
}

/// The new `(min, max)` bound pair for the narrowed side, once `op`
/// (written as `l op r`, `keyed_on_left` saying which of `l`/`r` is the
/// narrowed side) is known true (`branch_true`) or false - `None` only on
/// `i64` overflow computing an exclusive bound's own off-by-one (`bound -
/// 1`/`bound + 1`), an edge real code never hits (a literal compared
/// against `i64::MIN`/`i64::MAX`), where leaving `locals` untouched is the
/// same "give up gracefully" choice every other range-arithmetic helper
/// in this project already makes rather than risk a wrong bound.
pub fn comparison_narrowed_bounds(
    op: OrderingOp,
    keyed_on_left: bool,
    branch_true: bool,
    cur_min: Option<i64>,
    cur_max: Option<i64>,
    bound: i64,
) -> Option<(Option<i64>, Option<i64>)> {
    let outcome = match (op, keyed_on_left) {
        (OrderingOp::Lt, true) | (OrderingOp::Gt, false) => ComparisonOutcome::Smaller,
        (OrderingOp::LtEq, true) | (OrderingOp::GtEq, false) => ComparisonOutcome::SmallerOrEqual,
        (OrderingOp::Gt, true) | (OrderingOp::Lt, false) => ComparisonOutcome::Greater,
        (OrderingOp::GtEq, true) | (OrderingOp::LtEq, false) => ComparisonOutcome::GreaterOrEqual,
    };
    let outcome = if branch_true {
        outcome
    } else {
        outcome.negate()
    };
    match outcome {
        ComparisonOutcome::Smaller => {
            let candidate = bound.checked_sub(1)?;
            Some((
                cur_min,
                Some(cur_max.map_or(candidate, |m| m.min(candidate))),
            ))
        }
        ComparisonOutcome::SmallerOrEqual => {
            Some((cur_min, Some(cur_max.map_or(bound, |m| m.min(bound)))))
        }
        ComparisonOutcome::Greater => {
            let candidate = bound.checked_add(1)?;
            Some((
                Some(cur_min.map_or(candidate, |m| m.max(candidate))),
                cur_max,
            ))
        }
        ComparisonOutcome::GreaterOrEqual => {
            Some((Some(cur_min.map_or(bound, |m| m.max(bound))), cur_max))
        }
    }
}

impl Type {
    /// This array as a list: `array_is_list($x)`'s true branch.
    pub fn to_list(&self) -> Type {
        self.refine_arrays(&Type::accessory(AccessoryKind::ArrayList))
    }
}

impl Type {
    /// A failing `array_is_list($x)`: no list alternative survives, and what
    /// does is non-empty, as an empty array is a list.
    pub fn to_non_list(&self) -> Type {
        match self.repr() {
            Repr::Intersection(members)
                if members.contains(&Type::accessory(AccessoryKind::ArrayList)) =>
            {
                Type::never()
            }
            Repr::Union(members) => {
                let kept: Vec<Type> = members
                    .iter()
                    .map(Type::to_non_list)
                    .filter(|m| *m != Type::never())
                    .collect();
                Type::union_of(kept)
            }
            _ => self.to_non_empty_array(),
        }
    }
}

impl Type {
    /// This type narrowed by whether `key` is (`present = true`) or isn't
    /// (`present = false`) provably a real offset of it -
    /// `array_key_exists($key, $ty)`'s own true/false branches. A member
    /// declaring `key` required can't make the call `false`, and a sealed
    /// member omitting it can't make it `true`; an optional `key` becomes
    /// required when present and is dropped when absent, and a general array
    /// has it when present. Recurses through `Union`; anything else is left
    /// untouched.
    pub fn with_key_presence(&self, key: &ShapeKey, present: bool) -> Type {
        syntax::stack::guard(|| {
            match self.repr() {
                Repr::ArrayShape(shape) => match shape.field(key) {
                    Some(field) if !field.optional => {
                        if present {
                            self.clone()
                        } else {
                            Type::never()
                        }
                    }
                    Some(field) => {
                        let mut shape = Shape::clone(shape);
                        if present {
                            shape.set(ShapeField::new(field.key.clone(), field.ty.clone(), false));
                        } else {
                            shape.fields.retain(|f| f.key != field.key);
                        }
                        Type::array_shape(shape)
                    }
                    // An *unsealed* shape (`array{a: int, ...}`) can carry keys
                    // beyond the ones it lists - `key` simply not
                    // being one of the declared fields is never proof it's
                    // absent, unlike a sealed shape's identical situation just
                    // below. Only the `present` direction is affected: "is `key`
                    // known to be *present*" still correctly stays unprovable
                    // either way (an unsealed shape doesn't promise anything
                    // about a specific unlisted key being there, only that it
                    // *could* be).
                    None if !shape.is_sealed() => self.clone(),
                    None => {
                        if present {
                            Type::never()
                        } else {
                            self.clone()
                        }
                    }
                },
                // A shape refined non-empty: the shape's own answer, still
                // non-empty where the key is absent.
                Repr::Intersection(_) if let Some((shape, _)) = crate::union::shape_parts(self) => {
                    let narrowed = Type::array_shape(shape.clone()).with_key_presence(key, present);
                    match crate::union::shape_parts(&narrowed) {
                        Some((shape, _)) if !present => {
                            crate::union::shape_type(shape.clone(), true)
                        }
                        _ => narrowed,
                    }
                }
                // A general array now has that key.
                Repr::Array(..) | Repr::Intersection(_) if present => {
                    self.refine_arrays(&Type::has_offset(key.clone()))
                }
                Repr::Union(members) => {
                    let kept: Vec<Type> = members
                        .iter()
                        .map(|m| m.with_key_presence(key, present))
                        .filter(|m| *m != Type::never())
                        .collect();
                    Type::union_of(kept)
                }
                _ => self.clone(),
            }
        })
    }
}

impl Type {
    /// The container half of a true `isset($c[key])`: `$c` held an offset
    /// whose value isn't `null`, so it's none of the scalar/null types an
    /// offset read always yields `null` for, and a shape with `key` listed
    /// has it present and non-null. `key` is `None` for a non-literal
    /// subscript.
    pub fn to_isset_container(&self, key: Option<&ShapeKey>) -> Type {
        syntax::stack::guard(|| {
            match self.repr() {
                Repr::Null
                | Repr::Void
                | Repr::Bool
                | Repr::True
                | Repr::False
                | Repr::Int
                | Repr::IntLiteral(_)
                | Repr::IntRange(..)
                | Repr::Float
                | Repr::FloatLiteral(_) => Type::never(),
                Repr::ArrayShape(_) => {
                    let Some(key) = key else {
                        return self.clone();
                    };
                    match self.with_key_presence(key, true).into_repr() {
                        Repr::ArrayShape(shape) => {
                            let mut shape = std::sync::Arc::unwrap_or_clone(shape);
                            // An unsealed shape's tail held it when it isn't
                            // listed.
                            let held = match shape.field(key) {
                                Some(field) => field.ty.clone(),
                                None => shape
                                    .unsealed
                                    .as_deref()
                                    .map(|tail| tail.1.clone())
                                    .unwrap_or(Type::mixed()),
                            };
                            let value = held.without(&Type::null());
                            shape.set(ShapeField::new(key.clone(), value, false));
                            Type::array_shape(shape)
                        }
                        other => Type::new(other),
                    }
                }
                // A general array now has that key, with a non-null value.
                Repr::Array(_, value) => match key {
                    Some(key) => Type::new(Repr::Intersection(vec![
                        self.clone(),
                        Type::has_offset_value(key.clone(), value.without(&Type::null())),
                    ])),
                    None => self.clone(),
                },
                // A shape refined non-empty: the shape's own answer, whose
                // required key keeps it non-empty.
                Repr::Intersection(_) if let Some((shape, _)) = crate::union::shape_parts(self) => {
                    Type::array_shape(shape.clone()).to_isset_container(key)
                }
                // `list<T>`/`non-empty-array<T>` - an accessory-marked array.
                Repr::Intersection(members) => {
                    let element = members.iter().find_map(|m| match m.repr() {
                        Repr::Array(_, value) => Some(value),
                        _ => None,
                    });
                    match (key, element) {
                        (Some(key), Some(value)) => {
                            let mut members = members.clone();
                            members.push(Type::has_offset_value(
                                key.clone(),
                                value.without(&Type::null()),
                            ));
                            Type::new(Repr::Intersection(members))
                        }
                        _ => self.clone(),
                    }
                }
                Repr::Union(members) => {
                    let kept: Vec<Type> = members
                        .iter()
                        .map(|m| m.to_isset_container(key))
                        .filter(|m| *m != Type::never())
                        .collect();
                    Type::union_of(kept)
                }
                _ => self.clone(),
            }
        })
    }
}

impl Type {
    /// The values of this type PHP treats as false (`empty()` true, `!$x`):
    /// each of `null`, `false`, `0`, `0.0`, `''`, `'0'` and `[]` the type
    /// may hold, and a `SimpleXMLElement` (one with no children is
    /// false). A type over a template is left as it is.
    pub fn only_falsy(&self, relation: &Relation) -> Type {
        if self.references_template() {
            return self.clone();
        }
        let candidates = [
            Type::null(),
            Type::bool_literal(false),
            Type::int_literal(0),
            Type::float_literal(0.0),
            Type::string_literal(PhpString::default()),
            Type::string_literal(PhpString::from("0")),
            Type::array_shape(Shape::sealed(Vec::new())),
        ];
        let simple_xml = Type::object("SimpleXMLElement".into(), Vec::new());
        let mut falsy: Vec<Type> = candidates
            .into_iter()
            .filter(|c| relation.is_super_type_of(self, c).yes())
            .collect();
        if !relation.is_super_type_of(self, &simple_xml).no() {
            falsy.push(simple_xml);
        }
        falsy
            .into_iter()
            .reduce(Type::union)
            .unwrap_or_else(Type::never)
    }
}

/// The integers of `int<min, max>` except `0`.
fn without_zero(min: Option<i64>, max: Option<i64>) -> Type {
    let below = min.is_none_or(|m| m < 0);
    let above = max.is_none_or(|m| m > 0);
    let within = below && above;
    if !(within || min == Some(0) || max == Some(0)) {
        return Type::int_range(min, max);
    }
    let mut parts = Vec::new();
    if below {
        parts.push(Type::int_range(min, Some(-1)));
    }
    if above {
        parts.push(Type::int_range(Some(1), max));
    }
    parts
        .into_iter()
        .reduce(Type::union)
        .unwrap_or_else(Type::never)
}

impl Type {
    /// This type without its falsy values, where a truthiness check
    /// (`if ($x)`, `empty($x)`'s false branch) passed: `null`, `false`,
    /// `0`, `0.0`, `''`, `'0'` and `[]` go, `bool` is `true`, an `int` or a
    /// range around `0` loses it (`int<min, -1>|int<1, max>`), a string is
    /// `non-falsy-string` and an array non-empty.
    pub fn without_falsy(&self) -> Type {
        match self.repr() {
            Repr::Union(members) | Repr::BenevolentUnion(members) => {
                let kept: Vec<Type> = members
                    .iter()
                    .map(Type::without_falsy)
                    .filter(|t| *t != Type::never())
                    .collect();
                self.with_members(kept)
            }
            Repr::Null | Repr::False | Repr::IntLiteral(0) => Type::never(),
            Repr::FloatLiteral(f) if *f == 0.0 => Type::never(),
            Repr::Mixed(..) => self.without(&Type::union_of(vec![
                Type::null(),
                Type::bool_literal(false),
                Type::int_literal(0),
                Type::float_literal(0.0),
                Type::string_literal(PhpString::default()),
                Type::string_literal("0".into()),
                Type::array_shape(Shape::sealed(Vec::new())),
            ])),
            Repr::Bool => Type::bool_literal(true),
            Repr::Int => without_zero(None, None),
            Repr::IntRange(min, max) => without_zero(*min, *max),
            Repr::StringLiteral(s) if s.is_empty() || s == "0" => Type::never(),
            Repr::ArrayShape(shape) if shape.is_empty_array() => Type::never(),
            _ if self.is_array().yes() => self.to_non_empty_array(),
            // Delegates to the same general-purpose "add one more accessory
            // fact onto a string, bare or already-refined" helper the
            // `is_numeric()` upgrade already uses - a bare `String` gets the
            // identical `Intersection([String, NonFalsyString])` the old
            // hand-rolled arm built directly, but an already-refined value
            // (e.g. a known `non-empty-string`) now also gets upgraded to
            // the strictly narrower `non-falsy-string`, instead of falling
            // through unchanged.
            _ => self.with_string_accessory(AccessoryKind::NonFalsyString),
        }
    }
}
