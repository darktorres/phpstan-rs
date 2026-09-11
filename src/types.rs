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
pub use templates::{bind_template_param, substitute_template};

// A deliberately small slice of PHPStan's type system: enough to represent
// what our parser already captures as raw type-hint strings, plus the
// handful of pseudo-types (self/parent/static) that need special handling
// once a class-context resolver exists. `@template` generics (both a
// function/method's own and a class's - registry::mark_template_params /
// checker::class_template_bindings) are parsed and substituted at each
// call site; still no conditional return types, variance checking, or
// inheritance-chain substitution.
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
    // Phase 50: a real key type alongside the value type, mirroring real
    // PHPStan's own `ArrayType(keyType, valueType)`. `Array::unknown_key`
    // below builds the "don't know, could be either" default
    // (`int|string`, PHP's own full legal-key-type domain) every
    // construction site used before this phase - most call sites still
    // pass that; only ones that can cheaply prove a narrower key type
    // (an array literal's own keys, a stdlib function whose real PHPStan
    // stub declares one) do better. `array<V>`'s own 1-arg PHPDoc form
    // (key omitted) also means "unknown," the same default.
    Array(Box<Type>, Box<Type>),
    // `class-string<T>` - a string known to hold `T`'s (or a subclass's)
    // fully-qualified name, tracked precisely enough to participate in
    // `@template` binding (unlike a bare `class-string`, which collapses to
    // plain `String` since there's no `T` to bind - see
    // `parse_type_keyword_or_class`). Produced by `Foo::class` and
    // `get_class($obj)` when the target class is statically known.
    ClassString(Box<Type>),
    // `array{name: string, age?: int}` - PHPDoc's structural array type.
    // (field name, field type, optional)
    ArrayShape(Vec<(String, Type, bool)>),
    // Phase 80: `object{name: string, age?: int}` - PHPDoc's structural
    // *object* shape, the `object`-flavored sibling of `ArrayShape`
    // right above (identical (name, type, optional) field shape,
    // identical parsing precedent in `parse_type_atom`). Before this
    // variant existed, the raw `object{...}` text itself was mangled
    // into a bogus, namespace-qualified "class name" - a real, active
    // false positive (`class.notFound`) on every real use of this
    // syntax, not just a missing-precision gap. Structural, not a real
    // registered class: `registry::is_assignable`'s own `ObjectShape`
    // arm is where a real class instance gets checked against it
    // property-by-property.
    ObjectShape(Vec<(String, Type, bool)>),
    // A class name plus its resolved `@template` type arguments, if any
    // (`Collection<User>` -> `Object("Collection", [Object("User", [])])`)
    // - empty for a non-generic class, or a generic one referenced bare
    // (`Collection` alone, with no `<...>`, is `Object("Collection", [])`
    // too; a bare reference to a generic class isn't an error here, just
    // an unspecified/erased one, the same way real PHPStan treats it).
    // Deliberately invariant, call-site-only substitution - no variance
    // checking, template bound enforcement, or inheritance-chain
    // substitution (a subclass fixing its parent's own type parameter via
    // `@extends Base<Concrete>` isn't modeled).
    Object(String, Vec<Type>),
    // Phase 79: a single enum case's own singleton type (enum fqn, case
    // name) - `Suit::Hearts`'s real static type in real PHPStan, not the
    // coarse `Object(Suit, [])` every case previously collapsed to. Kept
    // as its own variant rather than folding the case name into
    // `Object`'s existing (always-empty, for an enum) template-args slot:
    // an enum genuinely can't be generic in real PHP, so that slot is
    // reliably free, but overloading it to mean something unrelated to
    // template arguments would be exactly the kind of undocumented,
    // action-at-a-distance encoding this project's own conventions
    // avoid elsewhere. Deliberately NOT a subtype target in
    // `types::is_subtype`'s own class-hierarchy sense (it has none) -
    // `registry::is_assignable` is where `EnumCase -> Object(enum_fqn)`
    // (and up through `UnitEnum`/`BackedEnum`/any implemented interface)
    // is actually recognized, mirroring how plain `Object`-vs-`Object`
    // hierarchy checks already work.
    EnumCase(String, String),
    // Phase 53: `None` is today's bare marker (any callable value at
    // all, no shape known); `Some(sig)` is a real, parsed `Closure(int):
    // string`/`callable(int): string` PHPDoc signature - `Arc`, not a
    // bare `Box`, since `Type` is cloned pervasively throughout this
    // checker and a parsed shape can be non-trivially sized (params plus
    // a nested return type).
    Callable(Option<Arc<CallableSig>>),
    SelfRef,
    ParentRef,
    StaticRef,
    // An unbound `@template T` - a placeholder for a type parameter that
    // hasn't been bound to a concrete type yet. Produced both for a
    // function/method's own declared template parameters and for a
    // class's own `@template` (registry::mark_template_params applies to
    // both, given the enclosing class's own template metadata alongside
    // the function's) - resolving one against an actual argument or an
    // instance's own carried type arguments is
    // checker::bind_template_param / substitute_template's job.
    //
    // Phase 54: carries its own bound/variance/scope/default (see
    // `TemplateParam` below) instead of being a bare name - PHPStan
    // splits "template bound to X" into ~19 concrete `Template*Type`
    // subclasses; this single variant plus a real bound is this
    // project's deliberately unified stand-in for all of them, not an
    // attempt to mirror each one as its own `Type` variant.
    TemplateParam(TemplateParam),
    Union(Vec<Type>),
    Intersection(Vec<Type>),
    // Phase 14: a `T` range constrained to `[min, max]` (either bound
    // `None` = unbounded) - `positive-int` = `IntRange(Some(1), None)`,
    // `int<0, 255>` = `IntRange(Some(0), Some(255))`. Fully replaces
    // `Int`'s own range, the same relationship `IntLiteral` has to
    // `Int` - never wrapped in an `Intersection`.
    IntRange(Option<i64>, Option<i64>),
    // Mirrors `IntLiteral`/`StringLiteral` for a real float literal
    // (`1.5`) appearing in a PHPDoc union (`1.5|2.5`) - a real float
    // literal in code still infers plain `Float` (no runtime-value
    // reasoning); this is purely for a PHPDoc-written literal.
    FloatLiteral(f64),
    // Only ever meaningful as a member of a `Type::Intersection`
    // alongside its own base type (`String` or `Array(_)`) - a bare
    // `Accessory(_)` outside an `Intersection` never appears by
    // construction.
    Accessory(AccessoryKind),
    // Phase 80 follow-up: `($subject is $target ? $if_true : $if_false)` -
    // PHPDoc's own conditional-return-type syntax, real PHPStan's
    // `ConditionalTypeNode` (`subjectType`/`targetType`/`if`/`else`/
    // `negated`), lazily resolved once a real call site's own bound
    // argument type for `subject` is known (`types::conditional::
    // resolve_conditional`, wired into `Checker::infer_call_return_type`)
    // rather than eagerly collapsed to `if_true|if_false` at parse time
    // the way this project's own parser used to (discarding the
    // condition entirely). `subject` is the raw parameter name with no
    // leading `$` - only a bare `$param` subject is modeled (real
    // PHPStan also allows a non-parameter type-expression subject, e.g.
    // a template name, which this project doesn't represent); every
    // consumer that can't resolve this variant (an unbound/`Mixed`
    // subject, or one that leaks into a position `resolve_conditional`
    // never runs against, e.g. a `@param`/`@var` tag) falls back to the
    // same eager `Union([if_true, if_false])` this project's parser
    // always produced before this variant existed - never `_ => false`/
    // an unconditional non-match, which would be a false-positive-
    // producing direction this project's own conventions avoid
    // throughout.
    Conditional {
        subject: String,
        target: Box<Type>,
        if_true: Box<Type>,
        if_false: Box<Type>,
        negated: bool,
    },
}

// Phase 54: mirrors real PHPStan's own `TemplateTypeVariance` - whether a
// class's own `@template` may vary co-/contravariantly with a subclass
// relationship, or must stay exactly fixed (`Invariant`, the default for
// a plain `@template T` with neither `-covariant` nor `-contravariant`).
// `checker::generics::check_variance` composes this through nested
// generics (`Collection<T>` inherits/flips based on `Collection`'s own
// declared variance for its own template slot) rather than treating
// every occurrence the same regardless of nesting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variance {
    #[default]
    Invariant,
    Covariant,
    Contravariant,
}

// Phase 54: a `@template` parameter's own metadata - what PHPStan spreads
// across ~19 concrete `Template*Type` subclasses plus `TemplateType`/
// `TemplateTypeVariance`/`TemplateTypeScope`, this project keeps as one
// struct embedded directly in `Type::TemplateParam`.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateParam {
    pub name: String,
    // `@template T of Bound` - `Mixed` (PHPStan's own implicit default
    // upper bound) when no `of` clause was written. Lets an unbound `T`
    // be used as its own bound's type prior to substitution (indexed as
    // an array, compared as a bool, treated as the bound's own object
    // members) instead of `is_subtype`'s old blanket "always compatible"
    // escape hatch - see `is_subtype`'s own `TemplateParam` arm below.
    pub bound: Box<Type>,
    pub variance: Variance,
    // Which class or function declared this template - `None` only for
    // a placeholder built with no real declaration site to name (see
    // `TemplateParam::unbounded`). Two unrelated classes in the same
    // inheritance chain commonly reuse the same short name (`T` above
    // all); `scope` is what lets `checker::generics::check_variance`
    // attribute a `T` occurrence to the class whose own
    // `@template-covariant`/`-contravariant` actually declared it,
    // rather than to an unrelated same-named ancestor or method template.
    pub scope: Option<String>,
    // `@template T = Default` (Phase 54) - the type an unbound occurrence
    // of this template falls back to instead of unconditionally `Mixed`,
    // whether that's a function-own template never bound from any real
    // call-site argument (`substitute_template` below) or an "erased"
    // class reference with fewer type arguments than declared, including
    // a bare one with none at all (`checker::generics::substitute_scoped`).
    pub default: Option<Box<Type>>,
}

impl TemplateParam {
    /// A bare, metadata-free placeholder: unbounded, invariant, no known
    /// declaration site, no default - used only where a template's own
    /// name needs to be recognized structurally (so `mark_template_params`
    /// classifies an `Object(name, _)` as a template reference at all)
    /// but its real metadata isn't available or doesn't matter, e.g. a
    /// sibling template mentioned inside another's own bound text before
    /// real substitution ever reads that inner node's own fields -
    /// substitution replaces the whole node by name regardless of what
    /// placeholder metadata it happened to carry.
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

/// Phase 53: one parameter of a parsed `Closure(...)`/`callable(...)`
/// PHPDoc shape - deliberately a small, dedicated struct rather than
/// reusing `registry::ParamSig` (that struct's sibling, `FunctionSig`,
/// can't cheaply derive `PartialEq` - it embeds `body: Option<Arc<Vec<
/// Stmt>>>`, and `Stmt` only derives `Debug, Clone`, so making the whole
/// AST `PartialEq`-able just for this one variant would be a real,
/// disproportionate blast radius - confirmed, not assumed). No `name`
/// field: the real phpdoc-parser grammar's own optional `$name` token is
/// parsed and discarded, never needed for anything this struct is used
/// for.
#[derive(Debug, Clone, PartialEq)]
pub struct CallableParam {
    pub ty: Type,
    pub variadic: bool,
    // A trailing `=` in the shape grammar (`Closure(int=): void`).
    pub optional: bool,
    // A leading `&` in the shape grammar (`Closure(&int): void`).
    pub by_ref: bool,
}

/// Phase 53: a parsed `Closure(...)`/`callable(...)` PHPDoc shape's own
/// params-plus-return-type slice - deliberately *not* the rest of what
/// real PHPStan's own `CallableType` tracks (a template-type map, purity,
/// an assertions set) - see `Type::Callable`'s own doc comment and this
/// phase's own design notes for why that's a verified architectural
/// mismatch, not an oversight.
#[derive(Debug, Clone, PartialEq)]
pub struct CallableSig {
    pub params: Vec<CallableParam>,
    pub return_type: Box<Type>,
}

impl Type {
    /// PHP's own full legal-array-key domain (`int|string`) - real
    /// PHPStan's own default `keyType` for a bare `array`/`array<V>`
    /// with no key argument given (`getIterableKeyType()`). The right
    /// "don't know, could be either" default for any construction site
    /// that can't cheaply prove a narrower key type.
    pub(crate) fn unknown_array_key() -> Type {
        Type::Union(vec![Type::Int, Type::String])
    }

    /// `Type::Array(unknown_array_key(), value)` - the drop-in
    /// replacement for every pre-Phase-50 `Type::Array(Box::new(value))`
    /// call site that has no more precise key type available. Most
    /// construction sites use this; only ones that can cheaply prove a
    /// narrower key (an array literal's own keys, a stdlib function
    /// whose real PHPStan stub declares one) build `Type::Array(k, v)`
    /// directly instead.
    pub(crate) fn array_of(value: Type) -> Type {
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(value))
    }
}

// Phase 14: mirrors real PHPStan's own `Accessory*Type` family
// (`vendor/phpstan-src/src/Type/Accessory/*.php`) - each kind only ever
// appears wrapped in a `Type::Intersection` alongside its own base type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessoryKind {
    NonEmptyString,
    NonFalsyString,
    LiteralString,
    NumericString,
    ArrayList,
    NonEmptyArray,
}

#[cfg(test)]
mod tests;
