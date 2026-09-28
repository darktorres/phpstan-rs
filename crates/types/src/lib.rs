//! The type model: `Type`, the relations between types, and the algebra
//! and coercions over them.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub use syntax::ast::ClassKind;
pub use syntax::name::Name;
pub use syntax::php_string::PhpString;

pub mod arithmetic;
mod coercion;
mod concat;
mod conditional;
mod constructors;
mod decide;
mod display;
mod extension;
mod hierarchy;
mod intersection;
mod late;
mod missing;
pub mod narrowing;
mod native;
mod offsets;
mod parse;
mod php_float_format;
pub mod php_version;
mod queries;
pub mod relation;
mod shape;
mod subtype;
mod template_references;
mod templates;
mod transform;
mod trinary;
mod union;

pub use concat::concat_types;
pub use conditional::conditional_branch;
pub use conditional::resolve_conditional;
pub use decide::reconcile_with_native;
pub use extension::ExtensionTypeData;
pub use hierarchy::{ClassFacts, ClassHierarchy, NoClasses, effective_slot_variance};
pub use intersection::intersect_types;
pub use late::{
    ClassConstants, contains_late, references_template, resolve_late_to_bounds, settle,
};
pub use native::native_declaration;
pub use parse::{canonical_int_key, has_explicit_mixed_nested_somewhere, strip_call_site_variance};
pub use parse::{parse_native_type_string, parse_type_string};
pub use php_float_format::format_php_float;
pub use php_version::{
    KNOWN_PHP_VERSIONS, PhpFeature, PhpVersion, PhpVersions, known_php_versions_where,
    parse_php_version,
};
pub use relation::{AcceptsResult, Relation};
pub use shape::{Shape, ShapeField, ShapeKey};
pub use subtype::{
    callable_sig_is_subtype, is_numeric_string_literal, shape_fields_are_list_like, shape_key_type,
    types_equal_for_union,
};
pub use template_references::PositionVariance;
pub use templates::{
    bind_template_param, promote_class_string_literals, substitute_template,
    substitute_template_resolving_to_bounds, substitute_templates_of,
};
pub use trinary::TrinaryLogic;
pub use union::generalize_by_key_set;
pub use union::{has_offset_value_parts, union_types};

// A slice of PHPStan's type system covering what the parser captures as
// type-hint strings, plus self/parent/static and `@template` generics.
// Inheritance-chain substitution (a subclass fixing its parent's own type
// parameter via `@extends Base<Concrete>`) is not modeled.
/// A PHP type. What it is, what its values can be used as and how it
/// relates to other types are asked through its methods; how it is
/// represented is this crate's own.
#[derive(Clone)]
pub struct Type(pub(crate) Repr);

impl std::fmt::Debug for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Repr {
    // Real PHPStan's own `MixedType::isExplicitMixed()` (Phase 84):
    // `true` when a type-resolution process completed and its own
    // deliberate, definitive answer is `mixed` (the literal `mixed`
    // keyword written somewhere in a type expression; an unbounded
    // `@template T` with no `of` clause, which real PHPStan's own
    // `TemplateTypeFactory`/`PhpDocNodeResolver` both resolve to
    // `MixedType(true)`, not `false`, confirmed directly against
    // `vendor/phpstan-src`); `false` for a genuine "we have no type
    // information at all" fallback (an unresolvable expression, a
    // missing-typehint default, an error-recovery path). Real PHPStan's
    // own `MixedType::equals()` ignores this flag entirely (`vendor/
    // phpstan-src/src/Type/MixedType.php`, confirmed directly) - `Type`'s
    // own `PartialEq` impl below (deliberately not derived) does the
    // same, so every one of this project's own pre-existing `==
    // Type::Mixed(false)`-shaped comparisons keeps working unchanged regardless
    // of which flag either side carries.
    Mixed(bool),
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
    Array(Box<Type>, Box<Type>),
    // `class-string<T>` - holds `T`'s (or a subclass's) fully-qualified
    // name, tracked so it can participate in `@template` binding; a bare
    // `class-string` has no `T` to bind and collapses to plain `String`.
    ClassString(Box<Type>),
    // `interface-string`/`trait-string`: the name of some interface or
    // trait. PHPStan reads both as a bare `class-string`; the kind is
    // checked here too.
    ClassKindString(ClassKind),
    // `array{name: string, age?: int}` - PHPDoc's structural array type.
    ArrayShape(Shape),
    // `object{name: string, age?: int}` - the `object`-flavored sibling
    // of `ArrayShape`. Structural, not a real registered class:
    // `registry::accepts` checks a real instance against it
    // property-by-property.
    ObjectShape(Shape),
    // A class name plus its resolved `@template` type arguments, if any;
    // empty for a non-generic class or a generic one referenced bare
    // (unspecified/erased, as real PHPStan treats it). Deliberately
    // invariant, call-site-only substitution - no variance checking or
    // template bound enforcement.
    Object(Name, Vec<Type>),
    // The native `object` type: an instance of no particular class.
    ObjectWithoutClass,
    // A `resource` (file/curl/... handle); `closed` for PHPDoc's
    // `closed-resource`, the kind `is_resource()` rejects.
    Resource {
        closed: bool,
    },
    // A single enum case's singleton type (enum fqn, case name), distinct
    // from the coarser `Object(Suit, [])`. Not a subtype target in
    // `Relation::is_super_type_of`'s class-hierarchy sense; `registry::accepts`
    // handles `EnumCase -> Object(enum_fqn)` assignability instead.
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
    VarianceArg(CallSiteVariance, Box<Type>),
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
    // Only ever a member of a `Type::Intersection` alongside its own base
    // type (`String` or `Array(_)`).
    Accessory(AccessoryKind),
    // Mirrors real PHPStan's `HasOffsetValueType` - only ever a member of
    // a `Type::Intersection` alongside its own coarse `Type::Array(K, V)`
    // base, tracking one specific literal key's own precise value type
    // (`key`, `value`) without converting the whole container into an
    // `ArrayShape`. Confirmed directly against the vendored real
    // `phpstan.phar` via `assertType()`: writing a literal key onto an
    // already-coarse array keeps this precision (`non-empty-array<string,
    // mixed>&hasOffsetValue('a', array{1, 2, 3})`), and a second
    // literal-key write accumulates a further, separate
    // `Type::HasOffsetValue` member rather than replacing the first (see
    // `checker::assignment::write_literal_key_aware`, this project's own
    // construction site). A dynamic (non-literal) key write, or one
    // inside a loop body currently under analysis, never produces this -
    // real PHPStan doesn't either, confirmed the same way: a loop
    // variable narrowed to a literal during one simulated pass still
    // generalizes away before the merge that produces the loop's own
    // exit type.
    HasOffsetValue(ShapeKey, Box<Type>),
    // PHPStan's `HasOffsetType`/`HasMethodType`/`HasPropertyType`: only
    // ever an `Intersection` member, refining its base with an offset,
    // method or property it's proven to have (`array_key_exists()`,
    // `method_exists()`, `property_exists()`).
    HasOffset(ShapeKey),
    HasMethod(Name),
    HasProperty(Name),
    // A `ReflectionMethod`/`ReflectionProperty` bound to one specific,
    // literally-named member (owner type, member name) of a
    // `ReflectionClass<T>`'s own reflected `T` - Phase 77's "binding a
    // specific reflected member's own value type" stretch item. Only
    // ever a member of a `Type::Intersection` alongside its own coarse
    // `Type::Object("ReflectionMethod"|"ReflectionProperty", [TClass])`
    // base (mirrors `HasOffsetValue`'s own shape exactly), constructed at
    // `getMethod('name')`/`getProperty('name')`'s own call site
    // (`checker::expr::member_resolution`) and consumed at
    // `invoke()`/`getValue()`'s own call site, which resolves the real
    // member's declared type on the owner, substituting the owner's own
    // `@template` bindings the same way an ordinary generic method call
    // already does. The owner is stored as a full `Type` (not a bare
    // class-name string) so a generic reflected class
    // (`ReflectionClass<Foo<Bar>>`) keeps its own resolved type
    // arguments too. A dynamic (non-literal) member name, or a variable
    // whose own inferred type isn't a string literal either, never
    // produces this - matching this project's own "don't guess" leniency
    // for every other literal-key-aware construction site.
    ReflectedMember(Box<Type>, String),
    // `value-of<T>` whose `T` a parse alone can't see through - an enum,
    // a class-constant reference or a template - resolved once the
    // registry exists (`Registry::resolve_deferred_value_of`).
    ValueOf(Box<Type>),
    // `key-of<T>` over a `T` not known yet (a template): resolved once
    // `T` is bound (`crate::resolve_late`).
    KeyOf(Box<Type>),
    // `T['key']` (PHPStan's `OffsetAccessType`) over a `T` not known yet:
    // the base, and the offset as its literal key type.
    OffsetAccess(Box<Type>, Box<Type>),
    // `Foo::BAR`/`Foo::BAR_*` (real PHPStan's `ConstTypeNode`, a `*`
    // matching any run of characters): the type of the matching class
    // constants, resolved once the registry exists.
    ClassConstant {
        class: Box<Type>,
        pattern: String,
    },
    // `($subject is $target ? $if_true : $if_false)` - PHPDoc's
    // conditional-return-type syntax, lazily resolved once the real
    // call-site argument type for `subject` is known
    // (`crate::conditional::resolve_conditional`). `subject` is the raw
    // parameter name with no leading `$`; only a bare `$param` subject is
    // modeled. A consumer that can't resolve this variant falls back to
    // the eager `Union([if_true, if_false])`.
    // An extension's own payload (`crate::ExtensionTypeData`).
    Extension(Arc<dyn ExtensionTypeData>),
    Conditional {
        subject: String,
        target: Box<Type>,
        if_true: Box<Type>,
        if_false: Box<Type>,
        negated: bool,
        // `subject` names a `@template` (`(T is int ? A : B)`), resolved
        // once the template is bound, rather than a parameter.
        template_subject: bool,
    },
}

/// Deliberately not derived: `Mixed`'s own `bool` is ignored entirely
/// (matching real PHPStan's own `MixedType::equals()`, see `Type::Mixed`'s
/// doc comment), and two NaN float literals are equal (PHPStan's
/// `ConstantFloatType::equals()`), which makes the relation an `Eq`.
/// `Hash` below agrees with it.
impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type(Repr::Mixed(_)), Type(Repr::Mixed(_))) => true,
            (Type(Repr::Error), Type(Repr::Error)) => true,
            (Type(Repr::Never), Type(Repr::Never)) => true,
            (Type(Repr::Void), Type(Repr::Void)) => true,
            (Type(Repr::Null), Type(Repr::Null)) => true,
            (Type(Repr::Bool), Type(Repr::Bool)) => true,
            (Type(Repr::True), Type(Repr::True)) => true,
            (Type(Repr::False), Type(Repr::False)) => true,
            (Type(Repr::Int), Type(Repr::Int)) => true,
            (Type(Repr::IntLiteral(a)), Type(Repr::IntLiteral(b))) => a == b,
            (Type(Repr::Float), Type(Repr::Float)) => true,
            (Type(Repr::String), Type(Repr::String)) => true,
            (Type(Repr::StringLiteral(a)), Type(Repr::StringLiteral(b))) => a == b,
            (Type(Repr::Array(ak, av)), Type(Repr::Array(bk, bv))) => ak == bk && av == bv,
            (Type(Repr::ClassString(a)), Type(Repr::ClassString(b))) => a == b,
            (Type(Repr::ClassKindString(a)), Type(Repr::ClassKindString(b))) => a == b,
            (Type(Repr::ArrayShape(a)), Type(Repr::ArrayShape(b))) => a == b,
            (Type(Repr::ObjectShape(a)), Type(Repr::ObjectShape(b))) => a == b,
            (Type(Repr::Object(an, aa)), Type(Repr::Object(bn, ba))) => {
                an.eq_ignore_case(bn) && aa == ba
            }
            (Type(Repr::ObjectWithoutClass), Type(Repr::ObjectWithoutClass)) => true,
            (Type(Repr::Resource { closed: a }), Type(Repr::Resource { closed: b })) => a == b,
            (Type(Repr::EnumCase(ae, ac)), Type(Repr::EnumCase(be, bc))) => {
                ae.eq_ignore_case(be) && ac == bc
            }
            (Type(Repr::Callable(a)), Type(Repr::Callable(b))) => a == b,
            (Type(Repr::SelfRef(a)), Type(Repr::SelfRef(b))) => a == b,
            (Type(Repr::ParentRef), Type(Repr::ParentRef)) => true,
            (Type(Repr::StaticRef(a)), Type(Repr::StaticRef(b))) => a == b,
            (Type(Repr::This), Type(Repr::This)) => true,
            (Type(Repr::TemplateParam(a)), Type(Repr::TemplateParam(b))) => a == b,
            (Type(Repr::VarianceArg(av, a)), Type(Repr::VarianceArg(bv, b))) => av == bv && a == b,
            (Type(Repr::Union(a)), Type(Repr::Union(b))) => a == b,
            (Type(Repr::BenevolentUnion(a)), Type(Repr::BenevolentUnion(b))) => a == b,
            (Type(Repr::Intersection(a)), Type(Repr::Intersection(b))) => a == b,
            (Type(Repr::IntRange(amin, amax)), Type(Repr::IntRange(bmin, bmax))) => {
                amin == bmin && amax == bmax
            }
            (Type(Repr::FloatLiteral(a)), Type(Repr::FloatLiteral(b))) => {
                a == b || (a.is_nan() && b.is_nan())
            }
            (Type(Repr::Accessory(a)), Type(Repr::Accessory(b))) => a == b,
            (Type(Repr::HasOffsetValue(ak, av)), Type(Repr::HasOffsetValue(bk, bv))) => {
                ak == bk && av == bv
            }
            (Type(Repr::HasOffset(a)), Type(Repr::HasOffset(b))) => a == b,
            (Type(Repr::HasMethod(a)), Type(Repr::HasMethod(b))) => a.eq_ignore_case(b),
            (Type(Repr::HasProperty(a)), Type(Repr::HasProperty(b))) => a == b,
            (Type(Repr::ReflectedMember(ao, an)), Type(Repr::ReflectedMember(bo, bn))) => {
                ao == bo && an == bn
            }
            (Type(Repr::ValueOf(a)), Type(Repr::ValueOf(b))) => a == b,
            (Type(Repr::Extension(a)), Type(Repr::Extension(b))) => a.eq_data(&**b),
            (Type(Repr::KeyOf(a)), Type(Repr::KeyOf(b))) => a == b,
            (Type(Repr::OffsetAccess(ab, ak)), Type(Repr::OffsetAccess(bb, bk))) => {
                ab == bb && ak == bk
            }
            (
                Type(Repr::ClassConstant {
                    class: ac,
                    pattern: ap,
                }),
                Type(Repr::ClassConstant {
                    class: bc,
                    pattern: bp,
                }),
            ) => ac == bc && ap == bp,
            (
                Type(Repr::Conditional {
                    subject: a_subject,
                    target: a_target,
                    if_true: a_if_true,
                    if_false: a_if_false,
                    negated: a_negated,
                    template_subject: a_template,
                }),
                Type(Repr::Conditional {
                    subject: b_subject,
                    target: b_target,
                    if_true: b_if_true,
                    if_false: b_if_false,
                    negated: b_negated,
                    template_subject: b_template,
                }),
            ) => {
                a_template == b_template
                    && a_subject == b_subject
                    && a_target == b_target
                    && a_if_true == b_if_true
                    && a_if_false == b_if_false
                    && a_negated == b_negated
            }
            (
                Type(Repr::Mixed(_))
                | Type(Repr::Error)
                | Type(Repr::Never)
                | Type(Repr::Void)
                | Type(Repr::Null)
                | Type(Repr::Bool)
                | Type(Repr::True)
                | Type(Repr::False)
                | Type(Repr::Int)
                | Type(Repr::IntLiteral(_))
                | Type(Repr::Float)
                | Type(Repr::String)
                | Type(Repr::StringLiteral(_))
                | Type(Repr::Array(..))
                | Type(Repr::ClassString(_))
                | Type(Repr::ClassKindString(_))
                | Type(Repr::ArrayShape(_))
                | Type(Repr::ObjectShape(_))
                | Type(Repr::Object(..))
                | Type(Repr::ObjectWithoutClass)
                | Type(Repr::Resource { .. })
                | Type(Repr::EnumCase(..))
                | Type(Repr::Callable(_))
                | Type(Repr::SelfRef(_))
                | Type(Repr::ParentRef)
                | Type(Repr::StaticRef(_))
                | Type(Repr::This)
                | Type(Repr::TemplateParam(_))
                | Type(Repr::VarianceArg(..))
                | Type(Repr::Union(_))
                | Type(Repr::BenevolentUnion(_))
                | Type(Repr::Intersection(_))
                | Type(Repr::IntRange(..))
                | Type(Repr::FloatLiteral(_))
                | Type(Repr::Accessory(_))
                | Type(Repr::HasOffsetValue(..))
                | Type(Repr::HasOffset(_))
                | Type(Repr::HasMethod(_))
                | Type(Repr::HasProperty(_))
                | Type(Repr::ReflectedMember(..))
                | Type(Repr::ValueOf(_))
                | Type(Repr::KeyOf(_))
                | Type(Repr::OffsetAccess(..))
                | Type(Repr::ClassConstant { .. })
                | Type(Repr::Extension(_))
                | Type(Repr::Conditional { .. }),
                _,
            ) => false,
        }
    }
}

impl Eq for Type {}

impl Hash for Type {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(&self.0).hash(state);
        match self {
            Type(Repr::Mixed(_))
            | Type(Repr::Error)
            | Type(Repr::Never)
            | Type(Repr::Void)
            | Type(Repr::Null)
            | Type(Repr::Bool)
            | Type(Repr::True)
            | Type(Repr::False)
            | Type(Repr::Int)
            | Type(Repr::Float)
            | Type(Repr::String)
            | Type(Repr::ObjectWithoutClass)
            | Type(Repr::ParentRef)
            | Type(Repr::This) => {}
            Type(Repr::IntLiteral(n)) => n.hash(state),
            Type(Repr::StringLiteral(s)) => s.hash(state),
            Type(Repr::Array(k, v)) | Type(Repr::OffsetAccess(k, v)) => {
                k.hash(state);
                v.hash(state);
            }
            Type(Repr::ClassString(inner))
            | Type(Repr::ValueOf(inner))
            | Type(Repr::KeyOf(inner)) => inner.hash(state),
            Type(Repr::ClassKindString(kind)) => kind.hash(state),
            Type(Repr::ArrayShape(shape)) | Type(Repr::ObjectShape(shape)) => shape.hash(state),
            Type(Repr::Object(name, args)) => {
                name.lower().hash(state);
                args.hash(state);
            }
            Type(Repr::Resource { closed }) => closed.hash(state),
            Type(Repr::EnumCase(class, case)) => {
                class.lower().hash(state);
                case.hash(state);
            }
            Type(Repr::Callable(sig)) => sig.hash(state),
            Type(Repr::SelfRef(args))
            | Type(Repr::StaticRef(args))
            | Type(Repr::Union(args))
            | Type(Repr::BenevolentUnion(args))
            | Type(Repr::Intersection(args)) => args.hash(state),
            Type(Repr::TemplateParam(tp)) => tp.hash(state),
            Type(Repr::VarianceArg(variance, inner)) => {
                variance.hash(state);
                inner.hash(state);
            }
            Type(Repr::IntRange(min, max)) => {
                min.hash(state);
                max.hash(state);
            }
            // Equal floats hash alike: both zeros, and every NaN.
            Type(Repr::FloatLiteral(f)) => {
                let bits = if *f == 0.0 {
                    0
                } else if f.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    f.to_bits()
                };
                bits.hash(state)
            }
            Type(Repr::Accessory(kind)) => kind.hash(state),
            Type(Repr::HasOffsetValue(key, value)) => {
                key.hash(state);
                value.hash(state);
            }
            Type(Repr::HasOffset(key)) => key.hash(state),
            Type(Repr::HasMethod(name)) => name.lower().hash(state),
            Type(Repr::HasProperty(name)) => name.hash(state),
            Type(Repr::ReflectedMember(owner, member)) => {
                owner.hash(state);
                member.hash(state);
            }
            Type(Repr::ClassConstant { class, pattern }) => {
                class.hash(state);
                pattern.hash(state);
            }
            Type(Repr::Extension(data)) => {
                let state: &mut dyn Hasher = state;
                data.hash_data(state);
            }
            Type(Repr::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
                template_subject,
            }) => {
                subject.hash(state);
                target.hash(state);
                if_true.hash(state);
                if_false.hash(state);
                negated.hash(state);
                template_subject.hash(state);
            }
        }
    }
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
            bound: Box::new(Type(Repr::Mixed(true))),
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
    /// `self` with every occurrence of its own templates replaced by the
    /// current `templates` entries - after their bounds changed.
    pub fn rebind_templates(mut self) -> CallableSig {
        fn rebind(ty: &Type, templates: &[TemplateParam]) -> Type {
            match ty {
                Type(Repr::TemplateParam(tp))
                    if tp.scope.as_deref() == Some(CALLABLE_TEMPLATE_SCOPE) =>
                {
                    match templates.iter().find(|t| t.name == tp.name) {
                        Some(t) => Type(Repr::TemplateParam(t.clone())),
                        None => ty.clone(),
                    }
                }
                other => other.map_children(&mut |c| rebind(c, templates)),
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

impl Type {
    /// Every string value this type can only be: a string literal, each
    /// member's of an intersection, and a union's only when every member
    /// has some (PHPStan's `Type::getConstantStrings()`). Empty for
    /// anything else.
    pub fn constant_strings(&self) -> Vec<&str> {
        match self {
            Type(Repr::StringLiteral(s)) => s.as_str().into_iter().collect(),
            Type(Repr::Intersection(members)) => {
                members.iter().flat_map(Type::constant_strings).collect()
            }
            Type(Repr::Union(members)) => {
                let mut all = Vec::new();
                for member in members {
                    let strings = member.constant_strings();
                    if strings.is_empty() {
                        return Vec::new();
                    }
                    all.extend(strings);
                }
                all
            }
            _ => Vec::new(),
        }
    }

    /// `VerbosityLevel::typeOnly()`: literal values and array shapes
    /// collapse to their plain type.
    pub fn describe_type_only(&self) -> String {
        match self {
            Type(Repr::IntLiteral(_)) | Type(Repr::IntRange(..)) => "int".to_string(),
            Type(Repr::FloatLiteral(_)) => "float".to_string(),
            Type(Repr::StringLiteral(_)) => "string".to_string(),
            Type(Repr::True) | Type(Repr::False) => "bool".to_string(),
            Type(Repr::Array(..)) | Type(Repr::ArrayShape(_)) => "array".to_string(),
            Type(Repr::Object(class, _)) | Type(Repr::EnumCase(class, _)) => class.to_string(),
            Type(Repr::Intersection(members)) => members
                .iter()
                .find(|m| !m.is_accessory())
                .map_or_else(|| self.to_string(), Type::describe_type_only),
            Type(Repr::Union(members)) => {
                let mut parts: Vec<String> = Vec::new();
                for member in members {
                    let part = member.describe_type_only();
                    if !parts.contains(&part) {
                        parts.push(part);
                    }
                }
                parts.join("|")
            }
            other => other.to_string(),
        }
    }

    /// This type with `f` applied to each directly nested type (generic
    /// arguments, array key/value, shape fields, union/intersection
    /// members, callable parameters and return, conditional branches);
    /// a leaf is returned unchanged. `f` decides whether to recurse.
    pub fn map_children(&self, f: &mut impl FnMut(&Type) -> Type) -> Type {
        match self {
            Type(Repr::Array(k, v)) => Type(Repr::Array(Box::new(f(k)), Box::new(f(v)))),
            Type(Repr::ClassString(inner)) => Type(Repr::ClassString(Box::new(f(inner)))),
            Type(Repr::ArrayShape(shape)) => Type(Repr::ArrayShape(shape.map_types(f))),
            Type(Repr::ObjectShape(shape)) => Type(Repr::ObjectShape(shape.map_types(f))),
            Type(Repr::Object(name, args)) => Type(Repr::Object(
                name.clone(),
                args.iter().map(&mut *f).collect(),
            )),
            Type(Repr::SelfRef(args)) => Type(Repr::SelfRef(args.iter().map(&mut *f).collect())),
            Type(Repr::StaticRef(args)) => {
                Type(Repr::StaticRef(args.iter().map(&mut *f).collect()))
            }
            Type(Repr::Callable(Some(sig))) => Type(Repr::Callable(Some(Arc::new(CallableSig {
                params: sig
                    .params
                    .iter()
                    .map(|p| CallableParam {
                        ty: f(&p.ty),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(f(&sig.return_type)),
                templates: sig.templates.clone(),
            })))),
            Type(Repr::VarianceArg(variance, inner)) => {
                Type(Repr::VarianceArg(*variance, Box::new(f(inner))))
            }
            Type(Repr::Union(ms)) => Type(Repr::Union(ms.iter().map(&mut *f).collect())),
            Type(Repr::BenevolentUnion(ms)) => {
                Type(Repr::BenevolentUnion(ms.iter().map(&mut *f).collect()))
            }
            Type(Repr::Intersection(ms)) => {
                Type(Repr::Intersection(ms.iter().map(&mut *f).collect()))
            }
            Type(Repr::HasOffsetValue(key, inner)) => {
                Type(Repr::HasOffsetValue(key.clone(), Box::new(f(inner))))
            }
            Type(Repr::ReflectedMember(owner, member)) => {
                Type(Repr::ReflectedMember(Box::new(f(owner)), member.clone()))
            }
            Type(Repr::ValueOf(inner)) => Type(Repr::ValueOf(Box::new(f(inner)))),
            Type(Repr::KeyOf(inner)) => Type(Repr::KeyOf(Box::new(f(inner)))),
            Type(Repr::OffsetAccess(base, key)) => {
                Type(Repr::OffsetAccess(Box::new(f(base)), Box::new(f(key))))
            }
            Type(Repr::ClassConstant { class, pattern }) => Type(Repr::ClassConstant {
                class: Box::new(f(class)),
                pattern: pattern.clone(),
            }),
            Type(Repr::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
                template_subject,
            }) => Type(Repr::Conditional {
                subject: subject.clone(),
                target: Box::new(f(target)),
                if_true: Box::new(f(if_true)),
                if_false: Box::new(f(if_false)),
                negated: *negated,
                template_subject: *template_subject,
            }),
            leaf => leaf.clone(),
        }
    }

    /// Calls `f` on each directly nested type - the children
    /// `map_children` rebuilds, in the same order.
    pub fn for_each_child<'t>(&'t self, f: &mut impl FnMut(&'t Type)) {
        match self {
            Type(Repr::Array(k, v)) => {
                f(k);
                f(v);
            }
            Type(Repr::ClassString(inner))
            | Type(Repr::VarianceArg(_, inner))
            | Type(Repr::HasOffsetValue(_, inner))
            | Type(Repr::ReflectedMember(inner, _))
            | Type(Repr::ValueOf(inner))
            | Type(Repr::KeyOf(inner))
            | Type(Repr::ClassConstant { class: inner, .. }) => f(inner),
            Type(Repr::OffsetAccess(base, key)) => {
                f(base);
                f(key);
            }
            Type(Repr::ArrayShape(shape)) | Type(Repr::ObjectShape(shape)) => {
                shape.types().for_each(f)
            }
            Type(Repr::Object(_, args))
            | Type(Repr::SelfRef(args))
            | Type(Repr::StaticRef(args)) => args.iter().for_each(f),
            Type(Repr::Callable(Some(sig))) => {
                for p in &sig.params {
                    f(&p.ty);
                }
                f(&sig.return_type);
            }
            Type(Repr::Union(ms))
            | Type(Repr::BenevolentUnion(ms))
            | Type(Repr::Intersection(ms)) => ms.iter().for_each(f),
            Type(Repr::Conditional {
                target,
                if_true,
                if_false,
                ..
            }) => {
                f(target);
                f(if_true);
                f(if_false);
            }
            _ => {}
        }
    }

    /// Whether `f` holds for some directly nested type.
    pub fn any_child(&self, mut f: impl FnMut(&Type) -> bool) -> bool {
        let mut found = false;
        self.for_each_child(&mut |t| found = found || f(t));
        found
    }

    /// Whether this is an accessory refinement - only ever an
    /// `Intersection` member alongside the base type it refines.
    pub fn is_accessory(&self) -> bool {
        matches!(
            self,
            Type(Repr::Accessory(_))
                | Type(Repr::HasOffsetValue(..))
                | Type(Repr::HasOffset(_))
                | Type(Repr::HasMethod(_))
                | Type(Repr::HasProperty(_))
        )
    }

    /// A union of `members` of the same kind as `self`: benevolent when
    /// `self` is a benevolent union, plain otherwise.
    pub fn with_members(&self, members: Vec<Type>) -> Type {
        match self {
            Type(Repr::BenevolentUnion(_)) => Type(Repr::BenevolentUnion(members)),
            _ => Type(Repr::Union(members)),
        }
    }

    /// A closure/arrow-fn literal's own value: a real `Closure` instance
    /// (so `->bindTo()`/`->call()`/`instanceof Closure` keep resolving
    /// against the `Closure` class) that also carries the literal's own
    /// signature - real PHPStan's `ClosureType`, which is both at once.
    /// `Display` renders this exact pair as `Closure(params): return`.
    pub fn closure_literal(sig: CallableSig) -> Type {
        Type(Repr::Intersection(vec![
            Type(Repr::Object("Closure".into(), Vec::new())),
            Type(Repr::Callable(Some(Arc::new(sig)))),
        ]))
    }

    /// The signature a value of this type can be invoked with, when one
    /// is known: a shaped `Closure(...)`/`callable(...)` PHPDoc type, or a
    /// closure literal's `closure_literal` intersection.
    pub fn callable_sig(&self) -> Option<&Arc<CallableSig>> {
        match self {
            Type(Repr::Callable(Some(sig))) => Some(sig),
            Type(Repr::Intersection(members)) => members.iter().find_map(|m| match m {
                Type(Repr::Callable(Some(sig))) => Some(sig),
                _ => None,
            }),
            _ => None,
        }
    }

    /// `list<element>`: an array whose keys are `0..n` in order.
    pub fn list_of(element: Type) -> Type {
        Type(Repr::Intersection(vec![
            Type(Repr::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(element),
            )),
            Type(Repr::Accessory(AccessoryKind::ArrayList)),
        ]))
    }

    /// PHP's full legal-array-key domain (`int|string`) - the default for
    /// a bare `array`/`array<V>` with no key argument given.
    pub fn unknown_array_key() -> Type {
        Type(Repr::Union(vec![Type(Repr::Int), Type(Repr::String)]))
    }

    /// `Type::Array(unknown_array_key(), value)`, for a construction site
    /// with no more precise key type available.
    pub fn array_of(value: Type) -> Type {
        Type(Repr::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(value),
        ))
    }

    /// The `int|string` key type PHPDoc's `Type[]` array-shorthand
    /// (`Item[]`), the single-generic-arg `array<V>` form, and the
    /// array-union `+` operator's own "neither side had a more precise
    /// key" fallback all use - a real `BenevolentUnionType([Int,
    /// String])` marker (mirrors real PHPStan's
    /// `TypeNodeResolver::resolveArrayTypeNode`/`resolveGenericTypeNode`'s
    /// own `array<V>` arm/`InitializerExprTypeResolver::getPlusType`,
    /// confirmed reading all three directly), *not* the plain, strict
    /// `unknown_array_key()`. `RuleLevelHelper::accepts()` treats a
    /// benevolent union specially - accepted if *any* member matches,
    /// not only when *all* do - the real difference: a `@var Item[]`
    /// foreach-key passed to a `string`-typed parameter produces zero
    /// errors in real PHPStan (confirmed via `dumpType()`), unlike a
    /// plain, strict `int|string` checked the same way. Deliberately a
    /// *narrower* helper than `unknown_array_key()`, not a blanket
    /// replacement for it: real PHPStan's own benevolence for a bare
    /// `array` (no generic args at all)'s *stored* `Mixed` key type is a
    /// separate, read-time-only substitution
    /// (`ArrayType::getIterableKeyType()`) keyed on a distinction
    /// (`Mixed` vs. an already-real key type) this project's own eagerly-
    /// constructed `unknown_array_key()` has no way to preserve, and a
    /// 2-arg `array<K, V>` with an explicit key never gets the marker
    /// either - see this function's own real call sites (`types/parse.rs`'s
    /// `Type[]`-shorthand and `array<V>` parses, `checker/expr.rs`'s `+`
    /// operator fallback) for exactly which real-PHPStan-confirmed
    /// construction sites this marker belongs at, not every "no more
    /// precise key" site project-wide.
    pub fn benevolent_array_key() -> Type {
        Type(Repr::BenevolentUnion(vec![
            Type(Repr::Int),
            Type(Repr::String),
        ]))
    }
}

// Mirrors real PHPStan's `Accessory*Type` family
// (`vendor/phpstan-src/src/Type/Accessory/*.php`) - each kind only ever
// appears wrapped in a `Type::Intersection` alongside its base type.
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
