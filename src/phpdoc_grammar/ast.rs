use crate::token::Span;

/// The whole parsed doc comment - every tag this Stage A grammar
/// recognized, plus every recoverable parse failure encountered along
/// the way. Never fails outright: a malformed tag/type produces
/// `TypeNode::Error`/`PhpDocTagValue::Generic` plus a matching
/// `PhpDocParseError`, and parsing continues with the rest of the
/// comment - real PHPStan's own `phpDoc.parseError` is reported the same
/// way, one comment can carry several independent findings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhpDocNode {
    pub tags: Vec<PhpDocTag>,
    pub errors: Vec<PhpDocParseError>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhpDocParseError {
    pub message: String,
    pub span: Span,
}

/// One `@tag ...` line (already joined with any of its own wrapped
/// continuation lines - see `phpdoc_grammar::parser`'s own doc comment).
/// `name` is exactly as written, including a `phpstan-` prefix if
/// present - unlike `docblock::parse`'s own two-pass plain-then-
/// `phpstan-`-prefixed precedence resolution, that's a Stage-B/consumer
/// concern, not a parsing one.
#[derive(Debug, Clone, PartialEq)]
pub struct PhpDocTag {
    pub name: String,
    pub span: Span,
    pub value: PhpDocTagValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PhpDocTagValue {
    /// `@param`/`@param-out`/`@property`/`@property-read`/
    /// `@property-write`/`@var` - every tag shaped `[Type] [&][...]$name
    /// [description]`. `ty`/`variable` are independently optional - real
    /// PHPStan tolerates a bare `@var $x` (type omitted, inferred
    /// elsewhere) and a bare `@return Type` has no variable at all
    /// (handled by the separate `Type` variant below, not this one).
    TypedVar {
        ty: Option<TypeNode>,
        variable: Option<String>,
        by_ref: bool,
        variadic: bool,
        description: String,
    },
    /// `@return`/`@throws`/`@param-out`'s bodyless sibling shape - just a
    /// type, no variable.
    Type {
        ty: Option<TypeNode>,
        description: String,
    },
    /// `@template[-covariant|-contravariant] Name [of Bound] [= Default]`
    Template {
        name: String,
        bound: Option<TypeNode>,
        default: Option<TypeNode>,
    },
    /// `@extends`/`@implements`/`@use Name<Arg, ...>` - a single generic
    /// type reference, no variable/description slot at all.
    ExtendsLike { ty: TypeNode },
    /// `@method [static] ReturnType name(Type $param = default, ...)` -
    /// kept coarse: each parameter's own type is parsed for real (so a
    /// malformed one still reports), but default-value expressions are
    /// carried as raw text, not parsed as their own sub-grammar.
    Method {
        is_static: bool,
        return_type: Option<TypeNode>,
        name: String,
        params: Vec<MethodTagParam>,
    },
    /// Any tag this Stage A grammar doesn't specifically model
    /// (`@deprecated`, `@internal`, `@api`, `@phpstan-pure`, ...) - the
    /// remaining text after the tag name, entirely unparsed. Real PHPStan
    /// itself only defines a type-value grammar for the tags this file
    /// already covers; everything else is genuinely just free text.
    Generic(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodTagParam {
    pub ty: Option<TypeNode>,
    pub name: String,
    pub variadic: bool,
    pub by_ref: bool,
    pub has_default: bool,
}

/// The real type-expression grammar - Stage A's own headline addition
/// over the previous `take_type_token`-then-defer-to-`types::parse_
/// type_string`-later pipeline (`docblock.rs`'s own module doc comment):
/// a type is parsed into real structure right here, with the same
/// recoverable-error handling every other tag gets, rather than treated
/// as an opaque string until some later, separate pass touches it.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeNode {
    /// A bare name - `int`, `string`, `Foo\Bar`, `self`, `static`,
    /// `$this`, `mixed`, `class-string`, `non-empty-array`, ... - every
    /// scalar/class/pseudo-type keyword this grammar doesn't otherwise
    /// give its own variant to. Hyphenated compound keywords
    /// (`class-string`, `positive-int`, ...) lex as one identifier - see
    /// `phpdoc_grammar::lexer`'s own doc comment.
    Identifier(String),
    /// `?Type`
    Nullable(Box<TypeNode>),
    Union(Vec<TypeNode>),
    Intersection(Vec<TypeNode>),
    /// `Name<Arg, Arg, ...>` - covers ordinary generics
    /// (`Collection<int, User>`) and every generic-shaped built-in
    /// keyword (`class-string<Foo>`, `array<K, V>`, `list<T>`,
    /// `key-of<T>`, `value-of<T>`) uniformly; `Name` is whatever
    /// `Identifier` would have lexed.
    Generic(String, Vec<TypeNode>),
    /// `Type[]` - repeatable (`Type[][]`), tracked one level of nesting
    /// per node, same as real PHPStan's own `ArrayShapeType`-adjacent
    /// sugar.
    ArrayOf(Box<TypeNode>),
    /// `array{...}`/`list{...}`/`object{...}` - `keyword` is whichever of
    /// those three introduced it (also accepted, disclosed-lenient: any
    /// other bare identifier immediately followed by `{`, so a real
    /// corpus's occasional `SomeShape{...}`-style custom keyword still
    /// parses rather than erroring). `sealed` is `false` when the shape
    /// ends in `, ...}` (real PHPStan's own "open" shape marker,
    /// unsealed - more fields may exist at runtime).
    ArrayShape {
        keyword: String,
        fields: Vec<ArrayShapeField>,
        sealed: bool,
    },
    /// `callable(...)`/`Closure(...)` (`keyword` distinguishes the two,
    /// both parsed identically otherwise) `: ReturnType`.
    Callable {
        keyword: String,
        params: Vec<CallableParam>,
        return_type: Option<Box<TypeNode>>,
    },
    /// `int<0, 100>`/`int<min, 100>`/`int<0, max>` - `None` bound means
    /// the literal `min`/`max` keyword (unbounded on that side).
    IntRange(Option<i64>, Option<i64>),
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    /// `Foo::BAR`/`Foo::BAR_*` (a wildcard class-constant reference,
    /// real PHPStan's own literal-constant-type shape) - `const_name`
    /// keeps a trailing `*` verbatim if present.
    ConstFetch {
        class: String,
        const_name: String,
    },
    /// `($paramName is Type ? IfTrue : IfFalse)` - `negated` is `true`
    /// for the `is not` spelling. Real PHPStan's own conditional-type
    /// grammar also allows the subject to itself be a nested generic
    /// template reference (`T is Type`, not just `$param`) - not
    /// distinguished here, `subject` is just whatever raw token preceded
    /// `is`/`is not`, `$`-prefixed or not.
    Conditional {
        subject: String,
        negated: bool,
        target: Box<TypeNode>,
        if_true: Box<TypeNode>,
        if_false: Box<TypeNode>,
    },
    /// `(Type)` - a parenthesized group, kept as its own node (not
    /// silently unwrapped) only so a round-trip re-print could preserve
    /// the original grouping; every consumer that cares about the
    /// underlying type should see through it via `TypeNode::unwrap_
    /// paren`.
    Paren(Box<TypeNode>),
    /// A genuinely malformed sub-expression - parsing recovered and
    /// continued, but this node itself carries no real type information.
    /// Always paired with a `PhpDocParseError` at the same span.
    Error,
}

impl TypeNode {
    /// See `TypeNode::Paren`'s own doc comment.
    pub fn unwrap_paren(&self) -> &TypeNode {
        match self {
            TypeNode::Paren(inner) => inner.unwrap_paren(),
            other => other,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayShapeField {
    pub key: Option<ShapeKey>,
    pub ty: TypeNode,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShapeKey {
    Int(i64),
    String(String),
    Identifier(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallableParam {
    pub ty: TypeNode,
    pub variadic: bool,
    pub by_ref: bool,
    pub optional: bool,
}
