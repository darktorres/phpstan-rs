use std::sync::Arc;

use crate::php_string::PhpString;

/// A single point in a source file, 1-indexed (matching editor convention).
/// `col` counts UTF-8 characters, not raw bytes - real PHP source can and
/// does contain multi-byte content inside actual code, not just comments
/// (string literals with em-dashes/curly quotes/accented characters), and
/// a byte-offset column would silently overshoot the true character
/// position on every such line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

/// A source range - real LSP `range`s and most compiler-style diagnostics
/// want both a start and an end, not just a single anchor point.
///
/// `Hash`: `Locals`'s own declaration-provenance tracking uses a `Span`
/// as a `HashSet` element, one entry per candidate declaration site for
/// a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum Keyword {
    Function,
    Class,
    Interface,
    Trait,
    Enum,
    Extends,
    Implements,
    Public,
    Private,
    Protected,
    Static,
    Const,
    Final,
    Abstract,
    Readonly,
    If,
    ElseIf,
    Else,
    While,
    Do,
    For,
    Foreach,
    As,
    Switch,
    Case,
    Default,
    Break,
    Continue,
    Return,
    Echo,
    Print,
    New,
    Clone,
    Instanceof,
    True,
    False,
    Null,
    Namespace,
    Use,
    Global,
    Try,
    Catch,
    Finally,
    Throw,
    Array,
    List,
    Fn,
    Match,
    And,
    Or,
    Xor,
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
    Yield,
    Goto,
    EndIf,
    EndWhile,
    EndFor,
    EndForeach,
    EndSwitch,
}

impl Keyword {
    /// No real PHP keyword this table recognizes is longer than this
    /// (the longest, "require_once"/"include_once", is 12 bytes) - an
    /// identifier longer than this bound can never match, so `try_from_
    /// bytes` bails out before ever touching it, and every identifier
    /// short enough to possibly match gets lowercased into a stack
    /// buffer instead of a heap allocation - found and fixed via a
    /// perf-optimization investigation workflow, since this runs on
    /// every identifier-shaped token the lexer produces, keyword or not.
    /// Operates on raw bytes (not `&str`) specifically so `Lexer::
    /// next_token`'s identifier branch can check "is this a keyword"
    /// *before* ever allocating the owned `String` an ordinary
    /// `Token::Identifier` needs - a real second, wasted allocation
    /// otherwise, since a matched keyword never keeps that string at all.
    const MAX_KEYWORD_LEN: usize = 16;

    pub fn try_from_str(s: &str) -> Option<Keyword> {
        Self::try_from_bytes(s.as_bytes())
    }

    pub fn try_from_bytes(bytes: &[u8]) -> Option<Keyword> {
        use Keyword::*;
        if bytes.len() > Self::MAX_KEYWORD_LEN {
            return None;
        }
        let mut buf = [0u8; Self::MAX_KEYWORD_LEN];
        for (dst, src) in buf.iter_mut().zip(bytes) {
            *dst = src.to_ascii_lowercase();
        }
        // `bytes` is a raw identifier-shaped byte span - `is_ident_
        // continue` accepts any byte `>= 0x80` without tracking UTF-8
        // sequence boundaries (the same reason `Lexer::lex_ident_raw`
        // uses `from_utf8_lossy`, not `from_utf8`, to build the owned
        // `Identifier` string), so a boundary-split multi-byte character
        // could make this slice invalid UTF-8. ASCII-lowercasing never
        // changes that either way - a real keyword is always pure ASCII,
        // so any such slice was never going to match one regardless;
        // `None` here is exactly the right answer, not a fallback for an
        // impossible case.
        let lowered = std::str::from_utf8(&buf[..bytes.len()]).ok()?;
        Some(match lowered {
            "function" => Function,
            "class" => Class,
            "interface" => Interface,
            "trait" => Trait,
            "enum" => Enum,
            "extends" => Extends,
            "implements" => Implements,
            "public" => Public,
            "private" => Private,
            "protected" => Protected,
            "static" => Static,
            "const" => Const,
            "final" => Final,
            "abstract" => Abstract,
            "readonly" => Readonly,
            "if" => If,
            "elseif" => ElseIf,
            "else" => Else,
            "while" => While,
            "do" => Do,
            "for" => For,
            "foreach" => Foreach,
            "as" => As,
            "switch" => Switch,
            "case" => Case,
            "default" => Default,
            "break" => Break,
            "continue" => Continue,
            "return" => Return,
            "echo" => Echo,
            "print" => Print,
            "new" => New,
            "clone" => Clone,
            "instanceof" => Instanceof,
            "true" => True,
            "false" => False,
            "null" => Null,
            "namespace" => Namespace,
            "use" => Use,
            "global" => Global,
            "try" => Try,
            "catch" => Catch,
            "finally" => Finally,
            "throw" => Throw,
            "array" => Array,
            "and" => And,
            "or" => Or,
            "xor" => Xor,
            "include" => Include,
            "include_once" => IncludeOnce,
            "require" => Require,
            "require_once" => RequireOnce,
            "yield" => Yield,
            "list" => List,
            "fn" => Fn,
            "match" => Match,
            "endif" => EndIf,
            "endwhile" => EndWhile,
            "endfor" => EndFor,
            "endforeach" => EndForeach,
            "endswitch" => EndSwitch,
            "goto" => Goto,
            _ => return None,
        })
    }

    /// The inverse of `try_from_str` - needed because most of these keywords
    /// are only reserved in *declaration* position in real PHP ("semi-
    /// reserved words"): `enum`, `match`, `new`, `list`, `static`, etc are
    /// all valid namespace segments, method names, and property names.
    /// `use Foo\Enum\Bar;` or `$x->match()` would otherwise misparse just
    /// because this lexer doesn't distinguish position the way PHP's own
    /// grammar does.
    pub fn as_str(&self) -> &'static str {
        use Keyword::*;
        match self {
            Function => "function",
            Class => "class",
            Interface => "interface",
            Trait => "trait",
            Enum => "enum",
            Extends => "extends",
            Implements => "implements",
            Public => "public",
            Private => "private",
            Protected => "protected",
            Static => "static",
            Const => "const",
            Final => "final",
            Abstract => "abstract",
            Readonly => "readonly",
            If => "if",
            ElseIf => "elseif",
            Else => "else",
            While => "while",
            Do => "do",
            For => "for",
            Foreach => "foreach",
            As => "as",
            Switch => "switch",
            Case => "case",
            Default => "default",
            Break => "break",
            Continue => "continue",
            Return => "return",
            Echo => "echo",
            Print => "print",
            New => "new",
            Clone => "clone",
            Instanceof => "instanceof",
            True => "true",
            False => "false",
            Null => "null",
            Namespace => "namespace",
            Use => "use",
            Global => "global",
            Try => "try",
            Catch => "catch",
            Finally => "finally",
            Throw => "throw",
            Array => "array",
            And => "and",
            Or => "or",
            Xor => "xor",
            Include => "include",
            IncludeOnce => "include_once",
            Require => "require",
            RequireOnce => "require_once",
            Yield => "yield",
            List => "list",
            Fn => "fn",
            Match => "match",
            EndIf => "endif",
            EndWhile => "endwhile",
            EndFor => "endfor",
            EndForeach => "endforeach",
            EndSwitch => "endswitch",
            Goto => "goto",
        }
    }
}

// A piece of a double-quoted (interpolated) string. `ComplexExpr` holds raw
// PHP source text (the inside of `{$...}`, or a `$var[key]` access whose own
// key doesn't fit `Index`'s `SimpleIndexKey`) that gets re-lexed and
// re-parsed as a full expression - this avoids writing a second, cut-down
// expression grammar just for strings. `PropertyAccess`/`Index` cover the
// two "simple syntax" shapes (`$var->prop`, `$var[key]`) directly instead -
// the lexer already has the variable/property/key text as separate pieces
// at the point it would otherwise throw them away into a synthesized
// `format!("${name}->{prop}")`-style source string, so building the `Expr`
// straight from those pieces (`parser::build_interpolated`) skips a full
// second `Lexer`/`Parser` round-trip per occurrence. `$var->prop`'s own
// property name is always a well-formed identifier by construction (the
// lexer only enters that branch after confirming it starts with an
// identifier byte), so it has no fallback; `[key]` does, since anything can
// sit between the brackets - see `SimpleIndexKey`'s own doc comment.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum StringPart {
    // A literal chunk and the span of its source text.
    Literal(PhpString, Span),
    // `$var`, spanning the `$` and the name.
    SimpleVar(Arc<str>, Span),
    // The fragment's own source text, plus the real file position it
    // starts at - `parser::build_interpolated` re-lexes the fragment on
    // its own, starting at this position, so an error inside
    // `"{$obj->prop}"` still points at its real position. Deliberately kept as plain `String`, not interned: this
    // text is re-lexed by a throwaway sub-`Lexer`/`Parser` and then
    // immediately discarded (`Parser::build_interpolated`'s own
    // `ComplexExpr` arm is its only consumer) - interning it would pay a
    // real lookup/lock cost for text that's read exactly once and never
    // compared/deduplicated again, with no downstream `Arc<str>` field to
    // carry the saved allocation into anyway.
    ComplexExpr(String, Position),
    // `$var->prop` (or `$var?->prop`) simple syntax. `var_start` is the
    // `$`'s own position, `var_end` the position right after the variable
    // name (where the arrow starts), `end` the position right after `prop`.
    PropertyAccess {
        var: Arc<str>,
        var_start: Position,
        var_end: Position,
        prop: Arc<str>,
        nullsafe: bool,
        end: Position,
    },
    // `$var[key]` simple syntax, `key` restricted to `SimpleIndexKey`'s
    // three shapes - anything else falls back to `ComplexExpr` instead
    // (see `SimpleIndexKey`'s own doc comment). Position fields mirror
    // `PropertyAccess`'s own: `var_start`/`var_end` bracket the variable
    // name, `end` is the position right after the closing `]`.
    Index {
        var: Arc<str>,
        var_start: Position,
        var_end: Position,
        key: SimpleIndexKey,
        end: Position,
    },
}

// The three key shapes PHP's own `$var[key]` string-interpolation simple
// syntax accepts natively (a bare identifier reads as a string key, not a
// constant reference, unlike ordinary PHP code - real PHP's own lexer
// special-cases this exact position). Anything else (a nested expression,
// a quoted string, whitespace, an operator) isn't valid simple syntax at
// all and falls back to `StringPart::ComplexExpr`'s re-lex path, which is
// a defensive fallback (a well-formed key not fitting one of these three
// shapes shouldn't occur in real, already-lexable PHP source) rather than
// a real, expected case.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum SimpleIndexKey {
    Var(Arc<str>),
    Int(i64),
    Str(Arc<str>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum Token {
    InlineHtml(Arc<str>),
    // Raw `/** ... */` text, including the delimiters. Regular `/* */` and
    // `//` comments are still discarded as whitespace - only doc comments
    // are worth carrying through, so the parser can pair one with the
    // declaration it precedes.
    DocComment(Arc<str>),

    Variable(Arc<str>),
    Integer(i64),
    Float(f64),
    // Deliberately plain `String`, not interned - unlike an identifier or
    // variable name, a string literal's own contents rarely repeat
    // (real, distinct message/SQL/template text almost every time), so
    // interning it would pay a real lookup/lock cost for little to no
    // dedup benefit.
    StringLit(PhpString),
    InterpString(Vec<StringPart>),
    // `` `shell command $with $interpolation` `` - same interpolation
    // rules as a double-quoted string, different delimiter and runtime
    // meaning (shell_exec).
    ShellExec(Vec<StringPart>),
    Identifier(Arc<str>),
    Keyword(Keyword),

    // punctuation / operators
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semicolon,
    Comma,
    Colon,
    DoubleColon,
    Arrow,
    NullsafeArrow,
    FatArrow,
    Question,
    QuestionQuestion,
    QuestionQuestionEquals,
    Dot,
    DotEquals,
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    Percent,
    PlusEquals,
    MinusEquals,
    StarEquals,
    StarStarEquals,
    SlashEquals,
    PercentEquals,
    AmpEquals,
    PipeEquals,
    CaretEquals,
    LtLtEquals,
    GtGtEquals,
    Equals,
    EqualsEquals,
    EqualsEqualsEquals,
    BangEquals,
    BangEqualsEquals,
    Lt,
    Gt,
    LtEquals,
    GtEquals,
    Spaceship,
    AmpAmp,
    PipePipe,
    Bang,
    Amp,
    Pipe,
    Caret,
    Tilde,
    LtLt,
    GtGt,
    Increment,
    Decrement,
    At,
    Backslash,
    Ellipsis,
    // PHP 8.5's pipe operator (`|>`).
    PipeGt,
    // `${` - starts the older "variable variable" syntax `${expr}`.
    DollarBrace,
    // `#[` - opens an attribute group (`#[Attribute(...), Other]`). Real
    // tokens, not skipped: everything between this and the matching `]`
    // (the attribute's name, `(`, its arguments, `,` between multiple
    // attributes in one group) is ordinary tokenization from here on, so
    // the parser can build a real `Attribute` node instead of the content
    // being thrown away entirely. `#[PhpStormStubsElementAvailable(...)]`
    // and `#[LanguageLevelTypeAware(...)]` are the two narrow exceptions
    // (still consumed as raw text by `skip_whitespace_and_comments`, never
    // reaching the parser as tokens at all) - see that function's own
    // comment.
    AttributeStart,
    // A lone `$` not immediately followed by an identifier or `{` - the
    // brace-less variable-variable form (`$$name`, meaning "the variable
    // named by the value of $name", same as `${$name}`).
    Dollar,

    Eof,
}
