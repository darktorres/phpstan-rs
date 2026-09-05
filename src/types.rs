use std::collections::HashMap;
use std::fmt;

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
    Callable,
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

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Mixed => write!(f, "mixed"),
            Type::Never => write!(f, "never"),
            Type::Void => write!(f, "void"),
            Type::Null => write!(f, "null"),
            Type::Bool => write!(f, "bool"),
            Type::True => write!(f, "true"),
            Type::False => write!(f, "false"),
            Type::Int => write!(f, "int"),
            Type::IntLiteral(n) => write!(f, "{n}"),
            Type::Float => write!(f, "float"),
            Type::String => write!(f, "string"),
            Type::StringLiteral(s) => write!(f, "'{s}'"),
            Type::Array(key, value) => {
                let known_key = **key != Type::unknown_array_key();
                match (known_key, value.as_ref()) {
                    (false, Type::Mixed) => write!(f, "array"),
                    (false, v) => write!(f, "array<{v}>"),
                    (true, v) => write!(f, "array<{key}, {v}>"),
                }
            }
            Type::ClassString(inner) => write!(f, "class-string<{inner}>"),
            Type::ArrayShape(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(name, ty, optional)| {
                        let q = if *optional { "?" } else { "" };
                        format!("{name}{q}: {ty}")
                    })
                    .collect();
                write!(f, "array{{{}}}", parts.join(", "))
            }
            Type::ObjectShape(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(name, ty, optional)| {
                        let q = if *optional { "?" } else { "" };
                        format!("{name}{q}: {ty}")
                    })
                    .collect();
                write!(f, "object{{{}}}", parts.join(", "))
            }
            Type::Object(name, args) => {
                write!(f, "{name}")?;
                if !args.is_empty() {
                    let parts: Vec<String> = args.iter().map(Type::to_string).collect();
                    write!(f, "<{}>", parts.join(", "))?;
                }
                Ok(())
            }
            Type::EnumCase(fqn, case) => write!(f, "{fqn}::{case}"),
            Type::Callable => write!(f, "callable"),
            Type::SelfRef => write!(f, "self"),
            Type::ParentRef => write!(f, "parent"),
            Type::StaticRef => write!(f, "static"),
            Type::TemplateParam(tp) => write!(f, "{}", tp.name),
            Type::Union(members) => {
                let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                write!(f, "{}", parts.join("|"))
            }
            Type::Intersection(members) => {
                if let Some(name) = describe_accessory_intersection(members) {
                    return write!(f, "{name}");
                }
                let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                write!(f, "{}", parts.join("&"))
            }
            Type::IntRange(min, max) => write!(
                f,
                "int<{}, {}>",
                min.map_or("min".to_string(), |n| n.to_string()),
                max.map_or("max".to_string(), |n| n.to_string()),
            ),
            Type::FloatLiteral(n) => write!(f, "{n}"),
            Type::Accessory(kind) => write!(f, "{}", accessory_kind_name(*kind)),
        }
    }
}

fn accessory_kind_name(kind: AccessoryKind) -> &'static str {
    match kind {
        AccessoryKind::NonEmptyString => "non-empty-string",
        AccessoryKind::NonFalsyString => "non-falsy-string",
        AccessoryKind::LiteralString => "literal-string",
        AccessoryKind::NumericString => "numeric-string",
        AccessoryKind::ArrayList => "list",
        AccessoryKind::NonEmptyArray => "non-empty-array",
    }
}

/// Recognizes the canonical `[base, Accessory(kind), ...]` shapes
/// `parse_type_keyword_or_class` constructs (Phase 14) and renders them
/// as PHPStan's own atomic pseudo-type name, rather than the generic
/// `&`-joined form `Type::Intersection`'s own `Display` otherwise uses
/// (correct for a real user intersection like `Countable&Iterator`, but
/// wrong - `"string&non-empty-string"` - for an accessory-refined one).
/// Checks for the *presence* of each member independently, not an exact
/// positional slice pattern, so a semantically-identical intersection
/// built in a different member order still renders correctly. Returns
/// `None` for anything that doesn't match a known shape (most commonly
/// a real class intersection), leaving the generic `&`-joined fallback
/// in place.
fn describe_accessory_intersection(members: &[Type]) -> Option<String> {
    let has = |k: AccessoryKind| members.contains(&Type::Accessory(k));
    if let Some((key, elem)) = members.iter().find_map(|m| match m {
        Type::Array(k, e) => Some((k.as_ref(), e.as_ref())),
        _ => None,
    }) {
        let is_list = has(AccessoryKind::ArrayList);
        // Phase 73 corpus find: this used to discard the key type
        // entirely, always formatting via the plain, key-less `<V>`
        // form (or nothing, for `Mixed`) - harmless while every
        // accessory-marked array this project produced also had an
        // unknown key, but `array_flip()`'s own new key/value-swapping
        // return type can genuinely carry a known, non-default key
        // (e.g. `non-empty-array<string, int>`) that this was silently
        // rendering as `non-empty-array<int>`. Mirrors `Type::Array`'s
        // own `Display` arm just above: a known key always shows both
        // positions, even when the value happens to be `Mixed`. Never
        // shown for a `list`/`non-empty-list`, though, even though its
        // own key really is a "known" `Type::Int` under the same
        // `!= unknown_array_key()` test - a list's keys are *always*
        // sequential ints by definition, so real PHPStan's own
        // `list<V>` convention never states it either.
        let known_key = !is_list && *key != Type::unknown_array_key();
        let elem_str = if known_key {
            format!("<{key}, {elem}>")
        } else if *elem == Type::Mixed {
            String::new()
        } else {
            format!("<{elem}>")
        };
        return match (is_list, has(AccessoryKind::NonEmptyArray)) {
            (true, true) => Some(format!("non-empty-list{elem_str}")),
            (true, false) => Some(format!("list{elem_str}")),
            (false, true) => Some(format!("non-empty-array{elem_str}")),
            (false, false) => None,
        };
    }
    if members.contains(&Type::String) {
        return match (
            has(AccessoryKind::NonEmptyString),
            has(AccessoryKind::NonFalsyString),
            has(AccessoryKind::LiteralString),
            has(AccessoryKind::NumericString),
        ) {
            (_, true, false, false) => Some("non-falsy-string".to_string()),
            (true, false, false, false) => Some("non-empty-string".to_string()),
            (false, false, true, false) => Some("literal-string".to_string()),
            (false, false, false, true) => Some("numeric-string".to_string()),
            _ => None,
        };
    }
    None
}

/// Parses one of our parser's raw type-hint strings (`"?string"`,
/// `"int|string"`, `"\App\Foo"`, `"Countable&Iterator"`, or a PHPDoc type
/// like `"array<int, User>"` / `"array{name: string, age?: int}"`) into a
/// `Type`. Does not handle mixed union+intersection (DNF types) or
/// parentheses - PHPStan's own stub/native syntax rarely needs them outside
/// advanced generic bounds.
pub fn parse_type_string(raw: &str) -> Type {
    let mut s = raw.trim();
    let nullable = s.starts_with('?');
    if nullable {
        s = &s[1..];
    }

    // Splitting must respect <>/{} nesting - `Collection<int|string>` has
    // no top-level union, but a naive `s.contains('|')` would still split
    // on the one hiding inside the angle brackets.
    let pipe_parts = split_top_level(s, '|');
    let amp_parts = split_top_level(s, '&');
    let ty = if pipe_parts.len() > 1 {
        let members: Vec<Type> = pipe_parts.into_iter().map(parse_type_atom).collect();
        // Phase 32 Finding H: `Mixed` already includes every other
        // possible value, so a union with `Mixed` sitting alongside
        // anything else (`end()`'s real stub: `@return mixed|false`,
        // never updated after `end(): mixed` dropped its own native
        // `|false`) is simply `Mixed` - `union_types` (`checker/expr.rs`)
        // already special-cases this at *construction* time everywhere
        // it's used, but this is the lower-level parser building a
        // `Type::Union` directly from doc text, with no such
        // normalization. A stray non-`Mixed` member surviving otherwise
        // fails an ordinary argument-compatibility check on its own,
        // even though the whole union is vacuously satisfiable.
        if members.contains(&Type::Mixed) {
            return Type::Mixed;
        }
        Type::Union(members)
    } else if amp_parts.len() > 1 {
        Type::Intersection(amp_parts.into_iter().map(parse_type_atom).collect())
    } else {
        parse_type_atom(s)
    };

    if nullable {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    }
}

/// Splits `s` on `sep`, but only where `sep` appears outside any
/// `<>`/`{}`/`()`/`[]` nesting. Always returns at least one part, mirroring
/// `str::split` - callers check `.len() > 1` to know whether a real
/// top-level separator was found at all.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// PHPStan's own conditional return type syntax: `($param is SomeType ?
/// TrueType : FalseType)` - real PHPStan evaluates this per call site,
/// binding `$param`'s own actual argument; this project has no
/// per-call-site type-expression evaluation at all. Previously, the
/// DNF-group unwrap right above this function's own caller silently
/// mis-parsed the whole expression as an ordinary intersection group,
/// mangling it into nonsense (`$param`/`is`/etc. each getting treated as
/// class-name fragments, then namespace-qualified into garbage like
/// `Piwigo\Mail\$action`). Takes the safe, imprecise-but-correct answer
/// instead: the union of both branches, matching "can't evaluate, don't
/// guess which one" the same way this project's other unevaluable-
/// expression leniency already works elsewhere. Requires the literal
/// `is`/`?`/`:` tokens at the top level (`split_top_level`'s own
/// bracket-depth tracking) so this never misfires on an ordinary DNF
/// group, which has no ternary syntax at all.
fn try_parse_conditional_type(s: &str) -> Option<Type> {
    let q_parts = split_top_level(s, '?');
    let [cond, branches] = q_parts.as_slice() else {
        return None;
    };
    if !cond.contains(" is ") {
        return None;
    }
    let c_parts = split_top_level(branches, ':');
    let [true_branch, false_branch] = c_parts.as_slice() else {
        return None;
    };
    let true_ty = parse_type_string(true_branch);
    let false_ty = parse_type_string(false_branch);
    Some(if true_ty == false_ty {
        true_ty
    } else {
        Type::Union(vec![true_ty, false_ty])
    })
}

/// `array{name: string, age?: int}` - PHPDoc's structural array shape.
/// Field values can themselves be full type strings (unions, generics,
/// nested shapes), so each is re-run through `parse_type_string`.
fn try_parse_array_shape(s: &str) -> Option<Type> {
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with("array") {
        return None;
    }
    let rest = s[5..].trim_start();
    if !rest.starts_with('{') || !rest.ends_with('}') {
        return None;
    }
    let inner = &rest[1..rest.len() - 1];

    let mut fields = Vec::new();
    let mut next_index: i64 = 0;
    for field_str in split_top_level(inner, ',') {
        let field_str = field_str.trim();
        if field_str.is_empty() {
            continue;
        }
        // PHPStan/Psalm's "unsealed shape" marker (`...<string, mixed>`
        // or bare `...`) says the array may carry other, unlisted keys
        // too - a real, if rare, extra-precision feature this project
        // doesn't model (same as the accessory/refinement types Phase
        // 10 already skips). Found the hard way: `?` below used to
        // propagate this field's missing `:` into failing the ENTIRE
        // shape parse, silently falling through to treating the whole
        // `array{...}` text as a literal (and then namespace-mangled)
        // class name - degrading just this one field is the safe
        // choice, not aborting every other real field alongside it.
        if field_str.starts_with("...") {
            continue;
        }
        // Phase 33 Finding 3: a *positional* (unkeyed) field
        // (`array{int, string}`, real PHPStan/Psalm tuple syntax) has no
        // `key:` prefix at all - `split_once(':')` alone can't tell that
        // apart from a keyed field whose own type happens to contain a
        // colon (`Closure(int): string`), so `looks_like_shape_key` also
        // checks that whatever comes before the candidate colon is
        // actually shaped like a key before trusting the split. Numbers
        // a positional field the same way `infer_array_literal_type`
        // numbers a real array literal's own positional elements, so a
        // doc-declared shape and a real literal's inferred shape end up
        // structurally identical.
        let keyed = field_str
            .split_once(':')
            .filter(|(key_part, _)| looks_like_shape_key(key_part.trim()));
        match keyed {
            Some((key_part, type_part)) => {
                let key_part = key_part.trim();
                let (key, optional) = match key_part.strip_suffix('?') {
                    Some(k) => (k.trim(), true),
                    None => (key_part, false),
                };
                if let Ok(n) = key.parse::<i64>() {
                    next_index = next_index.max(n + 1);
                }
                fields.push((
                    key.to_string(),
                    parse_type_string(type_part.trim()),
                    optional,
                ));
            }
            None => {
                let key = next_index.to_string();
                next_index += 1;
                fields.push((key, parse_type_string(field_str), false));
            }
        }
    }
    Some(Type::ArrayShape(fields))
}

/// `object{name: string, age?: int}` - PHPDoc's structural object shape.
/// Mirrors `try_parse_array_shape` exactly (same field grammar, same
/// per-field `parse_type_string` recursion) - just builds `ObjectShape`
/// instead of `ArrayShape`, and has no positional-field/`...`-unsealed-
/// marker concept to carry over (an object's own properties are always
/// named, never positional).
fn try_parse_object_shape(s: &str) -> Option<Type> {
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with("object") {
        return None;
    }
    let rest = s[6..].trim_start();
    if !rest.starts_with('{') || !rest.ends_with('}') {
        return None;
    }
    let inner = &rest[1..rest.len() - 1];

    let mut fields = Vec::new();
    for field_str in split_top_level(inner, ',') {
        let field_str = field_str.trim();
        if field_str.is_empty() {
            continue;
        }
        let Some((key_part, type_part)) = field_str
            .split_once(':')
            .filter(|(key_part, _)| looks_like_shape_key(key_part.trim()))
        else {
            // Same "degrade just this one field" leniency `try_parse_
            // array_shape`'s own `...`-marker arm documents - an
            // unrecognized field shouldn't abort every other real field
            // alongside it.
            continue;
        };
        let key_part = key_part.trim();
        let (key, optional) = match key_part.strip_suffix('?') {
            Some(k) => (k.trim(), true),
            None => (key_part, false),
        };
        fields.push((
            key.to_string(),
            parse_type_string(type_part.trim()),
            optional,
        ));
    }
    Some(Type::ObjectShape(fields))
}

/// Whether `key_part` (the text before a candidate `:` inside an
/// `array{...}` field) actually looks like a shape key - a bare
/// identifier (hyphen included: `Translator::toDictionaryEntry`'s real
/// `@return array{..., plural-forms: string, ...}`, gettext's own
/// dictionary-key convention, is unquoted in real code and was already
/// accepted before this project ever validated key shape at all - a
/// real regression, caught by the full corpus re-check, not assumed
/// safe), an integer, or a quoted string, optionally `?`-suffixed - as
/// opposed to being part of a *positional* field's own type text that
/// merely happens to contain a colon (a callable-shape return type,
/// `Closure(int): string`). See `try_parse_array_shape`'s own call site.
/// `key-of<array{foo: string, 5: int}>`'s own per-field answer - a
/// shape's own field name is always stored as a plain `String` (see
/// `ArrayShape`'s own doc comment), the same way a real array literal's
/// keys are canonicalized, so a field name that parses as an integer
/// names an int key, matching PHP's own key-canonicalization rule.
fn shape_key_literal(name: &str) -> Type {
    match name.parse::<i64>() {
        Ok(n) => Type::IntLiteral(n),
        Err(_) => Type::StringLiteral(name.to_string()),
    }
}

fn looks_like_shape_key(key_part: &str) -> bool {
    let key_part = key_part.strip_suffix('?').unwrap_or(key_part).trim();
    if key_part.is_empty() {
        return false;
    }
    if key_part.parse::<i64>().is_ok() {
        return true;
    }
    if key_part.starts_with('\'') && key_part.ends_with('\'') && key_part.len() >= 2 {
        return true;
    }
    if key_part.starts_with('"') && key_part.ends_with('"') && key_part.len() >= 2 {
        return true;
    }
    let mut chars = key_part.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Splits a trailing `<...>` generic argument list off a type atom, e.g.
/// `"array<int, User>"` -> `("array", ["int", "User"])`. No arguments (or a
/// malformed/unclosed `<`) just returns the whole string with an empty list.
fn split_generic_args(s: &str) -> (&str, Vec<&str>) {
    let Some(open) = s.find('<') else {
        return (s, Vec::new());
    };
    if !s.ends_with('>') {
        return (s, Vec::new());
    }
    let base = &s[..open];
    let inner = &s[open + 1..s.len() - 1];
    let args = split_top_level(inner, ',')
        .into_iter()
        .map(str::trim)
        .collect();
    (base, args)
}

fn parse_type_atom(s: &str) -> Type {
    let s = s.trim();
    if let Some(shape) = try_parse_array_shape(s) {
        return shape;
    }
    if let Some(shape) = try_parse_object_shape(s) {
        return shape;
    }
    // PHP 8.2 DNF (disjunctive normal form) group: `(Countable&Iterator)`
    // as one term of a union like `(Countable&Iterator)|null`. Only one
    // level of parens is legal in real PHP here, so this doesn't need to
    // recurse through parse_type_string - split_top_level's own `(`/`)`
    // depth tracking already kept this whole parenthesized chunk together
    // as a single union member; just strip the parens and parse what's
    // inside as the intersection it has to be.
    if let Some(inner) = s.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if let Some(conditional) = try_parse_conditional_type(inner) {
            return conditional;
        }
        return parse_type_string(inner);
    }

    // PHPStan/Psalm's callable-shape syntax: `Closure(ParamType, ...):
    // ReturnType` or `callable(...): ReturnType` - describes a callable
    // value's own signature, not a generic type-argument list (only
    // `<...>` is one, per `split_generic_args`) and not a real class
    // name despite `Closure(...)` superficially resembling one. This
    // project doesn't track a callable's own per-parameter/return
    // signature at all, so - matching its own established leniency for a
    // bare `callable`/`Closure` type elsewhere (`is_assignable`'s
    // `(Type::Array(_) | Type::ArrayShape(_), Type::Callable) => true`
    // and friends) - this degrades to the same `Type::Callable` a bare
    // `callable` hint already produces. Real corpus shape: Doctrine
    // DBAL's own `@param Closure(self):T $func` - without this, the
    // whole literal string `"Closure(self):T"` fell through to the
    // generic class-name branch below and got mangled into a bogus,
    // namespace-qualified "class name" (`Doctrine\DBAL\Closure(self):T`).
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("closure(")
        || lower.starts_with("closure (")
        || lower.starts_with("callable(")
        || lower.starts_with("callable (")
    {
        return Type::Callable;
    }

    // A real, documented PHPStan/Psalm convention: a quoted literal
    // (`'ASC'|'DESC'`, `"foo"`) as one member of a union means "exactly
    // this string value," not a class reference - without this, real
    // code in this project's own corpus (`@param 'ASC'|'DESC' $dir`)
    // fell through to the generic-class branch below and got the quotes
    // themselves treated as part of a literal class name, then
    // nonsensically namespace-qualified into e.g. `Piwigo\Db\'ASC'`.
    if let Some(inner) = s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Type::StringLiteral(inner.to_string());
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Type::StringLiteral(inner.to_string());
    }

    // A real, documented PHPStan/Psalm convention, the same idea as the
    // quoted-string case just above: a bare integer literal as one
    // member of a union (`1|2|3`, a common way to constrain a value to a
    // fixed set of integers - log levels, status codes, mode flags, ...)
    // means "exactly this integer value," not a class reference -
    // without this it fell through to the generic-class branch below and
    // became a bogus `Type::Object("1", [])`, rejecting every value
    // (including genuinely valid ones) regardless of `is_subtype`'s own
    // otherwise-correct `IntLiteral`/`Union` handling.
    if let Ok(n) = s.parse::<i64>() {
        return Type::IntLiteral(n);
    }
    // Phase 14: mirrors the int-literal case just above, for a real
    // float literal (`1.5|2.5`) - checked after the int case so an
    // integer-looking string is still caught as `IntLiteral` first.
    if let Ok(f) = s.parse::<f64>() {
        return Type::FloatLiteral(f);
    }

    // PHPDoc's `Type[]` array shorthand (`int[]`, even `string[][]`) has no
    // native-hint equivalent, but reusing this parser for doc-comment types
    // makes it worth handling here rather than in a separate PHPDoc-only
    // parser.
    let mut s = s;
    let mut array_depth = 0;
    while let Some(stripped) = s.strip_suffix("[]") {
        s = stripped.trim();
        array_depth += 1;
    }

    // `(A|B)[]`/`(A&B)[]` - a parenthesized union/intersection wrapped in
    // the same array-shorthand syntax, needed whenever the element type
    // itself isn't a single bare name (`@param (string|int)[] $keys`,
    // a real, common PHPDoc idiom - PHPStan's own array-shorthand grammar
    // requires the parens here since `A|B[]` alone would ambiguously read
    // as `A|(B[])`). Recurses into `parse_type_string`, not `parse_type_
    // atom` - the inner content can have its own top-level union/
    // intersection that needs `split_top_level`'s own handling, which
    // this function never does itself (its caller, `parse_type_string`,
    // already owns that). Without this, the whole parenthesized text -
    // parens, pipe, and all - fell through to the generic class-name
    // branch below and got nonsensically namespace-qualified as if
    // `"(string|int)"` were a real class name.
    if array_depth > 0
        && let Some(inner) = s.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
    {
        let mut ty = parse_type_string(inner);
        for _ in 0..array_depth {
            ty = Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty));
        }
        return ty;
    }

    let (base, generic_args) = split_generic_args(s);
    let original = base.trim();
    let bare = original.trim_start_matches('\\');
    let mut ty = if bare.contains("::") {
        // A class-constant-reference type (`self::RFC_*`, `Foo::BAR`,
        // PHPStan's own wildcard-constant convention) isn't a class name
        // at all, even though it's shaped like one syntactically -
        // resolving it for real would mean looking up the named
        // constant('s value) on the referenced class, which this project
        // doesn't model. Left as `Mixed` (never flagged) rather than
        // falling through to the generic-class branch below, which used
        // to treat the whole string - including the "self"/class part -
        // as a literal class name, then (once namespace resolution
        // existed) nonsensically namespace-qualified it: real example,
        // monolog/monolog's own `@param self::RFC_* $rfc` became
        // `Monolog\Handler\self::RFC*`.
        Type::Mixed
    } else {
        parse_type_keyword_or_class(bare, original, &generic_args)
    };
    for _ in 0..array_depth {
        ty = Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty));
    }
    ty
}

/// `bare` (backslash-stripped) drives keyword matching; `original`
/// (backslash-preserved) is what the class-name fallback arm actually
/// constructs its `Type::Object` from - a leading `\` means "already
/// fully qualified from the global namespace," a real, meaningful
/// distinction `registry::NameContext::resolve` needs to see later (its
/// own first check is exactly `strip_prefix('\\')`) to know not to
/// re-qualify it against the current file's namespace. A real bug this
/// fixed: stripping the backslash *before* it ever reached `resolve`
/// made every absolute native type hint containing a `\` (e.g.
/// `QueryBuilder|\Doctrine\ORM\QueryBuilder`) look identical to a
/// relative one, silently mis-qualifying it as
/// `CurrentNamespace\Doctrine\ORM\QueryBuilder` - found via real code in
/// this project's own verification corpus.
/// Parses one bound of an `int<min, max>` range - real PHPStan's own
/// `"min"`/`"max"` keywords mean unbounded on that side (`Some(None)`),
/// a plain integer literal is a real bound (`Some(Some(n))`). A
/// malformed bound (neither) is `None` - distinct from "unbounded" so
/// the caller can fall back to plain `Type::Int` instead of silently
/// treating garbage as an open range.
fn parse_int_bound(s: &str) -> Option<Option<i64>> {
    match s {
        "min" | "max" => Some(None),
        s => s.parse().ok().map(Some),
    }
}

fn parse_type_keyword_or_class(bare: &str, original: &str, generic_args: &[&str]) -> Type {
    match bare.to_ascii_lowercase().as_str() {
        // Phase 14: `int<min, max>` and its named shorthands are real
        // ranges now, not aliased to plain `Int` - `IntRange` fully
        // replaces `Int`'s own range, the same relationship `IntLiteral`
        // has to `Int`. `int`/`integer` additionally read `generic_args`
        // when present (`int<0, 255>`, already structurally parsed as
        // generic args today) - a malformed bound falls back to plain
        // `Type::Int` rather than guessing.
        "int" | "integer" => match generic_args {
            [min, max] => match (parse_int_bound(min), parse_int_bound(max)) {
                (Some(min), Some(max)) => Type::IntRange(min, max),
                _ => Type::Int,
            },
            _ => Type::Int,
        },
        "positive-int" => Type::IntRange(Some(1), None),
        "negative-int" => Type::IntRange(None, Some(-1)),
        "non-negative-int" => Type::IntRange(Some(0), None),
        "non-positive-int" => Type::IntRange(None, Some(0)),
        // Real PHPStan represents this as a union of two ranges, not its
        // own type (confirmed via dumpType()) - composed the same way.
        "non-zero-int" => Type::Union(vec![
            Type::IntRange(None, Some(-1)),
            Type::IntRange(Some(1), None),
        ]),
        "float" | "double" => Type::Float,
        // Phase 14: the four accessory-refined string pseudo-types are
        // now real `Intersection([String, Accessory(_)])` types, mirroring
        // real PHPStan's own `IntersectionType([StringType,
        // AccessoryXType])` representation. `callable-string`/
        // `trait-string`/`interface-string`/`enum-string` stay aliased
        // to plain `string`: each names "a string believed to hold a
        // specific kind of identifier," with no literal-based or
        // narrowing-based way for this project to ever verify content -
        // `class-string`'s own optional `<Foo>` generic argument is
        // similarly just dropped rather than tracked.
        "non-empty-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonEmptyString),
        ]),
        "non-falsy-string" | "truthy-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonFalsyString),
        ]),
        "literal-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::LiteralString),
        ]),
        "numeric-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NumericString),
        ]),
        "string" | "callable-string" | "trait-string" | "interface-string" | "enum-string" => {
            Type::String
        }
        // Unlike its string-refinement neighbors above, `class-string<T>`
        // carries a real, template-bindable inner type when given one -
        // needed for `@template R of Base` / `@param class-string<R> $x`
        // shapes to bind `R` from a real `Foo::class` argument. A bare
        // `class-string` (no `<T>`, the common case) has nothing to
        // preserve and stays plain `String`, unchanged from before.
        "class-string" => match generic_args {
            [] => Type::String,
            [single] => Type::ClassString(Box::new(parse_type_string(single))),
            _ => Type::String,
        },
        "bool" | "boolean" => Type::Bool,
        "true" => Type::True,
        "false" => Type::False,
        // Real, documented PHPStan/Psalm pseudo-types - without these
        // they fell through to the generic-user-class branch below and
        // got treated as a literal (nonsensical) class reference, the
        // same failure mode this whole match arm already exists to avoid
        // for its other keywords. `numeric` is an honest approximation
        // of real PHPStan's own `int|float|numeric-string` - this
        // project doesn't model numeric-string as a distinct literal-set
        // from plain `string` anyway, so `string` is the closest fit.
        "array-key" => Type::Union(vec![Type::Int, Type::String]),
        "scalar" => Type::Union(vec![Type::Int, Type::Float, Type::String, Type::Bool]),
        "numeric" => Type::Union(vec![Type::Int, Type::Float, Type::String]),
        // A real, if untyped-at-the-language-level, PHP value kind
        // (file/curl/... handles) that still appears constantly in real
        // docblocks - represented the same way the native `object` hint
        // is (a sentinel `Object`, not a real class), and given the same
        // namespace-qualification exemption below for the same reason:
        // "resource" isn't a real class reference to resolve per-file.
        "resource" => Type::Object("resource".to_string(), Vec::new()),
        // Phase 80: `value-of<T>`/`key-of<T>` - real PHPStan's own
        // `ValueOfType`/`KeyOfType`, both `LateResolvableType`s evaluated
        // against `T`'s own resolved type only once actually needed.
        // This project has no lazy/deferred type-resolution mechanism at
        // all (a real, separate prerequisite this phase's own roadmap
        // entry names), so only the case answerable immediately, with no
        // registry access, is handled here: `T` itself already parsing
        // to a real `Array`/`ArrayShape` (no class/enum lookup needed at
        // all - real PHPStan's own `key-of<T>` never special-cases an
        // enum either, only `value-of<T>` does). `T` naming a class/enum
        // by reference (needing a registry lookup this function has no
        // access to) degrades to `Mixed` instead of guessing - before
        // this arm existed, `value-of<T>`/`key-of<T>` fell through to the
        // generic-user-class branch below and got mangled into a bogus,
        // namespace-qualified "class name" (`class.notFound` on every
        // real use) regardless of which case it was, so `Mixed` is
        // already a strict improvement even for the unhandled case.
        "value-of" => match generic_args {
            [single] => match parse_type_string(single) {
                Type::Array(_, value) => *value,
                Type::ArrayShape(fields) => match fields.len() {
                    0 => Type::Never,
                    1 => fields.into_iter().next().unwrap().1,
                    _ => Type::Union(fields.into_iter().map(|(_, ty, _)| ty).collect()),
                },
                _ => Type::Mixed,
            },
            _ => Type::Mixed,
        },
        "key-of" => match generic_args {
            [single] => match parse_type_string(single) {
                Type::Array(key, _) => *key,
                Type::ArrayShape(fields) => match fields.len() {
                    0 => Type::Never,
                    1 => shape_key_literal(&fields[0].0),
                    _ => Type::Union(
                        fields
                            .iter()
                            .map(|(name, _, _)| shape_key_literal(name))
                            .collect(),
                    ),
                },
                _ => Type::Mixed,
            },
            _ => Type::Mixed,
        },
        "null" => Type::Null,
        "void" => Type::Void,
        "mixed" => Type::Mixed,
        "never" | "noreturn" => Type::Never,
        // Phase 50: `array<K, V>`'s key argument is now real
        // (`Type::Array`'s own key slot), not discarded. `array<V>`
        // (1-arg) and bare `array` still mean "key unknown" -
        // `Type::unknown_array_key()`. `class-string-map<TKey, TValue>`
        // (Doctrine's own refinement of `array<class-string<TKey>,
        // TValue>`) wraps its own key in `ClassString` to match.
        "array" | "class-string-map" => match generic_args.len() {
            0 => Type::array_of(Type::Mixed),
            1 => Type::array_of(parse_type_string(generic_args[0])),
            _ => {
                let key_ty = parse_type_string(generic_args[0]);
                let key_ty = if bare == "class-string-map" {
                    Type::ClassString(Box::new(key_ty))
                } else {
                    key_ty
                };
                Type::Array(
                    Box::new(key_ty),
                    Box::new(parse_type_string(generic_args[1])),
                )
            }
        },
        // Phase 14: `list`/`non-empty-array`/`non-empty-list` are now
        // real `Intersection([Array(_), Accessory(_)])` types, mirroring
        // real PHPStan's own `IntersectionType([ArrayType,
        // AccessoryArrayListType/NonEmptyArrayType])` representation -
        // `elem_ty` computed the same "0/1/2-arg generic" way `array`
        // itself already is above.
        "list" | "non-empty-array" | "non-empty-list" => {
            let elem_ty = match generic_args.len() {
                0 => Type::Mixed,
                1 => parse_type_string(generic_args[0]),
                _ => parse_type_string(generic_args[1]),
            };
            // Phase 50: `list`/`non-empty-list` genuinely can only ever
            // have sequential int keys - a real precision win over the
            // default `int|string`, not just arity-matching.
            //
            // Phase 74 corpus find: `non-empty-array` makes no such
            // *implicit* promise (non-emptiness alone says nothing about
            // key type) - but an *explicit* 2-arg `non-empty-array<K, V>`
            // still names a real `K`, exactly like plain `array<K, V>`
            // does, and previously always got thrown away here in favor
            // of the unknown default regardless of what was actually
            // written - `key()`/`array_key_first()`/`array_key_last()`'s
            // own new precision (this same phase) immediately surfaced
            // it by silently losing a docblock's own `non-empty-array<
            // string, int>` key type.
            let key_ty = if bare == "non-empty-array" {
                match generic_args.len() {
                    2 => parse_type_string(generic_args[0]),
                    _ => Type::unknown_array_key(),
                }
            } else {
                Type::Int
            };
            let mut members = vec![Type::Array(Box::new(key_ty), Box::new(elem_ty))];
            if bare != "non-empty-array" {
                members.push(Type::Accessory(AccessoryKind::ArrayList));
            }
            if bare != "list" {
                members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
            }
            Type::Intersection(members)
        }
        // Unlike its neighbors above, `iterable` is real PHP's own native
        // type hint (not a PHPDoc-only pseudo-type), defined as exactly
        // `array|Traversable` - a broader type than `array`, not a
        // narrower refinement of it. Parsing it as bare `array` silently
        // dropped the entire `Traversable` half, rejecting any real
        // Generator/Iterator/IteratorAggregate passed to an
        // `iterable`-hinted parameter.
        "iterable" => {
            let element_ty = match generic_args.len() {
                0 => Type::Mixed,
                1 => parse_type_string(generic_args[0]),
                _ => parse_type_string(generic_args[1]),
            };
            Type::Union(vec![
                Type::Array(Box::new(Type::unknown_array_key()), Box::new(element_ty)),
                Type::Object("Traversable".to_string(), Vec::new()),
            ])
        }
        // Deliberately not conflated - a native `Closure $fn` and a
        // native `callable $fn` are different, if this project doesn't
        // reflect that: `Closure` is a concrete class only a real
        // closure object satisfies, strictly narrower than the four real
        // shapes `callable` itself accepts (see `is_assignable`'s own
        // `Type::Callable` arms). Representing `Closure` as a real
        // `Object` (it's a real, already-stubbed class - no new `Type`
        // variant needed) means `is_assignable`'s ordinary class-hierarchy
        // check already does the right thing for it, unaffected by
        // `callable`'s own broader acceptance rules below.
        "closure" => Type::Object("Closure".to_string(), Vec::new()),
        "callable" => Type::Callable,
        "self" => Type::SelfRef,
        "parent" => Type::ParentRef,
        // `@return $this` is a real, PHPStan-documented PHPDoc convention -
        // identical in meaning to `@return static` (the calling instance's
        // own type) - not a variable reference despite the `$` sigil.
        // Without this it fell through to the generic-user-class branch
        // and became a literal "class" literally named "$this", then got
        // nonsensically namespace-qualified once that existed (real
        // example: amphp/amp's `@return $this` on several fluent methods
        // became `Amp\$this`).
        "static" | "$this" => Type::StaticRef,
        "object" => Type::Object("object".to_string(), Vec::new()),
        // A user-defined generic class (`Collection<User>`): each type
        // argument is itself a full type string (could be a union, a
        // nested generic, ...), so it's re-run through `parse_type_string`
        // - the same recursive-field pattern `try_parse_array_shape`
        // already uses. A bare (non-generic) reference just gets an empty
        // arg list, indistinguishable from "erased"/unspecified generics
        // on a class that does declare `@template` - `TemplateParam`
        // substitution treats an unbound parameter as `Mixed` either way.
        _ => Type::Object(
            original.to_string(),
            generic_args.iter().map(|a| parse_type_string(a)).collect(),
        ),
    }
}

/// Is every value described by `sub` also described by `sup`? This is the
/// same relation PHPStan calls `isSuperTypeOf` (inverted) - the thing every
/// "does this assignment/return/argument type-check" question reduces to.
/// Class hierarchy isn't modeled yet, so `Object` subtyping is name-equality
/// only; self/parent/static are accepted against any `Object` as a
/// placeholder until a class-context resolver exists.
pub fn is_subtype(sub: &Type, sup: &Type) -> bool {
    match (sub, sup) {
        (_, Type::Mixed) => true,
        (Type::Never, _) => true,
        (a, b) if a == b => true,

        (Type::IntLiteral(_), Type::Int) => true,
        (Type::StringLiteral(_), Type::String) => true,
        (Type::True, Type::Bool) | (Type::False, Type::Bool) => true,
        // PHP's one documented exception to its own scalar type
        // declarations, even under strict_types=1: an int value is
        // always accepted wherever a float is declared (int-to-float
        // widening - explicit, load-bearing language behavior).
        (Type::Int | Type::IntLiteral(_), Type::Float) => true,
        (Type::FloatLiteral(_), Type::Float) => true,

        // Phase 14: `int<min, max>` ranges. `IntRange` fully replaces
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

        // Phase 14: string/array accessory kinds. A literal directly
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
        // Real containment between kinds: every non-falsy string is
        // non-empty; no other pair implies another.
        (
            Type::Accessory(AccessoryKind::NonFalsyString),
            Type::Accessory(AccessoryKind::NonEmptyString),
        ) => true,
        (Type::ArrayShape(fields), Type::Accessory(AccessoryKind::NonEmptyArray)) => {
            !fields.is_empty()
        }
        (Type::ArrayShape(fields), Type::Accessory(AccessoryKind::ArrayList)) => fields
            .iter()
            .enumerate()
            .all(|(i, (k, _, _))| k == &i.to_string()),

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
        (Type::Intersection(_), Type::Intersection(sup_members)) => {
            sup_members.iter().all(|m| is_subtype(sub, m))
        }
        (Type::Intersection(members), _) => members.iter().any(|m| is_subtype(m, sup)),
        (_, Type::Intersection(members)) => members.iter().all(|m| is_subtype(sub, m)),

        // Phase 50: a key-type check alongside the existing value-type
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
        (Type::ArrayShape(_), Type::Array(_, b)) => matches!(**b, Type::Mixed),

        // Phase 80: an object shape is structurally an object, so it
        // satisfies the bare native `object` hint - the same "shape
        // satisfies the ordinary bare hint" rule `ArrayShape` gets just
        // above, one type family over. Checking whether some *real*
        // class (`Type::Object(class, _)`) structurally has these exact
        // properties needs the registry - `registry::is_assignable`'s
        // own `ObjectShape` arm handles that hierarchy/property-aware
        // direction; this function has no class context to consult.
        (Type::ObjectShape(_), Type::Object(name, _)) if name == "object" => true,

        (Type::SelfRef | Type::ParentRef | Type::StaticRef, Type::Object(..)) => true,

        // Phase 79: a specific case is a real instance of its own enum -
        // the exact-name check here is the same non-hierarchy-aware
        // baseline `Object`-vs-`Object` gets just below; `registry::
        // is_assignable` is where the hierarchy-aware upgrade (matching
        // `UnitEnum`/`BackedEnum`/any interface the enum implements)
        // actually happens, mirroring how plain class inheritance already
        // works one layer up from this function.
        (Type::EnumCase(fqn, _), Type::Object(name, _)) => fqn == name,

        // An unbound template parameter is a placeholder, not a real type.
        // Call sites should have already substituted it away
        // (checker::infer_call_return_type); these are the safety net for
        // wherever that doesn't happen.
        //
        // Phase 54: used as `sub` (the actual/source side), it's now
        // bound-aware rather than an unconditional escape hatch - whatever
        // `T` ultimately gets instantiated to is guaranteed to satisfy its
        // own declared bound (real PHPStan's own upper-bound semantics),
        // so `T` is a subtype of `sup` exactly when its bound is. An
        // undeclared bound is `Mixed`, and `is_subtype(Mixed, sup)` is
        // *not* generally true (only when `sup` is `Mixed` too, via the
        // very first arm above) - so the extremely common bound-free
        // `@template T` gets an explicit, separate `true` here to keep
        // this arm's old blanket leniency exactly where no bound was ever
        // written to make a real judgment from.
        (Type::TemplateParam(tp), _) => {
            matches!(*tp.bound, Type::Mixed) || is_subtype(&tp.bound, sup)
        }
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
fn is_numeric_string_literal(s: &str) -> bool {
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

/// Unifies a template-bearing param type against an actual argument type,
/// recording the binding it implies. Only the two shapes that come up in
/// practice are handled: a bare `T` param, and `T[]`/`array<T>` unifying
/// against an array argument's element type. Anything else (a template
/// buried in a union, an array shape, etc) just isn't bound, which is safe
/// since `substitute_template` falls back to `Mixed` for anything left
/// unbound. First binding for a given name wins; conflicting uses of the
/// same `T` across multiple params aren't reconciled (no union-widening).
pub fn bind_template_param(param_ty: &Type, arg_ty: &Type, bindings: &mut HashMap<String, Type>) {
    match param_ty {
        Type::TemplateParam(tp) => {
            if *arg_ty != Type::Mixed {
                bindings
                    .entry(tp.name.clone())
                    .or_insert_with(|| arg_ty.clone());
            }
        }
        Type::Array(_, inner) => {
            if let Type::Array(_, arg_inner) = arg_ty {
                bind_template_param(inner, arg_inner, bindings);
            }
        }
        Type::ClassString(inner) => {
            if let Type::ClassString(arg_inner) = arg_ty {
                bind_template_param(inner, arg_inner, bindings);
            }
        }
        _ => {}
    }
}

/// Applies a set of template bindings through a type tree, replacing every
/// `TemplateParam` with its bound type. Phase 54: a name left unbound now
/// falls back to that template's own declared `@template T = Default`
/// when it has one, rather than unconditionally `Mixed` - e.g. a
/// function-own `@template T = int` whose `T` never actually gets bound
/// from any real call-site argument (used only in the return type, say)
/// now resolves to `int` instead of `Mixed`. `checker::generics::
/// substitute_scoped` mirrors this same fallback for the separate
/// class-level substitution path (an "erased" - too-few-arguments, or
/// bare - generic class reference).
pub fn substitute_template(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TemplateParam(tp) => bindings.get(&tp.name).cloned().unwrap_or_else(|| {
            tp.default
                .as_ref()
                .map(|d| substitute_template(d, bindings))
                .unwrap_or(Type::Mixed)
        }),
        // Phase 50: substitutes through the key type too (not just reset
        // to "unknown" every time) - a real, if rare, `array<TKey,
        // TValue>` bound key shouldn't be lost on every substitution
        // pass.
        Type::Array(key, inner) => Type::Array(
            Box::new(substitute_template(key, bindings)),
            Box::new(substitute_template(inner, bindings)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(substitute_template(inner, bindings)))
        }
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| substitute_template(m, bindings))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| substitute_template(m, bindings))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_template(t, bindings), *opt))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests;
