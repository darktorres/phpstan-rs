use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use crate::php_string::PhpString;
use crate::span::Span;
use crate::token::{SimpleIndexKey, StringPart};

impl Parser {
    /// `base` is where the whole string starts. Each part keeps its own
    /// span; a `ComplexExpr` fragment is re-lexed where it sits in the
    /// file, so an error inside `"{$obj->prop}"` points at its real
    /// position.
    pub(crate) fn build_interpolated(
        &mut self,
        parts: Vec<StringPart>,
        base: u32,
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
                    let tokens = Lexer::fragment(&src, frag_base)
                        .with_interner(&self.interner)
                        .tokenize_with_spans();
                    let mut sub = Parser::new(tokens).with_interner(&self.interner);
                    sub.next_node_id.set(self.next_node_id.get());
                    let parsed = sub.parse_expression()?;
                    sub.check_pattern_only()?;
                    // `{$x = 1}`: PHP's grammar takes a variable there, not
                    // any expression.
                    if !is_variable_chain(&parsed) {
                        return Err(ParseError {
                            message: "syntax error, a string's {$...} holds a variable, not an expression"
                                .to_string(),
                            span: parsed.span,
                        });
                    }
                    // `{$a b}`: the braces hold one expression.
                    if !sub.at_eof() {
                        return Err(ParseError {
                            message: format!(
                                "syntax error, unexpected {:?} in string interpolation",
                                sub.peek()
                            ),
                            span: sub.current_span(),
                        });
                    }
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
                    nullsafe,
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
                    let arrow_len = if nullsafe { 3 } else { 2 };
                    exprs.push(Expr {
                        id: self.node_id(),
                        kind: ExprKind::PropertyAccess(
                            Box::new(var_expr),
                            Ident {
                                name: self.interner.name(&prop),
                                // Past the arrow.
                                span: Span {
                                    start: var_end.saturating_add(arrow_len),
                                    end,
                                },
                            },
                            ObjectOperator::from_nullsafe(nullsafe),
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
        if let [only] = exprs.as_mut_slice()
            && let ExprKind::Literal(Literal::String(_)) = only.kind
        {
            only.span = Span {
                start: base,
                end: self.previous_end(),
            };
            return Ok(exprs.pop().expect("one part"));
        }
        Ok(self.expr(ExprKind::Interpolated(exprs), base))
    }
}

/// Whether `e` is what PHP's `variable` production allows: a variable
/// with property fetches, method or static calls, and offsets chained on
/// it (`$a->b[0]->c()`, `$f()`, `$c::$p`).
fn is_variable_chain(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Variable(_) | ExprKind::DynamicVariable(_) => true,
        ExprKind::PropertyAccess(base, ..)
        | ExprKind::MethodCall(base, ..)
        | ExprKind::DynamicPropertyAccess(base, _, _)
        | ExprKind::DynamicMethodCall(base, ..)
        | ExprKind::Index(base, _)
        | ExprKind::Call(base, _) => is_variable_chain(base),
        ExprKind::StaticPropertyFetch(StaticTarget::Expr(base), _)
        | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Expr(base), _)
        | ExprKind::StaticCall(StaticTarget::Expr(base), ..)
        | ExprKind::DynamicStaticCall(StaticTarget::Expr(base), ..) => is_variable_chain(base),
        _ => false,
    }
}
