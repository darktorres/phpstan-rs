use std::sync::Arc;

mod conditional;
mod display;
mod parse;
mod subtype;
mod templates;

pub use conditional::resolve_conditional;
pub use parse::parse_type_string;
pub(crate) use parse::{canonical_int_key, strip_call_site_variance};
pub use subtype::is_subtype;
pub(crate) use subtype::{
    is_numeric_string_literal, shape_fields_are_list_like, shape_key_type, types_equal_for_union,
};
pub use templates::{bind_template_param, promote_class_string_literals, substitute_template};

// A slice of PHPStan's type system covering what the parser captures as
// type-hint strings, plus self/parent/static and `@template` generics.
// Inheritance-chain substitution (a subclass fixing its parent's own type
// parameter via `@extends Base<Concrete>`) is not modeled.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Mixed,
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
    StringLiteral(String),
    // Key type alongside value type. Construction sites with no more
    // precise key generally pass `Type::unknown_array_key()`
    // (`int|string`).
    Array(Box<Type>, Box<Type>),
    // `class-string<T>` - holds `T`'s (or a subclass's) fully-qualified
    // name, tracked so it can participate in `@template` binding; a bare
    // `class-string` has no `T` to bind and collapses to plain `String`.
    ClassString(Box<Type>),
    // `array{name: string, age?: int}` - PHPDoc's structural array type.
    // (field name, field type, optional)
    ArrayShape(Vec<(String, Type, bool)>),
    // `object{name: string, age?: int}` - the `object`-flavored sibling
    // of `ArrayShape`. Structural, not a real registered class:
    // `registry::is_assignable` checks a real instance against it
    // property-by-property.
    ObjectShape(Vec<(String, Type, bool)>),
    // A class name plus its resolved `@template` type arguments, if any;
    // empty for a non-generic class or a generic one referenced bare
    // (unspecified/erased, as real PHPStan treats it). Deliberately
    // invariant, call-site-only substitution - no variance checking or
    // template bound enforcement.
    Object(String, Vec<Type>),
    // A single enum case's singleton type (enum fqn, case name), distinct
    // from the coarser `Object(Suit, [])`. Not a subtype target in
    // `types::is_subtype`'s class-hierarchy sense; `registry::is_assignable`
    // handles `EnumCase -> Object(enum_fqn)` assignability instead.
    EnumCase(String, String),
    // `None` = unknown shape; `Some(sig)` = a parsed `Closure(int):
    // string`/`callable(int): string` signature. `Arc` since `Type` is
    // cloned pervasively and a parsed shape can be non-trivially sized.
    Callable(Option<Arc<CallableSig>>),
    SelfRef,
    ParentRef,
    StaticRef,
    // An unbound `@template T` placeholder, produced for both a
    // function/method's own declared templates and a class's own
    // `@template`. Carries its own bound/variance/scope/default (see
    // `TemplateParam` below) rather than being a bare name.
    TemplateParam(TemplateParam),
    Union(Vec<Type>),
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
    HasOffsetValue(String, Box<Type>),
    // `($subject is $target ? $if_true : $if_false)` - PHPDoc's
    // conditional-return-type syntax, lazily resolved once the real
    // call-site argument type for `subject` is known
    // (`types::conditional::resolve_conditional`). `subject` is the raw
    // parameter name with no leading `$`; only a bare `$param` subject is
    // modeled. A consumer that can't resolve this variant falls back to
    // the eager `Union([if_true, if_false])`.
    Conditional {
        subject: String,
        target: Box<Type>,
        if_true: Box<Type>,
        if_false: Box<Type>,
        negated: bool,
    },
}

// Whether a class's own `@template` may vary co-/contravariantly with a
// subclass relationship, or must stay exactly fixed (`Invariant`, the
// default). `checker::generics::check_variance` composes this through
// nested generics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variance {
    #[default]
    Invariant,
    Covariant,
    Contravariant,
}

/// A `@template` parameter's own metadata.
#[derive(Debug, Clone, PartialEq)]
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
            bound: Box::new(Type::Mixed),
            variance: Variance::Invariant,
            scope: None,
            default: None,
        }
    }
}

/// One parameter of a parsed `Closure(...)`/`callable(...)` PHPDoc shape.
/// A dedicated struct rather than `registry::ParamSig`, whose sibling
/// `FunctionSig` can't cheaply derive `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub struct CallableParam {
    pub ty: Type,
    pub variadic: bool,
    /// A trailing `=` in the shape grammar (`Closure(int=): void`).
    pub optional: bool,
    /// A leading `&` in the shape grammar (`Closure(&int): void`).
    pub by_ref: bool,
}

/// A parsed `Closure(...)`/`callable(...)` PHPDoc shape's params-plus-
/// return-type slice - deliberately not the rest of what real PHPStan's
/// `CallableType` tracks (template-type map, purity, assertions).
#[derive(Debug, Clone, PartialEq)]
pub struct CallableSig {
    pub params: Vec<CallableParam>,
    pub return_type: Box<Type>,
}

impl Type {
    /// PHP's full legal-array-key domain (`int|string`) - the default for
    /// a bare `array`/`array<V>` with no key argument given.
    pub(crate) fn unknown_array_key() -> Type {
        Type::Union(vec![Type::Int, Type::String])
    }

    /// `Type::Array(unknown_array_key(), value)`, for a construction site
    /// with no more precise key type available.
    pub(crate) fn array_of(value: Type) -> Type {
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(value))
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
    pub(crate) fn benevolent_array_key() -> Type {
        Type::Intersection(vec![
            Type::unknown_array_key(),
            Type::Accessory(AccessoryKind::Benevolent),
        ])
    }
}

// Mirrors real PHPStan's `Accessory*Type` family
// (`vendor/phpstan-src/src/Type/Accessory/*.php`) - each kind only ever
// appears wrapped in a `Type::Intersection` alongside its base type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    // Mirrors real PHPStan's `BenevolentUnionType` - only ever wraps a
    // `Type::Union` (see `Type::benevolent_array_key()`'s own doc
    // comment for exactly which two construction sites use it and why).
    Benevolent,
}

#[cfg(test)]
mod tests;
