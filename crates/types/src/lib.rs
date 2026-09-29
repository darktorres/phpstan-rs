//! The type model: `Type`, the relations between types, and the algebra
//! and coercions over them.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub use syntax::ast::ClassKind;
pub use syntax::name::Name;
pub use syntax::php_string::PhpString;

mod arithmetic;
mod coercion;
mod concat;
mod conditional;
mod constructors;
mod decide;
mod display;
mod extension;
mod hierarchy;
mod intern;
mod intersection;
mod late;
mod missing;
mod narrowing;
mod native;
mod numeric;
mod offsets;
mod parse;
mod php_float_format;
pub mod php_version;
mod queries;
mod relation;
mod shape;
mod subtype;
mod template_references;
mod templates;
mod transform;
mod traverse;
mod trinary;
mod union;

pub use arithmetic::{ArithmeticOp, OrderingOp, compare_range_outcome};
pub use display::quote_php_string;
pub use extension::ExtensionTypeData;
pub(crate) use hierarchy::effective_slot_variance;
pub use hierarchy::{ClassFacts, ClassHierarchy, NoClasses};
pub(crate) use intersection::intersect_types;
pub use late::ClassConstants;
pub use narrowing::comparison_narrowed_bounds;
pub use numeric::{PhpNumber, numeric_value};
pub use parse::{canonical_int_key, text_mentions_mixed};
pub use parse::{native_type, parse_type_string, parse_written_type};
pub(crate) use php_float_format::format_php_float;
pub use php_version::{
    KNOWN_PHP_VERSIONS, PhpFeature, PhpVersion, PhpVersions, known_php_versions_where,
    parse_php_version,
};
pub use relation::{AcceptsResult, Relation};
pub use shape::{AppendKey, NextArrayIndex, Shape, ShapeField, ShapeKey};
pub use subtype::is_numeric_string_literal;
pub use template_references::PositionVariance;
pub use trinary::TrinaryLogic;
pub use union::generalize_by_key_set;

/// A PHP type. What it is, what its values can be used as and how it
/// relates to other types are asked through its methods; how it is
/// represented is this crate's own.
///
/// A type is shared: cloning one is a pointer copy, and a type is never
/// changed once built.
#[derive(Clone)]
pub struct Type(Arc<Node>);

/// A type's representation, interned (`intern`), with its hashes computed
/// once: equal types hash alike (`Hash for Type`), so unequal ones mostly
/// compare in one step, and one exact form is one node.
pub(crate) struct Node {
    pub(crate) repr: Repr,
    pub(crate) hash: u64,
    pub(crate) exact: u64,
}

impl Type {
    /// The type `repr` represents.
    pub(crate) fn new(repr: Repr) -> Type {
        Type(intern::intern(repr, semantic_hash))
    }

    /// The representation.
    pub(crate) fn repr(&self) -> &Repr {
        &self.0.repr
    }

    /// The representation, taken out of the type, or a copy of it while
    /// the type is shared.
    pub(crate) fn into_repr(self) -> Repr {
        match Arc::try_unwrap(self.0) {
            Ok(mut node) => std::mem::replace(&mut node.repr, Repr::Never),
            Err(shared) => shared.repr.clone(),
        }
    }
}

/// Dropping a type goes one level at a time on a guarded stack, so a
/// deeply nested type drops without overflowing it.
impl Drop for Node {
    fn drop(&mut self) {
        let repr = std::mem::replace(&mut self.repr, Repr::Never);
        syntax::stack::guard(|| drop(repr));
    }
}

impl std::fmt::Debug for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        syntax::stack::guard(|| self.repr().fmt(f))
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Repr {
    // PHPStan's `MixedType::isExplicitMixed()`: `true` when `mixed` was
    // written (the keyword, or an unbounded `@template T`), `false` when
    // nothing is known (an unresolvable expression, a missing type). Type
    // equality ignores the flag, as `MixedType::equals()` does.
    Mixed(bool, Option<Type>),
    // PHPStan's `ErrorType`: the value of an expression a rule reports as
    // invalid. A `mixed` that accepts and is accepted by everything, so the
    // reported error doesn't cascade into more.
    Error,
    Never,
    Void,
    Null,
    Bool,
    True,
    False,
    Int,
    IntLiteral(i64),
    Float,
    String,
    StringLiteral(PhpString),
    // Key type alongside value type. Construction sites with no more
    // precise key generally pass `Type::unknown_array_key()`
    // (`int|string`).
    Array(Type, Type),
    // `class-string<T>` - holds `T`'s (or a subclass's) fully-qualified
    // name, tracked so it can participate in `@template` binding; a bare
    // `class-string` has no `T` to bind and collapses to plain `String`.
    ClassString(Type),
    // `interface-string`/`trait-string`: the name of some interface or
    // trait. PHPStan reads both as a bare `class-string`; the kind is
    // checked here too.
    ClassKindString(ClassKind),
    // `array{name: string, age?: int}` - PHPDoc's structural array type.
    ArrayShape(Arc<Shape>),
    // `object{name: string, age?: int}` - the `object`-flavored sibling
    // of `ArrayShape`. Structural, not a registered class: an instance is
    // accepted when it has every listed property with an accepted type.
    ObjectShape(Arc<Shape>),
    // A class name plus its resolved `@template` type arguments, if any;
    // empty for a non-generic class or a generic one referenced bare
    // (unspecified/erased, as real PHPStan treats it). Arguments compare
    // by each template's declared variance.
    Object(Name, Vec<Type>),
    // The native `object` type: an instance of no particular class.
    ObjectWithoutClass,
    // A `resource` (file/curl/... handle); `closed` for PHPDoc's
    // `closed-resource`, the kind `is_resource()` rejects.
    Resource {
        closed: bool,
    },
    // A single enum case's singleton type (enum fqn, case name), distinct
    // from the coarser `Object(Suit, [])`: an instance of its enum and of
    // everything the enum implements.
    EnumCase(Name, Name),
    // `None` = unknown shape; `Some(sig)` = a parsed `Closure(int):
    // string`/`callable(int): string` signature. `Arc` since `Type` is
    // cloned pervasively and a parsed shape can be non-trivially sized.
    Callable(Option<Arc<CallableSig>>),
    // `self`/`static` as written (with `self<T>`/`static<T>`'s own
    // arguments), and `$this`: resolved against the class a signature is
    // read for (`Checker::resolve_pseudo`).
    SelfRef(Vec<Type>),
    ParentRef,
    StaticRef(Vec<Type>),
    This,
    // An unbound `@template T` placeholder, produced for both a
    // function/method's own declared templates and a class's own
    // `@template`. Carries its own bound/variance/scope/default (see
    // `TemplateParam` below) rather than being a bare name.
    TemplateParam(TemplateParam),
    // A generic type argument written with a call-site variance
    // annotation (`Collection<covariant Foo>`, `Collection<*>`); `*` is
    // bivariant over `mixed`. Overrides the class's declared variance
    // for that slot in one comparison.
    VarianceArg(CallSiteVariance, Type),
    Union(Vec<Type>),
    // Real PHPStan's `BenevolentUnionType`: a union for subtyping, but
    // one an argument check accepts when *any* member fits (see
    // `Type::benevolent_array_key()` for where it is constructed).
    BenevolentUnion(Vec<Type>),
    Intersection(Vec<Type>),
    // A `T` range constrained to `[min, max]` (`None` = unbounded) -
    // `positive-int` = `IntRange(Some(1), None)`, `int<0, 255>` =
    // `IntRange(Some(0), Some(255))`. Never wrapped in an `Intersection`.
    IntRange(Option<i64>, Option<i64>),
    // A real float literal (`1.5`) in a PHPDoc union (`1.5|2.5`); code
    // literals still infer plain `Float`.
    FloatLiteral(f64),
    // Only ever a member of a `Repr::Intersection` alongside its own base
    // type (`String` or `Array(_)`).
    Accessory(AccessoryKind),
    // Mirrors real PHPStan's `HasOffsetValueType` - only ever a member of
    // a `Repr::Intersection` alongside its own coarse `Repr::Array(K, V)`
    // base, tracking one specific literal key's own precise value type
    // (`key`, `value`) without converting the whole container into an
    // `ArrayShape`. Confirmed directly against the vendored real
    // `phpstan.phar` via `assertType()`: writing a literal key onto an
    // already-coarse array keeps this precision (`non-empty-array<string,
    // mixed>&hasOffsetValue('a', array{1, 2, 3})`), and a second
    // literal-key write accumulates a further, separate
    // `Repr::HasOffsetValue` member rather than replacing the first (see
    // `checker::assignment::write_literal_key_aware`, this project's own
    // construction site). A dynamic (non-literal) key write, or one
    // inside a loop body currently under analysis, never produces this -
    // real PHPStan doesn't either, confirmed the same way: a loop
    // variable narrowed to a literal during one simulated pass still
    // generalizes away before the merge that produces the loop's own
    // exit type.
    HasOffsetValue(ShapeKey, Type),
    // PHPStan's `HasOffsetType`/`HasMethodType`/`HasPropertyType`: only
    // ever an `Intersection` member, refining its base with an offset,
    // method or property it's proven to have (`array_key_exists()`,
    // `method_exists()`, `property_exists()`).
    HasOffset(ShapeKey),
    HasMethod(Name),
    HasProperty(Name),
    // A `ReflectionMethod`/`ReflectionProperty` bound to one literally
    // named member (owner type, member name) of a `ReflectionClass<T>`'s
    // `T`: a member of an intersection with the `ReflectionMethod`/
    // `ReflectionProperty` object, made by `getMethod('name')`/
    // `getProperty('name')` and read by `invoke()`/`getValue()` for the
    // member's declared type. The owner is a full type, so a generic
    // reflected class keeps its arguments.
    ReflectedMember(Type, Name),
    // `value-of<T>` whose `T` a parse alone can't see through - an enum,
    // a class-constant reference or a template - resolved once the
    // registry exists (`Registry::resolve_deferred_value_of`).
    ValueOf(Type),
    // `key-of<T>` over a `T` not known yet (a template): resolved once
    // `T` is bound (`crate::resolve_late`).
    KeyOf(Type),
    // `T['key']` (PHPStan's `OffsetAccessType`) over a `T` not known yet:
    // the base, and the offset as its literal key type.
    OffsetAccess(Type, Type),
    // `Foo::BAR`/`Foo::BAR_*` (real PHPStan's `ConstTypeNode`, a `*`
    // matching any run of characters): the type of the matching class
    // constants, resolved once the registry exists.
    ClassConstant {
        class: Type,
        pattern: Name,
    },
    // An extension's own payload (`crate::ExtensionTypeData`).
    Extension(Arc<dyn ExtensionTypeData>),
    // `($subject is $target ? $if_true : $if_false)` - PHPDoc's
    // conditional-return-type syntax, lazily resolved once the real type
    // of `subject` is known. A consumer that can't resolve this variant
    // falls back to the eager `Union([if_true, if_false])`.
    Conditional {
        subject: ConditionalSubject,
        target: Type,
        if_true: Type,
        if_false: Type,
        negated: bool,
    },
}

/// What a conditional type (`(subject is target ? A : B)`) tests.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConditionalSubject {
    /// A parameter (`$param is ...`), named without its `$`: resolved per
    /// call from the argument passed for it.
    Parameter(Name),
    /// A `@template` (`T is ...`): resolved once the template is bound.
    Template(Name),
}

impl ConditionalSubject {
    /// The parameter's or template's name.
    pub fn name(&self) -> &Name {
        match self {
            ConditionalSubject::Parameter(name) | ConditionalSubject::Template(name) => name,
        }
    }
}

/// Whether `a` and `b` hold the same members, in any order: a union or
/// an intersection is a set of types, `int|string` the same as
/// `string|int`.
fn same_members(a: &[Type], b: &[Type]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if a.iter().zip(b).all(|(x, y)| x == y) {
        return true;
    }
    // Equal types hash alike, so each side sorted by hash lines up equal
    // members within a run of one hash.
    fn by_hash(members: &[Type]) -> Vec<&Type> {
        let mut sorted: Vec<&Type> = members.iter().collect();
        sorted.sort_by_key(|m| m.0.hash);
        sorted
    }
    let (a, b) = (by_hash(a), by_hash(b));
    let mut start = 0;
    while start < a.len() {
        let hash = a[start].0.hash;
        let end = start + a[start..].iter().take_while(|m| m.0.hash == hash).count();
        if b.get(end - 1).is_none_or(|m| m.0.hash != hash)
            || b.get(start).is_none_or(|m| m.0.hash != hash)
            || b.get(end).is_some_and(|m| m.0.hash == hash)
        {
            return false;
        }
        let run_a = &a[start..end];
        let run_b = &b[start..end];
        let count = |run: &[&Type], m: &Type| run.iter().filter(|x| **x == m).count();
        if !run_a.iter().all(|m| count(run_a, m) == count(run_b, m)) {
            return false;
        }
        start = end;
    }
    true
}

/// Deliberately not derived: `Mixed`'s own `bool` is ignored entirely
/// (matching real PHPStan's own `MixedType::equals()`, see `Repr::Mixed`'s
/// doc comment), and two NaN float literals are equal (PHPStan's
/// `ConstantFloatType::equals()`), which makes the relation an `Eq`; a
/// union's or an intersection's members compare in any order. `Hash`
/// below agrees with it.
impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.hash == other.0.hash
                && syntax::stack::guard(|| repr_eq(self.repr(), other.repr())))
    }
}

/// Whether two representations are the same type, their members compared
/// as types.
fn repr_eq(a: &Repr, b: &Repr) -> bool {
    match (a, b) {
        (Repr::Mixed(_, a), Repr::Mixed(_, b)) => a == b,
        (Repr::Error, Repr::Error) => true,
        (Repr::Never, Repr::Never) => true,
        (Repr::Void, Repr::Void) => true,
        (Repr::Null, Repr::Null) => true,
        (Repr::Bool, Repr::Bool) => true,
        (Repr::True, Repr::True) => true,
        (Repr::False, Repr::False) => true,
        (Repr::Int, Repr::Int) => true,
        (Repr::IntLiteral(a), Repr::IntLiteral(b)) => a == b,
        (Repr::Float, Repr::Float) => true,
        (Repr::String, Repr::String) => true,
        (Repr::StringLiteral(a), Repr::StringLiteral(b)) => a == b,
        (Repr::Array(ak, av), Repr::Array(bk, bv)) => ak == bk && av == bv,
        (Repr::ClassString(a), Repr::ClassString(b)) => a == b,
        (Repr::ClassKindString(a), Repr::ClassKindString(b)) => a == b,
        (Repr::ArrayShape(a), Repr::ArrayShape(b)) => a == b,
        (Repr::ObjectShape(a), Repr::ObjectShape(b)) => a == b,
        (Repr::Object(an, aa), Repr::Object(bn, ba)) => an.eq_ignore_case(bn) && aa == ba,
        (Repr::ObjectWithoutClass, Repr::ObjectWithoutClass) => true,
        (Repr::Resource { closed: a }, Repr::Resource { closed: b }) => a == b,
        (Repr::EnumCase(ae, ac), Repr::EnumCase(be, bc)) => ae.eq_ignore_case(be) && ac == bc,
        (Repr::Callable(a), Repr::Callable(b)) => a == b,
        (Repr::SelfRef(a), Repr::SelfRef(b)) => a == b,
        (Repr::ParentRef, Repr::ParentRef) => true,
        (Repr::StaticRef(a), Repr::StaticRef(b)) => a == b,
        (Repr::This, Repr::This) => true,
        (Repr::TemplateParam(a), Repr::TemplateParam(b)) => a == b,
        (Repr::VarianceArg(av, a), Repr::VarianceArg(bv, b)) => av == bv && a == b,
        (Repr::Union(a), Repr::Union(b))
        | (Repr::BenevolentUnion(a), Repr::BenevolentUnion(b))
        | (Repr::Intersection(a), Repr::Intersection(b)) => same_members(a, b),
        (Repr::IntRange(amin, amax), Repr::IntRange(bmin, bmax)) => amin == bmin && amax == bmax,
        (Repr::FloatLiteral(a), Repr::FloatLiteral(b)) => a == b || (a.is_nan() && b.is_nan()),
        (Repr::Accessory(a), Repr::Accessory(b)) => a == b,
        (Repr::HasOffsetValue(ak, av), Repr::HasOffsetValue(bk, bv)) => ak == bk && av == bv,
        (Repr::HasOffset(a), Repr::HasOffset(b)) => a == b,
        (Repr::HasMethod(a), Repr::HasMethod(b)) => a.eq_ignore_case(b),
        (Repr::HasProperty(a), Repr::HasProperty(b)) => a == b,
        (Repr::ReflectedMember(ao, an), Repr::ReflectedMember(bo, bn)) => ao == bo && an == bn,
        (Repr::ValueOf(a), Repr::ValueOf(b)) => a == b,
        (Repr::Extension(a), Repr::Extension(b)) => a.eq_data(&**b),
        (Repr::KeyOf(a), Repr::KeyOf(b)) => a == b,
        (Repr::OffsetAccess(ab, ak), Repr::OffsetAccess(bb, bk)) => ab == bb && ak == bk,
        (
            Repr::ClassConstant {
                class: ac,
                pattern: ap,
            },
            Repr::ClassConstant {
                class: bc,
                pattern: bp,
            },
        ) => ac == bc && ap == bp,
        (
            Repr::Conditional {
                subject: a_subject,
                target: a_target,
                if_true: a_if_true,
                if_false: a_if_false,
                negated: a_negated,
            },
            Repr::Conditional {
                subject: b_subject,
                target: b_target,
                if_true: b_if_true,
                if_false: b_if_false,
                negated: b_negated,
            },
        ) => {
            a_subject == b_subject
                && a_target == b_target
                && a_if_true == b_if_true
                && a_if_false == b_if_false
                && a_negated == b_negated
        }
        (
            Repr::Mixed(..)
            | Repr::Error
            | Repr::Never
            | Repr::Void
            | Repr::Null
            | Repr::Bool
            | Repr::True
            | Repr::False
            | Repr::Int
            | Repr::IntLiteral(_)
            | Repr::Float
            | Repr::String
            | Repr::StringLiteral(_)
            | Repr::Array(..)
            | Repr::ClassString(_)
            | Repr::ClassKindString(_)
            | Repr::ArrayShape(_)
            | Repr::ObjectShape(_)
            | Repr::Object(..)
            | Repr::ObjectWithoutClass
            | Repr::Resource { .. }
            | Repr::EnumCase(..)
            | Repr::Callable(_)
            | Repr::SelfRef(_)
            | Repr::ParentRef
            | Repr::StaticRef(_)
            | Repr::This
            | Repr::TemplateParam(_)
            | Repr::VarianceArg(..)
            | Repr::Union(_)
            | Repr::BenevolentUnion(_)
            | Repr::Intersection(_)
            | Repr::IntRange(..)
            | Repr::FloatLiteral(_)
            | Repr::Accessory(_)
            | Repr::HasOffsetValue(..)
            | Repr::HasOffset(_)
            | Repr::HasMethod(_)
            | Repr::HasProperty(_)
            | Repr::ReflectedMember(..)
            | Repr::ValueOf(_)
            | Repr::KeyOf(_)
            | Repr::OffsetAccess(..)
            | Repr::ClassConstant { .. }
            | Repr::Extension(_)
            | Repr::Conditional { .. },
            _,
        ) => false,
    }
}

impl Eq for Type {}

impl Hash for Type {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.hash);
    }
}

/// `repr`'s hash, what `Hash for Type` gives: its members' own hashes
/// combined, a union's or an intersection's in any order.
fn semantic_hash(repr: &Repr) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    let state = &mut hasher;
    std::mem::discriminant(repr).hash(state);
    match repr {
        Repr::Mixed(_, subtracted) => subtracted.hash(state),
        Repr::Error
        | Repr::Never
        | Repr::Void
        | Repr::Null
        | Repr::Bool
        | Repr::True
        | Repr::False
        | Repr::Int
        | Repr::Float
        | Repr::String
        | Repr::ObjectWithoutClass
        | Repr::ParentRef
        | Repr::This => {}
        Repr::IntLiteral(n) => n.hash(state),
        Repr::StringLiteral(s) => s.hash(state),
        Repr::Array(k, v) | Repr::OffsetAccess(k, v) => {
            k.hash(state);
            v.hash(state);
        }
        Repr::ClassString(inner) | Repr::ValueOf(inner) | Repr::KeyOf(inner) => inner.hash(state),
        Repr::ClassKindString(kind) => kind.hash(state),
        Repr::ArrayShape(shape) | Repr::ObjectShape(shape) => shape.hash(state),
        Repr::Object(name, args) => {
            name.lower().hash(state);
            args.hash(state);
        }
        Repr::Resource { closed } => closed.hash(state),
        Repr::EnumCase(class, case) => {
            class.lower().hash(state);
            case.hash(state);
        }
        Repr::Callable(sig) => sig.hash(state),
        Repr::SelfRef(args) | Repr::StaticRef(args) => args.hash(state),
        Repr::Union(members) | Repr::BenevolentUnion(members) | Repr::Intersection(members) => {
            members.len().hash(state);
            members
                .iter()
                .map(|m| m.0.hash)
                .fold(0u64, u64::wrapping_add)
                .hash(state);
        }
        Repr::TemplateParam(tp) => tp.hash(state),
        Repr::VarianceArg(variance, inner) => {
            variance.hash(state);
            inner.hash(state);
        }
        Repr::IntRange(min, max) => {
            min.hash(state);
            max.hash(state);
        }
        // Equal floats hash alike: both zeros, and every NaN.
        Repr::FloatLiteral(f) => {
            let bits = if *f == 0.0 {
                0
            } else if f.is_nan() {
                f64::NAN.to_bits()
            } else {
                f.to_bits()
            };
            bits.hash(state)
        }
        Repr::Accessory(kind) => kind.hash(state),
        Repr::HasOffsetValue(key, value) => {
            key.hash(state);
            value.hash(state);
        }
        Repr::HasOffset(key) => key.hash(state),
        Repr::HasMethod(name) => name.lower().hash(state),
        Repr::HasProperty(name) => name.hash(state),
        Repr::ReflectedMember(owner, member) => {
            owner.hash(state);
            member.hash(state);
        }
        Repr::ClassConstant { class, pattern } => {
            class.hash(state);
            pattern.hash(state);
        }
        Repr::Extension(data) => {
            let state: &mut dyn Hasher = state;
            data.hash_data(state);
        }
        Repr::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        } => {
            subject.hash(state);
            target.hash(state);
            if_true.hash(state);
            if_false.hash(state);
            negated.hash(state);
        }
    }
    hasher.finish()
}

// Whether a class's own `@template` may vary co-/contravariantly with a
// subclass relationship, or must stay exactly fixed (`Invariant`, the
// default). `checker::generics::check_variance` composes this through
// nested generics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Variance {
    #[default]
    Invariant,
    Covariant,
    Contravariant,
}

/// The variance a generic type argument is annotated with at its use
/// site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallSiteVariance {
    Covariant,
    Contravariant,
    /// `*`: any type, never checked.
    Bivariant,
}

/// A `@template` parameter's own metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TemplateParam {
    pub name: String,
    // `@template T of Bound` - `Mixed` when no `of` clause was written.
    // Lets an unbound `T` be used as its bound's type prior to
    // substitution (indexed as an array, compared as a bool, etc).
    pub bound: Box<Type>,
    pub variance: Variance,
    // Which class or function declared this template; `None` only for a
    // metadata-free placeholder (see `unbounded`). Lets
    // `checker::generics::check_variance` attribute a `T` occurrence to
    // the right declaring class when unrelated ancestors reuse the same
    // short name.
    pub scope: Option<String>,
    // `@template T = Default` - the type an unbound occurrence falls back
    // to instead of `Mixed`.
    pub default: Option<Box<Type>>,
}

impl TemplateParam {
    /// A bare, metadata-free placeholder: unbounded, invariant, no known
    /// declaration site, no default. Used only where a template's name
    /// needs to be recognized structurally; substitution replaces the
    /// whole node by name regardless of placeholder metadata.
    pub fn unbounded(name: String) -> TemplateParam {
        TemplateParam {
            name,
            // An unbounded `@template` has no `of`/`as` clause at all -
            // real PHPStan's own `TemplateTypeFactory`/`PhpDocNodeResolver`
            // both resolve this exact shape to `MixedType(true)`
            // (explicit), not `false` (confirmed directly against
            // `vendor/phpstan-src`).
            bound: Box::new(Type::explicit_mixed()),
            variance: Variance::Invariant,
            scope: None,
            default: None,
        }
    }
}

/// One parameter of a parsed `Closure(...)`/`callable(...)` PHPDoc shape.
/// A dedicated struct rather than `registry::ParameterReflection`, whose sibling
/// `FunctionReflection` can't cheaply derive `PartialEq`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallableParam {
    pub ty: Type,
    pub variadic: bool,
    /// A trailing `=` in the shape grammar (`Closure(int=): void`).
    pub optional: bool,
    /// A leading `&` in the shape grammar (`Closure(&int): void`).
    pub by_ref: bool,
}

/// A parsed `Closure(...)`/`callable(...)` PHPDoc shape's params-plus-
/// return-type slice, with its own templates (`callable<T>(T): T`) -
/// deliberately not the rest of what real PHPStan's `CallableType`
/// tracks (purity, assertions).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallableSig {
    pub params: Vec<CallableParam>,
    pub return_type: Box<Type>,
    /// The callable's own templates; every occurrence inside `params`/
    /// `return_type` is a `TemplateParam` scoped `CALLABLE_TEMPLATE_SCOPE`.
    pub templates: Vec<TemplateParam>,
}

/// The `TemplateParam::scope` of a callable type's own template.
pub const CALLABLE_TEMPLATE_SCOPE: &str = "callable";

impl CallableSig {
    /// `self` with the templates whose scope `is_own` accepts (a
    /// function's or method's own, in its signature) made the callable's
    /// own: `f(...)` of a generic `f` is a generic `Closure`.
    pub fn with_own_templates(mut self, is_own: &dyn Fn(&str) -> bool) -> CallableSig {
        fn collect(ty: &Type, is_own: &dyn Fn(&str) -> bool, out: &mut Vec<TemplateParam>) {
            if let Repr::TemplateParam(tp) = ty.repr()
                && tp.scope.as_deref().is_some_and(is_own)
            {
                if !out.iter().any(|t| t.name == tp.name) {
                    out.push(TemplateParam {
                        scope: Some(CALLABLE_TEMPLATE_SCOPE.to_string()),
                        ..tp.clone()
                    });
                }
                return;
            }
            ty.for_each_child(&mut |c| collect(c, is_own, out));
        }
        fn rescope(ty: &Type, is_own: &dyn Fn(&str) -> bool) -> Type {
            match ty.repr() {
                Repr::TemplateParam(tp) if tp.scope.as_deref().is_some_and(is_own) => {
                    Type::template_param(TemplateParam {
                        scope: Some(CALLABLE_TEMPLATE_SCOPE.to_string()),
                        ..tp.clone()
                    })
                }
                _ => ty.map_children(&mut |c| rescope(c, is_own)),
            }
        }
        let mut own = Vec::new();
        for p in &self.params {
            collect(&p.ty, is_own, &mut own);
        }
        collect(&self.return_type, is_own, &mut own);
        if own.is_empty() {
            return self;
        }
        for p in &mut self.params {
            p.ty = rescope(&p.ty, is_own);
        }
        self.return_type = Box::new(rescope(&self.return_type, is_own));
        self.templates.extend(own);
        self.rebind_templates()
    }

    /// `self` with every occurrence of its own templates replaced by the
    /// current `templates` entries - after their bounds changed.
    pub fn rebind_templates(mut self) -> CallableSig {
        fn rebind(ty: &Type, templates: &[TemplateParam]) -> Type {
            match ty.repr() {
                Repr::TemplateParam(tp) if tp.scope.as_deref() == Some(CALLABLE_TEMPLATE_SCOPE) => {
                    match templates.iter().find(|t| t.name == tp.name) {
                        Some(t) => Type::template_param(t.clone()),
                        None => ty.clone(),
                    }
                }
                _ => ty.map_children(&mut |c| rebind(c, templates)),
            }
        }
        if self.templates.is_empty() {
            return self;
        }
        for p in &mut self.params {
            p.ty = rebind(&p.ty, &self.templates);
        }
        self.return_type = Box::new(rebind(&self.return_type, &self.templates));
        self
    }
}

// Mirrors real PHPStan's `Accessory*Type` family
// (`vendor/phpstan-src/src/Type/Accessory/*.php`) - each kind only ever
// appears wrapped in a `Repr::Intersection` alongside its base type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessoryKind {
    NonEmptyString,
    NonFalsyString,
    LiteralString,
    NumericString,
    ArrayList,
    NonEmptyArray,
    // `lowercase-string`/`uppercase-string`. ASCII-only, matching PHP
    // 8.2+'s locale-independent `strtolower`/`strtoupper`.
    LowercaseString,
    UppercaseString,
    // A string proven to parse as a canonical decimal integer (no
    // leading zero/`+`/whitespace, round-trips through `i64`).
    DecimalIntegerString,
}

#[cfg(test)]
mod tests;
