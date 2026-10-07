use std::sync::Arc;

use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use crate::php_string::PhpString;
use crate::span::Span;
use crate::token::{SimpleIndexKey, StringPart};

impl Parser<'_> {
    /// `base` is where the whole string starts. Each part keeps its own
    /// span; a `ComplexExpr` fragment is re-lexed where it sits in the
    /// file, so an error inside `"{$obj->prop}"` points at its real
    /// position.
    pub(crate) fn build_interpolated(
        &mut self,
        parts: Vec<StringPart>,
        base: u32,
    ) -> Result<ExprId, ParseError> {
        let exprs_start = self.begin_expr_id_list();
        for part in parts {
            match part {
                StringPart::Literal(s, span) => {
                    let id = self.expr_at(ExprKind::Literal(Literal::String(s)), span);
                    self.push_expr_id_list_item(id);
                }
                StringPart::SimpleVar(name, span) => {
                    let id = self.expr_at(ExprKind::Variable(name), span);
                    self.push_expr_id_list_item(id);
                }
                StringPart::ComplexExpr(src, frag_base) => {
                    // Lexed where it sits in the file, so every span in
                    // the fragment is already the real one.
                    let decoded_bytes_base = {
                        let arenas = self.node_arenas.borrow();
                        arenas
                            .decoded_bytes_base
                            .checked_add(
                                u32::try_from(arenas.decoded_bytes.len())
                                    .expect("decoded byte vector exhausted"),
                            )
                            .expect("decoded byte vector exhausted")
                    };
                    let lexer = Lexer::fragment(&src, frag_base)
                        .with_decoded_bytes_base(decoded_bytes_base)
                        // `src` is reconstructed PHP text, while its token
                        // spans are biased to the original file. Keep string
                        // bytes in the fragment's decoded store so a quoted
                        // literal cannot resolve against unrelated original
                        // source bytes at the same span.
                        .with_source_backed_strings(false);
                    let lexer = if self.tracks_name_ids {
                        lexer
                    } else {
                        lexer.without_file_names()
                    };
                    let fragment_arenas = self.node_arenas.borrow().new_fragment();
                    let mut sub = Parser::from_lexer_with_node_arenas(lexer, fragment_arenas);
                    sub.line_index = Arc::clone(&self.line_index);
                    // The fragment's names are read in the namespace and
                    // imports where it sits.
                    *sub.ctx.borrow_mut() = self.ctx.borrow().clone();
                    sub.next_expr_id.set(self.next_expr_id.get());
                    sub.next_stmt_id.set(self.next_stmt_id.get());
                    sub.expr_span_base = self.next_expr_id.get();
                    sub.stmt_span_base = self.next_stmt_id.get();
                    *sub.recorder.borrow_mut() = self.recorder.borrow().fork();
                    let parsed = sub.parse_expression()?;
                    sub.check_pattern_only()?;
                    // `{$x = 1}`: PHP's grammar takes a variable there, not
                    // any expression.
                    if !is_variable_chain(&sub.node_arenas.borrow(), parsed) {
                        return Err(ParseError::new(
                            "syntax error, a string's {$...} holds a variable, not an expression"
                                .to_string(),
                            sub.expr_span(parsed),
                        ));
                    }
                    // `{$a b}`: the braces hold one expression.
                    if !sub.at_eof() {
                        return Err(ParseError::new(
                            format!(
                                "syntax error, unexpected {:?} in string interpolation",
                                sub.peek()
                            ),
                            sub.current_span(),
                        ));
                    }
                    self.next_expr_id.set(sub.next_expr_id.get());
                    self.next_stmt_id.set(sub.next_stmt_id.get());
                    let fragment = sub.take_node_arenas();
                    self.node_arenas.borrow_mut().append_fragment(fragment);
                    // The parent file has already been lexed completely;
                    // the fragment's decoded bytes are appended directly
                    // to its owning arenas below.
                    self.recorder.borrow_mut().absorb(sub.take_references());
                    self.push_expr_id_list_item(parsed);
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
                    let var_expr = self.expr_at(
                        ExprKind::Variable(var),
                        Span {
                            start: var_start,
                            end: var_end,
                        },
                    );
                    let arrow_len = if nullsafe { 3 } else { 2 };
                    let id = self.expr_at(
                        ExprKind::PropertyAccess(
                            var_expr,
                            self.ident(
                                self.intern(&prop),
                                // Past the arrow.
                                Span {
                                    start: var_end.saturating_add(arrow_len),
                                    end,
                                },
                            ),
                            ObjectOperator::from_nullsafe(nullsafe),
                        ),
                        Span {
                            start: var_start,
                            end,
                        },
                    );
                    self.push_expr_id_list_item(id);
                }
                StringPart::Index {
                    var,
                    var_start,
                    var_end,
                    key,
                    end,
                } => {
                    let var_expr = self.expr_at(
                        ExprKind::Variable(var),
                        Span {
                            start: var_start,
                            end: var_end,
                        },
                    );
                    let key_kind = match key {
                        SimpleIndexKey::Var(name) => ExprKind::Variable(name),
                        SimpleIndexKey::Int(n) => ExprKind::Literal(Literal::Int(n)),
                        SimpleIndexKey::Str(s) => {
                            ExprKind::Literal(Literal::String(PhpString::from(s).into()))
                        }
                    };
                    let key_expr = self.expr_at(
                        key_kind,
                        Span {
                            start: var_end,
                            end,
                        },
                    );
                    let id = self.expr_at(
                        ExprKind::Index(var_expr, Some(key_expr)),
                        Span {
                            start: var_start,
                            end,
                        },
                    );
                    self.push_expr_id_list_item(id);
                }
            }
        }
        // A string with no interpolation is one literal spanning the whole
        // string, quotes included.
        let single_expr = match self.expr_id_list(exprs_start) {
            [only] => Some(*only),
            _ => None,
        };
        if let Some(only) = single_expr
            && matches!(
                self.node_arenas.borrow().expr(only).map(|node| &node.kind),
                Some(ExprKind::Literal(Literal::String(_)))
            )
        {
            let span = Span {
                start: base,
                end: self.previous_end(),
            };
            self.set_expr_span(only, span);
            self.discard_expr_id_list(exprs_start);
            return Ok(only);
        }
        let exprs = self.finish_expr_id_list(exprs_start);
        Ok(self.expr(ExprKind::Interpolated(exprs), base))
    }
}

/// Whether `e` is what PHP's `variable` production allows: a variable
/// with property fetches, method or static calls, and offsets chained on
/// it (`$a->b[0]->c()`, `$f()`, `$c::$p`).
fn is_variable_chain(arenas: &NodeArenas, id: ExprId) -> bool {
    let Some(e) = arenas.expr(id) else {
        return false;
    };
    match &e.kind {
        ExprKind::Variable(_) | ExprKind::DynamicVariable(_) => true,
        ExprKind::PropertyAccess(base, ..)
        | ExprKind::MethodCall(base, ..)
        | ExprKind::DynamicPropertyAccess(base, _, _)
        | ExprKind::DynamicMethodCall(base, ..)
        | ExprKind::Index(base, _)
        | ExprKind::Call(base, _) => is_variable_chain(arenas, *base),
        ExprKind::StaticPropertyFetch(StaticTarget::Expr(base), _)
        | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Expr(base), _)
        | ExprKind::StaticCall(StaticTarget::Expr(base), ..)
        | ExprKind::DynamicStaticCall(StaticTarget::Expr(base), ..) => {
            is_variable_chain(arenas, *base)
        }
        _ => false,
    }
}
