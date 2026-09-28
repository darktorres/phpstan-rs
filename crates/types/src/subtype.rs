use crate::{
    AccessoryKind, CallableParam, CallableSig, Relation, Repr, Shape, ShapeKey, Type, Variance,
};

impl Shape {
    /// Whether `shape` is a *definite* list: sealed, int-keyed sequentially
    /// from zero, *and* no optional field followed by a later, non-optional
    /// one. Per real PHPStan's own
    /// `ConstantArrayTypeBuilder` algorithm, `array{0: int, 1?: string, 2:
    /// int}` is only `Maybe`-a-list, since an absent optional field 1 would
    /// leave the remaining keys `{0, 2}` non-sequential - a `Maybe` counts
    /// as "not a definite list" here, the project's false-negative-favoring
    /// default.
    pub fn is_list(&self) -> bool {
        if !self.is_sealed() {
            return false;
        }
        let mut seen_optional = false;
        for (i, field) in self.fields.iter().enumerate() {
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
}

impl Shape {
    /// The aggregate key type real PHPStan's `ConstantArrayType::
    /// getIterableKeyType()` reports for a shape - the union of each
    /// field's key kind (an unsealed tail contributes nothing). Lets `registry::assignability`/`is_super_type_of` compare an
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
    pub fn key_type(&self) -> Type {
        let mut key_types: Vec<Type> = Vec::new();
        for field in &self.fields {
            let kt = match field.key {
                ShapeKey::Int(n) => Type(Repr::IntLiteral(n)),
                ShapeKey::Str(_) => Type(Repr::String),
            };
            if !key_types.contains(&kt) {
                key_types.push(kt);
            }
        }
        match key_types.len() {
            0 => Type(Repr::Never),
            1 => key_types.into_iter().next().expect("len checked above"),
            _ => Type(Repr::Union(key_types)),
        }
    }
}

impl Type {
    /// Is every value described by `sub` also described by `sup`? The same
    /// relation PHPStan calls `isSuperTypeOf` (inverted). Class hierarchy
    /// isn't modeled yet, so `Object` subtyping is name-equality only;
    /// self/parent/static are accepted against any `Object` as a placeholder
    /// until a class-context resolver exists.
    /// Whether `a`/`b` should be treated as the identical type for union
    /// deduplication - ordinary `==` for everything except a `list<T>`/
    /// `non-empty-list<T>` pair, which can be structurally unequal (a
    /// literal `Type::Int` key vs. the generic `unknown_array_key()`
    /// sentinel - see `is_super_type_of`'s `Intersection`-vs-`Intersection` list
    /// arm) while still the same type PHPStan-visibly. Without this,
    /// `union_types` would print a duplicate `list<int>|list<int>` whenever
    /// a docblock-declared `list<T>` and a runtime-inferred one of the same
    /// element type meet in one union.
    pub fn equals_for_union(&self, b: &Type) -> bool {
        if self == b {
            return true;
        }
        match (self, b) {
            (Type(Repr::Intersection(ma)), Type(Repr::Intersection(mb)))
                if ma.contains(&Type(Repr::Accessory(AccessoryKind::ArrayList)))
                    && mb.contains(&Type(Repr::Accessory(AccessoryKind::ArrayList))) =>
            {
                let keyless = |members: &[Type]| -> Vec<Type> {
                    members
                        .iter()
                        .map(|m| match m {
                            Type(Repr::Array(_, v)) => {
                                Type(Repr::Array(Box::new(Type::unknown_array_key()), v.clone()))
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
}

impl Relation<'_> {
    /// The structural half of `is_super_type_of`'s `Yes`: every value of
    /// `sub` provably is a `sup`, for the pairs `is_super_type_of` doesn't
    /// answer itself first.
    pub(crate) fn proves_super_type(&self, sup: &Type, sub: &Type) -> bool {
        match (sub, sup) {
            // A shaped callable is always compatible with a bare marker.
            (Type(Repr::Callable(Some(_))), Type(Repr::Callable(None))) => true,
            // Parameters contravariant, the return covariant.
            (Type(Repr::Callable(Some(a))), Type(Repr::Callable(Some(b)))) => {
                callable_sig_is_subtype(a, b, &|x, y| self.is_super_type_of(y, x).yes())
            }

            (Type(Repr::IntLiteral(_)), Type(Repr::Int)) => true,
            (Type(Repr::StringLiteral(_)), Type(Repr::String)) => true,
            // `$this`, `static`, `self` and `parent` are always objects.
            (
                Type(Repr::This)
                | Type(Repr::StaticRef(_))
                | Type(Repr::SelfRef(_))
                | Type(Repr::ParentRef),
                Type(Repr::ObjectWithoutClass),
            ) => true,
            (Type(Repr::True), Type(Repr::Bool)) | (Type(Repr::False), Type(Repr::Bool)) => true,
            // PHP's one documented exception to its own scalar type
            // declarations, even under strict_types=1: an int value is
            // always accepted wherever a float is declared (int-to-float
            // widening - explicit, load-bearing language behavior).
            (Type(Repr::FloatLiteral(_)), Type(Repr::Float)) => true,

            // `int<min, max>` ranges. `IntRange` fully replaces
            // `Int`'s own range (the same relationship `IntLiteral` has to
            // `Int`), so an `IntLiteral`/narrower `IntRange` widens to it
            // the same way a literal widens to its own base type.
            (Type(Repr::IntLiteral(n)), Type(Repr::IntRange(min, max))) => {
                min.is_none_or(|m| *n >= m) && max.is_none_or(|m| *n <= m)
            }
            (Type(Repr::IntRange(sub_min, sub_max)), Type(Repr::IntRange(sup_min, sup_max))) => {
                sup_min.is_none_or(|m| sub_min.is_some_and(|s| s >= m))
                    && sup_max.is_none_or(|m| sub_max.is_some_and(|s| s <= m))
            }
            (Type(Repr::IntRange(_, _)), Type(Repr::Int)) => true,

            // String/array accessory kinds. A literal directly
            // proves or disproves each kind from its own exact value - no
            // guessing. Only a real, structurally-provable `ArrayShape` (or
            // a value from a known list-producing source, e.g.
            // `array_values()`) can ever prove `ArrayList`/`NonEmptyArray` -
            // a plain `Type::Array(_)` (even with a known element type)
            // deliberately never satisfies either, matching real PHPStan's
            // own confirmed strictness (a `list<int>`-typed parameter
            // rejects even a known-element `int[]` argument).
            (
                Type(Repr::StringLiteral(s)),
                Type(Repr::Accessory(AccessoryKind::NonEmptyString)),
            ) => !s.is_empty(),
            (
                Type(Repr::StringLiteral(s)),
                Type(Repr::Accessory(AccessoryKind::NonFalsyString)),
            ) => !s.is_empty() && s != "0",
            (Type(Repr::StringLiteral(_)), Type(Repr::Accessory(AccessoryKind::LiteralString))) => {
                true
            }
            (Type(Repr::StringLiteral(s)), Type(Repr::Accessory(AccessoryKind::NumericString))) => {
                s.as_str().is_some_and(is_numeric_string_literal)
            }
            // Mirrors real `ConstantStringType::isLowercaseString`/
            // `isUppercaseString` (`strtolower($v) === $v`/`strtoupper($v) ===
            // $v`) - ASCII-only, matching PHP 8.2+'s own locale-independent
            // `strtolower`/`strtoupper` (this project's only supported PHP
            // version range).
            (
                Type(Repr::StringLiteral(s)),
                Type(Repr::Accessory(AccessoryKind::LowercaseString)),
            ) => s.as_bytes().iter().all(|b| !b.is_ascii_uppercase()),
            (
                Type(Repr::StringLiteral(s)),
                Type(Repr::Accessory(AccessoryKind::UppercaseString)),
            ) => s.as_bytes().iter().all(|b| !b.is_ascii_lowercase()),
            // Mirrors real `ConstantStringType::isDecimalIntegerString`
            // (`(string) (int) $v === $v`) - round-trips through `i64`, so a
            // leading zero (`"007"`), an explicit sign (`"+1"`), `"-0"`, an
            // empty string, or a non-integer numeric string (`"1.0"`) are all
            // correctly excluded.
            (
                Type(Repr::StringLiteral(s)),
                Type(Repr::Accessory(AccessoryKind::DecimalIntegerString)),
            ) => s.as_str().is_some_and(is_decimal_integer_string_literal),
            // Every non-falsy string is non-empty, and so is every numeric
            // one (`""` isn't numeric).
            (
                Type(Repr::Accessory(AccessoryKind::NonFalsyString | AccessoryKind::NumericString)),
                Type(Repr::Accessory(AccessoryKind::NonEmptyString)),
            ) => true,
            // A decimal integer string (`"0"`, `"123"`, `"-1"`) is always
            // also numeric, "lowercase" and "uppercase" (no letters to fail
            // either check), and non-empty (mirrors real
            // `AccessoryDecimalIntegerStringType`).
            (
                Type(Repr::Accessory(AccessoryKind::DecimalIntegerString)),
                Type(Repr::Accessory(
                    AccessoryKind::NumericString
                    | AccessoryKind::LowercaseString
                    | AccessoryKind::UppercaseString
                    | AccessoryKind::NonEmptyString,
                )),
            ) => true,
            (Type(Repr::ArrayShape(shape)), Type(Repr::HasOffset(key))) => {
                shape.field(key).is_some_and(|f| !f.optional)
            }
            (
                Type(Repr::ArrayShape(shape)),
                Type(Repr::Accessory(AccessoryKind::NonEmptyArray)),
            ) => shape.fields.iter().any(|f| !f.optional),
            // An array with a known offset isn't empty.
            (
                Type(Repr::HasOffset(_)) | Type(Repr::HasOffsetValue(..)),
                Type(Repr::Accessory(AccessoryKind::NonEmptyArray)),
            ) => true,
            (Type(Repr::ArrayShape(shape)), Type(Repr::Accessory(AccessoryKind::ArrayList))) => {
                shape.is_list()
            }

            // An unresolved `Conditional` - either it leaked into a
            // position `resolve_conditional` never runs against, or its
            // subject was left unbound - is either branch.
            (
                _,
                Type(Repr::Conditional {
                    if_true, if_false, ..
                }),
            ) => self
                .is_super_type_of(
                    &Type(Repr::Union(vec![(**if_true).clone(), (**if_false).clone()])),
                    sub,
                )
                .yes(),
            (Type(Repr::Intersection(_)), Type(Repr::Intersection(sup_members))) => sup_members
                .iter()
                .all(|m| self.is_super_type_of(m, sub).yes()),
            (Type(Repr::Intersection(members)), _) => {
                members.iter().any(|m| self.is_super_type_of(sup, m).yes())
            }
            (_, Type(Repr::Intersection(members))) => {
                members.iter().all(|m| self.is_super_type_of(m, sub).yes())
            }

            // A key-type check alongside the value-type one - `array<int, T>`
            // is not a subtype of `array<string, T>`. The common "don't know
            // the real key" case (`unknown_array_key()`, `int|string`) still
            // passes via the ordinary `Union` subtype rule above.
            (Type(Repr::Array(ka, a)), Type(Repr::Array(kb, b))) => {
                self.is_super_type_of(kb, ka).yes() && self.is_super_type_of(b, a).yes()
            }

            // `class-string<T>` is a string; two recurse like `Array` does.
            (Type(Repr::ClassString(a)), Type(Repr::ClassString(b))) => {
                self.is_super_type_of(b, a).yes()
            }
            (Type(Repr::ClassString(_)) | Type(Repr::ClassKindString(_)), Type(Repr::String)) => {
                true
            }
            (Type(Repr::ClassString(a)), Type(Repr::ClassKindString(kind))) => match &**a {
                Type(Repr::Object(name, _)) => self
                    .hierarchy
                    .find_class(name)
                    .is_some_and(|class| class.kind == *kind),
                _ => false,
            },

            // A shape is an array whose keys fit the array's key type and
            // whose values all fit its value type (an unsealed tail's too).
            // The reverse - does some array have these exact keys - is never
            // provable.
            (Type(Repr::ArrayShape(shape)), Type(Repr::Array(key, value))) => {
                self.is_super_type_of(key, &shape.key_type()).yes()
                    && shape.types().all(|t| self.is_super_type_of(value, t).yes())
            }

            // An object shape is structurally an object, as is every class
            // instance and enum case. Whether a class has a shape's
            // properties is `accepts`'s question.
            (Type(Repr::ObjectShape(_)), Type(Repr::ObjectWithoutClass)) => true,
            (Type(Repr::Object(..)) | Type(Repr::EnumCase(..)), Type(Repr::ObjectWithoutClass)) => {
                true
            }

            (Type(Repr::Object(a, args_a)), Type(Repr::Object(b, args_b))) => {
                self.object_subtype(a, args_a, b, args_b)
            }
            // A case is an instance of its own enum and of whatever that
            // enum implements (`UnitEnum`/`BackedEnum` included).
            (Type(Repr::EnumCase(fqn, _)), Type(Repr::Object(name, _))) => {
                fqn == name || self.hierarchy.is_subclass_of(fqn, name)
            }

            _ => false,
        }
    }

    /// `b<args_b>` a super-type of `a<args_a>`: `a` is `b` or extends
    /// or implements it, and each of `b`'s template slots holds, read
    /// with its variance (a call-site annotation wins; `*` and an erased
    /// `mixed` slot hold for anything). A class the hierarchy doesn't
    /// know compares its arguments as written.
    fn object_subtype(&self, a: &str, args_a: &[Type], b: &str, args_b: &[Type]) -> bool {
        let same_class = a.eq_ignore_ascii_case(b);
        if !same_class && !self.hierarchy.is_subclass_of(a, b) {
            return false;
        }
        if args_b.is_empty() {
            return true;
        }
        let Some(class_b) = self.hierarchy.find_class(b) else {
            return args_a == args_b;
        };
        let effective_args_a: Vec<Type> = if same_class {
            args_a.to_vec()
        } else {
            let bindings = self.hierarchy.ancestor_template_bindings(a, args_a, b);
            class_b
                .templates
                .iter()
                .map(|name| {
                    bindings
                        .get(name.as_ref())
                        .cloned()
                        .unwrap_or(Type(Repr::Mixed(false)))
                })
                .collect()
        };
        (0..class_b.templates.len()).all(|i| {
            let b_i = args_b.get(i).cloned().unwrap_or(Type(Repr::Mixed(false)));
            let Some(variance) = crate::effective_slot_variance(self.hierarchy, b, i, &b_i) else {
                return true;
            };
            let Some(b_i) = b_i.without_call_site_variance() else {
                return true;
            };
            if matches!(b_i, Type(Repr::Mixed(_))) {
                return true;
            }
            let a_i = effective_args_a
                .get(i)
                .cloned()
                .unwrap_or(Type(Repr::Mixed(false)));
            let a_i = a_i
                .without_call_site_variance()
                .cloned()
                .unwrap_or(Type(Repr::Mixed(false)));
            match variance {
                Variance::Covariant => self.is_super_type_of(b_i, &a_i).yes(),
                Variance::Contravariant => self.is_super_type_of(&a_i, b_i).yes(),
                Variance::Invariant => {
                    self.is_super_type_of(b_i, &a_i).yes() && self.is_super_type_of(&a_i, b_i).yes()
                }
            }
        })
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
/// `is_super_type_of` arm - is `a` safely substitutable wherever `b` is
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
        matches!(sub, Type(Repr::Mixed(_)))
            || matches!(sup, Type(Repr::Mixed(_)))
            || is_sub(sub, sup)
    };
    // A `void` return accepts `null` (`VoidType::accepts`).
    let returns_fit = mixed_as_any(&a.return_type, &b.return_type)
        || (*b.return_type == Type(Repr::Void) && *a.return_type == Type(Repr::Null));
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
        if let Type(Repr::TemplateParam(tp)) = &bp.ty {
            if !(matches!(*tp.bound, Type(Repr::Mixed(_))) || mixed_as_any(&ap.ty, &tp.bound)) {
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
pub fn is_numeric_string_literal(s: &str) -> bool {
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
