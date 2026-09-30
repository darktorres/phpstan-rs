//! Reading a type back from its display (`Type`'s `Display`): the grammar
//! type dumps and `assertType()` expectations are written in, which the
//! PHPDoc parser doesn't read (benevolent unions, `mixed~X`, accessory
//! parts, `*ERROR*`, double-quoted strings, `var_export` floats).

use std::sync::Arc;

use syntax::php_float::var_export_float;

use crate::{
    AccessoryKind, CallSiteVariance, CallableParam, CallableSig, ClassKind, ConditionalSubject,
    PhpString, Repr, Shape, ShapeField, ShapeKey, TemplateParam, Type,
};

/// The type `s` displays, or `None` when any part of it doesn't parse.
/// A bare name is a class.
pub fn try_parse_displayed(s: &str) -> Option<Type> {
    try_parse_displayed_with(s, &|_| None)
}

/// The type `s` displays, or `None` when any part of it doesn't parse.
/// `resolve` is asked for each bare name first (a template in scope),
/// and a name it doesn't know is a class.
pub fn try_parse_displayed_with(s: &str, resolve: &dyn Fn(&str) -> Option<Type>) -> Option<Type> {
    syntax::stack::guard(|| {
        let tokens = tokenize(s)?;
        let mut parser = Parser {
            tokens,
            at: 0,
            resolve,
            templates: Vec::new(),
        };
        let ty = parser.union()?;
        (parser.at == parser.tokens.len()).then_some(ty)
    })
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Punct(&'static str),
    Name(String),
    Variable(String),
    Str(PhpString),
    Int(i64),
    Float(f64),
}

const PUNCTS: [&str; 18] = [
    "...", "::", "|", "&", "<", ">", "{", "}", "(", ")", "[", "]", ",", ":", "?", "~", "*", "=",
];

fn tokenize(s: &str) -> Option<Vec<Token>> {
    let bytes = s.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'\'' || c == b'"' {
            let (text, end) = read_quoted(bytes, i)?;
            tokens.push(Token::Str(text));
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let (token, end) = read_number(s, i)?;
            tokens.push(token);
            i = end;
            continue;
        }
        if c == b'-' && s[i + 1..].starts_with("INF") && !is_name_byte(bytes.get(i + 4)) {
            tokens.push(Token::Float(f64::NEG_INFINITY));
            i += 4;
            continue;
        }
        if c == b'$' {
            let end = name_end(bytes, i + 1);
            if end == i + 1 {
                return None;
            }
            tokens.push(Token::Variable(s[i + 1..end].to_string()));
            i = end;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'\\' || c >= 0x80 {
            let mut end = name_end(bytes, i);
            // An anonymous class's name runs on through its file path and
            // line (`class@anonymous/src/a.php:12`).
            if s[i..end].eq_ignore_ascii_case("class") && bytes.get(end) == Some(&b'@') {
                end = anonymous_class_end(bytes, end);
            }
            tokens.push(Token::Name(s[i..end].to_string()));
            i = end;
            continue;
        }
        let punct = PUNCTS.iter().find(|p| s[i..].starts_with(**p))?;
        tokens.push(Token::Punct(punct));
        i += punct.len();
    }
    Some(tokens)
}

fn is_name_byte(b: Option<&u8>) -> bool {
    b.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'\\' || *b >= 0x80)
}

/// The end of a name starting at `start`: letters, digits, `_`, `\`, and
/// a `-` joining two name parts (`non-empty-string`, `key-of`).
fn name_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    loop {
        let joined =
            end > start && bytes.get(end) == Some(&b'-') && is_name_byte(bytes.get(end + 1));
        if !(is_name_byte(bytes.get(end)) || joined) {
            return end;
        }
        end += 1;
    }
}

fn anonymous_class_end(bytes: &[u8], mut end: usize) -> usize {
    while let Some(&b) = bytes.get(end) {
        let ends = match b {
            b':' => !bytes.get(end + 1).is_some_and(u8::is_ascii_digit),
            b' ' | b'|' | b'&' | b'<' | b'>' | b'{' | b'}' | b'(' | b')' | b',' | b'[' | b']' => {
                true
            }
            _ => false,
        };
        if ends {
            break;
        }
        end += 1;
    }
    end
}

/// A `'...'` or `"..."` string as `quote_php_string` writes it, and the
/// index past its closing quote.
fn read_quoted(bytes: &[u8], start: usize) -> Option<(PhpString, usize)> {
    let quote = bytes[start];
    let mut out = Vec::new();
    let mut i = start + 1;
    loop {
        let b = *bytes.get(i)?;
        if b == quote {
            return Some((PhpString::from_bytes(out.as_slice()), i + 1));
        }
        if b != b'\\' {
            out.push(b);
            i += 1;
            continue;
        }
        let escaped = *bytes.get(i + 1)?;
        i += 2;
        if quote == b'\'' {
            match escaped {
                b'\\' | b'\'' => out.push(escaped),
                _ => return None,
            }
            continue;
        }
        match escaped {
            b'\\' | b'"' | b'$' => out.push(escaped),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'x' => {
                let hex = std::str::from_utf8(bytes.get(i..i + 2)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            _ => return None,
        }
    }
}

/// An int, or a float as `var_export_float` writes one (`1.0`,
/// `1.0E+25`), and the index past it.
fn read_number(s: &str, start: usize) -> Option<(Token, usize)> {
    let bytes = s.as_bytes();
    let mut end = start;
    if bytes[end] == b'-' {
        end += 1;
    }
    let digits = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    end = digits(end);
    let mut float = false;
    if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
        float = true;
        end = digits(end + 1);
    }
    if matches!(bytes.get(end), Some(b'E' | b'e')) {
        let mut exp = end + 1;
        if matches!(bytes.get(exp), Some(b'+' | b'-')) {
            exp += 1;
        }
        if bytes.get(exp).is_some_and(u8::is_ascii_digit) {
            float = true;
            end = digits(exp);
        }
    }
    let text = &s[start..end];
    let token = if float {
        let value: f64 = text.parse().ok()?;
        // Only the spelling the display writes reads back.
        if var_export_float(value) != text {
            return None;
        }
        Token::Float(value)
    } else {
        Token::Int(text.parse().ok()?)
    };
    Some((token, end))
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    resolve: &'a dyn Fn(&str) -> Option<Type>,
    /// A callable signature's own templates, innermost last.
    templates: Vec<TemplateParam>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.at + offset)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    fn is(&self, punct: &str) -> bool {
        matches!(self.peek(), Some(Token::Punct(p)) if *p == punct)
    }

    fn eat(&mut self, punct: &str) -> bool {
        if self.is(punct) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, punct: &str) -> Option<()> {
        self.eat(punct).then_some(())
    }

    fn name(&mut self) -> Option<String> {
        match self.next()? {
            Token::Name(name) => Some(name),
            _ => None,
        }
    }

    fn is_name(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Token::Name(n)) if n == name)
    }

    /// `A|B|…`, or one member. The members are kept as displayed: the
    /// display shows a union as it was built.
    fn union(&mut self) -> Option<Type> {
        let mut members = vec![self.intersection()?];
        while self.eat("|") {
            members.push(self.intersection()?);
        }
        Some(if members.len() == 1 {
            members.pop()?
        } else {
            Type::new(Repr::Union(members))
        })
    }

    /// `A&B&…`, or one member, kept as displayed; a part that is itself
    /// refined (`non-empty-array` in `non-empty-array&hasOffset(0)`)
    /// contributes its members.
    fn intersection(&mut self) -> Option<Type> {
        let mut members = vec![self.postfix()?];
        while self.eat("&") {
            members.push(self.postfix()?);
        }
        if members.len() == 1 {
            return members.pop();
        }
        let mut flat = Vec::new();
        for member in members {
            match member.repr() {
                Repr::Intersection(inner) => flat.extend(inner.iter().cloned()),
                _ => flat.push(member),
            }
        }
        Some(Type::new(Repr::Intersection(flat)))
    }

    /// An atom with its offset accesses (`X['k']`).
    fn postfix(&mut self) -> Option<Type> {
        let mut ty = self.atom()?;
        while self.eat("[") {
            let key = self.union()?;
            self.expect("]")?;
            ty = Type::offset_access(ty, key);
        }
        Some(ty)
    }

    fn atom(&mut self) -> Option<Type> {
        match self.next()? {
            Token::Str(text) => Some(Type::string_literal(text)),
            Token::Int(n) => Some(Type::int_literal(n)),
            Token::Float(n) => Some(Type::float_literal(n)),
            Token::Variable(name) if name == "this" => Some(Type::this()),
            Token::Variable(_) => None,
            Token::Punct("(") => self.parenthesized(),
            Token::Punct("*") => {
                // `*ERROR*`; a bare `*` is only a generic argument.
                if self.is_name("ERROR") && self.peek_at(1) == Some(&Token::Punct("*")) {
                    self.at += 2;
                    Some(Type::error())
                } else {
                    None
                }
            }
            Token::Punct(_) => None,
            Token::Name(name) => self.named(name),
        }
    }

    /// `(A|B)`, a benevolent union, or a conditional
    /// (`($x is int ? A : B)`, `(T is int ? A : B)`).
    fn parenthesized(&mut self) -> Option<Type> {
        let subject = match (self.peek(), self.peek_at(1)) {
            (Some(Token::Variable(name)), Some(Token::Name(is))) if is == "is" => {
                Some(ConditionalSubject::Parameter(name.as_str().into()))
            }
            (Some(Token::Name(name)), Some(Token::Name(is))) if is == "is" => {
                Some(ConditionalSubject::Template(name.as_str().into()))
            }
            _ => None,
        };
        if let Some(subject) = subject {
            self.at += 2;
            let negated = self.is_name("not") && {
                self.at += 1;
                true
            };
            let target = self.union()?;
            self.expect("?")?;
            let if_true = self.union()?;
            self.expect(":")?;
            let if_false = self.union()?;
            self.expect(")")?;
            return Some(Type::conditional(
                subject, target, if_true, if_false, negated,
            ));
        }
        let mut members = vec![self.intersection()?];
        while self.eat("|") {
            members.push(self.intersection()?);
        }
        self.expect(")")?;
        (members.len() > 1).then(|| Type::new(Repr::BenevolentUnion(members)))
    }

    fn named(&mut self, name: String) -> Option<Type> {
        let has_args = self.is("<");
        let ty = match name.as_str() {
            "mixed" => return self.mixed(),
            "never" => Type::never(),
            "void" => Type::void(),
            "null" => Type::null(),
            "bool" => Type::bool(),
            "true" => Type::bool_literal(true),
            "false" => Type::bool_literal(false),
            "int" if has_args => return self.int_range(),
            "int" => Type::int(),
            "float" => Type::float(),
            "NAN" => Type::float_literal(f64::NAN),
            "INF" => Type::float_literal(f64::INFINITY),
            "string" => Type::string(),
            "array" | "non-empty-array" | "list" | "non-empty-list" => return self.array(&name),
            "object" if self.is("{") => {
                return Some(Type::object_shape(self.shape()?));
            }
            "object" => Type::object_without_class(),
            "resource" => Type::resource(false),
            "closed-resource" => Type::resource(true),
            "class-string" if has_args => {
                let args = self.generic_args()?;
                let [inner] = <[Type; 1]>::try_from(args).ok()?;
                Type::class_string(inner)
            }
            "class-string" => Type::class_kind_string(ClassKind::Class),
            "interface-string" => Type::class_kind_string(ClassKind::Interface),
            "trait-string" => Type::class_kind_string(ClassKind::Trait),
            "enum-string" => Type::class_kind_string(ClassKind::Enum),
            "key-of" | "value-of" => {
                let args = self.generic_args()?;
                let [inner] = <[Type; 1]>::try_from(args).ok()?;
                if name == "key-of" {
                    Type::key_of(inner)
                } else {
                    Type::value_of(inner)
                }
            }
            "callable" if self.is("(") || has_args => {
                return Some(Type::callable(Some(Arc::new(self.callable_sig()?))));
            }
            "callable" => Type::callable(None),
            "Closure" if self.is("(") || (has_args && self.closure_follows()) => {
                return Some(Type::closure_literal(self.callable_sig()?));
            }
            "self" => Type::self_ref(self.optional_generic_args()?),
            "static" => Type::static_ref(self.optional_generic_args()?),
            "parent" => Type::parent_ref(),
            "hasOffsetValue" => {
                self.expect("(")?;
                let key = self.shape_key_type()?;
                self.expect(",")?;
                let value = self.union()?;
                self.expect(")")?;
                Type::has_offset_value(key, value)
            }
            "hasOffset" => {
                self.expect("(")?;
                let key = self.shape_key_type()?;
                self.expect(")")?;
                Type::has_offset(key)
            }
            "hasMethod" | "hasProperty" => {
                self.expect("(")?;
                let member = self.name()?;
                self.expect(")")?;
                if name == "hasMethod" {
                    Type::has_method(member.as_str().into())
                } else {
                    Type::has_property(member.as_str().into())
                }
            }
            "reflectedMember" => {
                self.expect("(")?;
                let owner = self.union()?;
                self.expect(",")?;
                let Token::Str(member) = self.next()? else {
                    return None;
                };
                self.expect(")")?;
                Type::reflected_member(owner, member.as_str()?.into())
            }
            _ if name.ends_with("-string") => refined_string(&name)?,
            _ => return self.class_like(name),
        };
        Some(ty)
    }

    /// `mixed`, or `mixed~X` / `mixed~(A|B)` without `X`.
    fn mixed(&mut self) -> Option<Type> {
        if !self.eat("~") {
            return Some(Type::mixed());
        }
        let subtracted = if self.eat("(") {
            let inner = self.union()?;
            self.expect(")")?;
            inner
        } else {
            self.atom()?
        };
        Some(Type::new(Repr::Mixed(false, Some(subtracted))))
    }

    /// `int<min, 5>`.
    fn int_range(&mut self) -> Option<Type> {
        self.expect("<")?;
        let min = self.range_bound("min")?;
        self.expect(",")?;
        let max = self.range_bound("max")?;
        self.expect(">")?;
        Some(Type::int_range(min, max))
    }

    fn range_bound(&mut self, open: &str) -> Option<Option<i64>> {
        match self.next()? {
            Token::Int(n) => Some(Some(n)),
            Token::Name(name) if name == open => Some(None),
            _ => None,
        }
    }

    /// `array`, `array<V>`, `array<K, V>`, `array{…}`, and the `list` and
    /// `non-empty-` forms of each.
    fn array(&mut self, name: &str) -> Option<Type> {
        let list = name.ends_with("list");
        let non_empty = name.starts_with("non-empty");
        if !list && self.is("{") {
            let shape = Type::array_shape(self.shape()?);
            return Some(if non_empty {
                crate::intersect_types(vec![shape, Type::accessory(AccessoryKind::NonEmptyArray)])
            } else {
                shape
            });
        }
        let (key, value) = if self.is("<") {
            let mut args = self.generic_args()?;
            match args.len() {
                1 => (Type::unknown_array_key(), args.pop()?),
                2 if !list => {
                    let value = args.pop()?;
                    (args.pop()?, value)
                }
                _ => return None,
            }
        } else {
            (Type::unknown_array_key(), Type::mixed())
        };
        let mut members = vec![Type::array(key, value)];
        if list {
            members.push(Type::accessory(AccessoryKind::ArrayList));
        }
        if non_empty {
            members.push(Type::accessory(AccessoryKind::NonEmptyArray));
        }
        Some(if members.len() == 1 {
            members.pop()?
        } else {
            crate::intersect_types(members)
        })
    }

    /// `{key: T, key?: T, …}` with an optional `...` or `...<K, V>` tail.
    fn shape(&mut self) -> Option<Shape> {
        self.expect("{")?;
        let mut shape = Shape::sealed(Vec::new());
        if self.eat("}") {
            return Some(shape);
        }
        loop {
            if self.eat("...") {
                shape.unsealed = Some(Box::new(if self.is("<") {
                    let mut args = self.generic_args()?;
                    if args.len() != 2 {
                        return None;
                    }
                    let value = args.pop()?;
                    (args.pop()?, value)
                } else {
                    (Type::unknown_array_key(), Type::mixed())
                }));
                self.expect("}")?;
                return Some(shape);
            }
            let key = match self.next()? {
                Token::Name(name) => ShapeKey::from_name(&name),
                Token::Str(text) => ShapeKey::from_bytes(text.as_bytes()),
                Token::Int(n) => ShapeKey::Int(n),
                _ => return None,
            };
            let optional = self.eat("?");
            self.expect(":")?;
            let ty = self.union()?;
            shape.fields.push(ShapeField::new(key, ty, optional));
            if self.eat("}") {
                return Some(shape);
            }
            self.expect(",")?;
        }
    }

    /// An offset accessory's key, shown as its type (`0`, `'a'`).
    fn shape_key_type(&mut self) -> Option<ShapeKey> {
        match self.next()? {
            Token::Int(n) => Some(ShapeKey::Int(n)),
            Token::Str(text) => Some(ShapeKey::from_bytes(text.as_bytes())),
            _ => None,
        }
    }

    /// `<A, B>`, each possibly with a call-site variance.
    fn generic_args(&mut self) -> Option<Vec<Type>> {
        self.expect("<")?;
        let mut args = Vec::new();
        loop {
            args.push(self.generic_arg()?);
            if self.eat(">") {
                return Some(args);
            }
            self.expect(",")?;
        }
    }

    fn optional_generic_args(&mut self) -> Option<Vec<Type>> {
        if self.is("<") {
            self.generic_args()
        } else {
            Some(Vec::new())
        }
    }

    fn generic_arg(&mut self) -> Option<Type> {
        if self.is("*") && matches!(self.peek_at(1), Some(Token::Punct(",") | Token::Punct(">"))) {
            self.at += 1;
            return Some(Type::variance_arg(
                CallSiteVariance::Bivariant,
                Type::mixed(),
            ));
        }
        for (word, variance) in [
            ("covariant", CallSiteVariance::Covariant),
            ("contravariant", CallSiteVariance::Contravariant),
        ] {
            if self.is_name(word)
                && !matches!(self.peek_at(1), Some(Token::Punct(",") | Token::Punct(">")))
            {
                self.at += 1;
                return Some(Type::variance_arg(variance, self.union()?));
            }
        }
        self.union()
    }

    /// Whether the `<…>` at the cursor is a closure's templates, followed
    /// by its parameters.
    fn closure_follows(&self) -> bool {
        let mut depth = 0;
        for (i, token) in self.tokens[self.at..].iter().enumerate() {
            match token {
                Token::Punct("<") => depth += 1,
                Token::Punct(">") => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(self.at + i + 1) == Some(&Token::Punct("("));
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// `<T of X = D>(A, B=, C..., &D): R`, templates optional.
    fn callable_sig(&mut self) -> Option<CallableSig> {
        let mut templates = Vec::new();
        if self.eat("<") {
            loop {
                let name = self.name()?;
                let bound = if self.is_name("of") {
                    self.at += 1;
                    self.union()?
                } else {
                    Type::mixed()
                };
                let default = if self.eat("=") {
                    Some(Box::new(self.union()?))
                } else {
                    None
                };
                templates.push(TemplateParam {
                    name,
                    bound: Box::new(bound),
                    variance: crate::Variance::Invariant,
                    scope: Some(crate::CALLABLE_TEMPLATE_SCOPE.to_string()),
                    default,
                });
                if self.eat(">") {
                    break;
                }
                self.expect(",")?;
            }
        }
        let scope = self.templates.len();
        self.templates.extend(templates.iter().cloned());
        let sig = self.callable_params_and_return(templates);
        self.templates.truncate(scope);
        sig
    }

    fn callable_params_and_return(&mut self, templates: Vec<TemplateParam>) -> Option<CallableSig> {
        self.expect("(")?;
        let mut params = Vec::new();
        if !self.eat(")") {
            loop {
                let by_ref = self.eat("&");
                let ty = self.union()?;
                let variadic = self.eat("...");
                let optional = self.eat("=");
                params.push(CallableParam {
                    ty,
                    variadic,
                    optional,
                    by_ref,
                });
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        self.expect(":")?;
        let return_type = self.union()?;
        Some(CallableSig {
            params,
            return_type: Box::new(return_type),
            templates,
        })
    }

    /// A class, possibly generic; an enum case or class constant after
    /// `::`; or a template the resolver knows.
    fn class_like(&mut self, name: String) -> Option<Type> {
        if self.eat("::") {
            let mut pattern = String::new();
            loop {
                match self.peek() {
                    Some(Token::Name(part)) if pattern.is_empty() || pattern.ends_with('*') => {
                        pattern.push_str(part);
                        self.at += 1;
                    }
                    Some(Token::Punct("*")) => {
                        pattern.push('*');
                        self.at += 1;
                    }
                    _ => break,
                }
            }
            if pattern.is_empty() {
                return None;
            }
            let class = Type::object(name.as_str().into(), Vec::new());
            return Some(if pattern.contains('*') {
                Type::class_constant(class, pattern.as_str().into())
            } else {
                Type::enum_case(name.as_str().into(), pattern.as_str().into())
            });
        }
        if self.is("<") {
            let args = self.generic_args()?;
            return Some(Type::object(name.as_str().into(), args));
        }
        if let Some(template) = self.templates.iter().rev().find(|t| t.name == name) {
            return Some(Type::template_param(template.clone()));
        }
        if let Some(resolved) = (self.resolve)(&name) {
            return Some(resolved);
        }
        Some(Type::object(name.as_str().into(), Vec::new()))
    }
}

/// A string with refinements, as the display joins them
/// (`non-falsy-lowercase-numeric-string`).
fn refined_string(name: &str) -> Option<Type> {
    let mut rest = name.strip_suffix("-string")?;
    let mut members = vec![Type::string()];
    for (part, kind) in [
        ("non-falsy", AccessoryKind::NonFalsyString),
        ("non-empty", AccessoryKind::NonEmptyString),
        ("lowercase", AccessoryKind::LowercaseString),
        ("uppercase", AccessoryKind::UppercaseString),
        ("literal", AccessoryKind::LiteralString),
        ("decimal-int", AccessoryKind::DecimalIntegerString),
        ("non-decimal-int", AccessoryKind::NonDecimalIntegerString),
        ("numeric", AccessoryKind::NumericString),
    ] {
        if let Some(after) = rest.strip_prefix(part) {
            if !(after.is_empty() || after.starts_with('-')) {
                continue;
            }
            members.push(Type::accessory(kind));
            rest = after.strip_prefix('-').unwrap_or(after);
        }
    }
    if !rest.is_empty() {
        return None;
    }
    Some(crate::intersect_types(members))
}
