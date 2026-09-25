use std::sync::Arc;

pub use syntax::ast::ClassKind;
pub use syntax::name::Name;

mod conditional;
mod display;
mod late;
mod parse;
mod shape;
mod subtype;
mod templates;

pub use conditional::resolve_conditional;
pub(crate) use conditional::{conditional_branch, union_of_branches};
pub(crate) use late::{contains_late, resolve_late_to_bounds, settle};
pub use parse::parse_type_string;
pub(crate) use parse::{
    canonical_int_key, has_explicit_mixed_nested_somewhere, strip_call_site_variance,
};
pub use shape::{Shape, ShapeField, ShapeKey};
pub use subtype::is_subtype;
pub(crate) use subtype::types_definitely_disjoint;
pub(crate) use subtype::{
    callable_sig_is_subtype, is_numeric_string_literal, shape_fields_are_list_like, shape_key_type,
    types_equal_for_union,
};
pub use templates::{
    bind_template_param, promote_class_string_literals, substitute_template,
    substitute_template_resolving_to_bounds,
};

// A slice of PHPStan's type system covering what the parser captures as
// type-hint strings, plus self/parent/static and `@template` generics.
// Inheritance-chain substitution (a subclass fixing its parent's own type
// parameter via `@extends Base<Concrete>`) is not modeled.
#[derive(Debug, Clone)]
pub enum Type {
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
    // `interface-string`/`trait-string`: the name of some interface or
    // trait. PHPStan reads both as a bare `class-string`; the kind is
    // checked here too.
    ClassKindString(ClassKind),
    // `array{name: string, age?: int}` - PHPDoc's structural array type.
    ArrayShape(Shape),
    // `object{name: string, age?: int}` - the `object`-flavored sibling
    // of `ArrayShape`. Structural, not a real registered class:
    // `registry::is_assignable` checks a real instance against it
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
    // `types::is_subtype`'s class-hierarchy sense; `registry::is_assignable`
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
    // `T` is bound (`types::resolve_late`).
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
        // `subject` names a `@template` (`(T is int ? A : B)`), resolved
        // once the template is bound, rather than a parameter.
        template_subject: bool,
    },
}

/// Deliberately not derived - see `Type::Mixed`'s own doc comment: every
/// arm below mirrors exactly what `#[derive(PartialEq)]` would generate,
/// except `Mixed`, whose own `bool` this comparison ignores entirely
/// (matching real PHPStan's own `MixedType::equals()`), so every one of
/// this codebase's pre-existing `== Type::Mixed`-shaped comparisons keeps
/// its exact old meaning regardless of which side's `Mixed` is explicit.
impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type::Mixed(_), Type::Mixed(_)) => true,
            (Type::Never, Type::Never) => true,
            (Type::Void, Type::Void) => true,
            (Type::Null, Type::Null) => true,
            (Type::Bool, Type::Bool) => true,
            (Type::True, Type::True) => true,
            (Type::False, Type::False) => true,
            (Type::Int, Type::Int) => true,
            (Type::IntLiteral(a), Type::IntLiteral(b)) => a == b,
            (Type::Float, Type::Float) => true,
            (Type::String, Type::String) => true,
            (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
            (Type::Array(ak, av), Type::Array(bk, bv)) => ak == bk && av == bv,
            (Type::ClassString(a), Type::ClassString(b)) => a == b,
            (Type::ClassKindString(a), Type::ClassKindString(b)) => a == b,
            (Type::ArrayShape(a), Type::ArrayShape(b)) => a == b,
            (Type::ObjectShape(a), Type::ObjectShape(b)) => a == b,
            (Type::Object(an, aa), Type::Object(bn, ba)) => an == bn && aa == ba,
            (Type::ObjectWithoutClass, Type::ObjectWithoutClass) => true,
            (Type::Resource { closed: a }, Type::Resource { closed: b }) => a == b,
            (Type::EnumCase(ae, ac), Type::EnumCase(be, bc)) => ae == be && ac == bc,
            (Type::Callable(a), Type::Callable(b)) => a == b,
            (Type::SelfRef(a), Type::SelfRef(b)) => a == b,
            (Type::ParentRef, Type::ParentRef) => true,
            (Type::StaticRef(a), Type::StaticRef(b)) => a == b,
            (Type::This, Type::This) => true,
            (Type::TemplateParam(a), Type::TemplateParam(b)) => a == b,
            (Type::VarianceArg(av, a), Type::VarianceArg(bv, b)) => av == bv && a == b,
            (Type::Union(a), Type::Union(b)) => a == b,
            (Type::BenevolentUnion(a), Type::BenevolentUnion(b)) => a == b,
            (Type::Intersection(a), Type::Intersection(b)) => a == b,
            (Type::IntRange(amin, amax), Type::IntRange(bmin, bmax)) => {
                amin == bmin && amax == bmax
            }
            (Type::FloatLiteral(a), Type::FloatLiteral(b)) => a == b,
            (Type::Accessory(a), Type::Accessory(b)) => a == b,
            (Type::HasOffsetValue(ak, av), Type::HasOffsetValue(bk, bv)) => ak == bk && av == bv,
            (Type::ReflectedMember(ao, an), Type::ReflectedMember(bo, bn)) => ao == bo && an == bn,
            (Type::ValueOf(a), Type::ValueOf(b)) => a == b,
            (Type::KeyOf(a), Type::KeyOf(b)) => a == b,
            (Type::OffsetAccess(ab, ak), Type::OffsetAccess(bb, bk)) => ab == bb && ak == bk,
            (
                Type::ClassConstant {
                    class: ac,
                    pattern: ap,
                },
                Type::ClassConstant {
                    class: bc,
                    pattern: bp,
                },
            ) => ac == bc && ap == bp,
            (
                Type::Conditional {
                    subject: a_subject,
                    target: a_target,
                    if_true: a_if_true,
                    if_false: a_if_false,
                    negated: a_negated,
                    template_subject: a_template,
                },
                Type::Conditional {
                    subject: b_subject,
                    target: b_target,
                    if_true: b_if_true,
                    if_false: b_if_false,
                    negated: b_negated,
                    template_subject: b_template,
                },
            ) => {
                a_template == b_template
                    && a_subject == b_subject
                    && a_target == b_target
                    && a_if_true == b_if_true
                    && a_if_false == b_if_false
                    && a_negated == b_negated
            }
            _ => false,
        }
    }
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

/// The variance a generic type argument is annotated with at its use
/// site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSiteVariance {
    Covariant,
    Contravariant,
    /// `*`: any type, never checked.
    Bivariant,
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
            // An unbounded `@template` has no `of`/`as` clause at all -
            // real PHPStan's own `TemplateTypeFactory`/`PhpDocNodeResolver`
            // both resolve this exact shape to `MixedType(true)`
            // (explicit), not `false` (confirmed directly against
            // `vendor/phpstan-src`).
            bound: Box::new(Type::Mixed(true)),
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
/// return-type slice, with its own templates (`callable<T>(T): T`) -
/// deliberately not the rest of what real PHPStan's `CallableType`
/// tracks (purity, assertions).
#[derive(Debug, Clone, PartialEq)]
pub struct CallableSig {
    pub params: Vec<CallableParam>,
    pub return_type: Box<Type>,
    /// The callable's own templates; every occurrence inside `params`/
    /// `return_type` is a `TemplateParam` scoped `CALLABLE_TEMPLATE_SCOPE`.
    pub templates: Vec<TemplateParam>,
}

/// The `TemplateParam::scope` of a callable type's own template.
pub(crate) const CALLABLE_TEMPLATE_SCOPE: &str = "callable";

impl CallableSig {
    /// `self` with every occurrence of its own templates replaced by the
    /// current `templates` entries - after their bounds changed.
    pub(crate) fn rebind_templates(mut self) -> CallableSig {
        fn rebind(ty: &Type, templates: &[TemplateParam]) -> Type {
            match ty {
                Type::TemplateParam(tp) if tp.scope.as_deref() == Some(CALLABLE_TEMPLATE_SCOPE) => {
                    match templates.iter().find(|t| t.name == tp.name) {
                        Some(t) => Type::TemplateParam(t.clone()),
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
    pub(crate) fn constant_strings(&self) -> Vec<&str> {
        match self {
            Type::StringLiteral(s) => vec![s.as_str()],
            Type::Intersection(members) => {
                members.iter().flat_map(Type::constant_strings).collect()
            }
            Type::Union(members) => {
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
    pub(crate) fn describe_type_only(&self) -> String {
        match self {
            Type::IntLiteral(_) | Type::IntRange(..) => "int".to_string(),
            Type::FloatLiteral(_) => "float".to_string(),
            Type::StringLiteral(_) => "string".to_string(),
            Type::True | Type::False => "bool".to_string(),
            Type::Array(..) | Type::ArrayShape(_) => "array".to_string(),
            Type::Object(class, _) | Type::EnumCase(class, _) => class.to_string(),
            Type::Intersection(members) => members
                .iter()
                .find(|m| !matches!(m, Type::Accessory(_) | Type::HasOffsetValue(..)))
                .map_or_else(|| self.to_string(), Type::describe_type_only),
            Type::Union(members) => {
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
    pub(crate) fn map_children(&self, f: &mut impl FnMut(&Type) -> Type) -> Type {
        match self {
            Type::Array(k, v) => Type::Array(Box::new(f(k)), Box::new(f(v))),
            Type::ClassString(inner) => Type::ClassString(Box::new(f(inner))),
            Type::ArrayShape(shape) => Type::ArrayShape(shape.map_types(f)),
            Type::ObjectShape(shape) => Type::ObjectShape(shape.map_types(f)),
            Type::Object(name, args) => {
                Type::Object(name.clone(), args.iter().map(&mut *f).collect())
            }
            Type::SelfRef(args) => Type::SelfRef(args.iter().map(&mut *f).collect()),
            Type::StaticRef(args) => Type::StaticRef(args.iter().map(&mut *f).collect()),
            Type::Callable(Some(sig)) => Type::Callable(Some(Arc::new(CallableSig {
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
            }))),
            Type::VarianceArg(variance, inner) => Type::VarianceArg(*variance, Box::new(f(inner))),
            Type::Union(ms) => Type::Union(ms.iter().map(&mut *f).collect()),
            Type::BenevolentUnion(ms) => Type::BenevolentUnion(ms.iter().map(&mut *f).collect()),
            Type::Intersection(ms) => Type::Intersection(ms.iter().map(&mut *f).collect()),
            Type::HasOffsetValue(key, inner) => {
                Type::HasOffsetValue(key.clone(), Box::new(f(inner)))
            }
            Type::ReflectedMember(owner, member) => {
                Type::ReflectedMember(Box::new(f(owner)), member.clone())
            }
            Type::ValueOf(inner) => Type::ValueOf(Box::new(f(inner))),
            Type::KeyOf(inner) => Type::KeyOf(Box::new(f(inner))),
            Type::OffsetAccess(base, key) => {
                Type::OffsetAccess(Box::new(f(base)), Box::new(f(key)))
            }
            Type::ClassConstant { class, pattern } => Type::ClassConstant {
                class: Box::new(f(class)),
                pattern: pattern.clone(),
            },
            Type::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
                template_subject,
            } => Type::Conditional {
                subject: subject.clone(),
                target: Box::new(f(target)),
                if_true: Box::new(f(if_true)),
                if_false: Box::new(f(if_false)),
                negated: *negated,
                template_subject: *template_subject,
            },
            leaf => leaf.clone(),
        }
    }

    /// Calls `f` on each directly nested type - the children
    /// `map_children` rebuilds, in the same order.
    pub(crate) fn for_each_child<'t>(&'t self, f: &mut impl FnMut(&'t Type)) {
        match self {
            Type::Array(k, v) => {
                f(k);
                f(v);
            }
            Type::ClassString(inner)
            | Type::VarianceArg(_, inner)
            | Type::HasOffsetValue(_, inner)
            | Type::ReflectedMember(inner, _)
            | Type::ValueOf(inner)
            | Type::KeyOf(inner)
            | Type::ClassConstant { class: inner, .. } => f(inner),
            Type::OffsetAccess(base, key) => {
                f(base);
                f(key);
            }
            Type::ArrayShape(shape) | Type::ObjectShape(shape) => shape.types().for_each(f),
            Type::Object(_, args) | Type::SelfRef(args) | Type::StaticRef(args) => {
                args.iter().for_each(f)
            }
            Type::Callable(Some(sig)) => {
                for p in &sig.params {
                    f(&p.ty);
                }
                f(&sig.return_type);
            }
            Type::Union(ms) | Type::BenevolentUnion(ms) | Type::Intersection(ms) => {
                ms.iter().for_each(f)
            }
            Type::Conditional {
                target,
                if_true,
                if_false,
                ..
            } => {
                f(target);
                f(if_true);
                f(if_false);
            }
            _ => {}
        }
    }

    /// Whether `f` holds for some directly nested type.
    pub(crate) fn any_child(&self, mut f: impl FnMut(&Type) -> bool) -> bool {
        let mut found = false;
        self.for_each_child(&mut |t| found = found || f(t));
        found
    }

    /// A union of `members` of the same kind as `self`: benevolent when
    /// `self` is a benevolent union, plain otherwise.
    pub(crate) fn with_members(&self, members: Vec<Type>) -> Type {
        match self {
            Type::BenevolentUnion(_) => Type::BenevolentUnion(members),
            _ => Type::Union(members),
        }
    }

    /// A closure/arrow-fn literal's own value: a real `Closure` instance
    /// (so `->bindTo()`/`->call()`/`instanceof Closure` keep resolving
    /// against the `Closure` class) that also carries the literal's own
    /// signature - real PHPStan's `ClosureType`, which is both at once.
    /// `Display` renders this exact pair as `Closure(params): return`.
    pub(crate) fn closure_literal(sig: CallableSig) -> Type {
        Type::Intersection(vec![
            Type::Object("Closure".into(), Vec::new()),
            Type::Callable(Some(Arc::new(sig))),
        ])
    }

    /// The signature a value of this type can be invoked with, when one
    /// is known: a shaped `Closure(...)`/`callable(...)` PHPDoc type, or a
    /// closure literal's `closure_literal` intersection.
    pub(crate) fn callable_sig(&self) -> Option<&Arc<CallableSig>> {
        match self {
            Type::Callable(Some(sig)) => Some(sig),
            Type::Intersection(members) => members.iter().find_map(|m| match m {
                Type::Callable(Some(sig)) => Some(sig),
                _ => None,
            }),
            _ => None,
        }
    }

    /// `list<element>`: an array whose keys are `0..n` in order.
    pub(crate) fn list_of(element: Type) -> Type {
        Type::Intersection(vec![
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(element)),
            Type::Accessory(AccessoryKind::ArrayList),
        ])
    }

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
        Type::BenevolentUnion(vec![Type::Int, Type::String])
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
}

#[cfg(test)]
mod tests;
