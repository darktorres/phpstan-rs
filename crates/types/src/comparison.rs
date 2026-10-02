//! PHP's comparison (`zend_compare`) lifted to types: which outcomes
//! comparing two values can have, from which `==`, `<`, `<=`, `>`, `>=`
//! and `<=>` are read, and the narrowings a loose equality or an ordering
//! proves. PHP 7 and 8 differ where a number meets a non-numeric string:
//! PHP 7 reads the string as a number, PHP 8 the number as a string.

use crate::arithmetic::OrderingOp;
use crate::{PhpNumber, PhpVersion, Relation, Repr, Shape, TrinaryLogic, Type};

/// The outcomes comparing two values can have, a bit each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcomes(u8);

impl Outcomes {
    const NONE: Outcomes = Outcomes(0);
    /// `-1`.
    pub const LESS: Outcomes = Outcomes(1);
    /// `0`.
    pub const EQUAL: Outcomes = Outcomes(2);
    /// `1`.
    pub const GREATER: Outcomes = Outcomes(4);
    /// Neither is smaller: every ordering is false both ways, and they
    /// aren't equal (`NAN`, objects of different classes, arrays with
    /// different keys). `<=>` gives 1.
    pub const UNCOMPARABLE: Outcomes = Outcomes(8);
    const ORDERED: Outcomes = Outcomes(7);
    const ALL: Outcomes = Outcomes(15);

    /// Whether any of `other`'s outcomes is one of these.
    pub fn contains(self, other: Outcomes) -> bool {
        self.has(other)
    }

    fn or(self, other: Outcomes) -> Outcomes {
        Outcomes(self.0 | other.0)
    }

    fn has(self, other: Outcomes) -> bool {
        self.0 & other.0 != 0
    }

    fn without(self, other: Outcomes) -> Outcomes {
        Outcomes(self.0 & !other.0)
    }

    /// The outcomes with the operands swapped.
    fn swapped(self) -> Outcomes {
        let mut out = self.without(Outcomes::LESS.or(Outcomes::GREATER));
        if self.has(Outcomes::LESS) {
            out = out.or(Outcomes::GREATER);
        }
        if self.has(Outcomes::GREATER) {
            out = out.or(Outcomes::LESS);
        }
        out
    }

    /// Whether `holds` is true for every outcome, for none, or for some.
    fn decide(self, holds: Outcomes) -> TrinaryLogic {
        if self == Outcomes::NONE {
            TrinaryLogic::Maybe
        } else if self.without(holds) == Outcomes::NONE {
            TrinaryLogic::Yes
        } else if !self.has(holds) {
            TrinaryLogic::No
        } else {
            TrinaryLogic::Maybe
        }
    }
}

/// Which outcomes make `op` true.
fn holds(op: OrderingOp) -> Outcomes {
    match op {
        OrderingOp::Lt => Outcomes::LESS,
        OrderingOp::LtEq => Outcomes::LESS.or(Outcomes::EQUAL),
        OrderingOp::Gt => Outcomes::GREATER,
        OrderingOp::GtEq => Outcomes::GREATER.or(Outcomes::EQUAL),
    }
}

/// The PHP versions a comparison runs on: before 8.0, from 8.0, or both
/// when the range or version is unknown.
#[derive(Clone, Copy)]
struct Versions {
    php7: bool,
    php8: bool,
}

impl Versions {
    fn of(version: Option<PhpVersion>) -> Versions {
        match version {
            Some(v) if v >= PhpVersion::new(8, 0) => Versions {
                php7: false,
                php8: true,
            },
            Some(_) => Versions {
                php7: true,
                php8: false,
            },
            None => Versions {
                php7: true,
                php8: true,
            },
        }
    }
}

/// One member's value, as far as comparing it needs.
enum Value<'t> {
    Null,
    Bool(Option<bool>),
    /// An int in `[min, max]`.
    Int(Option<i64>, Option<i64>),
    /// A float, known when a literal.
    Float(Option<f64>),
    Str(Str<'t>),
    Array {
        least: i64,
        most: Option<i64>,
        shape: Option<&'t Shape>,
    },
    Object {
        class: Option<&'t str>,
        case: Option<&'t str>,
    },
    Unknown,
}

enum Str<'t> {
    Literal(&'t [u8]),
    General {
        non_empty: bool,
        non_falsy: bool,
        numeric: TrinaryLogic,
    },
}

impl Relation<'_> {
    /// Whether `a == b`: `Yes` when it holds for every pair of values,
    /// `No` when for none. `version` is the PHP version, `None` when the
    /// answer must hold on both sides of 8.0.
    pub fn loose_equal(&self, a: &Type, b: &Type, version: Option<PhpVersion>) -> TrinaryLogic {
        self.outcomes(a, b, version).decide(Outcomes::EQUAL)
    }

    /// Whether `a op b`, as `loose_equal` answers it.
    pub fn ordering(
        &self,
        a: &Type,
        op: OrderingOp,
        b: &Type,
        version: Option<PhpVersion>,
    ) -> TrinaryLogic {
        self.outcomes(a, b, version).decide(holds(op))
    }

    /// Every outcome comparing a value of `a` with one of `b` can have.
    pub fn outcomes(&self, a: &Type, b: &Type, version: Option<PhpVersion>) -> Outcomes {
        let versions = Versions::of(version);
        let mut out = Outcomes::NONE;
        for x in a.union_members() {
            for y in b.union_members() {
                out = out.or(self.member_outcomes(x, y, versions));
                if out == Outcomes::ALL {
                    return out;
                }
            }
        }
        out
    }

    fn member_outcomes(&self, a: &Type, b: &Type, versions: Versions) -> Outcomes {
        let (x, y) = (self.value(a), self.value(b));
        let mut out = Outcomes::NONE;
        if versions.php7 {
            out = out.or(self.compare(&x, &y, false));
        }
        if versions.php8 {
            out = out.or(self.compare(&x, &y, true));
        }
        out
    }

    fn value<'t>(&self, ty: &'t Type) -> Value<'t> {
        match ty.repr() {
            Repr::Null | Repr::Void => Value::Null,
            Repr::True => Value::Bool(Some(true)),
            Repr::False => Value::Bool(Some(false)),
            Repr::Bool => Value::Bool(None),
            Repr::IntLiteral(n) => Value::Int(Some(*n), Some(*n)),
            Repr::IntRange(min, max) => Value::Int(*min, *max),
            Repr::Int => Value::Int(None, None),
            Repr::FloatLiteral(f) => Value::Float(Some(*f)),
            Repr::Float => Value::Float(None),
            Repr::StringLiteral(s) => Value::Str(Str::Literal(s.as_bytes())),
            Repr::EnumCase(class, case) => Value::Object {
                class: Some(class.as_ref()),
                case: Some(case.as_ref()),
            },
            Repr::Object(class, _) => Value::Object {
                class: Some(class.as_ref()),
                case: None,
            },
            Repr::ObjectWithoutClass => Value::Object {
                class: None,
                case: None,
            },
            _ if ty.is_string().yes() => {
                let refined = |kind| {
                    self.is_super_type_of(&Type::string().with_string_accessory(kind), ty)
                        .yes()
                };
                let non_falsy = refined(crate::AccessoryKind::NonFalsyString);
                Value::Str(Str::General {
                    non_empty: non_falsy || refined(crate::AccessoryKind::NonEmptyString),
                    non_falsy,
                    numeric: if refined(crate::AccessoryKind::NumericString) {
                        TrinaryLogic::Yes
                    } else {
                        TrinaryLogic::Maybe
                    },
                })
            }
            _ if ty.is_array().yes() => {
                let (least, most) = ty.count_range().unwrap_or((0, None));
                Value::Array {
                    least,
                    most,
                    shape: crate::union::shape_parts(ty)
                        .map(|(shape, _)| shape)
                        .filter(|shape| shape.is_sealed()),
                }
            }
            _ => Value::Unknown,
        }
    }

    /// `zend_compare(a, b)`'s possible outcomes, on PHP 8 when `php8`.
    fn compare(&self, a: &Value<'_>, b: &Value<'_>, php8: bool) -> Outcomes {
        use Value::*;
        match (a, b) {
            (Unknown, _) | (_, Unknown) => Outcomes::ALL,
            (Int(..) | Float(_), Int(..) | Float(_)) => numbers(number_range(a), number_range(b)),
            (Array { .. }, Array { .. }) => self.compare_arrays(a, b, php8),
            (Null, Null) => Outcomes::EQUAL,
            (Null, Str(s)) => strings(&self::Str::Literal(b""), s, php8),
            (Str(s), Null) => strings(s, &self::Str::Literal(b""), php8),
            (Str(x), Str(y)) => strings(x, y, php8),
            (Bool(_) | Null, _) | (_, Bool(_) | Null) => bools(truthiness(a), truthiness(b)),
            (Str(s), Int(..) | Float(_)) => number_and_string(number_range(b), s, php8).swapped(),
            (Int(..) | Float(_), Str(s)) => number_and_string(number_range(a), s, php8),
            (Object { .. }, Object { .. }) => self.compare_objects(a, b),
            (Object { class, .. }, Str(s)) => self.object_and_string(*class, s, php8),
            (Str(s), Object { class, .. }) => self.object_and_string(*class, s, php8).swapped(),
            (Object { class, .. }, Int(..) | Float(_)) => {
                if self.compares_itself(*class) {
                    Outcomes::ALL
                } else {
                    numbers((Some(1.0), Some(1.0)), number_range(b))
                }
            }
            (Int(..) | Float(_), Object { class, .. }) => {
                if self.compares_itself(*class) {
                    Outcomes::ALL
                } else {
                    numbers(number_range(a), (Some(1.0), Some(1.0)))
                }
            }
            (Object { .. }, Array { .. }) => Outcomes::GREATER,
            (Array { .. }, Object { .. }) => Outcomes::LESS,
            (Array { .. }, _) => Outcomes::GREATER,
            (_, Array { .. }) => Outcomes::LESS,
        }
    }

    /// Whether a class compares its objects its own way (`DateTime`,
    /// extension classes): unknown classes are taken to.
    fn compares_itself(&self, class: Option<&str>) -> bool {
        let Some(class) = class else {
            return true;
        };
        self.hierarchy.find_class(class).is_none()
            || self.hierarchy.is_subclass_of(class, "DateTimeInterface")
            || matches!(class, "GMP" | "BcMath\\Number" | "Closure")
    }

    fn compare_objects(&self, a: &Value<'_>, b: &Value<'_>) -> Outcomes {
        let (
            Value::Object {
                class: ca,
                case: xa,
            },
            Value::Object {
                class: cb,
                case: xb,
            },
        ) = (a, b)
        else {
            return Outcomes::ALL;
        };
        match (xa, xb) {
            (Some(x), Some(y)) if ca == cb => {
                return if x == y {
                    Outcomes::EQUAL
                } else {
                    Outcomes::UNCOMPARABLE
                };
            }
            (Some(_), _) | (_, Some(_)) => return Outcomes::UNCOMPARABLE,
            _ => {}
        }
        if self.compares_itself(*ca) || self.compares_itself(*cb) {
            return Outcomes::ALL;
        }
        let (Some(ca), Some(cb)) = (ca, cb) else {
            return Outcomes::ALL;
        };
        let ta = Type::object((*ca).into(), Vec::new());
        let tb = Type::object((*cb).into(), Vec::new());
        if self.disjoint(&ta, &tb) {
            Outcomes::UNCOMPARABLE
        } else {
            Outcomes::ALL
        }
    }

    fn object_and_string(&self, class: Option<&str>, s: &Str<'_>, php8: bool) -> Outcomes {
        let Some(class) = class else {
            return Outcomes::ALL;
        };
        if self.hierarchy.find_class(class).is_none() {
            return Outcomes::ALL;
        }
        if self.hierarchy.has_method(class, "__toString") {
            return strings(
                &Str::General {
                    non_empty: false,
                    non_falsy: false,
                    numeric: TrinaryLogic::Maybe,
                },
                s,
                php8,
            );
        }
        // An object that can't be a string is greater than any string.
        Outcomes::GREATER
    }

    fn compare_arrays(&self, a: &Value<'_>, b: &Value<'_>, php8: bool) -> Outcomes {
        let (
            Value::Array {
                least: la,
                most: ma,
                shape: sa,
            },
            Value::Array {
                least: lb,
                most: mb,
                shape: sb,
            },
        ) = (a, b)
        else {
            return Outcomes::ALL;
        };
        if ma.is_some_and(|m| m < *lb) {
            return Outcomes::LESS;
        }
        if mb.is_some_and(|m| m < *la) {
            return Outcomes::GREATER;
        }
        let (Some(sa), Some(sb)) = (sa, sb) else {
            return Outcomes::ALL;
        };
        let exact = |s: &Shape| {
            s.fields()
                .iter()
                .all(|f| !f.optional)
                .then_some(s.fields().len())
        };
        match (exact(sa), exact(sb)) {
            (Some(x), Some(y)) if x == y => {}
            _ => return Outcomes::ALL,
        }
        // Key by key in the left's order; a key the right lacks leaves them
        // uncomparable (`zend_hash_compare` without ordering).
        let mut out = Outcomes::NONE;
        for field in sa.fields() {
            let Some(other) = sb.field(&field.key) else {
                return out.or(Outcomes::UNCOMPARABLE);
            };
            let pair = self.outcomes_on(&field.ty, &other.ty, php8);
            out = out.or(pair.without(Outcomes::EQUAL));
            if !pair.has(Outcomes::EQUAL) {
                return out;
            }
        }
        out.or(Outcomes::EQUAL)
    }

    fn outcomes_on(&self, a: &Type, b: &Type, php8: bool) -> Outcomes {
        self.outcomes(
            a,
            b,
            Some(if php8 {
                PhpVersion::new(8, 0)
            } else {
                PhpVersion::new(7, 4)
            }),
        )
    }
}

/// A number's possible values, `None` for an unbounded end; a float that
/// isn't a literal is unknown (`NAN` among its values).
fn number_range(v: &Value<'_>) -> (Option<f64>, Option<f64>) {
    match v {
        Value::Int(min, max) => (min.map(|n| n as f64), max.map(|n| n as f64)),
        Value::Float(Some(f)) => (Some(*f), Some(*f)),
        _ => (None, None),
    }
}

fn numbers(a: (Option<f64>, Option<f64>), b: (Option<f64>, Option<f64>)) -> Outcomes {
    if a.0.is_some_and(f64::is_nan) || b.0.is_some_and(f64::is_nan) {
        return Outcomes::UNCOMPARABLE;
    }
    let (a_lo, a_hi) = (
        a.0.unwrap_or(f64::NEG_INFINITY),
        a.1.unwrap_or(f64::INFINITY),
    );
    let (b_lo, b_hi) = (
        b.0.unwrap_or(f64::NEG_INFINITY),
        b.1.unwrap_or(f64::INFINITY),
    );
    let unbounded = a.0.is_none() && a.1.is_none() || b.0.is_none() && b.1.is_none();
    let mut out = Outcomes::NONE;
    if a_lo < b_hi {
        out = out.or(Outcomes::LESS);
    }
    if a_hi > b_lo {
        out = out.or(Outcomes::GREATER);
    }
    if a_lo <= b_hi && b_lo <= a_hi {
        out = out.or(Outcomes::EQUAL);
    }
    if unbounded {
        // An unknown float may be `NAN`.
        out = out.or(Outcomes::UNCOMPARABLE);
    }
    out
}

/// A value's truthiness, `None` when it may be either.
fn truthiness(v: &Value<'_>) -> Option<bool> {
    match v {
        Value::Null => Some(false),
        Value::Bool(b) => *b,
        Value::Int(min, max) => {
            if *min == Some(0) && *max == Some(0) {
                Some(false)
            } else if min.is_some_and(|m| m > 0) || max.is_some_and(|m| m < 0) {
                Some(true)
            } else {
                None
            }
        }
        Value::Float(Some(f)) => Some(*f != 0.0),
        Value::Float(None) => None,
        Value::Str(Str::Literal(s)) => Some(!(s.is_empty() || *s == b"0")),
        Value::Str(Str::General { non_falsy, .. }) => non_falsy.then_some(true),
        Value::Array { least, most, .. } => {
            if *least > 0 {
                Some(true)
            } else if *most == Some(0) {
                Some(false)
            } else {
                None
            }
        }
        Value::Object { .. } => Some(true),
        Value::Unknown => None,
    }
}

fn bools(a: Option<bool>, b: Option<bool>) -> Outcomes {
    let values = |x: Option<bool>| match x {
        Some(v) => vec![v],
        None => vec![false, true],
    };
    let mut out = Outcomes::NONE;
    for x in values(a) {
        for y in values(b) {
            out = out.or(match x.cmp(&y) {
                std::cmp::Ordering::Less => Outcomes::LESS,
                std::cmp::Ordering::Equal => Outcomes::EQUAL,
                std::cmp::Ordering::Greater => Outcomes::GREATER,
            });
        }
    }
    out
}

/// A numeric string's value on PHP 8 (whitespace around it allowed) or
/// PHP 7 (leading whitespace only).
fn numeric(s: &[u8], php8: bool) -> Option<f64> {
    if !php8 && s.last().is_some_and(u8::is_ascii_whitespace) {
        return None;
    }
    match crate::numeric_value(s)? {
        PhpNumber::Int(n) => Some(n as f64),
        PhpNumber::Float(f) => Some(f),
    }
}

/// PHP 7's reading of a non-numeric string as a number: its leading
/// numeric part, 0 when there is none.
fn leading_number(s: &[u8]) -> f64 {
    let start = s
        .iter()
        .position(|b| !b" \t\n\r\x0b\x0c".contains(b))
        .unwrap_or(s.len());
    (start..=s.len())
        .rev()
        .find_map(|end| {
            let prefix = &s[start..end];
            (!prefix.last().is_some_and(u8::is_ascii_whitespace))
                .then(|| crate::numeric_value(prefix))
                .flatten()
        })
        .map_or(0.0, |n| match n {
            PhpNumber::Int(n) => n as f64,
            PhpNumber::Float(f) => f,
        })
}

fn strings(a: &Str<'_>, b: &Str<'_>, php8: bool) -> Outcomes {
    match (a, b) {
        (Str::Literal(x), Str::Literal(y)) => {
            if let (Some(p), Some(q)) = (numeric(x, php8), numeric(y, php8)) {
                return numbers((Some(p), Some(p)), (Some(q), Some(q)));
            }
            ordering_of(x.cmp(y))
        }
        (Str::Literal(x), general @ Str::General { .. }) => literal_and_general(x, general, php8),
        (general @ Str::General { .. }, Str::Literal(y)) => {
            literal_and_general(y, general, php8).swapped()
        }
        _ => Outcomes::ORDERED,
    }
}

/// `x` against a string known only by its refinements.
fn literal_and_general(x: &[u8], general: &Str<'_>, php8: bool) -> Outcomes {
    let Str::General {
        non_empty,
        numeric: is_numeric,
        ..
    } = general
    else {
        return Outcomes::ORDERED;
    };
    if x.is_empty() {
        // `''` isn't numeric: compared byte by byte, it's smaller than any
        // other string.
        return if *non_empty {
            Outcomes::LESS
        } else {
            Outcomes::LESS.or(Outcomes::EQUAL)
        };
    }
    // A non-numeric literal equals only itself, which a numeric string
    // can't be.
    if is_numeric.yes() && numeric(x, php8).is_none() {
        return Outcomes::LESS.or(Outcomes::GREATER);
    }
    Outcomes::ORDERED
}

fn ordering_of(o: std::cmp::Ordering) -> Outcomes {
    match o {
        std::cmp::Ordering::Less => Outcomes::LESS,
        std::cmp::Ordering::Equal => Outcomes::EQUAL,
        std::cmp::Ordering::Greater => Outcomes::GREATER,
    }
}

/// A number against a string: numerically when the string is numeric;
/// otherwise PHP 8 compares the number's string form with it, PHP 7 the
/// string's leading number with the number.
fn number_and_string(n: (Option<f64>, Option<f64>), s: &Str<'_>, php8: bool) -> Outcomes {
    // `NAN` against a string is checked first: uncomparable.
    if n.0.is_some_and(f64::is_nan) {
        return Outcomes::UNCOMPARABLE;
    }
    match s {
        Str::Literal(bytes) => {
            if let Some(v) = numeric(bytes, php8) {
                return numbers(n, (Some(v), Some(v)));
            }
            if !php8 {
                let v = leading_number(bytes);
                return numbers(n, (Some(v), Some(v)));
            }
            match n {
                (Some(lo), Some(hi)) if lo == hi || lo.is_nan() => {
                    ordering_of(php_number_string(lo).as_bytes().cmp(bytes))
                }
                // A number's string form is numeric, or `INF`/`-INF`: it
                // equals no other non-numeric string.
                _ if !matches!(*bytes, b"INF" | b"-INF") => Outcomes::LESS
                    .or(Outcomes::GREATER)
                    .or(Outcomes::UNCOMPARABLE),
                _ => Outcomes::ALL,
            }
        }
        Str::General { .. } => Outcomes::ORDERED.or(if n.0.is_none() && n.1.is_none() {
            Outcomes::UNCOMPARABLE
        } else {
            Outcomes::NONE
        }),
    }
}

/// A number as `(string)` writes it on PHP 8: an integral float without
/// a fraction (`1`), others as `var_export` does.
fn php_number_string(f: f64) -> String {
    if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    let exported = syntax::php_float::var_export_float(f);
    exported
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(exported)
}

impl Relation<'_> {
    /// `current` where `current == other` is `equal`: a member that can't
    /// (or, for the false branch, surely does) equal is dropped. When
    /// `other` is one value, the true branch keeps only each member's
    /// values equal to it (`int|string` against `0` keeps `0` and numeric
    /// strings).
    pub fn narrow_loose(
        &self,
        current: &Type,
        other: &Type,
        equal: bool,
        version: Option<PhpVersion>,
    ) -> Type {
        let constants = other.finite_types();
        let kept: Vec<Type> = current
            .union_members()
            .iter()
            .filter_map(|member| {
                let verdict = self.loose_equal(member, other, version);
                if equal {
                    if verdict.no() {
                        return None;
                    }
                    if !constants.is_empty() && constants.len() <= crate::MAX_UNION_MEMBERS {
                        let parts: Vec<Type> = constants
                            .iter()
                            .map(|c| self.equal_part(member, c, version))
                            .filter(|t| !t.is_never())
                            .collect();
                        return (!parts.is_empty()).then(|| self.union_of(parts));
                    }
                    Some(member.clone())
                } else if verdict.yes() {
                    None
                } else {
                    Some(member.clone())
                }
            })
            .collect();
        self.union_of(kept)
    }

    /// `current` where `current op other` was `holds`: a member for which
    /// that can't be is dropped, and an int's range is cut by `other`'s
    /// numeric bounds (`$i < 2.5` keeps `int<min, 2>`). The false branch
    /// cuts only against a number that can't be `NAN`, as an uncomparable
    /// pair fails every ordering.
    pub fn narrow_ordering(
        &self,
        current: &Type,
        op: OrderingOp,
        other: &Type,
        holds: bool,
        version: Option<PhpVersion>,
    ) -> Type {
        let bounds = numeric_bounds(other);
        let kept: Vec<Type> = current
            .union_members()
            .iter()
            .filter_map(|member| {
                let verdict = self.ordering(member, op, other, version);
                if (holds && verdict.no()) || (!holds && verdict.yes()) {
                    return None;
                }
                let op = if holds { op } else { negated(op) };
                match (member.int_bounds(), bounds) {
                    (Some((min, max)), Some((lo, hi)))
                        if holds || !(lo.is_nan() || hi.is_nan()) =>
                    {
                        let (min, max) = cut_int(min, max, op, lo, hi)?;
                        Some(Type::int_range(min, max))
                    }
                    _ => Some(member.clone()),
                }
            })
            .collect();
        self.union_of(kept)
    }

    /// The part of `member` whose values loosely equal the constant `c`.
    fn equal_part(&self, member: &Type, c: &Type, version: Option<PhpVersion>) -> Type {
        match self.loose_equal(member, c, version) {
            TrinaryLogic::Yes => return member.clone(),
            TrinaryLogic::No => return Type::never(),
            TrinaryLogic::Maybe => {}
        }
        let versions = Versions::of(version);
        match self.value(c) {
            Value::Null => self.falsy_equal_to_null(member),
            Value::Bool(Some(true)) => member.without_falsy(),
            Value::Bool(Some(false)) => member.only_falsy(self),
            Value::Int(Some(n), Some(_)) => self.equal_to_number(member, n as f64, versions),
            Value::Float(Some(f)) => self.equal_to_number(member, f, versions),
            // Each version's cut, joined when the range spans both.
            Value::Str(Str::Literal(s)) => {
                let parts: Vec<Type> = [(versions.php8, true), (versions.php7, false)]
                    .into_iter()
                    .filter(|(applies, _)| *applies)
                    .map(|(_, php8)| self.equal_to_literal_string(member, s, c, php8))
                    .collect();
                self.union_of(parts)
            }
            _ => member.clone(),
        }
    }

    /// `member`'s values loosely equal to the literal string `c` (bytes
    /// `s`) on PHP 8 (`php8`) or PHP 7: a bool by the string's truthiness;
    /// a numeric string as its number and the strings of that value; a
    /// non-numeric one as itself among strings, and for a number as `INF`
    /// or `-INF` on PHP 8, or its leading number on PHP 7.
    fn equal_to_literal_string(&self, member: &Type, s: &[u8], c: &Type, php8: bool) -> Type {
        let versions = Versions { php7: !php8, php8 };
        if member.is_bool().yes() {
            let truthy = !(s.is_empty() || s == b"0");
            return self.intersect(vec![member.clone(), Type::bool_literal(truthy)]);
        }
        if let Some(x) = numeric(s, php8) {
            return self
                .union(self.equal_to_number(member, x, versions), c.clone())
                .intersect_with(member, self);
        }
        if member.is_string().yes() {
            return c.clone();
        }
        if member.is_int().yes() || member.is_float().yes() {
            if !php8 {
                return self.equal_to_number(member, leading_number(s), versions);
            }
            let infinity = match s {
                b"INF" => f64::INFINITY,
                b"-INF" => f64::NEG_INFINITY,
                _ => return Type::never(),
            };
            return if member.is_float().yes() {
                Type::float_literal(infinity)
            } else {
                Type::never()
            };
        }
        member.clone()
    }

    /// `member`'s values equal to `null`: `null`, `false`, `0`, `0.0`, `''`
    /// and `[]`.
    fn falsy_equal_to_null(&self, member: &Type) -> Type {
        let candidates = Type::union_of(vec![
            Type::null(),
            Type::bool_literal(false),
            Type::int_literal(0),
            Type::float_literal(0.0),
            Type::string_literal(crate::PhpString::from_bytes(&b""[..])),
            Type::array_shape(Shape::sealed(Vec::new())),
        ]);
        self.intersect(vec![member.clone(), candidates])
    }

    /// `member`'s values equal to the number `n`.
    fn equal_to_number(&self, member: &Type, n: f64, versions: Versions) -> Type {
        // A bool compares by the number's truthiness.
        if member.is_bool().yes() {
            return self.intersect(vec![member.clone(), Type::bool_literal(n != 0.0)]);
        }
        if member.is_int().yes() {
            return if n.fract() == 0.0 && n.abs() < 9.2e18 {
                self.intersect(vec![member.clone(), Type::int_literal(n as i64)])
            } else {
                Type::never()
            };
        }
        if member.is_float().yes() {
            return Type::float_literal(n);
        }
        if member.is_string().yes() {
            // PHP 7 reads `'abc'` as 0 too; PHP 8 only numeric strings of
            // the value.
            if versions.php7 && n == 0.0 {
                return member.clone();
            }
            return self.intersect(vec![
                member.clone(),
                Type::string().with_string_accessory(crate::AccessoryKind::NumericString),
            ]);
        }
        member.clone()
    }
}

impl Type {
    /// This type and `other`'s shared values.
    fn intersect_with(&self, other: &Type, relation: &Relation<'_>) -> Type {
        relation.intersect(vec![self.clone(), other.clone()])
    }
}

/// The least and greatest number `ty` may be, when every member is an int
/// or a float literal.
fn numeric_bounds(ty: &Type) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for member in ty.union_members() {
        let (a, b) = match member.repr() {
            Repr::FloatLiteral(f) => (*f, *f),
            _ => {
                let (min, max) = member.int_bounds()?;
                (
                    min.map_or(f64::NEG_INFINITY, |n| n as f64),
                    max.map_or(f64::INFINITY, |n| n as f64),
                )
            }
        };
        if a.is_nan() {
            return Some((f64::NAN, f64::NAN));
        }
        lo = lo.min(a);
        hi = hi.max(b);
    }
    (lo <= hi).then_some((lo, hi))
}

/// `op`'s complement over ordered values.
fn negated(op: OrderingOp) -> OrderingOp {
    match op {
        OrderingOp::Lt => OrderingOp::GtEq,
        OrderingOp::LtEq => OrderingOp::Gt,
        OrderingOp::Gt => OrderingOp::LtEq,
        OrderingOp::GtEq => OrderingOp::Lt,
    }
}

/// An int in `[min, max]` where `int op x` holds for some `x` in
/// `[lo, hi]`; `None` when none does. A float bound rounds inward.
fn cut_int(
    min: Option<i64>,
    max: Option<i64>,
    op: OrderingOp,
    lo: f64,
    hi: f64,
) -> Option<(Option<i64>, Option<i64>)> {
    if lo.is_nan() {
        return None;
    }
    let to_int =
        |x: f64| -> Option<i64> { (x.is_finite() && x.abs() < 9.2e18).then_some(x as i64) };
    let (mut min, mut max) = (min, max);
    match op {
        // Some `x` above the int: the int is below the largest `x`.
        OrderingOp::Lt => {
            let bound = if hi.fract() == 0.0 {
                hi - 1.0
            } else {
                hi.floor()
            };
            if let Some(b) = to_int(bound) {
                max = Some(max.map_or(b, |m| m.min(b)));
            }
        }
        OrderingOp::LtEq => {
            if let Some(b) = to_int(hi.floor()) {
                max = Some(max.map_or(b, |m| m.min(b)));
            }
        }
        OrderingOp::Gt => {
            let bound = if lo.fract() == 0.0 {
                lo + 1.0
            } else {
                lo.ceil()
            };
            if let Some(b) = to_int(bound) {
                min = Some(min.map_or(b, |m| m.max(b)));
            }
        }
        OrderingOp::GtEq => {
            if let Some(b) = to_int(lo.ceil()) {
                min = Some(min.map_or(b, |m| m.max(b)));
            }
        }
    }
    match (min, max) {
        (Some(a), Some(b)) if a > b => None,
        _ => Some((min, max)),
    }
}
