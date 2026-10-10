//! The canonical prints of what the lexer makes: tokens, the parts of an
//! interpolated string, warnings, the comments that hold an ignore marker,
//! and the facts of a file.

use crate::canonical::{Canonical, Printer};
use crate::lexer::{FileFacts, IgnoreMark, LexWarning};
use crate::parser::ParseError;
use crate::span::{LineCol, Range, Span};
use crate::token::{Keyword, SimpleIndexKey, StringPart, Token};
use crate::{canonical_enum, canonical_struct};

/// A keyword as the language spells it.
impl Canonical for Keyword {
    fn canonical(&self, p: &mut Printer) {
        p.atom(self.as_str());
    }
}

canonical_enum!(Token {
    InlineHtml(text),
    DocComment(text),
    Variable(name),
    Integer(value),
    Float(value),
    StringLit(value),
    InterpString(parts),
    ShellExec(parts),
    Identifier(name),
    Keyword(keyword),
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
    PipeGt,
    DollarBrace,
    AttributeStart,
    Dollar,
    Eof,
});

canonical_enum!(StringPart {
    Literal(text, span),
    SimpleVar(name, span),
    ComplexExpr(text, start),
    PropertyAccess {
        var,
        var_start,
        var_end,
        prop,
        nullsafe,
        end,
    },
    Index {
        var,
        var_start,
        var_end,
        key,
        end,
    },
});

canonical_enum!(SimpleIndexKey {
    Var(name),
    Int(value),
    Str(text),
});

canonical_struct!(LexWarning {
    span,
    message,
    is_error,
});

canonical_struct!(IgnoreMark {
    text,
    start,
    line,
    is_doc,
    code_before_on_line,
    next_code,
});

canonical_struct!(FileFacts {
    has_anonymous_class,
    has_nested_declaration,
    has_nested_function_like,
    unsets_property,
    mentions,
    syntax,
});

/// Each mark the file spells, with where it first does.
impl Canonical for crate::lexer::SyntaxMarks {
    fn canonical(&self, p: &mut Printer) {
        let marks: Vec<(String, Span)> = crate::lexer::SyntaxMark::ALL
            .into_iter()
            .filter_map(|mark| Some((format!("{mark:?}"), self.first(mark)?)))
            .collect();
        p.map(marks.iter().map(|(mark, span)| (mark, span)));
    }
}

impl Canonical for ParseError {
    fn canonical(&self, p: &mut Printer) {
        p.node(
            "ParseError",
            &[("message", &self.message), ("span", &self.span)],
        );
    }
}

/// `line:col`.
impl Canonical for LineCol {
    fn canonical(&self, p: &mut Printer) {
        let LineCol { line, col } = self;
        p.atom(&format!("{line}:{col}"));
    }
}

/// `start-end`.
impl Canonical for Range {
    fn canonical(&self, p: &mut Printer) {
        let Range { start, end } = self;
        start.canonical(p);
        p.atom("-");
        end.canonical(p);
    }
}
