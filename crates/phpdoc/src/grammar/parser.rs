use super::ast::{
    ArgVariance, ArrayShapeField, CallableParam, CallableTemplate, MethodTagParam, PhpDocNode,
    PhpDocParseError, PhpDocTag, PhpDocTagValue, ShapeKey, TypeNode,
};
use super::lexer::{Lexer, PhpDocToken};
use syntax::token::{Position, Span};

/// The whole doc-comment-parsing entry point - preprocesses `raw` into
/// one logical text span per `@tag` (joining wrapped continuation
/// lines, exactly matching `docblock::parse`'s own existing "every line
/// joins onto the most recent `@tag` line" convention, so a later Stage
/// B migration doesn't have to reconcile two different joining rules),
/// then parses each tag's own text independently. A malformed tag never
/// aborts the others - `errors` accumulates across the whole comment.
pub(crate) fn parse(raw: &str) -> PhpDocNode {
    let mut node = PhpDocNode::default();
    let inner = raw.trim().trim_start_matches("/**").trim_end_matches("*/");
    let mut tag_lines: Vec<String> = Vec::new();
    for raw_line in inner.lines() {
        let line = raw_line.trim();
        let line = line.strip_prefix('*').map(str::trim).unwrap_or(line);
        let line = line
            .split_once("//")
            .map_or(line, |(before, _)| before.trim());
        if line.starts_with('@') || tag_lines.is_empty() {
            tag_lines.push(line.to_string());
        } else if let Some(last) = tag_lines.last_mut() {
            last.push(' ');
            last.push_str(line);
        }
    }
    for line in &tag_lines {
        let Some(tag_body) = line.strip_prefix('@') else {
            continue;
        };
        let mut parts = tag_body.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        let rest = parts.next().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        let tokens = Lexer::new(rest).tokenize();
        let span = tokens.first().map(|(_, s)| *s).unwrap_or(Span {
            start: Position { line: 1, col: 1 },
            end: Position { line: 1, col: 1 },
        });
        let mut parser = TagParser::new(tokens);
        let value = parser.parse_tag_value(&name);
        let errors = std::mem::take(&mut parser.errors);
        node.errors.extend(errors.iter().cloned());
        node.tags.push(PhpDocTag {
            name,
            span,
            value,
            errors,
        });
    }
    node
}

struct TagParser {
    tokens: Vec<(PhpDocToken, Span)>,
    pos: usize,
    errors: Vec<PhpDocParseError>,
}

/// Bare-word tag names carrying a `[Type] [&][...]$name [description]`
/// shape - `TypeNode::TypedVar`'s own consumers. Matched case-
/// insensitively against the tag's own un-prefixed name (a leading
/// `phpstan-` is stripped by the caller's own dispatch below, mirroring
/// `docblock.rs`'s identical `@phpstan-*`-is-a-precise-override
/// convention).
const TYPED_VAR_TAGS: &[&str] = &[
    "param",
    "param-out",
    "var",
    "property",
    "property-read",
    "property-write",
];
const TYPE_ONLY_TAGS: &[&str] = &["return", "throws"];

impl TagParser {
    fn new(tokens: Vec<(PhpDocToken, Span)>) -> Self {
        TagParser {
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    fn peek(&self) -> &PhpDocToken {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].0
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos.min(self.tokens.len() - 1)].1
    }

    fn advance(&mut self) -> (PhpDocToken, Span) {
        let t = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), PhpDocToken::Eof)
    }

    /// `( [arg (, arg)*] )`, a trailing comma allowed - phpdoc-parser's
    /// `parseDoctrineArguments`. Stops at the first error.
    fn parse_doctrine_arguments(&mut self) {
        if !self.eat(&PhpDocToken::LParen) {
            return;
        }
        loop {
            if matches!(self.peek(), PhpDocToken::RParen) {
                break;
            }
            if !self.parse_doctrine_argument() {
                return;
            }
            if !self.eat(&PhpDocToken::Comma) {
                break;
            }
        }
        if !self.eat(&PhpDocToken::RParen) {
            self.error_unexpected_token("')'");
        }
    }

    /// `name = value` or a bare `value` - `parseDoctrineArgument`, whose
    /// named form is tried first and rolled back when no `=` follows the
    /// identifier. `false` once an error has been recorded.
    fn parse_doctrine_argument(&mut self) -> bool {
        if matches!(self.peek(), PhpDocToken::Identifier(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|(t, _)| t),
                Some(PhpDocToken::Equals)
            )
        {
            self.advance();
            self.advance();
        }
        self.parse_doctrine_value()
    }

    /// `parseDoctrineArgumentValue`: a nested `@Annotation(...)`, a
    /// `{...}` array, an identifier, or a constant expression (a number, a
    /// string, a `Foo::BAR` constant fetch). A `[...]` constant array is
    /// rejected there (`expected TOKEN_IDENTIFIER`), and so is anything
    /// else.
    fn parse_doctrine_value(&mut self) -> bool {
        let errors_before = self.errors.len();
        match self.peek().clone() {
            PhpDocToken::At => {
                self.advance();
                if matches!(self.peek(), PhpDocToken::Identifier(_)) {
                    self.advance();
                    if matches!(self.peek(), PhpDocToken::LParen) {
                        self.parse_doctrine_arguments();
                    }
                } else {
                    self.error_unexpected_token("TOKEN_IDENTIFIER");
                }
            }
            PhpDocToken::LBrace => {
                self.advance();
                loop {
                    if matches!(self.peek(), PhpDocToken::RBrace) {
                        break;
                    }
                    if !self.parse_doctrine_array_item() {
                        return false;
                    }
                    if !self.eat(&PhpDocToken::Comma) {
                        break;
                    }
                }
                if !self.eat(&PhpDocToken::RBrace) {
                    self.error_unexpected_token("'}'");
                }
            }
            PhpDocToken::Identifier(_) => {
                self.advance();
                if self.eat(&PhpDocToken::ColonColon) {
                    if matches!(self.peek(), PhpDocToken::Identifier(_) | PhpDocToken::Star) {
                        self.advance();
                    } else {
                        self.error_unexpected_token("TOKEN_IDENTIFIER");
                    }
                }
            }
            PhpDocToken::IntLiteral(_)
            | PhpDocToken::FloatLiteral(_)
            | PhpDocToken::StringLiteral(_) => {
                self.advance();
            }
            _ => self.error_unexpected_token("TOKEN_IDENTIFIER"),
        }
        self.errors.len() == errors_before
    }

    /// `parseDoctrineArrayItem`: `key = value` / `key: value` (an integer,
    /// a string, or an identifier or constant fetch as the key), else a
    /// bare value.
    fn parse_doctrine_array_item(&mut self) -> bool {
        let checkpoint = self.pos;
        let keyed = match self.peek() {
            PhpDocToken::IntLiteral(_) | PhpDocToken::StringLiteral(_) => {
                self.advance();
                true
            }
            PhpDocToken::Identifier(_) => {
                self.advance();
                if self.eat(&PhpDocToken::ColonColon) {
                    matches!(self.peek(), PhpDocToken::Identifier(_) | PhpDocToken::Star) && {
                        self.advance();
                        true
                    }
                } else {
                    true
                }
            }
            _ => false,
        };
        if keyed && (self.eat(&PhpDocToken::Equals) || self.eat(&PhpDocToken::Colon)) {
            return self.parse_doctrine_value();
        }
        self.pos = checkpoint;
        self.parse_doctrine_value()
    }

    /// phpdoc-parser's own `Unexpected token "X", expected Y` wording,
    /// without its trailing byte-offset/line suffix (this project's
    /// tokens don't keep a docblock-relative offset).
    fn error_unexpected_token(&mut self, expected: &str) {
        let message = if self.at_eof() {
            format!("Unexpected end of tag, expected {expected}")
        } else {
            format!(
                "Unexpected token \"{}\", expected {expected}",
                token_text(self.peek())
            )
        };
        self.error(message);
    }

    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(PhpDocParseError {
            message: message.into(),
            span: self.peek_span(),
        });
    }

    fn eat(&mut self, tok: &PhpDocToken) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consumes a `Variable` token if present, discarding its name -
    /// `eat` can't be used here since `PhpDocToken::Variable` carries
    /// data, so there's no fixed value to compare `peek()` against.
    fn eat_variable(&mut self) -> bool {
        if matches!(self.peek(), PhpDocToken::Variable(_)) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &PhpDocToken, what: &str) -> bool {
        if self.eat(tok) {
            true
        } else {
            self.error(format!("expected {what}"));
            false
        }
    }

    /// Case-insensitive keyword match against a bare `Identifier` token -
    /// `is`/`not`/`min`/`max` and the array-shape/callable introducer
    /// keywords are all matched this way, mirroring real PHPStan's own
    /// case-insensitive type-keyword handling.
    fn eat_keyword(&mut self, word: &str) -> bool {
        if let PhpDocToken::Identifier(s) = self.peek()
            && s.eq_ignore_ascii_case(word)
        {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Whatever raw text remains once type/structural parsing has
    /// consumed everything it recognizes - the free-text description
    /// half of `@param`/`@return`/etc. Reconstructed from the remaining
    /// tokens' own literal text rather than a byte-offset slice, since
    /// the lexer doesn't keep the original source around once tokenized.
    fn remaining_description(&mut self) -> String {
        let mut parts = Vec::new();
        while !self.at_eof() {
            let (tok, _) = self.advance();
            parts.push(token_text(&tok));
        }
        parts.join(" ")
    }

    fn parse_tag_value(&mut self, name: &str) -> PhpDocTagValue {
        // phpdoc-parser parses the `@psalm-`/`@phan-` spellings with the
        // same grammar.
        let bare = ["phpstan-", "psalm-", "phan-"]
            .iter()
            .find_map(|p| name.strip_prefix(p))
            .unwrap_or(name);
        let lower = bare.to_ascii_lowercase();
        if TYPED_VAR_TAGS.contains(&lower.as_str()) {
            return self.parse_typed_var(&lower);
        }
        if TYPE_ONLY_TAGS.contains(&lower.as_str()) {
            let ty = self.try_parse_type();
            if ty.is_some() {
                self.check_description_start_after_type();
            }
            let description = self.remaining_description();
            return PhpDocTagValue::Type { ty, description };
        }
        match lower.as_str() {
            "template" | "template-covariant" | "template-contravariant" => self.parse_template(),
            "extends"
            | "implements"
            | "use"
            | "template-extends"
            | "template-implements"
            | "template-use"
            | "inherits" => self.parse_extends_like(),
            "method" => self.parse_method(),
            _ => PhpDocTagValue::Generic(self.remaining_description()),
        }
    }

    /// phpdoc-parser's own `parseParamTagValue`/`parseParamOutTagValue`/
    /// `parsePropertyTagValue` require a `$variable` (`parseRequired
    /// VariableName`), and `parseVarTagValue` requires a type (the
    /// variable is optional there) - a tag missing its required half is an
    /// `InvalidTagValueNode` in real PHPStan (`phpDoc.parseError`), not a
    /// tag with an absent field. `tag` is the lowercased, un-prefixed name.
    fn parse_typed_var(&mut self, tag: &str) -> PhpDocTagValue {
        let ty = self.try_parse_type_if_present();
        if tag == "var" && ty.is_none() {
            self.error_unexpected_token("type");
        }
        let mut by_ref = self.eat(&PhpDocToken::Amp);
        let variadic = self.eat(&PhpDocToken::Ellipsis);
        if !by_ref {
            by_ref = self.eat(&PhpDocToken::Amp);
        }
        let variable = if let PhpDocToken::Variable(v) = self.peek() {
            let v = v.clone();
            self.advance();
            Some(v)
        } else {
            if tag != "var" {
                self.error_unexpected_token("variable");
            } else if ty.is_some() {
                self.check_description_start_after_type();
            }
            None
        };
        let description = self.remaining_description();
        PhpDocTagValue::TypedVar {
            ty,
            variable,
            by_ref,
            variadic,
            description,
        }
    }

    /// A type is present here whenever the tag's own text doesn't start
    /// straight off with `$name`/`&$name`/`...$name` - real PHPStan
    /// tolerates a type-less `@var $x`/`@param $x` (the type resolved
    /// some other way, e.g. a native hint already covers it).
    fn try_parse_type_if_present(&mut self) -> Option<TypeNode> {
        if matches!(self.peek(), PhpDocToken::Variable(_))
            || matches!(self.peek(), PhpDocToken::Amp | PhpDocToken::Ellipsis)
        {
            return None;
        }
        self.try_parse_type()
    }

    fn try_parse_type(&mut self) -> Option<TypeNode> {
        if self.at_eof() {
            return None;
        }
        Some(self.parse_type())
    }

    fn parse_template(&mut self) -> PhpDocTagValue {
        let name = if let PhpDocToken::Identifier(s) = self.peek() {
            let s = s.clone();
            self.advance();
            s
        } else {
            self.error("expected a @template name");
            String::new()
        };
        let bound = if self.eat_keyword("of") || self.eat_keyword("as") {
            Some(self.parse_type())
        } else {
            None
        };
        let default = if self.eat(&PhpDocToken::Equals) {
            Some(self.parse_type())
        } else {
            None
        };
        PhpDocTagValue::Template {
            name,
            bound,
            default,
        }
    }

    fn parse_extends_like(&mut self) -> PhpDocTagValue {
        PhpDocTagValue::ExtendsLike {
            ty: self.parse_type(),
        }
    }

    fn parse_method(&mut self) -> PhpDocTagValue {
        let is_static = self.eat_keyword("static");
        // Real PHPStan's own ambiguity: a bare identifier here could be
        // the return type or the method name itself (`@method foo()`,
        // no return type at all, implicitly `void`) - only committing to
        // "this was the return type" once we see a further identifier
        // token following it that itself is immediately followed by `(`.
        let checkpoint = self.pos;
        let first = self.parse_type();
        let (return_type, name) = if let PhpDocToken::Identifier(n) = self.peek() {
            let n = n.clone();
            if self.peek_ahead_is_lparen() || self.peek_ahead_is_langle() {
                self.advance();
                (Some(first), n)
            } else {
                self.pos = checkpoint;
                (None, self.take_identifier_or_error())
            }
        } else {
            self.pos = checkpoint;
            (None, self.take_identifier_or_error())
        };
        let templates = if matches!(self.peek(), PhpDocToken::LAngle) {
            self.parse_callable_templates().unwrap_or_else(|| {
                self.error("expected a template list");
                Vec::new()
            })
        } else {
            Vec::new()
        };
        self.expect(&PhpDocToken::LParen, "'('");
        let mut params = Vec::new();
        if !self.eat(&PhpDocToken::RParen) {
            loop {
                let ty = self.try_parse_type_if_present();
                let by_ref = self.eat(&PhpDocToken::Amp);
                let variadic = self.eat(&PhpDocToken::Ellipsis);
                let param_name = if let PhpDocToken::Variable(v) = self.peek() {
                    let v = v.clone();
                    self.advance();
                    v
                } else {
                    self.error("expected a $parameter name");
                    String::new()
                };
                let has_default = self.eat(&PhpDocToken::Equals);
                if has_default {
                    // The default value's own expression grammar is real
                    // PHP, not this file's own - skip forward past it
                    // rather than trying to parse it, matching this
                    // whole tag's own "kept coarse" scope.
                    while !matches!(self.peek(), PhpDocToken::Comma | PhpDocToken::RParen)
                        && !self.at_eof()
                    {
                        self.advance();
                    }
                }
                params.push(MethodTagParam {
                    ty,
                    name: param_name,
                    variadic,
                    by_ref,
                    has_default,
                });
                if !self.eat(&PhpDocToken::Comma) {
                    break;
                }
            }
            self.expect(&PhpDocToken::RParen, "')'");
        }
        PhpDocTagValue::Method {
            is_static,
            return_type,
            name,
            templates,
            params,
        }
    }

    fn peek_ahead_is_langle(&self) -> bool {
        self.tokens
            .get(self.pos + 1)
            .map(|(t, _)| matches!(t, PhpDocToken::LAngle))
            .unwrap_or(false)
    }

    fn peek_ahead_is_lparen(&self) -> bool {
        self.tokens
            .get(self.pos + 1)
            .map(|(t, _)| matches!(t, PhpDocToken::LParen))
            .unwrap_or(false)
    }

    fn take_identifier_or_error(&mut self) -> String {
        if let PhpDocToken::Identifier(n) = self.peek() {
            let n = n.clone();
            self.advance();
            n
        } else {
            self.error("expected a method name");
            String::new()
        }
    }

    // ---- Type-expression grammar ----

    fn parse_type(&mut self) -> TypeNode {
        self.parse_union()
    }

    fn parse_union(&mut self) -> TypeNode {
        let first = self.parse_intersection();
        if !matches!(self.peek(), PhpDocToken::Pipe) {
            return first;
        }
        let mut members = vec![first];
        while self.eat(&PhpDocToken::Pipe) {
            members.push(self.parse_intersection());
        }
        TypeNode::Union(members)
    }

    fn parse_intersection(&mut self) -> TypeNode {
        let first = self.parse_nullable();
        if !self.amp_starts_another_type() {
            return first;
        }
        let mut members = vec![first];
        while self.amp_starts_another_type() {
            self.advance();
            members.push(self.parse_nullable());
        }
        TypeNode::Intersection(members)
    }

    /// A real ambiguity in `@param`/`@method`-parameter position: `&`
    /// right after a type can mean either "intersect with the next type"
    /// or the unrelated by-ref parameter marker (`@param int &...$xs`,
    /// `@param int &$x`) - real PHPStan's own grammar resolves it the
    /// same way, by lookahead: only a genuine type-shaped token
    /// following `&` makes it an intersection continuation.
    fn amp_starts_another_type(&self) -> bool {
        if !matches!(self.peek(), PhpDocToken::Amp) {
            return false;
        }
        !matches!(
            self.tokens.get(self.pos + 1).map(|(t, _)| t),
            Some(PhpDocToken::Variable(_) | PhpDocToken::Ellipsis | PhpDocToken::Eof)
        )
    }

    fn parse_nullable(&mut self) -> TypeNode {
        if self.eat(&PhpDocToken::Question) {
            TypeNode::Nullable(Box::new(self.parse_array_suffixed()))
        } else {
            self.parse_array_suffixed()
        }
    }

    /// `T[]` array shorthand, and phpdoc-parser's `tryParseArrayOrOffset
    /// Access` offset-access form `T[K]` - only when the `[` directly
    /// follows the type (preceded by whitespace, it starts a description).
    /// An offset that doesn't parse up to its `]` rolls back, leaving the
    /// `[` for the caller, the same as phpdoc-parser's own savepoint.
    fn parse_array_suffixed(&mut self) -> TypeNode {
        let mut ty = self.parse_atom();
        loop {
            if !matches!(self.peek(), PhpDocToken::LBracket) {
                break;
            }
            if matches!(
                self.tokens.get(self.pos + 1).map(|(t, _)| t),
                Some(PhpDocToken::RBracket)
            ) {
                self.advance();
                self.advance();
                ty = TypeNode::ArrayOf(Box::new(ty));
                continue;
            }
            if self.preceded_by_whitespace() {
                break;
            }
            let checkpoint = self.pos;
            let errors_checkpoint = self.errors.len();
            self.advance();
            let offset = self.parse_type();
            if self.errors.len() == errors_checkpoint && self.eat(&PhpDocToken::RBracket) {
                ty = TypeNode::OffsetAccess(Box::new(ty), Box::new(offset));
            } else {
                self.pos = checkpoint;
                self.errors.truncate(errors_checkpoint);
                break;
            }
        }
        ty
    }

    /// Whether the current token is separated from the previous one by
    /// whitespace - phpdoc-parser's `isPrecededByHorizontalWhitespace`.
    fn preceded_by_whitespace(&self) -> bool {
        self.pos == 0
            || self
                .tokens
                .get(self.pos)
                .zip(self.tokens.get(self.pos - 1))
                .is_none_or(|((_, cur), (_, prev))| prev.end != cur.start)
    }

    /// phpdoc-parser's `parseOptionalDescription(..., limitStartToken:
    /// true)`, called right after a tag's type: a description can't start
    /// with `|`/`&` (a dangling union/intersection operator), and has to be
    /// whitespace-separated from the type (`array(width, height)` after
    /// the callable parse rolls back, `array<array><string>`).
    fn check_description_start_after_type(&mut self) {
        if self.at_eof() {
            return;
        }
        if matches!(self.peek(), PhpDocToken::Pipe | PhpDocToken::Amp) {
            self.error_unexpected_token("TOKEN_OTHER");
        } else if !self.preceded_by_whitespace() {
            self.error_unexpected_token("TOKEN_HORIZONTAL_WS");
        }
    }

    fn parse_atom(&mut self) -> TypeNode {
        match self.peek().clone() {
            PhpDocToken::LParen => {
                self.advance();
                self.parse_paren_or_conditional()
            }
            PhpDocToken::Variable(name) => {
                // `$this` is a real type (phpdoc-parser's
                // `TOKEN_THIS_VARIABLE`); any other variable where a type
                // is expected is a parse error there. Kept as a node for
                // recovery either way.
                if &*name != "this" {
                    self.error_unexpected_token("type");
                }
                self.advance();
                TypeNode::Identifier(format!("${name}"))
            }
            PhpDocToken::IntLiteral(n) => {
                self.advance();
                TypeNode::IntLiteral(n)
            }
            PhpDocToken::FloatLiteral(n) => {
                self.advance();
                TypeNode::FloatLiteral(n)
            }
            PhpDocToken::StringLiteral(s) => {
                self.advance();
                TypeNode::StringLiteral(s)
            }
            PhpDocToken::Identifier(name) => {
                self.advance();
                self.parse_identifier_led(name)
            }
            // Psalm's own negated-type syntax (`!empty`, `!null`) -
            // real text from `webmozart/assert`'s own docblocks, mixed
            // into otherwise-PHPStan-flavored stubs. Not a real parse
            // error (the old raw-substring pipeline never flagged this
            // either) and not worth its own real negation node just for
            // one third-party package's own style - degrade to the
            // wrapped type itself, silently dropping the negation
            // rather than the whole type.
            PhpDocToken::Unknown('!') => {
                self.advance();
                self.parse_atom()
            }
            _ => {
                self.error("expected a type");
                if !self.at_eof() {
                    self.advance();
                }
                TypeNode::Error
            }
        }
    }

    /// One generic-argument position, checked first for real PHPStan's
    /// own call-site variance syntax (`Collection<covariant Foo>`/
    /// `Collection<contravariant Foo>`/a bare `Collection<*>`) before
    /// falling through to an ordinary type. Case-sensitive and only when
    /// a real type token follows: a bare `Collection<covariant>` (no
    /// following type at all) is a plain type literally named
    /// `covariant`, not a variance annotation with nothing to wrap.
    fn parse_generic_arg(&mut self) -> TypeNode {
        if matches!(self.peek(), PhpDocToken::Star)
            && matches!(
                self.tokens.get(self.pos + 1).map(|(t, _)| t),
                Some(PhpDocToken::Comma | PhpDocToken::RAngle)
            )
        {
            self.advance();
            return TypeNode::Wildcard;
        }
        for (keyword, variance) in [
            ("covariant", ArgVariance::Covariant),
            ("contravariant", ArgVariance::Contravariant),
        ] {
            if let PhpDocToken::Identifier(s) = self.peek()
                && s == keyword
                && !matches!(
                    self.tokens.get(self.pos + 1).map(|(t, _)| t),
                    Some(PhpDocToken::Comma | PhpDocToken::RAngle | PhpDocToken::Eof)
                )
            {
                self.advance();
                return TypeNode::VarianceArg(variance, Box::new(self.parse_type()));
            }
        }
        self.parse_type()
    }

    /// Everything that can follow a bare identifier already consumed:
    /// `array{...}`/`list{...}`/`object{...}`, `callable(...)`/
    /// `Closure(...)`, `int<...>`, `Name::CONST`, or an ordinary
    /// `Name<Args>` generic (or a bare `Name` alone).
    fn parse_identifier_led(&mut self, name: String) -> TypeNode {
        // A fully-qualified name keeps its own leading `\` in `name`
        // itself (a real, meaningful part of the reference - `\Closure`
        // deliberately forces the global namespace, common real-world
        // style) but every keyword comparison below ignores it, so
        // `\Closure(...)`/`\array{...}`/`\int<0, 100>` are still
        // recognized the same as their unqualified spelling.
        let lower = name.trim_start_matches('\\').to_ascii_lowercase();
        if matches!(self.peek(), PhpDocToken::LBrace)
            && matches!(
                lower.as_str(),
                "array" | "list" | "non-empty-array" | "non-empty-list" | "object"
            )
        {
            return self.parse_array_shape(name);
        }
        let callable_keyword = matches!(
            lower.as_str(),
            "callable" | "closure" | "pure-callable" | "pure-closure"
        );
        if matches!(self.peek(), PhpDocToken::LParen) && callable_keyword {
            return self.parse_callable(name, Vec::new());
        }
        if matches!(self.peek(), PhpDocToken::LAngle)
            && callable_keyword
            && let Some(templates) = self.try_parse_callable_templates()
        {
            return self.parse_callable(name, templates);
        }
        if matches!(self.peek(), PhpDocToken::LAngle)
            && lower.ends_with("int")
            && let Some(range) = self.try_parse_int_range()
        {
            return range;
        }
        if self.eat(&PhpDocToken::ColonColon) {
            let const_name = if let PhpDocToken::Identifier(c) = self.peek() {
                let c = c.clone();
                self.advance();
                if self.eat(&PhpDocToken::Star) {
                    format!("{c}*")
                } else {
                    c
                }
            } else if self.eat(&PhpDocToken::Star) {
                "*".to_string()
            } else {
                self.error("expected a class constant name");
                String::new()
            };
            return TypeNode::ConstFetch {
                class: name,
                const_name,
            };
        }
        if matches!(self.peek(), PhpDocToken::LAngle) && !self.looks_like_html() {
            self.advance();
            let mut args = vec![self.parse_generic_arg()];
            while self.eat(&PhpDocToken::Comma) {
                // A trailing comma before `>` (real, common style,
                // `Collection<int, string,>`) - stop instead of trying
                // to parse one more type from an empty remainder.
                if matches!(self.peek(), PhpDocToken::RAngle) {
                    break;
                }
                args.push(self.parse_generic_arg());
            }
            self.expect(&PhpDocToken::RAngle, "'>'");
            return TypeNode::Generic(name, args);
        }
        // A bare wildcard suffix with no `::` at all (`\SIG*`, real text
        // from `symfony/console`'s own `SignalableCommandInterface::
        // getSubscribedSignals()` docblock - "any `SIG_*`-style
        // constant", informally) - keep the `*` as part of the
        // identifier's own text (same idea as `ConstFetch`'s own
        // wildcard `const_name`, just with no class scope to attach it
        // to) rather than silently dropping it once this token is
        // reached. Only when directly adjacent, no preceding whitespace
        // - this lexer discards whitespace entirely rather than keeping
        // it as its own token the way `vendor/phpstan/phpdoc-parser`'s
        // own lexer does, so span adjacency is this grammar's only way
        // to tell `\SIG*` apart from a type followed by free-text
        // markdown emphasis (`@return int *0* if ASCII...`, real text
        // from Piwigo's own `StringHelper::qualifyUtf8()` - genuinely
        // just a plain `int` return type, not a wildcard).
        if matches!(self.peek(), PhpDocToken::Star) && self.is_adjacent_to_previous() {
            self.advance();
            return TypeNode::Identifier(format!("{name}*"));
        }
        TypeNode::Identifier(name)
    }

    /// Whether the current token immediately follows the previous one
    /// with no whitespace between them - see `parse_identifier_led`'s
    /// own wildcard-suffix check, its only caller.
    fn is_adjacent_to_previous(&self) -> bool {
        self.pos > 0 && self.tokens[self.pos].1.start == self.tokens[self.pos - 1].1.end
    }

    fn token_at(&self, idx: usize) -> Option<&PhpDocToken> {
        self.tokens.get(idx).map(|(t, _)| t)
    }

    /// `phpstorm-stubs`' own docblocks routinely trail a type with a
    /// literal HTML fragment (`@return array <p>an array of...</p>`,
    /// real text from `Core.php`'s own `debug_backtrace()`) - genuine
    /// free-text description, not the start of a `Name<Args>` generic.
    /// Mirrors `vendor/phpstan/phpdoc-parser`'s own `TypeParser::isHtml`
    /// exactly: once `<name>` is seen, scan the remaining tokens for a
    /// matching `</name>` closing tag before treating `<` as opening a
    /// generic-argument list at all - real PHPStan hits the identical
    /// ambiguity (its lexer also tokenizes `<` as a bare angle-bracket
    /// token unconditionally) and resolves it the same way.
    fn looks_like_html(&self) -> bool {
        let Some(PhpDocToken::Identifier(tag_name)) = self.token_at(self.pos + 1) else {
            return false;
        };
        if !matches!(self.token_at(self.pos + 2), Some(PhpDocToken::RAngle)) {
            return false;
        }
        let mut i = self.pos + 3;
        loop {
            match self.token_at(i) {
                None | Some(PhpDocToken::Eof) => return false,
                Some(PhpDocToken::LAngle)
                    if matches!(self.token_at(i + 1), Some(PhpDocToken::Unknown('/')))
                        && matches!(self.token_at(i + 2), Some(PhpDocToken::Identifier(s)) if s == tag_name)
                        && matches!(self.token_at(i + 3), Some(PhpDocToken::RAngle)) =>
                {
                    return true;
                }
                _ => i += 1,
            }
        }
    }

    fn try_parse_int_range(&mut self) -> Option<TypeNode> {
        let checkpoint = self.pos;
        let errors_checkpoint = self.errors.len();
        self.advance(); // consume '<'
        let min = self.parse_int_bound();
        if !self.eat(&PhpDocToken::Comma) {
            self.pos = checkpoint;
            self.errors.truncate(errors_checkpoint);
            return None;
        }
        let max = self.parse_int_bound();
        if !self.eat(&PhpDocToken::RAngle) {
            self.pos = checkpoint;
            self.errors.truncate(errors_checkpoint);
            return None;
        }
        match (min, max) {
            (Some(min), Some(max)) => Some(TypeNode::IntRange(min, max)),
            _ => {
                self.pos = checkpoint;
                self.errors.truncate(errors_checkpoint);
                None
            }
        }
    }

    /// `Some(None)` = the literal `min`/`max` keyword (unbounded on that
    /// side); `None` = this wasn't a valid int-range bound at all, so
    /// the caller should abandon the whole `int<...>` interpretation and
    /// fall back to ordinary generic parsing.
    fn parse_int_bound(&mut self) -> Option<Option<i64>> {
        if self.eat_keyword("min") || self.eat_keyword("max") {
            return Some(None);
        }
        if let PhpDocToken::IntLiteral(n) = self.peek() {
            let n = *n;
            self.advance();
            return Some(Some(n));
        }
        None
    }

    /// `object{...}` follows phpdoc-parser's separate object-shape
    /// grammar: every item needs a `key:` and there is no `...` tail.
    /// Only `array{...}` takes a two-argument `...<K, V>` tail; the list
    /// and `non-empty-array` kinds take a single `...<V>`.
    fn parse_array_shape(&mut self, keyword: String) -> TypeNode {
        self.advance(); // consume '{'
        let lower = keyword.trim_start_matches('\\').to_ascii_lowercase();
        let is_object = lower == "object";
        let mut fields = Vec::new();
        let mut sealed = true;
        let mut unsealed_type = None;
        if !matches!(self.peek(), PhpDocToken::RBrace) {
            loop {
                if !is_object && self.eat(&PhpDocToken::Ellipsis) {
                    sealed = false;
                    if matches!(self.peek(), PhpDocToken::LAngle) {
                        unsealed_type = Some(self.parse_unsealed_shape_type(lower == "array"));
                    }
                    self.eat(&PhpDocToken::Comma);
                    break;
                }
                let checkpoint = self.pos;
                let key = self.try_parse_shape_key();
                let optional = key.is_some() && self.eat(&PhpDocToken::Question);
                let has_colon = key.is_some() && self.eat(&PhpDocToken::Colon);
                let (key, optional, ty) = if has_colon {
                    (key, optional, self.parse_type())
                } else if is_object {
                    self.pos = checkpoint;
                    if key.is_some() {
                        self.advance();
                        if optional {
                            self.advance();
                        }
                        self.error_unexpected_token("TOKEN_COLON");
                    } else {
                        self.error_unexpected_token("TOKEN_IDENTIFIER");
                    }
                    break;
                } else {
                    // Wasn't actually `key: Type` - backtrack and this
                    // whole field is just a bare, unkeyed type
                    // (positional/list-style shape entry).
                    self.pos = checkpoint;
                    (None, false, self.parse_type())
                };
                fields.push(ArrayShapeField { key, ty, optional });
                if !self.eat(&PhpDocToken::Comma) {
                    break;
                }
                if matches!(self.peek(), PhpDocToken::RBrace) {
                    break;
                }
            }
        }
        self.expect(&PhpDocToken::RBrace, "'}'");
        TypeNode::ArrayShape {
            keyword,
            fields,
            sealed,
            unsealed_type,
        }
    }

    /// `<V>` or (when `with_key`) `<K, V>` after a shape's `...`, returned
    /// as `(key, value)`.
    fn parse_unsealed_shape_type(
        &mut self,
        with_key: bool,
    ) -> (Option<Box<TypeNode>>, Box<TypeNode>) {
        self.advance(); // consume '<'
        let first = self.parse_type();
        let result = if with_key && self.eat(&PhpDocToken::Comma) {
            (Some(Box::new(first)), Box::new(self.parse_type()))
        } else {
            (None, Box::new(first))
        };
        if !self.eat(&PhpDocToken::RAngle) {
            self.error_unexpected_token("TOKEN_CLOSE_ANGLE_BRACKET");
        }
        result
    }

    fn try_parse_shape_key(&mut self) -> Option<ShapeKey> {
        match self.peek().clone() {
            PhpDocToken::IntLiteral(n) => {
                self.advance();
                Some(ShapeKey::Int(n))
            }
            PhpDocToken::StringLiteral(s) => {
                self.advance();
                Some(ShapeKey::String(s))
            }
            PhpDocToken::Identifier(s) => {
                self.advance();
                Some(ShapeKey::Identifier(s))
            }
            _ => None,
        }
    }

    /// `<T of Bound super Lower = Default, ...>` directly followed by a
    /// callable's `(` - phpdoc-parser's `parseCallableTemplates`. Any
    /// other `<...>` rolls back (a generic `Closure<...>` reference).
    fn try_parse_callable_templates(&mut self) -> Option<Vec<CallableTemplate>> {
        let checkpoint = self.pos;
        let errors_checkpoint = self.errors.len();
        let parsed = self.parse_callable_templates();
        if parsed.is_some()
            && self.errors.len() == errors_checkpoint
            && matches!(self.peek(), PhpDocToken::LParen)
        {
            return parsed;
        }
        self.pos = checkpoint;
        self.errors.truncate(errors_checkpoint);
        None
    }

    fn parse_callable_templates(&mut self) -> Option<Vec<CallableTemplate>> {
        self.advance(); // consume '<'
        let mut templates = Vec::new();
        loop {
            let PhpDocToken::Identifier(name) = self.peek().clone() else {
                return None;
            };
            self.advance();
            let word = |p: &Self, w: &str| matches!(p.peek(), PhpDocToken::Identifier(s) if s == w);
            let bound = if word(self, "of") || word(self, "as") {
                self.advance();
                Some(self.parse_type())
            } else {
                None
            };
            let lower = if word(self, "super") {
                self.advance();
                Some(self.parse_type())
            } else {
                None
            };
            let default = if self.eat(&PhpDocToken::Equals) {
                Some(self.parse_type())
            } else {
                None
            };
            templates.push(CallableTemplate {
                name,
                bound,
                lower,
                default,
            });
            if !self.eat(&PhpDocToken::Comma) || matches!(self.peek(), PhpDocToken::RAngle) {
                break;
            }
        }
        self.eat(&PhpDocToken::RAngle).then_some(templates)
    }

    fn parse_callable(&mut self, keyword: String, templates: Vec<CallableTemplate>) -> TypeNode {
        self.advance(); // consume '('
        let mut params = Vec::new();
        if !matches!(self.peek(), PhpDocToken::RParen) {
            loop {
                let ty = self.parse_type();
                let by_ref = self.eat(&PhpDocToken::Amp);
                let variadic = self.eat(&PhpDocToken::Ellipsis);
                // A callable parameter's own name is real, legal,
                // optional syntax (`callable(Node $firstNode): bool`,
                // real text from Rector's own docblocks) - purely
                // documentation, per `vendor/phpstan/phpdoc-parser`'s
                // own `parseCallableParameter` (`CallableTypeParameterNode`
                // keeps it, this grammar has no use for it and just
                // discards it) - consumed here so it doesn't break the
                // rest of this parameter list's own parse.
                self.eat_variable();
                let optional = self.eat(&PhpDocToken::Equals);
                params.push(CallableParam {
                    ty,
                    variadic,
                    by_ref,
                    optional,
                });
                if !self.eat(&PhpDocToken::Comma) {
                    break;
                }
                if matches!(self.peek(), PhpDocToken::RParen) {
                    break;
                }
            }
        }
        self.expect(&PhpDocToken::RParen, "')'");
        let return_type = if self.eat(&PhpDocToken::Colon) {
            Some(Box::new(self.parse_type()))
        } else {
            None
        };
        TypeNode::Callable {
            keyword,
            templates,
            params,
            return_type,
        }
    }

    /// Called right after consuming a `(` - tries the conditional-type
    /// grammar (`$subject is Type ? A : B`) first, backtracking to plain
    /// parenthesized-type parsing (discarding any errors the failed
    /// attempt recorded) if it doesn't match.
    fn parse_paren_or_conditional(&mut self) -> TypeNode {
        let checkpoint = self.pos;
        let errors_checkpoint = self.errors.len();
        if let Some(conditional) = self.try_parse_conditional() {
            return conditional;
        }
        self.pos = checkpoint;
        self.errors.truncate(errors_checkpoint);
        let inner = self.parse_type();
        self.expect(&PhpDocToken::RParen, "')'");
        TypeNode::Paren(Box::new(inner))
    }

    fn try_parse_conditional(&mut self) -> Option<TypeNode> {
        let subject = match self.peek().clone() {
            PhpDocToken::Variable(v) => {
                self.advance();
                format!("${v}")
            }
            PhpDocToken::Identifier(s) => {
                self.advance();
                s
            }
            _ => return None,
        };
        if !self.eat_keyword("is") {
            return None;
        }
        let negated = self.eat_keyword("not");
        let target = self.parse_type();
        if !self.eat(&PhpDocToken::Question) {
            return None;
        }
        let if_true = self.parse_type();
        if !self.eat(&PhpDocToken::Colon) {
            return None;
        }
        let if_false = self.parse_type();
        if !self.eat(&PhpDocToken::RParen) {
            return None;
        }
        Some(TypeNode::Conditional {
            subject,
            negated,
            target: Box::new(target),
            if_true: Box::new(if_true),
            if_false: Box::new(if_false),
        })
    }
}

/// Parses one tag's own already tag-name-stripped `rest` text into a
/// `PhpDocTagValue`, exactly the shape the whole-comment `parse` entry
/// point itself would build for a tag named `name` - used by `docblock::
/// apply_tag`'s Stage B migration to source one tag's structured value at
/// a time, alongside its own byte-exact `rest` text (rather than
/// re-deriving the whole comment's own `tag_lines` join a second time).
pub(crate) fn parse_tag_rest(name: &str, rest: &str) -> (PhpDocTagValue, Vec<PhpDocParseError>) {
    let tokens = Lexer::new(rest).tokenize();
    let mut parser = TagParser::new(tokens);
    let value = parser.parse_tag_value(name);
    (value, parser.errors)
}

/// The errors real phpdoc-parser's `parseDoctrineArguments` would raise
/// for `rest`, a tag value starting with `(` on a tag it has no dedicated
/// parser for (`@ORM\Column(type="string")`, `@url (image.jpg) ...`) -
/// such a tag is a Doctrine annotation there, and a malformed argument
/// list makes it an `InvalidTagValueNode` (`phpDoc.parseError`). Empty
/// when the arguments parse, including when `rest` doesn't start with
/// `(` at all.
pub(crate) fn doctrine_argument_errors(rest: &str) -> Vec<PhpDocParseError> {
    let tokens = Lexer::new(rest).tokenize();
    let mut parser = TagParser::new(tokens);
    parser.parse_doctrine_arguments();
    parser.errors
}

/// Parses a single leading type expression off `rest`, returning it
/// alongside `rest`'s own remaining raw text - untouched, byte-exact, not
/// reconstructed from tokens - plus any error the type itself produced.
/// `None` when `rest` doesn't start with anything type-shaped at all.
/// Used by `docblock::apply_tag`'s Stage B migration for the handful of
/// tags real PHPStan gives no dedicated tag-value shape to (`@mixin`,
/// `@sealed`, `@phpstan-type`, ...) that are nonetheless, in practice, one
/// leading type expression followed by trailing free text.
pub fn parse_type_prefix(rest: &str) -> (Option<TypeNode>, &str, Vec<PhpDocParseError>) {
    let tokens = Lexer::new(rest).tokenize();
    let mut parser = TagParser::new(tokens);
    let ty = parser.try_parse_type();
    let byte_idx = byte_offset_of(rest, parser.peek_span().start);
    (ty, &rest[byte_idx..], parser.errors)
}

/// The byte offset in `text` of the lexer's 1-based `pos` (line, and char
/// column within that line). A tag's `rest` can span several lines (a
/// wrapped tag's continuation lines are joined with `\n`, see
/// `docblock::parse`), and Rust slicing is byte-indexed while `text` can
/// hold multi-byte UTF-8, so both coordinates matter.
fn byte_offset_of(text: &str, pos: syntax::token::Position) -> usize {
    let line_start = if pos.line <= 1 {
        0
    } else {
        text.match_indices('\n')
            .nth(pos.line as usize - 2)
            .map_or(text.len(), |(i, _)| i + 1)
    };
    let col = (pos.col as usize).saturating_sub(1);
    text[line_start..]
        .char_indices()
        .nth(col)
        .map_or(text.len(), |(b, _)| line_start + b)
}

fn token_text(t: &PhpDocToken) -> String {
    match t {
        PhpDocToken::At => "@".to_string(),
        PhpDocToken::Identifier(s) => s.clone(),
        PhpDocToken::Variable(s) => format!("${s}"),
        PhpDocToken::IntLiteral(n) => n.to_string(),
        PhpDocToken::FloatLiteral(n) => n.to_string(),
        PhpDocToken::StringLiteral(s) => s.clone(),
        PhpDocToken::Pipe => "|".to_string(),
        PhpDocToken::Amp => "&".to_string(),
        PhpDocToken::Question => "?".to_string(),
        PhpDocToken::LAngle => "<".to_string(),
        PhpDocToken::RAngle => ">".to_string(),
        PhpDocToken::LBrace => "{".to_string(),
        PhpDocToken::RBrace => "}".to_string(),
        PhpDocToken::LParen => "(".to_string(),
        PhpDocToken::RParen => ")".to_string(),
        PhpDocToken::LBracket => "[".to_string(),
        PhpDocToken::RBracket => "]".to_string(),
        PhpDocToken::Comma => ",".to_string(),
        PhpDocToken::Colon => ":".to_string(),
        PhpDocToken::ColonColon => "::".to_string(),
        PhpDocToken::Equals => "=".to_string(),
        PhpDocToken::Ellipsis => "...".to_string(),
        PhpDocToken::Star => "*".to_string(),
        PhpDocToken::Unknown(c) => c.to_string(),
        PhpDocToken::Eof => String::new(),
    }
}
