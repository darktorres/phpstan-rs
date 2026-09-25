use crate::types::{AccessoryKind, CallableParam, CallableSig, Shape, ShapeKey, Type};

/// Whether `shape` is a *definite* list: sealed, int-keyed sequentially
/// from zero, *and* no optional field followed by a later, non-optional
/// one. Per real PHPStan's own
/// `ConstantArrayTypeBuilder` algorithm, `array{0: int, 1?: string, 2:
/// int}` is only `Maybe`-a-list, since an absent optional field 1 would
/// leave the remaining keys `{0, 2}` non-sequential - a `Maybe` counts
/// as "not a definite list" here, the project's false-negative-favoring
/// default.
pub(crate) fn shape_fields_are_list_like(shape: &Shape) -> bool {
    if !shape.is_sealed() {
        return false;
    }
    let mut seen_optional = false;
    for (i, field) in shape.fields.iter().enumerate() {
        if field.key != ShapeKey::Int(i as i64) {
            return false;
        }
        if seen_optional && !field.optional {
            return false;
        }
        seen_optional |= field.optional;
    }
    true
}

/// The aggregate key type real PHPStan's `ConstantArrayType::
/// getIterableKeyType()` reports for a shape - the union of each
/// field's key kind (an unsealed tail contributes nothing). Lets `registry::assignability`/`is_subtype` compare an
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
pub(crate) fn shape_key_type(shape: &Shape) -> Type {
    let mut key_types: Vec<Type> = Vec::new();
    for field in &shape.fields {
        let kt = match field.key {
            ShapeKey::Int(n) => Type::IntLiteral(n),
            ShapeKey::Str(_) => Type::String,
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
        (_, Type::Mixed(_)) => true,
        (Type::Never, _) => true,
        // A `key-of<T>`/`T[K]` still over a template is what `T`'s bound
        // makes it.
        (Type::KeyOf(_) | Type::OffsetAccess(..), _) => {
            is_subtype(&crate::types::resolve_late_to_bounds(sub), sup)
        }
        (_, Type::KeyOf(_) | Type::OffsetAccess(..)) => {
            is_subtype(sub, &crate::types::resolve_late_to_bounds(sup))
        }
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

        // A shaped callable is always compatible with a bare marker -
        // no shape to compare against on the other side at all.
        (Type::Callable(Some(_)), Type::Callable(None)) => true,
        (Type::Callable(None), Type::Callable(Some(_))) => true,
        // Real contravariant-parameter/covariant-return comparison
        // between two *shaped* callables, reusing the identical
        // variance-composition pattern `registry/assignability.rs`
        // already has for generic template arguments (parameters
        // contravariant, return type covariant is that same pattern's
        // textbook motivating example, not a new concept).
        (Type::Callable(Some(a)), Type::Callable(Some(b))) => {
            callable_sig_is_subtype(a, b, &is_subtype)
        }

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
        (Type::ArrayShape(shape), Type::HasOffset(key)) => {
            shape.field(key).is_some_and(|f| !f.optional)
        }
        (Type::ArrayShape(shape), Type::Accessory(AccessoryKind::NonEmptyArray)) => {
            !shape.fields.is_empty()
        }
        (Type::ArrayShape(shape), Type::Accessory(AccessoryKind::ArrayList)) => {
            shape_fields_are_list_like(shape)
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
            matches!(*tp.bound, Type::Mixed(_)) || is_subtype(&tp.bound, sup)
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
        (
            Type::Intersection(sub_members),
            Type::Union(sup_members) | Type::BenevolentUnion(sup_members),
        ) => {
            sub_members.iter().any(|m| is_subtype(m, sup))
                || sup_members.iter().any(|s| is_subtype(sub, s))
        }
        (Type::Union(members) | Type::BenevolentUnion(members), _) => {
            members.iter().all(|m| is_subtype(m, sup))
        }
        (_, Type::Union(members) | Type::BenevolentUnion(members)) => {
            members.iter().any(|m| is_subtype(sub, m))
        }

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
        (Type::ClassString(_) | Type::ClassKindString(_), Type::String) => true,
        (Type::StringLiteral(_), Type::ClassString(_) | Type::ClassKindString(_)) => true,
        (Type::String, Type::ClassString(_) | Type::ClassKindString(_)) => true,

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
        (Type::ArrayShape(shape), Type::Array(key, b)) => {
            matches!(**b, Type::Mixed(_)) && is_subtype(&shape_key_type(shape), key)
        }

        // An object shape is structurally an object, so it
        // satisfies the bare native `object` hint - the same "shape
        // satisfies the ordinary bare hint" rule `ArrayShape` gets just
        // above, one type family over. Checking whether some *real*
        // class (`Type::Object(class, _)`) structurally has these exact
        // properties needs the registry - `registry::is_assignable`'s
        // own `ObjectShape` arm handles that hierarchy/property-aware
        // direction; this function has no class context to consult.
        (Type::ObjectShape(_), Type::ObjectWithoutClass) => true,
        // Every class instance and enum case is an `object`.
        (Type::Object(..) | Type::EnumCase(..), Type::ObjectWithoutClass) => true,

        (
            Type::SelfRef(_) | Type::ParentRef | Type::StaticRef(_) | Type::This,
            Type::Object(..) | Type::ObjectWithoutClass,
        ) => true,

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

/// `params[i]`, or - once `i` runs past the end - `params`' own trailing
/// variadic parameter standing in for every later position too (a
/// variadic tail absorbs any number of positions beyond where it's
/// written). `None` once `i` is past the end and there's no variadic
/// tail to absorb it - this parameter list simply has nothing to say
/// about position `i` at all.
fn effective_callable_param_at(params: &[CallableParam], i: usize) -> Option<&CallableParam> {
    params
        .get(i)
        .or_else(|| params.last().filter(|p| p.variadic))
}

/// `Type::Callable(Some(a))` vs `Type::Callable(Some(b))`'s own
/// `is_subtype` arm - is `a` safely substitutable wherever `b` is
/// expected? Real contravariant-parameter/covariant-return function-type
/// subtyping: `a`'s return type must satisfy wherever `b`'s return is
/// relied on (covariant), and `a` must accept at least everything `b`
/// itself promises to accept - the same "textbook motivating example"
/// `registry/assignability.rs`'s own generic-template variance
/// composition already models for `Covariant`/`Contravariant` positions,
/// not a new concept. Mirrors real PHPStan's `CallableTypeHelper::
/// isParametersAcceptorSuperTypeOf` on its `accepts` path
/// (`treatMixedAsAny = true`), with `a` as "theirs" (the passed callable)
/// and `b` as "ours" (the accepting one):
///
/// - Only `a`'s own positions are walked (a variadic last parameter of
///   `a` first expanded to cover every extra position of `b`). `a` having
///   *fewer* parameters than `b` is fine: PHP silently drops extra
///   arguments passed to a user-defined callable, so `fn() => 1` stands
///   in for `Closure(int): int`.
/// - A position `b` doesn't have is fine only when `a`'s parameter there
///   is optional or variadic - a required one would never be supplied.
/// - `b` optional where `a` is required is a rejection: `b`'s callers may
///   omit the argument `a` needs.
/// - `mixed` on either side of a parameter or return comparison is
///   treated as "any", the same leniency an ordinary argument check gets
///   below level 9.
///
/// `is_sub` decides each individual parameter/return comparison, so the
/// registry can supply its hierarchy-aware relation while this module
/// passes its own structural one.
pub(crate) fn callable_sig_is_subtype(
    a: &CallableSig,
    b: &CallableSig,
    is_sub: &dyn Fn(&Type, &Type) -> bool,
) -> bool {
    let mixed_as_any = |sub: &Type, sup: &Type| {
        matches!(sub, Type::Mixed(_)) || matches!(sup, Type::Mixed(_)) || is_sub(sub, sup)
    };
    // A `void` return accepts `null` (`VoidType::accepts`).
    let returns_fit = mixed_as_any(&a.return_type, &b.return_type)
        || (*b.return_type == Type::Void && *a.return_type == Type::Null);
    if !returns_fit {
        return false;
    }
    let a_positions = match a.params.last() {
        Some(last) if last.variadic => a.params.len().max(b.params.len()),
        _ => a.params.len(),
    };
    for i in 0..a_positions {
        let Some(ap) = effective_callable_param_at(&a.params, i) else {
            continue;
        };
        let Some(bp) = b.params.get(i) else {
            if !ap.optional && !ap.variadic {
                return false;
            }
            continue;
        };
        if (bp.optional || bp.variadic) && !ap.optional && !ap.variadic {
            return false;
        }
        // A still-unresolved template in the accepting shape's parameter
        // (`callable(T): ...` on a generic method) is inferred from the
        // passed callable itself in real PHPStan - `T` becomes `ap`'s own
        // parameter type - so the only real constraint is that type
        // fitting within `T`'s bound.
        if let Type::TemplateParam(tp) = &bp.ty {
            if !(matches!(*tp.bound, Type::Mixed(_)) || mixed_as_any(&ap.ty, &tp.bound)) {
                return false;
            }
            continue;
        }
        if !mixed_as_any(&bp.ty, &ap.ty) {
            return false;
        }
    }
    true
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

/// A narrow, conservative type-disjointness primitive. Declares two types
/// disjoint in exactly two situations: two different values of one
/// literal kind (`'send'` vs `'list_to_send'`, `1` vs `2`, `true` vs
/// `false`, two cases of one enum, an int literal outside an int range),
/// or two types in different coarse, mutually-exclusive "scalar families"
/// (`scalar_family` below - `string` vs `int`, `array` vs `object`).
/// Recurses into `Union` on either side (every member of a union side
/// must be disjoint from the other side for the whole comparison to
/// count - the same "stay on the lenient side" direction this project's
/// own union-legality check already established).
/// Returns `false` (not confidently disjoint - "don't guess") for
/// anything else: `Mixed`/`Never`/`Void`, two non-literal same-family
/// types (e.g. two unrelated object types - real PHPStan's own
/// object-vs-object disjointness needs a `final`-class/no-subtype-relation
/// check this primitive deliberately doesn't attempt), `Intersection`,
/// `Callable`, `Accessory`, templates, and every pseudo-type.
pub(crate) fn types_definitely_disjoint(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Union(members) | Type::BenevolentUnion(members), _) => {
            members.iter().all(|m| types_definitely_disjoint(m, b))
        }
        (_, Type::Union(members) | Type::BenevolentUnion(members)) => {
            members.iter().all(|m| types_definitely_disjoint(a, m))
        }
        // Two different values of the same kind can never overlap - real
        // PHPStan's `ConstantStringType('send')->isSuperTypeOf(
        // 'list_to_send')` is a definite no, not a maybe.
        (Type::StringLiteral(x), Type::StringLiteral(y)) => x != y,
        (Type::IntLiteral(x), Type::IntLiteral(y)) => x != y,
        (Type::True, Type::False) | (Type::False, Type::True) => true,
        // An open and a closed resource are never the same value.
        (Type::Resource { closed: a }, Type::Resource { closed: b }) => a != b,
        (Type::EnumCase(class_a, case_a), Type::EnumCase(class_b, case_b)) => {
            class_a == class_b && case_a != case_b
        }
        (Type::IntLiteral(n), Type::IntRange(min, max))
        | (Type::IntRange(min, max), Type::IntLiteral(n)) => {
            min.is_some_and(|m| *n < m) || max.is_some_and(|m| *n > m)
        }
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
    Resource,
}

fn scalar_family(ty: &Type) -> Option<ScalarFamily> {
    match ty {
        Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => Some(ScalarFamily::Int),
        Type::Float | Type::FloatLiteral(_) => Some(ScalarFamily::Float),
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) | Type::ClassKindString(_) => {
            Some(ScalarFamily::String)
        }
        Type::Bool | Type::True | Type::False => Some(ScalarFamily::Bool),
        Type::Null => Some(ScalarFamily::Null),
        Type::Array(_, _) | Type::ArrayShape(_) => Some(ScalarFamily::Array),
        Type::Object(_, _)
        | Type::ObjectWithoutClass
        | Type::EnumCase(_, _)
        | Type::ObjectShape(_) => Some(ScalarFamily::Object),
        Type::Resource { .. } => Some(ScalarFamily::Resource),
        _ => None,
    }
}
