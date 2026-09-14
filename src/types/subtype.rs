use crate::types::{AccessoryKind, Type};

/// Whether `fields` (an `ArrayShape`'s own field list) is a
/// *definite* list - canonical-int-key-sequential from zero *and* no
/// optional field followed by a later, non-optional one. Shared by three
/// call sites (this file, `checker/expr/array_ops.rs`'s `is_list_like`,
/// `checker/calls/format_string.rs`'s `vsprintf`/`vprintf` arg-count
/// check) so each field's own `optional` flag is honored consistently:
/// per real PHPStan's own `ConstantArrayTypeBuilder` algorithm,
/// `array{0: int, 1?: string, 2: int}` is genuinely `Maybe`-a-list (if
/// the optional field 1 is truly absent at runtime, the remaining keys
/// `{0, 2}` aren't list-sequential at all). Staying on this project's
/// own established false-
/// negative-favoring side (a `Maybe` counts as "not a definite list"
/// here, not "close enough") - once an optional field is seen, every
/// field after it must also be optional (a trailing run of optional
/// fields is fine; an optional field immediately followed by a required
/// one is exactly the case that makes the "list" claim unsound).
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

/// The aggregate key type real PHPStan's own `ConstantArrayType::
/// getIterableKeyType()` reports for a shape - the union of each *real*
/// field's own key kind (the `"..."` unsealed-tail sentinel contributes
/// nothing, same reasoning as `Checker::array_key_type`'s own identical
/// skip, `checker/expr/array_ops.rs`). Standalone here (not `checker`-
/// scoped) so `registry::assignability`/this module's own `is_subtype`
/// can compare an `ArrayShape`'s real key type against a `Type::Array`'s
/// declared one, so `array{name: string}` (an all-string-keyed shape) is
/// not wrongly assignable to `array<int, string>`. `Type::Never` for a
/// shape with no real fields
/// at all (every field is `"..."`, or the list is empty) - vacuously a
/// subtype of any key type via `is_subtype`'s own existing
/// `(Never, _) => true` arm just below, so callers need no special case.
///
/// An int-canonical key contributes its own real `Type::IntLiteral`, not
/// the bare `Type::Int` widening - needed so, e.g., `array{0: ...}` (a
/// real, known-non-negative literal `0` key) doesn't wrongly fail against
/// an `array<int<0, max>, ...>`-typed parameter
/// (a real shape, e.g. Doctrine DBAL's own `executeStatement`/several of
/// Piwigo's own local `fetchCount`-style helpers) purely because `Type::
/// Int` isn't a confirmed subtype of `int<0, max>` (it could be
/// negative), even though the one concrete key actually written always
/// is. `IntLiteral -> IntRange` subtyping already exists (`is_subtype`'s
/// own arm just below this function) - this only needed the literal
/// itself preserved, not new subtyping logic.
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

/// Is every value described by `sub` also described by `sup`? This is the
/// same relation PHPStan calls `isSuperTypeOf` (inverted) - the thing every
/// "does this assignment/return/argument type-check" question reduces to.
/// Class hierarchy isn't modeled yet, so `Object` subtyping is name-equality
/// only; self/parent/static are accepted against any `Object` as a
/// placeholder until a class-context resolver exists.
/// Whether `a`/`b` should be treated as the identical type for union
/// deduplication - ordinary `==` for everything except a `list<T>`/
/// `non-empty-list<T>` pair, which can be structurally unequal (a
/// literal `Type::Int` key vs. the generic `unknown_array_key()`
/// sentinel - see the `is_subtype` `Intersection`-vs-`Intersection` list
/// arm's own doc comment for why both exist) while still being the same
/// type PHPStan-visibly. Without this, `union_types` would keep both as
/// separate members whenever a docblock-declared `list<T>` and a
/// runtime-inferred one of the same element type meet in one union,
/// printing a duplicate `list<int>|list<int>` (real shape:
/// `SearchService::getRegularSearchResults()`).
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

        // A shaped callable is always compatible with a bare
        // marker, in both directions - deliberately lenient, matching
        // this file's own established "can't verify, don't guess wrong"
        // pattern one screen down (`ClassString`/`String` interchange-
        // ability). Two *different* shapes are also declared
        // unconditionally compatible rather than attempting real
        // contravariant-parameter/covariant-return comparison
        // (`CallableTypeHelper::isParametersAcceptorSuperTypeOf`'s own
        // job in real PHPStan) - reproducing that correctly needs
        // variance machinery this project deliberately declines to
        // build. This means a
        // parsed callable shape is only ever used *forward* (describing
        // what happens when a value is invoked) and never *backward* (to
        // reject an assignment) - the single biggest false-positive-
        // avoidance call in this whole sub-item.
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
        // Real containment between kinds: every non-falsy string is
        // non-empty; no other pair implies another.
        (
            Type::Accessory(AccessoryKind::NonFalsyString),
            Type::Accessory(AccessoryKind::NonEmptyString),
        ) => true,
        // Mirrors real `AccessoryDecimalIntegerStringType`'s own
        // (non-inverse) `isNumericString`/`isLowercaseString`/
        // `isUppercaseString` (all `TrinaryLogic::createYes()`,
        // `AccessoryDecimalIntegerStringType.php:364-397`) and
        // `isNonEmptyString` (`:374-377`) - a decimal integer string
        // (`"0"`, `"123"`, `"-1"`) is always also numeric, "lowercase" and
        // "uppercase" (it has no letters to fail either check), and
        // non-empty.
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

        // An unbound template parameter is a placeholder, not a real type.
        // Call sites should have already substituted it away
        // (checker::infer_call_return_type); these are the safety net for
        // wherever that doesn't happen.
        //
        // Used as `sub` (the actual/source side), this is bound-aware
        // rather than an unconditional escape hatch - whatever `T`
        // ultimately gets instantiated to is guaranteed to satisfy its
        // own declared bound (real PHPStan's own upper-bound semantics),
        // so `T` is a subtype of `sup` exactly when its bound is. An
        // undeclared bound is `Mixed`, and `is_subtype(Mixed, sup)` is
        // *not* generally true (only when `sup` is `Mixed` too, via the
        // very first arm above) - so the extremely common bound-free
        // `@template T` gets an explicit, separate `true` here.
        //
        // Placed deliberately *before* the `(_, Union(members))` arm
        // below, not just after the ordinary early arms: that arm's own
        // "is `sub` a subtype of *any one* union member" splitting is
        // wrong for a `TemplateParam` whose own bound is *itself* a union
        // (`@template TKey of array-key`, i.e. bound `int|string`) - it
        // recursed into `is_subtype(TemplateParam(TKey), Int)` and
        // `is_subtype(TemplateParam(TKey), String)` *separately*, each
        // one correctly false (`int|string` isn't a subtype of `Int`
        // alone, nor of `String` alone), concluding the *whole* check
        // failed even though `TKey`'s own bound is exactly the target
        // union as a whole. Real corpus shape: doctrine/collections' own
        // `Collection<TKey, T>` self-reference inside its own `map()`/
        // `filter()`/`partition()` methods' `@phpstan-return` tags,
        // where `TKey` (this same class's own template, correctly bound
        // to `array-key`) is passed right back as the argument satisfying
        // that identical bound - `generics.templateBound` would otherwise
        // wrongly fire as if `TKey` didn't satisfy its own declared bound
        // at all.
        (Type::TemplateParam(tp), _) => {
            matches!(*tp.bound, Type::Mixed) || is_subtype(&tp.bound, sup)
        }
        // An unresolved `Conditional` - either it
        // leaked into a position `types::conditional::resolve_conditional`
        // never runs against (a `@param`/`@var` tag, since `parse_type_
        // string` is shared across every tag kind, not scoped to
        // `@return` only, or a `return` *statement*'s own compliance
        // check against its declaring function's signature, which has no
        // call-site argument to resolve the subject against at all), or
        // resolution ran but the subject was left unbound/`Mixed`. Falls
        // back to the eager `Union([if_true, if_false])` comparison -
        // literally builds that `Union` and delegates to `is_subtype`
        // again, rather than hand-rolling a narrower
        // `is_subtype(sub, if_true) || is_subtype(sub, if_false)`-style
        // check: that narrower form would reject a `sub` that's *itself*
        // a `Union` straddling both branches (e.g.
        // `list<string>|list<X>`, entirely valid piecewise) unless the
        // *whole* union happened to fit in just one branch alone -
        // delegating to the real `Union`-vs-`Union` rule two arms below
        // (`.all` over `sub`'s own members, `.any` against the
        // reconstructed branch set) is what correctly allows that.
        // Placed *before* the `Union` arms below, not after: `sub`/`sup`
        // must be able to reach this arm however they're nested (a
        // `Conditional` buried inside a real `Union`, or vice versa)
        // before either generic `Union` arm gets a chance to consume the
        // match first.
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
        // `A & B <: B` is a basic law of intersection types that must hold
        // unconditionally, but with only the two generic rules below
        // (and the identical pair further down for `Intersection`), an
        // `Intersection` whose own member happens to *be* a `Union`
        // (e.g. `Intersection([Void, Union([Null, Bool])])` checked
        // against `Union([Null, Bool])`) fell through to the plain
        // `(_, Union(members)) => any(is_subtype(sub, m))` rule just
        // below - decomposing the *target* Union's own members
        // (`Null`/`Bool`) and asking whether the *whole, undecomposed*
        // intersection satisfies either alone, which it doesn't
        // (correctly - the intersection's other member, `Void`, isn't
        // `Null` either) - never getting a chance to try the
        // intersection's own member-projection instead (`sub_members.any
        // (is_subtype(m, sup))`, which the generic `Intersection`-vs-`_`
        // rule further down already handles correctly on its own, just
        // never reached here because this more general `Union`-sup rule
        // intercepts first). This arm tries both projections whenever
        // both shapes are present at once, succeeding if either proves
        // it - strictly additive (only turns a previously-wrong `false`
        // into the mathematically-correct `true`, never the reverse).
        (Type::Intersection(sub_members), Type::Union(sup_members)) => {
            sub_members.iter().any(|m| is_subtype(m, sup))
                || sup_members.iter().any(|s| is_subtype(sub, s))
        }
        (Type::Union(members), _) => members.iter().all(|m| is_subtype(m, sup)),
        (_, Type::Union(members)) => members.iter().any(|m| is_subtype(sub, m)),

        // An intersection-vs-intersection pair needs its own rule, not
        // just the two generic ones below composed together: `sub`
        // satisfies `sup` as a whole only if it satisfies *every* one of
        // `sup`'s own members (checked via `is_subtype(sub, m)`, which
        // still correctly uses "any one of sub's own members satisfies
        // this single target" for each). Without this, e.g.
        // `non-falsy-string` (`Intersection([String, NonFalsyString])`)
        // failed to prove itself a subtype of `non-empty-string`
        // (`Intersection([String, NonEmptyString])`) even though every
        // non-falsy string genuinely is non-empty: the generic `(sub is
        // Intersection) => any(...)` rule alone only checks whether one
        // *bare* member of `sub` satisfies the whole `sup` intersection,
        // never whether `sub`'s own combination does.
        // A `list<T>`/`non-empty-list<T>` pair needs its own rule ahead
        // of the generic one below: this project builds the exact same
        // `list<T>` shape two different ways that both stay internally
        // consistent on their own but disagree structurally - a
        // docblock-parsed one (`parse_type_string`'s `"list"` arm) uses a
        // literal `Type::Int` key (needed so stripping the list marker
        // later, e.g. `array_diff()`'s own result, still shows a known
        // `array<int, T>` rather than collapsing to a keyless `array<T>`
        // - verified via dumpType()), while every runtime-inferred one
        // (`array_values()`, `sort()`, ...) uses the generic
        // `unknown_array_key()` sentinel instead (deliberately, per
        // `upgrade_array_to_list`'s own doc comment - a list's key is
        // always int *by definition*, so encoding it twice was judged
        // redundant). The generic `Array` key check below is directionally
        // asymmetric between the two (`is_subtype(unknown_array_key(),
        // Int)` is false, `is_subtype(Int, unknown_array_key())` is true),
        // so without this a real runtime `list<int>` failed to satisfy an
        // otherwise-identical docblock-declared `list<int>` parameter/
        // return type - corpus-verified via `SearchService::
        // getRegularSearchResults()`'s own real return-type mismatch.
        // Keys are skipped entirely here (both are provably int either
        // way); only the element type and `NonEmptyArray` need comparing.
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

        // A key-type check alongside the existing value-type
        // one - `array<int, T>` is not a subtype of `array<string, T>`.
        // The common "don't know the real key" case (`Type::
        // unknown_array_key()`, `int|string`) still passes here via the
        // ordinary `Union` subtype rule above, so this is additive
        // precision, not a new source of false positives against the
        // vast majority of call sites that haven't proven a narrower key.
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
