use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use crate::php_string::PhpString;
use crate::token::{Position, SimpleIndexKey, Span, StringPart};

impl Parser {
    /// `base` is where the whole string starts. Each part keeps its own
    /// span; a `ComplexExpr` fragment is re-lexed where it sits in the
    /// file, so an error inside `"{$obj->prop}"` points at its real
    /// position.
    pub(crate) fn build_interpolated(
        &mut self,
        parts: Vec<StringPart>,
        base: Position,
    ) -> Result<Expr, ParseError> {
        let mut exprs = Vec::new();
        for part in parts {
            match part {
                StringPart::Literal(s, span) => exprs.push(Expr {
                    id: self.node_id(),
                    kind: ExprKind::Literal(Literal::String(s)),
                    span,
                }),
                StringPart::SimpleVar(name, span) => exprs.push(Expr {
                    id: self.node_id(),
                    kind: ExprKind::Variable(name),
                    span,
                }),
                StringPart::ComplexExpr(src, frag_base) => {
                    // Lexed where it sits in the file, so every span in
                    // the fragment is already the real one.
                    let tokens = Lexer::new_in_php_at(&src, frag_base)
                        .with_interner(&self.interner)
                        .tokenize_with_spans();
                    let mut sub = Parser::new(tokens).with_interner(&self.interner);
                    sub.next_node_id.set(self.next_node_id.get());
                    let parsed = sub.parse_expression()?;
                    self.next_node_id.set(sub.next_node_id.get());
                    exprs.push(parsed);
                }
                // `$var->prop`/`$var[key]` simple syntax - built directly
                // from the pieces the lexer already extracted, with real
                // (not fragment-relative) positions, instead of round-
                // tripping through a synthesized source string and a full
                // second `Lexer`/`Parser` the way `ComplexExpr` above does.
                StringPart::PropertyAccess {
                    var,
                    var_start,
                    var_end,
                    prop,
                    end,
                } => {
                    let var_expr = Expr {
                        id: self.node_id(),
                        kind: ExprKind::Variable(var),
                        span: Span {
                            start: var_start,
                            end: var_end,
                        },
                    };
                    exprs.push(Expr {
                        id: self.node_id(),
                        // Real PHP's simple interpolation syntax has no
                        // `?->` form - always non-nullsafe.
                        kind: ExprKind::PropertyAccess(
                            Box::new(var_expr),
                            Ident {
                                name: self.interner.name(&prop),
                                // Past the `->`, on the variable's line.
                                span: Span {
                                    start: Position {
                                        line: var_end.line,
                                        col: var_end.col + 2,
                                    },
                                    end,
                                },
                            },
                            false,
                        ),
                        span: Span {
                            start: var_start,
                            end,
                        },
                    });
                }
                StringPart::Index {
                    var,
                    var_start,
                    var_end,
                    key,
                    end,
                } => {
                    let var_expr = Expr {
                        id: self.node_id(),
                        kind: ExprKind::Variable(var),
                        span: Span {
                            start: var_start,
                            end: var_end,
                        },
                    };
                    let key_kind = match key {
                        SimpleIndexKey::Var(name) => ExprKind::Variable(name),
                        SimpleIndexKey::Int(n) => ExprKind::Literal(Literal::Int(n)),
                        SimpleIndexKey::Str(s) => {
                            ExprKind::Literal(Literal::String(PhpString::from(s)))
                        }
                    };
                    let key_expr = Expr {
                        id: self.node_id(),
                        kind: key_kind,
                        span: Span {
                            start: var_end,
                            end,
                        },
                    };
                    exprs.push(Expr {
                        id: self.node_id(),
                        kind: ExprKind::Index(Box::new(var_expr), Some(Box::new(key_expr))),
                        span: Span {
                            start: var_start,
                            end,
                        },
                    });
                }
            }
        }
        // A string with no interpolation is one literal spanning the whole
        // string, quotes included.
        if exprs.len() == 1
            && let Some(ExprKind::Literal(Literal::String(_))) = exprs.first().map(|e| &e.kind)
        {
            let kind = exprs.into_iter().next().unwrap().kind;
            return Ok(self.expr(kind, base));
        }
        Ok(self.expr(ExprKind::Interpolated(exprs), base))
    }
}
