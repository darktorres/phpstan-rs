use syntax::span::Span;

/// The whole parsed doc comment - every tag this grammar
/// recognized, plus every recoverable parse failure encountered along
/// the way. Never fails outright: a malformed tag/type produces
/// `TypeNode::Error`/`PhpDocTagValue::Generic` plus a matching
/// `PhpDocParseError`, and parsing continues with the rest of the
/// comment - real PHPStan's own `phpDoc.parseError` is reported the same
/// way, one comment can carry several independent findings.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PhpDocNode {
    pub tags: Vec<PhpDocTag>,
    pub errors: Vec<PhpDocParseError>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhpDocParseError {
    pub message: String,
    pub span: Span,
}

/// One `@tag ...` line (already joined with any of its own wrapped
/// continuation lines - see `grammar::parser`'s own doc comment).
/// `name` is exactly as written, including a `phpstan-` prefix if
/// present - unlike `docblock::parse`'s own two-pass plain-then-
/// `phpstan-`-prefixed precedence resolution, that's a consumer
/// concern, not a parsing one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhpDocTag {
    pub name: String,
    pub span: Span,
    pub value: PhpDocTagValue,
    /// This tag's own parse errors (also in `PhpDocNode::errors`); a tag
    /// with any is invalid and contributes nothing.
    pub errors: Vec<PhpDocParseError>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
        description: Description,
    },
    /// `@return`/`@throws`/`@param-out`'s bodyless sibling shape - just a
    /// type, no variable.
    Type {
        ty: Option<TypeNode>,
        description: Description,
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
        /// `@method T name<T of Bound>(...)`'s own templates.
        templates: Vec<CallableTemplate>,
        params: Vec<MethodTagParam>,
    },
    /// Any tag this grammar doesn't specifically model
    /// (`@deprecated`, `@internal`, `@api`, `@phpstan-pure`, ...) - the
    /// remaining text after the tag name, entirely unparsed. Real PHPStan
    /// itself only defines a type-value grammar for the tags this file
    /// already covers; everything else is genuinely just free text.
    Generic(Description),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MethodTagParam {
    pub ty: Option<TypeNode>,
    pub name: String,
    pub variadic: bool,
    pub by_ref: bool,
    pub has_default: bool,
}

/// A generic argument's written call-site variance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ArgVariance {
    Covariant,
    Contravariant,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TypeNode {
    /// A bare name - `int`, `string`, `Foo\Bar`, `self`, `static`,
    /// `$this`, `mixed`, `class-string`, `non-empty-array`, ... - every
    /// scalar/class/pseudo-type keyword this grammar doesn't otherwise
    /// give its own variant to. Hyphenated compound keywords
    /// (`class-string`, `positive-int`, ...) lex as one identifier - see
    /// `grammar::lexer`'s own doc comment.
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
    /// `*` as a generic argument: any type, never checked.
    Wildcard,
    /// `covariant Type`/`contravariant Type` as a generic argument.
    VarianceArg(ArgVariance, Box<TypeNode>),
    /// `Type[]` - repeatable (`Type[][]`), tracked one level of nesting
    /// per node, same as real PHPStan's own `ArrayShapeType`-adjacent
    /// sugar.
    ArrayOf(Box<TypeNode>),
    /// `Base[Offset]` - phpdoc-parser's `OffsetAccessTypeNode`, written
    /// with no whitespace before the `[` (with whitespace, a `[` starts a
    /// description instead).
    OffsetAccess(Box<TypeNode>, Box<TypeNode>),
    /// `array{...}`/`list{...}`/`non-empty-array{...}`/
    /// `non-empty-list{...}`/`object{...}` - `keyword` is whichever of
    /// those introduced it. `sealed` is `false` when the shape ends in
    /// `, ...}` (more fields may exist at runtime), and `unsealed_type`
    /// carries that tail's optional `...<V>`/`...<K, V>` as `(key, value)`.
    ArrayShape {
        keyword: String,
        fields: Vec<ArrayShapeField>,
        sealed: bool,
        unsealed_type: Option<(Option<Box<TypeNode>>, Box<TypeNode>)>,
    },
    /// `callable(...)`/`Closure(...)` (`keyword` distinguishes the two,
    /// both parsed identically otherwise) `: ReturnType`.
    Callable {
        keyword: String,
        /// `callable<T of Bound = Default>(...)`'s own templates.
        templates: Vec<CallableTemplate>,
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

/// Drops the children inside the recursion guard, so a deeply nested type
/// drops on any stack: the derived drop glue recurses through every child
/// with no stack check.
impl Drop for TypeNode {
    fn drop(&mut self) {
        let mut children = Vec::new();
        self.detach_children(&mut children);
        if !children.is_empty() {
            stack::guard(|| drop(children));
        }
    }
}

impl TypeNode {
    /// Moves every direct child out of `self`, leaving `Error` in its place,
    /// so that `self` drops as a leaf.
    fn detach_children(&mut self, out: &mut Vec<TypeNode>) {
        match self {
            TypeNode::Nullable(inner)
            | TypeNode::VarianceArg(_, inner)
            | TypeNode::ArrayOf(inner)
            | TypeNode::Paren(inner) => detach_boxed(inner, out),
            TypeNode::Union(items)
            | TypeNode::Intersection(items)
            | TypeNode::Generic(_, items) => out.append(items),
            TypeNode::OffsetAccess(base, offset) => {
                detach_boxed(base, out);
                detach_boxed(offset, out);
            }
            TypeNode::ArrayShape {
                fields,
                unsealed_type,
                ..
            } => {
                out.extend(
                    fields
                        .iter_mut()
                        .map(|field| std::mem::replace(&mut field.ty, TypeNode::Error)),
                );
                if let Some((key, value)) = unsealed_type.take() {
                    out.extend(key.map(|key| *key));
                    out.push(*value);
                }
            }
            TypeNode::Callable {
                templates,
                params,
                return_type,
                ..
            } => {
                for template in templates.iter_mut() {
                    out.extend(
                        [
                            template.bound.take(),
                            template.lower.take(),
                            template.default.take(),
                        ]
                        .into_iter()
                        .flatten(),
                    );
                }
                out.extend(
                    params
                        .iter_mut()
                        .map(|param| std::mem::replace(&mut param.ty, TypeNode::Error)),
                );
                if let Some(return_type) = return_type.take() {
                    out.push(*return_type);
                }
            }
            TypeNode::Conditional {
                target,
                if_true,
                if_false,
                ..
            } => {
                detach_boxed(target, out);
                detach_boxed(if_true, out);
                detach_boxed(if_false, out);
            }
            TypeNode::Identifier(_)
            | TypeNode::Wildcard
            | TypeNode::IntRange(..)
            | TypeNode::IntLiteral(_)
            | TypeNode::FloatLiteral(_)
            | TypeNode::StringLiteral(_)
            | TypeNode::ConstFetch { .. }
            | TypeNode::Error => {}
        }
    }
}

fn detach_boxed(child: &mut Box<TypeNode>, out: &mut Vec<TypeNode>) {
    out.push(std::mem::replace(&mut **child, TypeNode::Error));
}

impl TypeNode {
    /// See `TypeNode::Paren`'s own doc comment.
    pub fn unwrap_paren(&self) -> &TypeNode {
        match self {
            TypeNode::Paren(inner) => inner.unwrap_paren(),
            other => other,
        }
    }

    /// Calls `f` with each name in the type that can be a class's, as it
    /// is written: the names of the types it holds, a generic's own, a
    /// class constant's class and the class a `Closure(...)` is, not the
    /// words a written type is read from (`int`, `array`, `self`, ...) nor
    /// the subject of a conditional.
    pub fn for_each_class_name(&self, f: &mut dyn FnMut(&str)) {
        fn name(text: &str, f: &mut dyn FnMut(&str)) {
            let text = text.trim_end_matches('\\');
            if !text.trim_start_matches('\\').is_empty() && !types::is_reserved_type_word(text) {
                f(text);
            }
        }
        match self {
            TypeNode::Identifier(text) => name(text, f),
            TypeNode::Nullable(inner)
            | TypeNode::VarianceArg(_, inner)
            | TypeNode::ArrayOf(inner)
            | TypeNode::Paren(inner) => inner.for_each_class_name(f),
            TypeNode::Union(members) | TypeNode::Intersection(members) => {
                for member in members {
                    member.for_each_class_name(f);
                }
            }
            TypeNode::Generic(head, args) => {
                name(head, f);
                for arg in args {
                    arg.for_each_class_name(f);
                }
            }
            TypeNode::OffsetAccess(base, offset) => {
                base.for_each_class_name(f);
                offset.for_each_class_name(f);
            }
            TypeNode::ArrayShape {
                keyword,
                fields,
                unsealed_type,
                ..
            } => {
                name(keyword, f);
                for field in fields {
                    field.ty.for_each_class_name(f);
                }
                if let Some((key, value)) = unsealed_type {
                    if let Some(key) = key {
                        key.for_each_class_name(f);
                    }
                    value.for_each_class_name(f);
                }
            }
            TypeNode::Callable {
                keyword,
                templates,
                params,
                return_type,
            } => {
                name(keyword, f);
                for template in templates {
                    for bound in [&template.bound, &template.lower, &template.default]
                        .into_iter()
                        .flatten()
                    {
                        bound.for_each_class_name(f);
                    }
                }
                for param in params {
                    param.ty.for_each_class_name(f);
                }
                if let Some(return_type) = return_type {
                    return_type.for_each_class_name(f);
                }
            }
            TypeNode::ConstFetch { class, .. } => name(class, f),
            TypeNode::Conditional {
                target,
                if_true,
                if_false,
                ..
            } => {
                target.for_each_class_name(f);
                if_true.for_each_class_name(f);
                if_false.for_each_class_name(f);
            }
            TypeNode::Wildcard
            | TypeNode::IntRange(..)
            | TypeNode::IntLiteral(_)
            | TypeNode::FloatLiteral(_)
            | TypeNode::StringLiteral(_)
            | TypeNode::Error => {}
        }
    }

    /// Re-stringifies a parsed node back into PHPStan type-expression
    /// syntax - not necessarily byte-identical to whatever text originally
    /// produced it (whitespace is normalized, a string literal's own
    /// internal quotes aren't re-escaped), but always something `types::
    /// parse_type_string` parses into the same underlying `Type`.
    pub fn to_source_string(&self) -> String {
        match self {
            TypeNode::Identifier(s) => s.clone(),
            TypeNode::Nullable(inner) => format!("?{}", inner.to_source_string()),
            TypeNode::Union(members) => members
                .iter()
                .map(TypeNode::to_source_string)
                .collect::<Vec<_>>()
                .join("|"),
            TypeNode::Intersection(members) => members
                .iter()
                .map(TypeNode::to_source_string)
                .collect::<Vec<_>>()
                .join("&"),
            TypeNode::Wildcard => "*".to_string(),
            TypeNode::VarianceArg(ArgVariance::Covariant, inner) => {
                format!("covariant {}", inner.to_source_string())
            }
            TypeNode::VarianceArg(ArgVariance::Contravariant, inner) => {
                format!("contravariant {}", inner.to_source_string())
            }
            TypeNode::Generic(name, args) => format!(
                "{name}<{}>",
                args.iter()
                    .map(TypeNode::to_source_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            TypeNode::ArrayOf(inner) => format!("{}[]", inner.to_source_string()),
            TypeNode::OffsetAccess(base, offset) => {
                format!("{}[{}]", base.to_source_string(), offset.to_source_string())
            }
            TypeNode::ArrayShape {
                keyword,
                fields,
                sealed,
                unsealed_type,
            } => {
                let mut parts: Vec<String> = fields
                    .iter()
                    .map(|f| {
                        let value = f.ty.to_source_string();
                        match &f.key {
                            Some(key) => {
                                let key = match key {
                                    ShapeKey::Int(n) => n.to_string(),
                                    ShapeKey::String(s) => format!("'{s}'"),
                                    ShapeKey::Identifier(s) => s.clone(),
                                };
                                let opt = if f.optional { "?" } else { "" };
                                format!("{key}{opt}: {value}")
                            }
                            None => value,
                        }
                    })
                    .collect();
                if !*sealed {
                    parts.push(match unsealed_type {
                        Some((Some(key), value)) => format!(
                            "...<{}, {}>",
                            key.to_source_string(),
                            value.to_source_string()
                        ),
                        Some((None, value)) => format!("...<{}>", value.to_source_string()),
                        None => "...".to_string(),
                    });
                }
                format!("{keyword}{{{}}}", parts.join(", "))
            }
            TypeNode::Callable {
                keyword,
                templates,
                params,
                return_type,
            } => {
                let keyword = if templates.is_empty() {
                    keyword.clone()
                } else {
                    let list: Vec<String> = templates
                        .iter()
                        .map(|t| {
                            let mut s = t.name.clone();
                            if let Some(b) = &t.bound {
                                s.push_str(&format!(" of {}", b.to_source_string()));
                            }
                            if let Some(l) = &t.lower {
                                s.push_str(&format!(" super {}", l.to_source_string()));
                            }
                            if let Some(d) = &t.default {
                                s.push_str(&format!(" = {}", d.to_source_string()));
                            }
                            s
                        })
                        .collect();
                    format!("{keyword}<{}>", list.join(", "))
                };
                let params_str = params
                    .iter()
                    .map(|p| {
                        let mut s = p.ty.to_source_string();
                        if p.by_ref {
                            s.push('&');
                        }
                        if p.variadic {
                            s.push_str("...");
                        }
                        if p.optional {
                            s.push('=');
                        }
                        s
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                match return_type {
                    Some(rt) => format!("{keyword}({params_str}): {}", rt.to_source_string()),
                    None => format!("{keyword}({params_str})"),
                }
            }
            TypeNode::IntRange(min, max) => {
                let min = min
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "min".to_string());
                let max = max
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "max".to_string());
                format!("int<{min}, {max}>")
            }
            TypeNode::IntLiteral(n) => n.to_string(),
            TypeNode::FloatLiteral(n) => format!("{n:?}"),
            TypeNode::StringLiteral(s) => format!("'{s}'"),
            TypeNode::ConstFetch { class, const_name } => format!("{class}::{const_name}"),
            TypeNode::Conditional {
                subject,
                negated,
                target,
                if_true,
                if_false,
            } => {
                let is = if *negated { "is not" } else { "is" };
                format!(
                    "({subject} {is} {} ? {} : {})",
                    target.to_source_string(),
                    if_true.to_source_string(),
                    if_false.to_source_string()
                )
            }
            TypeNode::Paren(inner) => format!("({})", inner.to_source_string()),
            TypeNode::Error => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ArrayShapeField {
    pub key: Option<ShapeKey>,
    pub ty: TypeNode,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ShapeKey {
    Int(i64),
    String(String),
    Identifier(String),
}

/// One entry of a callable type's template list.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CallableTemplate {
    pub name: String,
    pub bound: Option<TypeNode>,
    pub lower: Option<TypeNode>,
    pub default: Option<TypeNode>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CallableParam {
    pub ty: TypeNode,
    pub variadic: bool,
    pub by_ref: bool,
    pub optional: bool,
}

/// The free text after a tag's type and variable. It keeps the source
/// from its first token on and makes the words when asked: no reader of
/// most tags looks at it, and lexing it is most of what the tag lexer did.
///
/// The words are the tokens' own text joined by single spaces, not the
/// source: a `//` comment is dropped, a string literal loses its quotes
/// and escapes, a number is written the way the lexer read it. So
/// `text` lexes the source again.
#[derive(Debug, Clone, Default)]
pub struct Description {
    /// The tag's text from the description's first token; `None` when the
    /// tag has no description.
    source: Option<std::sync::Arc<str>>,
}

impl Description {
    /// The description that starts at `source`'s first byte.
    pub(super) fn from_source(source: &str) -> Description {
        Description {
            source: (!source.is_empty()).then(|| source.into()),
        }
    }

    /// The tokens' text joined by single spaces.
    pub fn text(&self) -> String {
        let mut words = Vec::new();
        let mut lexer = super::lexer::Lexer::new(self.source());
        loop {
            let (token, _) = lexer.next_token();
            if matches!(token, super::lexer::PhpDocToken::Eof) {
                break;
            }
            words.push(super::parser::token_text(&token));
        }
        words.join(" ")
    }

    /// Whether `text()` with its spaces removed starts with `[optional]`,
    /// answered from the first tokens only.
    pub fn starts_with_optional_marker(&self) -> bool {
        const MARKER: &[u8] = b"[optional]";
        let mut lexer = super::lexer::Lexer::new(self.source());
        let mut seen = 0;
        loop {
            let (token, _) = lexer.next_token();
            if matches!(token, super::lexer::PhpDocToken::Eof) {
                return false;
            }
            for c in super::parser::token_text(&token)
                .chars()
                .filter(|c| *c != ' ')
            {
                if c != char::from(MARKER[seen]) {
                    return false;
                }
                seen += 1;
                if seen == MARKER.len() {
                    return true;
                }
            }
        }
    }

    fn source(&self) -> &str {
        self.source.as_deref().unwrap_or("")
    }
}

impl PartialEq for Description {
    fn eq(&self, other: &Description) -> bool {
        self.source() == other.source()
    }
}

impl From<&str> for Description {
    /// The description a tag with this text after its type and variable has.
    fn from(text: &str) -> Description {
        Description::from_source(text)
    }
}

impl serde::Serialize for Description {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.source())
    }
}

impl<'de> serde::Deserialize<'de> for Description {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Description, D::Error> {
        let source = String::deserialize(deserializer)?;
        Ok(Description::from_source(&source))
    }
}
