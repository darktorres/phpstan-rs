#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;

/// A small fixed pool of legal PHP identifier spellings - picked by
/// index rather than built character-by-character, so every generated
/// name is always a valid identifier with no encoding/first-char edge
/// cases to get wrong.
const IDENT_POOL: &[&str] = &[
    "a", "b", "x", "y", "foo", "bar", "Baz", "Widget", "value", "count", "items", "handler",
];

#[derive(Debug, Clone, Copy)]
struct Ident(&'static str);

impl<'a> Arbitrary<'a> for Ident {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let idx = u.int_in_range(0..=(IDENT_POOL.len() - 1))?;
        Ok(Ident(IDENT_POOL[idx]))
    }
}

/// One call argument - `name:` (a named argument, `postfix.rs`'s own
/// `name:`-label handling) and `...` (a spread argument) are both real,
/// separate call-argument shapes this parser recognizes alongside a bare
/// positional one. A generated `spread` argument never also carries a
/// `name` (real PHP doesn't allow combining them, and `render` just
/// prioritizes `spread` when both happen to be generated) - a harmless
/// generator simplification, not a grammar this project's own parser
/// enforces here.
#[derive(Debug, Arbitrary)]
struct CallArg {
    name: Option<Ident>,
    spread: bool,
    value: Expr,
}

impl CallArg {
    fn render(&self, out: &mut String) {
        if self.spread {
            out.push_str("...");
        } else if let Some(name) = &self.name {
            out.push_str(name.0);
            out.push_str(": ");
        }
        self.value.render(out);
    }
}

/// Not a real expression grammar - a small pool of literal/operator/call
/// ingredients, enough variety to exercise `parse_assignment`/
/// `parse_coalesce`'s precedence-climbing chain with real, randomized
/// structure instead of replaying fixed strings.
#[derive(Debug, Arbitrary)]
enum Expr {
    IntLit(u16),
    StringLit(Ident),
    Var(Ident),
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Call(Ident, Vec<CallArg>),
    ArrayLit(Vec<Expr>),
    // PHP 7.3+ flexible heredoc/nowdoc (`src/lexer/strings.rs::lex_heredoc`) -
    // a real, separate lexer state machine from a plain double-quoted
    // string, worth its own generated shape rather than only ever
    // appearing as a hand-written seed.
    Heredoc(Ident),
    Nowdoc(Ident),
    // `$obj?->prop` / `$obj?->method(...)` - `postfix.rs`'s own
    // `Token::Arrow | Token::NullsafeArrow` chain, alongside the plain
    // (non-nullsafe) `->` forms.
    PropAccess(Box<Expr>, Ident, bool),
    MethodCall(Box<Expr>, Ident, Vec<CallArg>, bool),
    // `foo(...)` - PHP 8.1's first-class callable syntax
    // (`postfix.rs:212`: a bare `...` immediately followed by `)`,
    // distinct from a spread *argument*, which is `...` followed by a
    // real expression).
    FirstClassCallable(Ident),
}

#[derive(Debug, Arbitrary)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Concat,
    Eq,
    Lt,
    And,
    Or,
    Coalesce,
    Spaceship,
}

impl BinOp {
    fn as_str(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Concat => ".",
            BinOp::Eq => "===",
            BinOp::Lt => "<",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Coalesce => "??",
            BinOp::Spaceship => "<=>",
        }
    }
}

impl Expr {
    fn render(&self, out: &mut String) {
        match self {
            Expr::IntLit(n) => out.push_str(&n.to_string()),
            Expr::StringLit(id) => {
                out.push('"');
                out.push_str(id.0);
                out.push('"');
            }
            Expr::Var(id) => {
                out.push('$');
                out.push_str(id.0);
            }
            Expr::Binary(lhs, op, rhs) => {
                out.push('(');
                lhs.render(out);
                out.push(' ');
                out.push_str(op.as_str());
                out.push(' ');
                rhs.render(out);
                out.push(')');
            }
            Expr::Call(name, args) => {
                out.push_str(name.0);
                out.push('(');
                for (i, arg) in args.iter().take(4).enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    arg.render(out);
                }
                out.push(')');
            }
            Expr::ArrayLit(items) => {
                out.push('[');
                for (i, item) in items.iter().take(4).enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.render(out);
                }
                out.push(']');
            }
            // A label alone on its own line, immediately followed by
            // `;`/`,`/`)` with no leading whitespace, is what
            // `lex_heredoc` requires to recognize the closing label -
            // `EOT` here satisfies that unconditionally regardless of
            // `id.0`'s own content.
            Expr::Heredoc(id) => {
                out.push_str("<<<EOT\n");
                out.push_str(id.0);
                out.push_str("\nEOT");
            }
            Expr::Nowdoc(id) => {
                out.push_str("<<<'EOT'\n");
                out.push_str(id.0);
                out.push_str("\nEOT");
            }
            Expr::PropAccess(recv, prop, nullsafe) => {
                recv.render(out);
                out.push_str(if *nullsafe { "?->" } else { "->" });
                out.push_str(prop.0);
            }
            Expr::MethodCall(recv, name, args, nullsafe) => {
                recv.render(out);
                out.push_str(if *nullsafe { "?->" } else { "->" });
                out.push_str(name.0);
                out.push('(');
                for (i, arg) in args.iter().take(4).enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    arg.render(out);
                }
                out.push(')');
            }
            Expr::FirstClassCallable(name) => {
                out.push_str(name.0);
                out.push_str("(...)");
            }
        }
    }
}

#[derive(Debug, Arbitrary)]
enum DeclKind {
    Function,
    Class,
    Interface,
    Trait,
    Enum,
}

impl DeclKind {
    fn keyword(&self) -> &'static str {
        match self {
            DeclKind::Function => "function",
            DeclKind::Class => "class",
            DeclKind::Interface => "interface",
            DeclKind::Trait => "trait",
            DeclKind::Enum => "enum",
        }
    }
}

/// Statement-shaped building blocks - deliberately *not* a full grammar:
/// a `Decl`'s own body is rendered the same way a function body would be
/// (a list of statements), even for `class`/`interface`/`trait`/`enum`
/// where real PHP only allows member declarations - the parser is
/// expected to hand back an ordinary `ParseError` for that mismatch, not
/// panic, and stressing its error-recovery path with plausible-looking
/// content is still more useful than pure byte noise.
#[derive(Debug, Arbitrary)]
enum Fragment {
    If(Box<Fragment>, Option<Box<Fragment>>),
    While(Box<Fragment>),
    For(Box<Fragment>),
    Foreach(Ident, Box<Fragment>),
    Match(Ident, Vec<(u16, Box<Fragment>)>),
    Switch(Ident, Vec<(u16, Box<Fragment>)>),
    Decl(DeclKind, Ident, Vec<Fragment>),
    Assign(Ident, Expr),
    Closure(Vec<Ident>, Box<Fragment>),
    ArrowFn(Ident, Expr),
    Attribute(Ident),
    ExprStmt(Expr),
    Block(Vec<Fragment>),
}

// Defensive only: the generated tree is already finite (bounded by how
// much of the fuzzer's own input `Unstructured` had left to consume), so
// this cap never changes correctness - it just keeps rendering from ever
// producing a pathologically large source string on the rare generation
// that comes out unusually deep.
const MAX_RENDER_DEPTH: usize = 6;

impl Fragment {
    fn render(&self, out: &mut String, depth: usize) {
        if depth > MAX_RENDER_DEPTH {
            out.push_str("1;\n");
            return;
        }
        match self {
            Fragment::If(then, else_) => {
                out.push_str("if (true) {\n");
                then.render(out, depth + 1);
                out.push_str("}\n");
                if let Some(else_) = else_ {
                    out.push_str("else {\n");
                    else_.render(out, depth + 1);
                    out.push_str("}\n");
                }
            }
            Fragment::While(body) => {
                out.push_str("while (true) {\n");
                body.render(out, depth + 1);
                out.push_str("}\n");
            }
            Fragment::For(body) => {
                out.push_str("for ($i = 0; $i < 10; $i++) {\n");
                body.render(out, depth + 1);
                out.push_str("}\n");
            }
            Fragment::Foreach(id, body) => {
                out.push_str("foreach ($");
                out.push_str(id.0);
                out.push_str(" as $v) {\n");
                body.render(out, depth + 1);
                out.push_str("}\n");
            }
            Fragment::Match(subject, arms) => {
                out.push_str("match ($");
                out.push_str(subject.0);
                out.push_str(") {\n");
                for (n, body) in arms.iter().take(4) {
                    out.push_str(&n.to_string());
                    out.push_str(" => {\n");
                    body.render(out, depth + 1);
                    out.push_str("},\n");
                }
                out.push_str("default => 0,\n};\n");
            }
            Fragment::Switch(subject, arms) => {
                out.push_str("switch ($");
                out.push_str(subject.0);
                out.push_str(") {\n");
                for (n, body) in arms.iter().take(4) {
                    out.push_str("case ");
                    out.push_str(&n.to_string());
                    out.push_str(":\n");
                    body.render(out, depth + 1);
                    out.push_str("break;\n");
                }
                out.push_str("default:\nbreak;\n}\n");
            }
            Fragment::Decl(kind, name, body) => {
                out.push_str(kind.keyword());
                out.push(' ');
                out.push_str(name.0);
                out.push_str(" {\n");
                for stmt in body.iter().take(6) {
                    stmt.render(out, depth + 1);
                }
                out.push_str("}\n");
            }
            Fragment::Assign(id, expr) => {
                out.push('$');
                out.push_str(id.0);
                out.push_str(" = ");
                expr.render(out);
                out.push_str(";\n");
            }
            Fragment::Closure(params, body) => {
                out.push_str("$f = function(");
                for (i, p) in params.iter().take(4).enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('$');
                    out.push_str(p.0);
                }
                out.push_str(") {\n");
                body.render(out, depth + 1);
                out.push_str("};\n");
            }
            Fragment::ArrowFn(param, expr) => {
                out.push_str("$f = fn($");
                out.push_str(param.0);
                out.push_str(") => ");
                expr.render(out);
                out.push_str(";\n");
            }
            Fragment::Attribute(name) => {
                out.push_str("#[");
                out.push_str(name.0);
                out.push_str("]\n");
            }
            Fragment::ExprStmt(expr) => {
                expr.render(out);
                out.push_str(";\n");
            }
            Fragment::Block(stmts) => {
                for stmt in stmts.iter().take(6) {
                    stmt.render(out, depth + 1);
                }
            }
        }
    }
}

#[derive(Debug, Arbitrary)]
struct FuzzProgram {
    fragments: Vec<Fragment>,
}

impl FuzzProgram {
    /// Always opens with a real `<?php` tag - the concrete, specific
    /// reason raw byte-mutation (the `lex_and_parse` target) rarely
    /// reaches deep parser logic on its own: without it, the lexer stays
    /// in `InlineHtml`-scanning mode and never even starts tokenizing PHP.
    fn to_source(&self) -> String {
        let mut out = String::from("<?php\n");
        for fragment in self.fragments.iter().take(24) {
            fragment.render(&mut out, 0);
        }
        out
    }
}

// See fuzz/src/lib.rs's own doc comment for what guarded_parse guards
// against (the parser's deliberate MAX_EXPR_DEPTH panic, and a real stack
// overflow below it on an ordinary-sized thread stack).
fuzz_target!(|program: FuzzProgram| {
    phpstan_rs_fuzz::guarded_parse(program.to_source());
});
