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
    /// Every key the shape can have (`ConstantArrayType::getIterableKeyType()`):
    /// each field's literal key, and an unsealed tail's key type; `never`
    /// for `array{}`. Literal, not widened, so `array{0: int}` fits
    /// `array<int<0, max>, int>` and `array{a: int}` fits
    /// `array<'a'|'b', int>`.
    pub fn key_type(&self) -> Type {
        let mut key_types: Vec<Type> = Vec::new();
        for field in &self.fields {
            let kt = field.key.to_type();
            if !key_types.contains(&kt) {
                key_types.push(kt);
            }
        }
        // Distinct literals: a union past the member cap must not collapse.
        let literals = match key_types.len() {
            0 => Type::never(),
            1 => key_types.into_iter().next().expect("len checked above"),
            _ => Type::new(Repr::Union(key_types)),
        };
        match self.unsealed.as_deref() {
            Some(tail) => Type::union(literals, tail.0.clone()),
            None => literals,
        }
    }
}

impl Relation<'_> {
    /// The structural half of `is_super_type_of`'s `Yes`: every value of
    /// `sub` provably is a `sup`, for the pairs `is_super_type_of` doesn't
    /// answer itself first.
    pub(crate) fn proves_super_type(&self, sup: &Type, sub: &Type) -> bool {
        syntax::stack::guard(|| {
            match (sub.repr(), sup.repr()) {
                // A shaped callable is always compatible with a bare marker.
                (Repr::Callable(Some(_)), Repr::Callable(None)) => true,
                // Parameters contravariant, the return covariant.
                (Repr::Callable(Some(a)), Repr::Callable(Some(b))) => {
                    self.callable_sig_fits(a, b, &|x, y| self.is_super_type_of(y, x).yes(), false)
                }

                (Repr::IntLiteral(_), Repr::Int) => true,
                (Repr::StringLiteral(_), Repr::String) => true,
                // `$this`, `static`, `self` and `parent` are always objects.
                (
                    Repr::This | Repr::StaticRef(_) | Repr::SelfRef(_) | Repr::ParentRef,
                    Repr::ObjectWithoutClass,
                ) => true,
                (Repr::True, Repr::Bool) | (Repr::False, Repr::Bool) => true,
                (Repr::FloatLiteral(_), Repr::Float) => true,

                // `int<min, max>` ranges. `IntRange` fully replaces
                // `Int`'s own range (the same relationship `IntLiteral` has to
                // `Int`), so an `IntLiteral`/narrower `IntRange` widens to it
                // the same way a literal widens to its own base type.
                (Repr::IntLiteral(n), Repr::IntRange(min, max)) => {
                    min.is_none_or(|m| *n >= m) && max.is_none_or(|m| *n <= m)
                }
                (Repr::IntRange(sub_min, sub_max), Repr::IntRange(sup_min, sup_max)) => {
                    sup_min.is_none_or(|m| sub_min.is_some_and(|s| s >= m))
                        && sup_max.is_none_or(|m| sub_max.is_some_and(|s| s <= m))
                }
                (Repr::IntRange(_, _), Repr::Int) => true,

                // String/array accessory kinds. A literal directly
                // proves or disproves each kind from its own exact value - no
                // guessing. Only a real, structurally-provable `ArrayShape` (or
                // a value from a known list-producing source, e.g.
                // `array_values()`) can ever prove `ArrayList`/`NonEmptyArray` -
                // a plain `Repr::Array(_)` (even with a known element type)
                // deliberately never satisfies either, matching real PHPStan's
                // own confirmed strictness (a `list<int>`-typed parameter
                // rejects even a known-element `int[]` argument).
                (Repr::StringLiteral(s), Repr::Accessory(AccessoryKind::NonEmptyString)) => {
                    !s.is_empty()
                }
                (Repr::StringLiteral(s), Repr::Accessory(AccessoryKind::NonFalsyString)) => {
                    !s.is_empty() && s != "0"
                }
                (Repr::StringLiteral(_), Repr::Accessory(AccessoryKind::LiteralString)) => true,
                (Repr::StringLiteral(s), Repr::Accessory(AccessoryKind::NumericString)) => {
                    s.as_str().is_some_and(is_numeric_string_literal)
                }
                // Mirrors real `ConstantStringType::isLowercaseString`/
                // `isUppercaseString` (`strtolower($v) === $v`/`strtoupper($v) ===
                // $v`) - ASCII-only, matching PHP 8.2+'s own locale-independent
                // `strtolower`/`strtoupper` (this project's only supported PHP
                // version range).
                (Repr::StringLiteral(s), Repr::Accessory(AccessoryKind::LowercaseString)) => {
                    s.as_bytes().iter().all(|b| !b.is_ascii_uppercase())
                }
                (Repr::StringLiteral(s), Repr::Accessory(AccessoryKind::UppercaseString)) => {
                    s.as_bytes().iter().all(|b| !b.is_ascii_lowercase())
                }
                // Mirrors real `ConstantStringType::isDecimalIntegerString`
                // (`(string) (int) $v === $v`) - round-trips through `i64`, so a
                // leading zero (`"007"`), an explicit sign (`"+1"`), `"-0"`, an
                // empty string, or a non-integer numeric string (`"1.0"`) are all
                // correctly excluded.
                (Repr::StringLiteral(s), Repr::Accessory(AccessoryKind::DecimalIntegerString)) => {
                    s.as_str().is_some_and(is_decimal_integer_string_literal)
                }
                // Every non-falsy string is non-empty, and so is every numeric
                // one (`""` isn't numeric).
                (
                    Repr::Accessory(AccessoryKind::NonFalsyString | AccessoryKind::NumericString),
                    Repr::Accessory(AccessoryKind::NonEmptyString),
                ) => true,
                // A decimal integer string (`"0"`, `"123"`, `"-1"`) is always
                // also numeric, "lowercase" and "uppercase" (no letters to fail
                // either check), and non-empty (mirrors real
                // `AccessoryDecimalIntegerStringType`).
                (
                    Repr::Accessory(AccessoryKind::DecimalIntegerString),
                    Repr::Accessory(
                        AccessoryKind::NumericString
                        | AccessoryKind::LowercaseString
                        | AccessoryKind::UppercaseString
                        | AccessoryKind::NonEmptyString,
                    ),
                ) => true,
                (Repr::ArrayShape(shape), Repr::HasOffset(key)) => {
                    shape.field(key).is_some_and(|f| !f.optional)
                }
                (Repr::ArrayShape(shape), Repr::Accessory(AccessoryKind::NonEmptyArray)) => {
                    shape.fields.iter().any(|f| !f.optional)
                }
                // An array with a known offset isn't empty.
                (
                    Repr::HasOffset(_) | Repr::HasOffsetValue(..),
                    Repr::Accessory(AccessoryKind::NonEmptyArray),
                ) => true,
                (Repr::ArrayShape(shape), Repr::Accessory(AccessoryKind::ArrayList)) => {
                    shape.is_list()
                }

                // An unresolved `Conditional` - either it leaked into a
                // position `resolve_conditional` never runs against, or its
                // subject was left unbound - is either branch.
                (
                    _,
                    Repr::Conditional {
                        if_true, if_false, ..
                    },
                ) => self
                    .is_super_type_of(
                        &Type::new(Repr::Union(vec![(**if_true).clone(), (**if_false).clone()])),
                        sub,
                    )
                    .yes(),
                (Repr::Intersection(_), Repr::Intersection(sup_members)) => sup_members
                    .iter()
                    .all(|m| self.is_super_type_of(m, sub).yes()),
                (Repr::Intersection(members), _) => {
                    members.iter().any(|m| self.is_super_type_of(sup, m).yes())
                }
                (_, Repr::Intersection(members)) => {
                    members.iter().all(|m| self.is_super_type_of(m, sub).yes())
                }

                // A key-type check alongside the value-type one - `array<int, T>`
                // is not a subtype of `array<string, T>`. The common "don't know
                // the real key" case (`unknown_array_key()`, `int|string`) still
                // passes via the ordinary `Union` subtype rule above.
                (Repr::Array(ka, a), Repr::Array(kb, b)) => {
                    self.is_super_type_of(kb, ka).yes() && self.is_super_type_of(b, a).yes()
                }

                // `class-string<T>` is a string; two recurse like `Array` does.
                (Repr::ClassString(a), Repr::ClassString(b)) => self.is_super_type_of(b, a).yes(),
                (Repr::ClassString(_) | Repr::ClassKindString(_), Repr::String) => true,
                (Repr::ClassString(a), Repr::ClassKindString(kind)) => match a.repr() {
                    Repr::Object(name, _) => self
                        .hierarchy
                        .find_class(name)
                        .is_some_and(|class| class.kind == *kind),
                    _ => false,
                },

                // A shape is an array whose keys fit the array's key type and
                // whose values all fit its value type (an unsealed tail's too).
                // The reverse - does some array have these exact keys - is never
                // provable.
                (Repr::ArrayShape(shape), Repr::Array(key, value)) => {
                    self.is_super_type_of(key, &shape.key_type()).yes()
                        && shape.types().all(|t| self.is_super_type_of(value, t).yes())
                }

                // An object shape is structurally an object, as is every class
                // instance and enum case. Whether a class has a shape's
                // properties is `accepts`'s question.
                (Repr::ObjectShape(_), Repr::ObjectWithoutClass) => true,
                (Repr::Object(..) | Repr::EnumCase(..), Repr::ObjectWithoutClass) => true,

                (Repr::Object(a, args_a), Repr::Object(b, args_b)) => {
                    self.object_subtype(a, args_a, b, args_b)
                }
                // A case is an instance of its own enum and of whatever that
                // enum implements (`UnitEnum`/`BackedEnum` included).
                (Repr::EnumCase(fqn, _), Repr::Object(name, _)) => {
                    fqn == name || self.hierarchy.is_subclass_of(fqn, name)
                }

                _ => false,
            }
        })
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
                        .unwrap_or(Type::mixed())
                })
                .collect()
        };
        (0..class_b.templates.len()).all(|i| {
            let b_i = args_b.get(i).cloned().unwrap_or(Type::mixed());
            let Some(variance) = crate::effective_slot_variance(self.hierarchy, b, i, &b_i) else {
                return true;
            };
            let Some(b_i) = b_i.without_call_site_variance() else {
                return true;
            };
            if matches!(b_i.repr(), Repr::Mixed(..)) {
                return true;
            }
            let a_i = effective_args_a.get(i).cloned().unwrap_or(Type::mixed());
            let a_i = a_i
                .without_call_site_variance()
                .cloned()
                .unwrap_or(Type::mixed());
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

impl Relation<'_> {
    /// Whether a callable of signature `a` can stand in wherever one of
    /// `b` is expected: contravariant parameters, covariant return
    /// (PHPStan's `CallableTypeHelper::isParametersAcceptorSuperTypeOf`,
    /// `a` "theirs" and `b` "ours").
    ///
    /// - `b`'s own templates are their bounds (`a` must work for any of
    ///   them); `a`'s own are bound from `b`'s types.
    /// - Only `a`'s positions are walked (a variadic last parameter of `a`
    ///   covers every extra position of `b`): PHP drops extra arguments,
    ///   so `fn() => 1` stands in for `Closure(int): int`. A position `b`
    ///   lacks is fine only when `a`'s parameter there is optional or
    ///   variadic, and `b` optional where `a` requires is a rejection.
    /// - A by-reference parameter of `a` needs `b` to pass one by
    ///   reference.
    /// - A template of the enclosing function in `b`'s parameter is
    ///   inferred from `a`'s, so `a`'s type only has to fit its bound.
    /// - With `mixed_as_any` (the `accepts` path), `mixed` on either side
    ///   of a comparison is "any", as an argument check below level 9.
    ///
    /// `is_sub` decides each comparison, so `accepts` passes its own.
    pub(crate) fn callable_sig_fits(
        &self,
        a: &CallableSig,
        b: &CallableSig,
        is_sub: &dyn Fn(&Type, &Type) -> bool,
        mixed_as_any: bool,
    ) -> bool {
        let b = b.own_templates_as_bounds(self);
        let a = a.instantiated_against(&b, self);
        let fits = |sub: &Type, sup: &Type| {
            (mixed_as_any
                && (matches!(sub.repr(), Repr::Mixed(..)) || matches!(sup.repr(), Repr::Mixed(..))))
                || is_sub(sub, sup)
        };
        // A `void` return accepts `null` (`VoidType::accepts`).
        let returns_fit = fits(&a.return_type, &b.return_type)
            || (*b.return_type == Type::void() && *a.return_type == Type::null());
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
            if ap.by_ref && !bp.by_ref {
                return false;
            }
            if let Repr::TemplateParam(tp) = bp.ty.repr() {
                if !(matches!((*tp.bound).repr(), Repr::Mixed(..)) || fits(&ap.ty, &tp.bound)) {
                    return false;
                }
                continue;
            }
            if !fits(&bp.ty, &ap.ty) {
                return false;
            }
        }
        true
    }
}

/// Whether `s` is a numeric string, as `is_numeric()` decides
/// (`numeric_value`).
pub fn is_numeric_string_literal(s: &str) -> bool {
    crate::numeric_value(s.as_bytes()).is_some()
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
