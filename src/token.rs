/// A single point in a source file, 1-indexed (matching editor convention).
/// `col` counts UTF-8 characters, not raw bytes - real PHP source can and
/// does contain multi-byte content inside actual code (not just comments;
/// found the hard way against the real Piwigo corpus this project verifies
/// against - string literals with em-dashes/curly quotes/accented
/// characters), and a byte-offset column would silently overshoot the true
/// character position on every such line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

/// A source range - real LSP `range`s and most compiler-style diagnostics
/// want both a start and an end, not just a single anchor point.
///
/// `Hash` (Phase 17 follow-up): `Locals`'s own declaration-provenance
/// tracking uses a `Span` as a `HashSet` element, one entry per candidate
/// declaration site for a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq)]
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
    pub fn try_from_str(s: &str) -> Option<Keyword> {
        use Keyword::*;
        Some(match s.to_ascii_lowercase().as_str() {
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
// PHP source text (the inside of `{$...}`, or a reconstructed `$var->prop` /
// `$var[key]` access) that gets re-lexed and re-parsed as a full expression -
// this avoids writing a second, cut-down expression grammar just for strings.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Literal(String),
    SimpleVar(String),
    // The fragment's own source text, plus the real file position it
    // starts at - `parser::build_interpolated` re-lexes the fragment in
    // isolation (its own byte offsets have no connection to the real
    // file), then biases the fragment-relative spans it gets back using
    // this as the base, so an error inside `"{$obj->prop}"` still points
    // at its real position instead of somewhere relative to a throwaway
    // re-lex.
    ComplexExpr(String, Position),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    InlineHtml(String),
    // Raw `/** ... */` text, including the delimiters. Regular `/* */` and
    // `//` comments are still discarded as whitespace - only doc comments
    // are worth carrying through, so the parser can pair one with the
    // declaration it precedes.
    DocComment(String),

    Variable(String),
    Integer(i64),
    Float(f64),
    StringLit(String),
    InterpString(Vec<StringPart>),
    // `` `shell command $with $interpolation` `` - same interpolation
    // rules as a double-quoted string, different delimiter and runtime
    // meaning (shell_exec).
    ShellExec(Vec<StringPart>),
    Identifier(String),
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
