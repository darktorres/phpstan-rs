use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use crate::token::{Position, SimpleIndexKey, Span, StringPart};

impl Parser {
    /// `base` is the whole interpolated string literal's own starting
    /// position - used as a reasonable anchor for the synthesized
    /// `Literal`/`Variable` parts (which have no tokens of their own to
    /// derive a more specific position from). A `ComplexExpr` fragment
    /// gets a more precise treatment: it's re-lexed and re-parsed in
    /// isolation (its own sub-`Parser`, starting fresh at line 1 col 1),
    /// then `bias_expr_spans` walks the resulting tree once, adding
    /// `StringPart::ComplexExpr`'s own second field (the fragment's real
    /// starting position, computed correctly back in the lexer) to every
    /// span in it - so an error inside `"{$obj->prop}"` points at its
    /// real position instead of somewhere relative to the throwaway
    /// re-lex.
    pub(crate) fn build_interpolated(
        &mut self,
        parts: Vec<StringPart>,
        base: Position,
    ) -> Result<Expr, ParseError> {
        let mut exprs = Vec::new();
        for part in parts {
            match part {
                StringPart::Literal(s) => {
                    exprs.push(self.expr(ExprKind::Literal(Literal::String(s)), base))
                }
                StringPart::SimpleVar(name) => {
                    exprs.push(self.expr(ExprKind::Variable(name), base))
                }
                StringPart::ComplexExpr(src, frag_base) => {
                    let tokens = Lexer::new_in_php(&src).tokenize_with_spans();
                    let mut sub = Parser::new(tokens);
                    let mut parsed = sub.parse_expression()?;
                    bias_expr_spans(&mut parsed, frag_base);
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
                        kind: ExprKind::Variable(var),
                        span: Span {
                            start: var_start,
                            end: var_end,
                        },
                    };
                    exprs.push(Expr {
                        // Real PHP's simple interpolation syntax has no
                        // `?->` form - always non-nullsafe.
                        kind: ExprKind::PropertyAccess(Box::new(var_expr), prop, false),
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
                        kind: ExprKind::Variable(var),
                        span: Span {
                            start: var_start,
                            end: var_end,
                        },
                    };
                    let key_kind = match key {
                        SimpleIndexKey::Var(name) => ExprKind::Variable(name),
                        SimpleIndexKey::Int(n) => ExprKind::Literal(Literal::Int(n)),
                        SimpleIndexKey::Str(s) => ExprKind::Literal(Literal::String(s)),
                    };
                    let key_expr = Expr {
                        kind: key_kind,
                        span: Span {
                            start: var_end,
                            end,
                        },
                    };
                    exprs.push(Expr {
                        kind: ExprKind::Index(Box::new(var_expr), Some(Box::new(key_expr))),
                        span: Span {
                            start: var_start,
                            end,
                        },
                    });
                }
            }
        }
        if exprs.len() == 1
            && let Some(ExprKind::Literal(Literal::String(_))) = exprs.first().map(|e| &e.kind)
        {
            return Ok(exprs.into_iter().next().unwrap());
        }
        Ok(self.expr(ExprKind::Interpolated(exprs), base))
    }
}

/// A sub-parser re-lexing a `{$...}` fragment in isolation starts
/// counting positions fresh at line 1, col 1 - biases one such
/// fragment-relative `Position` into real, absolute file coordinates
/// using `frag_base` (the fragment's own real starting position) as the
/// anchor. Only the fragment's first line needs its column offset by
/// `frag_base`'s own column - a `Position` past the first line already
/// starts counting from column 1 on its own, real line.
///
/// Saturating, not plain, arithmetic: no real file gets remotely close
/// to `u32::MAX` lines/columns (this project's own lexer comment notes
/// a real, extreme corpus file's longest line at ~1.25 million
/// characters, still far below it), but `token_stream_parse`'s own
/// `Arbitrary`-derived `Position` values are unconstrained, and a
/// `frag_base` near `u32::MAX` must not panic - a clamped (not wrapped
/// or crashing) result is still an honest answer even in that
/// physically-impossible tail case.
fn bias_position(pos: Position, frag_base: Position) -> Position {
    if pos.line == 1 {
        Position {
            line: frag_base.line,
            col: frag_base.col.saturating_add(pos.col.saturating_sub(1)),
        }
    } else {
        Position {
            line: frag_base.line.saturating_add(pos.line.saturating_sub(1)),
            col: pos.col,
        }
    }
}

fn bias_span(span: Span, frag_base: Position) -> Span {
    Span {
        start: bias_position(span.start, frag_base),
        end: bias_position(span.end, frag_base),
    }
}

/// Recursively rewrites every `Span` in `expr`'s own tree from a
/// `{$...}` fragment's re-lexed-in-isolation coordinates into real,
/// absolute file coordinates (see `bias_position`). Mirrors `deps.rs`'s
/// own `collect_expr` walk (same variants, same recursion shape, no
/// wildcard arm - both need to visit every real sub-expression a parsed
/// `Expr` tree can contain) - rewriting a span here instead of
/// collecting a name. A `Closure`'s own multi-statement body isn't
/// walked (a closure literal defined inline inside `{$...}`
/// interpolation is vanishingly rare, and its body would need a
/// matching `Stmt`-level walker to bias fully) - a narrow, honest gap,
/// not silently wrong: the closure expression's own span is still
/// correctly biased, just not its inner statements'.
fn bias_expr_spans(expr: &mut Expr, frag_base: Position) {
    expr.span = bias_span(expr.span, frag_base);
    match &mut expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::Name(_) => {}
        ExprKind::StaticAccess(_, _) => {}
        ExprKind::Closure { .. } => {}
        ExprKind::DynamicVariable(e)
        | ExprKind::Unary(_, e)
        | ExprKind::Grouping(e)
        | ExprKind::Cast(_, e, _)
        | ExprKind::Throw(e)
        | ExprKind::YieldFrom(e)
        | ExprKind::FirstClassCallable(e)
        | ExprKind::ShellExec(e)
        | ExprKind::PropertyAccess(e, _, _) => bias_expr_spans(e, frag_base),
        ExprKind::ArrowFn { body, .. } => bias_expr_spans(body, frag_base),
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                if let Some(k) = &mut item.key {
                    bias_expr_spans(k, frag_base);
                }
                bias_expr_spans(&mut item.value, frag_base);
            }
        }
        ExprKind::Binary(_, l, r)
        | ExprKind::Assign(l, r)
        | ExprKind::AssignRef(l, r)
        | ExprKind::CompoundAssign(_, l, r) => {
            bias_expr_spans(l, frag_base);
            bias_expr_spans(r, frag_base);
        }
        ExprKind::Ternary(c, t, e) => {
            bias_expr_spans(c, frag_base);
            if let Some(t) = t {
                bias_expr_spans(t, frag_base);
            }
            bias_expr_spans(e, frag_base);
        }
        ExprKind::Call(callee, args) => {
            bias_expr_spans(callee, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::New(target, args) => {
            bias_expr_spans(target, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::MethodCall(recv, _, args, _) => {
            bias_expr_spans(recv, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::DynamicPropertyAccess(base, name_expr) => {
            bias_expr_spans(base, frag_base);
            bias_expr_spans(name_expr, frag_base);
        }
        ExprKind::DynamicMethodCall(recv, name_expr, args) => {
            bias_expr_spans(recv, frag_base);
            bias_expr_spans(name_expr, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::StaticCall(_, _, args) => {
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::Index(base, idx) => {
            bias_expr_spans(base, frag_base);
            if let Some(i) = idx {
                bias_expr_spans(i, frag_base);
            }
        }
        ExprKind::Instanceof(inner, target) => {
            bias_expr_spans(inner, frag_base);
            if let InstanceofTarget::Expr(e) = target {
                bias_expr_spans(e, frag_base);
            }
        }
        ExprKind::Clone(inner, overrides) => {
            bias_expr_spans(inner, frag_base);
            if let Some(o) = overrides {
                bias_expr_spans(o, frag_base);
            }
        }
        ExprKind::NewAnonymousClass(_, args) => {
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::Yield(k, v) => {
            if let Some(k) = k {
                bias_expr_spans(k, frag_base);
            }
            if let Some(v) = v {
                bias_expr_spans(v, frag_base);
            }
        }
        ExprKind::Interpolated(parts) => {
            for p in parts {
                bias_expr_spans(p, frag_base);
            }
        }
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            bias_expr_spans(subject, frag_base);
            for (conds, result) in arms {
                for c in conds {
                    bias_expr_spans(c, frag_base);
                }
                bias_expr_spans(result, frag_base);
            }
            if let Some(d) = default {
                bias_expr_spans(d, frag_base);
            }
        }
    }
}
