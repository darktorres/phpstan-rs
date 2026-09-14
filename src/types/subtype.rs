use crate::types::{AccessoryKind, Type};

/// Whether `fields` (an `ArrayShape`'s field list) is a *definite* list,
/// canonical-int-key-sequential from zero *and* no optional field
/// followed by a later, non-optional one. Per real PHPStan's own
/// `ConstantArrayTypeBuilder` algorithm, `array{0: int, 1?: string, 2:
/// int}` is only `Maybe`-a-list, since an absent optional field 1 would
/// leave the remaining keys `{0, 2}` non-sequential - a `Maybe` counts
/// as "not a definite list" here, the project's false-negative-favoring
/// default.
pub(crate) fn shape_fields_are_list_like(fields: &[(String, Type, bool)]) -> bool {
    let mut seen_optional = false;
    for (i, (k, _, optional)) in fields.iter().enumerate() {
        if k != &i.to_string() {
            return false;
        }
        if seen_optional && !optional {
            return false;
        }
        seen_optional |= optional;
    }
    true
}

/// The aggregate key type real PHPStan's `ConstantArrayType::
/// getIterableKeyType()` reports for a shape - the union of each *real*
/// field's key kind (the `"..."` unsealed-tail sentinel contributes
/// nothing). Lets `registry::assignability`/`is_subtype` compare an
/// `ArrayShape`'s real key type against a `Type::Array`'s declared one,
/// so `array{name: string}` isn't wrongly assignable to
/// `array<int, string>`. `Type::Never` for a shape with no real fields
/// (vacuously a subtype of any key type via the `(Never, _) => true` arm
/// below).
///
/// An int-canonical key contributes its own `Type::IntLiteral`, not the
/// bare `Type::Int` widening - needed so `array{0: ...}` doesn't wrongly
/// fail against an `array<int<0, max>, ...>`-typed parameter (`Type::Int`
/// isn't a confirmed subtype of `int<0, max>`, even though the one
/// concrete key written always is non-negative).
pub(crate) fn shape_key_type(fields: &[(String, Type, bool)]) -> Type {
    let mut key_types: Vec<Type> = Vec::new();
    for (key, _, _) in fields.iter().filter(|(k, _, _)| k != "...") {
        let kt = match crate::types::canonical_int_key(key) {
            Some(n) => Type::IntLiteral(n),
            None => Type::String,
        };
        if !key_types.contains(&kt) {
            key_types.push(kt);
        }
    }
    match key_types.len() {
        0 => Type::Never,
        1 => key_types.into_iter().next().expect("len checked above"),
        _ => Type::Union(key_types),
    }
}

/// Is every value described by `sub` also described by `sup`? The same
/// relation PHPStan calls `isSuperTypeOf` (inverted). Class hierarchy
/// isn't modeled yet, so `Object` subtyping is name-equality only;
/// self/parent/static are accepted against any `Object` as a placeholder
/// until a class-context resolver exists.
/// Whether `a`/`b` should be treated as the identical type for union
/// deduplication - ordinary `==` for everything except a `list<T>`/
/// `non-empty-list<T>` pair, which can be structurally unequal (a
/// literal `Type::Int` key vs. the generic `unknown_array_key()`
/// sentinel - see `is_subtype`'s `Intersection`-vs-`Intersection` list
/// arm) while still the same type PHPStan-visibly. Without this,
/// `union_types` would print a duplicate `list<int>|list<int>` whenever
/// a docblock-declared `list<T>` and a runtime-inferred one of the same
/// element type meet in one union.
pub(crate) fn types_equal_for_union(a: &Type, b: &Type) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Type::Intersection(ma), Type::Intersection(mb))
            if ma.contains(&Type::Accessory(AccessoryKind::ArrayList))
                && mb.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            let keyless = |members: &[Type]| -> Vec<Type> {
                members
                    .iter()
                    .map(|m| match m {
                        Type::Array(_, v) => {
                            Type::Array(Box::new(Type::unknown_array_key()), v.clone())
                        }
                        other => other.clone(),
                    })
                    .collect()
            };
            keyless(ma) == keyless(mb)
        }
        _ => false,
    }
}

pub fn is_subtype(sub: &Type, sup: &Type) -> bool {
    match (sub, sup) {
        (_, Type::Mixed) => true,
        (Type::Never, _) => true,
        // `Type` derives plain structural `PartialEq`, so the
        // `a == b` fast path just below is `false` for `FloatLiteral
        // (f64::NAN)` compared to itself (Rust's own `f64: PartialEq`
        // makes `NAN != NAN`) - needs its own arm for reflexivity to hold
        // (see `types/tests/subtyping_properties.rs`). Mirrors real PHPStan's own
        // `ConstantFloatType::equals()` (`vendor/phpstan-src/src/Type/
        // Constant/ConstantFloatType.php:42-45`), which explicitly
        // special-cases exactly this (`is_nan(a) && is_nan(b)`).
        (Type::FloatLiteral(a), Type::FloatLiteral(b)) if a.is_nan() && b.is_nan() => true,
        (a, b) if a == b => true,

        // A shaped callable is always compatible with a bare marker,
        // and two *different* shapes are unconditionally compatible with
        // each other - deliberately lenient, since real
        // contravariant-parameter/covariant-return comparison
        // (`CallableTypeHelper::isParametersAcceptorSuperTypeOf`) needs
        // variance machinery this project declines to build. A parsed
        // callable shape is thus only ever used *forward* (describing
        // what happens when invoked), never *backward* (to reject an
        // assignment).
        (Type::Callable(Some(_)), Type::Callable(None)) => true,
        (Type::Callable(None), Type::Callable(Some(_))) => true,
        (Type::Callable(Some(_)), Type::Callable(Some(_))) => true,

        (Type::IntLiteral(_), Type::Int) => true,
        (Type::StringLiteral(_), Type::String) => true,
        (Type::True, Type::Bool) | (Type::False, Type::Bool) => true,
        // PHP's one documented exception to its own scalar type
        // declarations, even under strict_types=1: an int value is
        // always accepted wherever a float is declared (int-to-float
        // widening - explicit, load-bearing language behavior).
        (Type::Int | Type::IntLiteral(_), Type::Float) => true,
        (Type::FloatLiteral(_), Type::Float) => true,

        // `int<min, max>` ranges. `IntRange` fully replaces
        // `Int`'s own range (the same relationship `IntLiteral` has to
        // `Int`), so an `IntLiteral`/narrower `IntRange` widens to it
        // the same way a literal widens to its own base type.
        (Type::IntLiteral(n), Type::IntRange(min, max)) => {
            min.is_none_or(|m| *n >= m) && max.is_none_or(|m| *n <= m)
        }
        (Type::IntRange(sub_min, sub_max), Type::IntRange(sup_min, sup_max)) => {
            sup_min.is_none_or(|m| sub_min.is_some_and(|s| s >= m))
                && sup_max.is_none_or(|m| sub_max.is_some_and(|s| s <= m))
        }
        (Type::IntRange(_, _), Type::Int) => true,
        (Type::IntRange(_, _), Type::Float) => true,

        // String/array accessory kinds. A literal directly
        // proves or disproves each kind from its own exact value - no
        // guessing. Only a real, structurally-provable `ArrayShape` (or
        // a value from a known list-producing source, e.g.
        // `array_values()`) can ever prove `ArrayList`/`NonEmptyArray` -
        // a plain `Type::Array(_)` (even with a known element type)
        // deliberately never satisfies either, matching real PHPStan's
        // own confirmed strictness (a `list<int>`-typed parameter
        // rejects even a known-element `int[]` argument).
        (Type::StringLiteral(s), Type::Accessory(AccessoryKind::NonEmptyString)) => !s.is_empty(),
        (Type::StringLiteral(s), Type::Accessory(AccessoryKind::NonFalsyString)) => {
            !s.is_empty() && s != "0"
        }
        (Type::StringLiteral(_), Type::Accessory(AccessoryKind::LiteralString)) => true,
        (Type::StringLiteral(s), Type::Accessory(AccessoryKind::NumericString)) => {
            is_numeric_string_literal(s)
        }
        // Mirrors real `ConstantStringType::isLowercaseString`/
        // `isUppercaseString` (`strtolower($v) === $v`/`strtoupper($v) ===
        // $v`) - ASCII-only, matching PHP 8.2+'s own locale-independent
        // `strtolower`/`strtoupper` (this project's only supported PHP
        // version range).
        (Type::StringLiteral(s), Type::Accessory(AccessoryKind::LowercaseString)) => {
            s.bytes().all(|b| !b.is_ascii_uppercase())
        }
        (Type::StringLiteral(s), Type::Accessory(AccessoryKind::UppercaseString)) => {
            s.bytes().all(|b| !b.is_ascii_lowercase())
        }
        // Mirrors real `ConstantStringType::isDecimalIntegerString`
        // (`(string) (int) $v === $v`) - round-trips through `i64`, so a
        // leading zero (`"007"`), an explicit sign (`"+1"`), `"-0"`, an
        // empty string, or a non-integer numeric string (`"1.0"`) are all
        // correctly excluded.
        (Type::StringLiteral(s), Type::Accessory(AccessoryKind::DecimalIntegerString)) => {
            is_decimal_integer_string_literal(s)
        }
        // Every non-falsy string is non-empty; no other pair implies
        // another.
        (
            Type::Accessory(AccessoryKind::NonFalsyString),
            Type::Accessory(AccessoryKind::NonEmptyString),
        ) => true,
        // A decimal integer string (`"0"`, `"123"`, `"-1"`) is always
        // also numeric, "lowercase" and "uppercase" (no letters to fail
        // either check), and non-empty (mirrors real
        // `AccessoryDecimalIntegerStringType`).
        (
            Type::Accessory(AccessoryKind::DecimalIntegerString),
            Type::Accessory(
                AccessoryKind::NumericString
                | AccessoryKind::LowercaseString
                | AccessoryKind::UppercaseString
                | AccessoryKind::NonEmptyString,
            ),
        ) => true,
        (Type::ArrayShape(fields), Type::Accessory(AccessoryKind::NonEmptyArray)) => {
            fields.iter().any(|(name, _, _)| name != "...")
        }
        (Type::ArrayShape(fields), Type::Accessory(AccessoryKind::ArrayList)) => {
            shape_fields_are_list_like(fields)
        }

        // An unbound template parameter is a placeholder; call sites
        // should have already substituted it away
        // (checker::infer_call_return_type) - this is the safety net.
        //
        // Used as `sub`, bound-aware rather than an unconditional escape
        // hatch: `T` is a subtype of `sup` exactly when its bound is
        // (real PHPStan's own upper-bound semantics). An undeclared
        // bound is `Mixed`, and `is_subtype(Mixed, sup)` isn't generally
        // true, so bound-free `@template T` gets an explicit `true`.
        //
        // Placed *before* the `(_, Union(members))` arm below: that arm's
        // "is `sub` a subtype of *any one* union member" splitting is
        // wrong when a `TemplateParam`'s own bound is *itself* a union
        // (`@template TKey of array-key`) - it would check
        // `is_subtype(TemplateParam(TKey), Int)` and `...String)`
        // *separately* (both false) instead of recognizing `TKey`'s bound
        // as exactly the target union as a whole (real shape:
        // doctrine/collections' `Collection<TKey, T>` self-reference in
        // its own `map()`/`filter()`/`partition()` `@phpstan-return`
        // tags).
        (Type::TemplateParam(tp), _) => {
            matches!(*tp.bound, Type::Mixed) || is_subtype(&tp.bound, sup)
        }
        // An unresolved `Conditional` - either it leaked into a
        // position `resolve_conditional` never runs against (a
        // `@param`/`@var` tag, or a `return` statement's own compliance
        // check, which has no call-site argument to resolve against), or
        // the subject was left unbound/`Mixed`. Falls back to the eager
        // `Union([if_true, if_false])`, delegating to `is_subtype` rather
        // than a narrower `is_subtype(sub, if_true) ||
        // is_subtype(sub, if_false)` check, which would wrongly reject a
        // `sub` that's itself a `Union` straddling both branches (e.g.
        // `list<string>|list<X>`). Placed *before* the `Union` arms
        // below so a `Conditional` nested inside a `Union` (or vice
        // versa) still reaches this arm.
        (
            Type::Conditional {
                if_true, if_false, ..
            },
            _,
        ) => is_subtype(
            &Type::Union(vec![(**if_true).clone(), (**if_false).clone()]),
            sup,
        ),
        (
            _,
            Type::Conditional {
                if_true, if_false, ..
            },
        ) => is_subtype(
            sub,
            &Type::Union(vec![(**if_true).clone(), (**if_false).clone()]),
        ),
        // `A & B <: B` must hold unconditionally, but the generic
        // `(_, Union(members)) => any(is_subtype(sub, m))` rule below
        // would decompose the *target* union's members and ask whether
        // the whole, undecomposed intersection satisfies either alone
        // (e.g. `Intersection([Void, Union([Null, Bool])])` against
        // `Union([Null, Bool])` - correctly false either way alone),
        // never trying the intersection's own member-projection instead.
        // This arm tries both projections, succeeding if either proves
        // it - strictly additive.
        (Type::Intersection(sub_members), Type::Union(sup_members)) => {
            sub_members.iter().any(|m| is_subtype(m, sup))
                || sup_members.iter().any(|s| is_subtype(sub, s))
        }
        (Type::Union(members), _) => members.iter().all(|m| is_subtype(m, sup)),
        (_, Type::Union(members)) => members.iter().any(|m| is_subtype(sub, m)),

        // An intersection-vs-intersection pair needs its own rule:
        // `sub` satisfies `sup` as a whole only if it satisfies *every*
        // one of `sup`'s members - the generic `(sub is Intersection) =>
        // any(...)` rule alone only checks whether one *bare* member of
        // `sub` satisfies the whole `sup` intersection, never whether
        // `sub`'s own combination does (e.g. `non-falsy-string` proving
        // itself a subtype of `non-empty-string`).
        //
        // A `list<T>`/`non-empty-list<T>` pair also needs its own rule:
        // this project builds the same `list<T>` shape two different
        // ways - a docblock-parsed one uses a literal `Type::Int` key
        // (so stripping the list marker later still shows a known
        // `array<int, T>`, not a keyless `array<T>`), while a
        // runtime-inferred one (`array_values()`, `sort()`, ...) uses the
        // generic `unknown_array_key()` sentinel (a list's key is always
        // int by definition, so encoding it twice is redundant - see
        // `upgrade_array_to_list`). The generic `Array` key check below
        // is directionally asymmetric between the two, so without this a
        // real runtime `list<int>` would fail to satisfy an
        // otherwise-identical docblock-declared `list<int>` type. Keys
        // are skipped here (both are provably int either way); only the
        // element type and `NonEmptyArray` need comparing.
        (Type::Intersection(sub_members), Type::Intersection(sup_members))
            if sub_members.contains(&Type::Accessory(AccessoryKind::ArrayList))
                && sup_members.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            fn elem_of(members: &[Type]) -> Option<&Type> {
                members.iter().find_map(|m| match m {
                    Type::Array(_, v) => Some(v.as_ref()),
                    _ => None,
                })
            }
            match (elem_of(sub_members), elem_of(sup_members)) {
                (Some(sub_elem), Some(sup_elem)) if is_subtype(sub_elem, sup_elem) => {
                    !sup_members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray))
                        || sub_members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray))
                }
                _ => false,
            }
        }
        (Type::Intersection(_), Type::Intersection(sup_members)) => {
            sup_members.iter().all(|m| is_subtype(sub, m))
        }
        (Type::Intersection(members), _) => members.iter().any(|m| is_subtype(m, sup)),
        (_, Type::Intersection(members)) => members.iter().all(|m| is_subtype(sub, m)),

        // A key-type check alongside the value-type one - `array<int, T>`
        // is not a subtype of `array<string, T>`. The common "don't know
        // the real key" case (`unknown_array_key()`, `int|string`) still
        // passes via the ordinary `Union` subtype rule above.
        (Type::Array(ka, a), Type::Array(kb, b)) => is_subtype(ka, kb) && is_subtype(a, b),

        // `class-string<T>` stays freely interchangeable with plain
        // `string` in both directions - this project has no way to verify
        // a string's actual content names a real class, so a `::class`
        // literal (now precisely tracked) must still satisfy an ordinary
        // `string`-typed sink, and a plain, unrefined `string` must still
        // satisfy a `class-string<T>`-typed one. Two `ClassString`s
        // compared against each other recurse the same way `Array` does
        // just above, for a `class-string<T>` result flowing into another
        // `class-string<T>`-typed sink without erasing to bare `String`
        // along the way.
        (Type::ClassString(a), Type::ClassString(b)) => is_subtype(a, b),
        (Type::ClassString(_), Type::String) => true,
        (Type::StringLiteral(_), Type::ClassString(_)) => true,
        (Type::String, Type::ClassString(_)) => true,

        // A shape is structurally an array, so it satisfies the bare
        // `array` hint; the reverse (does some homogeneous array actually
        // have these exact keys?) isn't verifiable without shape-aware
        // literal inference, so it's left unrelated rather than guessed.
        // Two shapes are only related via the general `a == b` case above -
        // no shape subtyping (e.g. "has at least these keys") is modeled.
        // On top of the "only when the array's
        // own value type is Mixed" rule (this function does no per-field
        // value comparison of its own - that's `registry::is_assignable`'s
        // job), also require the shape's own real aggregate key type to
        // be a subtype of the array's own declared key type -
        // `array{name: string}` (all-string-keyed) must never satisfy
        // `array<int, mixed>` just because the value side is lenient.
        (Type::ArrayShape(fields), Type::Array(key, b)) => {
            matches!(**b, Type::Mixed) && is_subtype(&shape_key_type(fields), key)
        }

        // An object shape is structurally an object, so it
        // satisfies the bare native `object` hint - the same "shape
        // satisfies the ordinary bare hint" rule `ArrayShape` gets just
        // above, one type family over. Checking whether some *real*
        // class (`Type::Object(class, _)`) structurally has these exact
        // properties needs the registry - `registry::is_assignable`'s
        // own `ObjectShape` arm handles that hierarchy/property-aware
        // direction; this function has no class context to consult.
        (Type::ObjectShape(_), Type::Object(name, _)) if name == "object" => true,

        (Type::SelfRef | Type::ParentRef | Type::StaticRef, Type::Object(..)) => true,

        // A specific case is a real instance of its own enum -
        // the exact-name check here is the same non-hierarchy-aware
        // baseline `Object`-vs-`Object` gets just below; `registry::
        // is_assignable` is where the hierarchy-aware upgrade (matching
        // `UnitEnum`/`BackedEnum`/any interface the enum implements)
        // actually happens, mirroring how plain class inheritance already
        // works one layer up from this function.
        (Type::EnumCase(fqn, _), Type::Object(name, _)) => fqn == name,

        // Used as `sup` (the sink/target side) stays maximally lenient
        // regardless of any declared bound: knowing `sub` satisfies `T`'s
        // *upper* bound doesn't prove it satisfies whatever `T` is
        // actually instantiated to (a `Shape`-bound `T` could still be
        // narrowed to `Circle` alone), so there's no sound tighter check
        // to make here without real instantiation - unlike the `sub` arm
        // above, which only ever needs `T`'s bound to hold, not `T` itself.
        (_, Type::TemplateParam(_)) => true,

        _ => false,
    }
}

/// A practical approximation of PHP's real numeric-string grammar, not
/// a byte-exact port - matches this project's existing "approximation,
/// not full grammar" convention already used for `is_scalar`. Trims
/// leading/trailing whitespace (real PHP's own numeric-string leniency)
/// then accepts an optional sign, digits, an optional single `.` with
/// more digits, and an optional exponent.
pub(crate) fn is_numeric_string_literal(s: &str) -> bool {
    let s = s.trim();
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    let (mantissa, exponent) = match s.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (s, None),
    };
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => {
            !(int_part.is_empty() && frac_part.is_empty())
                && int_part.bytes().all(|b| b.is_ascii_digit())
                && frac_part.bytes().all(|b| b.is_ascii_digit())
        }
        None => !mantissa.is_empty() && mantissa.bytes().all(|b| b.is_ascii_digit()),
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
    });
    mantissa_ok && exponent_ok
}

/// Mirrors real `ConstantStringType::isDecimalIntegerString`
/// exactly - `(string) (int) $v === $v`, ported as a round-trip `i64`
/// parse (PHP's own native int width on every platform this project
/// targets) rather than reimplementing the sign/leading-zero grammar by
/// hand, so the same edge cases (`"007"`, `"+1"`, `"-0"`, `""`, `"1.0"`)
/// are excluded for the identical reason real PHP's int cast excludes
/// them - a mismatched round trip, not a hand-maintained rule list.
pub(crate) fn is_decimal_integer_string_literal(s: &str) -> bool {
    s.parse::<i64>().is_ok_and(|n| n.to_string() == s)
}
