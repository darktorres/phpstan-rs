use crate::ast::*;
use crate::token::{Keyword, Position, Span, Token};

// Real PHP's own engine imposes a call-stack-depth guard specifically so
// pathologically nested input (`~~~~~~...~~~~~1`, deeply chained `??`,
// deeply nested parens/arrays) produces a normal fatal error instead of
// crashing the process - php-src's own Zend/tests/stack_limit/*.phpt exist
// to test exactly that. This parser's few genuinely self-recursive (not
// while-loop) productions need the same guard: a stack overflow is a hard
// fault, not a panic, so it can't be caught by anything upstream (a batch
// checker's catch_unwind included) and takes the whole process down.
const MAX_EXPR_DEPTH: usize = 1000;

/// A recoverable parse failure - genuinely malformed syntax, as opposed to
/// `enter_expr`'s own `MAX_EXPR_DEPTH` guard (a resource-exhaustion limit,
/// not a syntax error, and kept as a panic - see its own doc comment).
/// `message` mirrors this parser's own former panic message text.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ParseError {}

pub struct Parser {
    pub(crate) tokens: Vec<Token>,
    // Parallel to `tokens` (same length, same indices) - `spans[i]` is
    // `tokens[i]`'s own source span. Kept separate rather than zipped
    // into one `Vec<(Token, Span)>` so every existing `self.tokens[...]`
    // access throughout this file (there are many) keeps working
    // unchanged.
    pub(crate) spans: Vec<Span>,
    pub(crate) pos: usize,
    // A doc comment's raw text, keyed by the index (in `tokens`, after
    // filtering) of the token it immediately precedes. Built once up front
    // so the rest of the parser never has to know DocComment tokens
    // existed - only the specific call sites that attach one to a
    // declaration look here.
    pub(crate) doc_comments: std::collections::HashMap<usize, String>,
    // Incremented/decremented around the parser's self-recursive
    // expression productions (parse_unary, parse_assignment,
    // parse_coalesce) - see MAX_EXPR_DEPTH.
    pub(crate) expr_depth: usize,
}

mod classes;
mod expr;
mod functions;
mod interpolation;
mod postfix;
mod stmt;
mod tokens;
mod type_hints;

impl Parser {
    pub fn new(tokens: Vec<(Token, Span)>) -> Self {
        let mut filtered = Vec::with_capacity(tokens.len());
        let mut filtered_spans = Vec::with_capacity(tokens.len());
        let mut doc_comments = std::collections::HashMap::new();
        let mut pending: Option<String> = None;
        for (tok, span) in tokens {
            match tok {
                Token::DocComment(s) => pending = Some(s),
                other => {
                    if let Some(doc) = pending.take() {
                        doc_comments.insert(filtered.len(), doc);
                    }
                    filtered.push(other);
                    filtered_spans.push(span);
                }
            }
        }
        Parser {
            tokens: filtered,
            spans: filtered_spans,
            pos: 0,
            doc_comments,
            expr_depth: 0,
        }
    }

    /// The position a `Stmt`/`Expr` about to be parsed starts at - call
    /// before consuming its first token. Falls back to the last known
    /// span rather than indexing directly: `self.pos` can run one past
    /// the last real token (e.g. `__halt_compiler()`'s handling
    /// deliberately jumps there), the same real edge case `peek()`/
    /// `advance()` already guard against.
    pub(crate) fn current_pos(&self) -> Position {
        self.spans
            .get(self.pos)
            .or_else(|| self.spans.last())
            .map(|s| s.start)
            .unwrap_or_default()
    }

    /// The end of the last token actually consumed - call right before
    /// returning a `Stmt`/`Expr` to close out its span. Same defensive
    /// fallback as `current_pos` for the same reason.
    pub(crate) fn previous_end(&self) -> Position {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.spans.get(i))
            .or_else(|| self.spans.last())
            .map(|s| s.end)
            .unwrap_or_default()
    }

    /// Builds a `Stmt` spanning from `start` (the position captured before
    /// this statement's first token was consumed) to whatever token was
    /// last consumed.
    pub(crate) fn stmt(&self, kind: StmtKind, start: Position) -> Stmt {
        Stmt {
            kind,
            span: Span {
                start,
                end: self.previous_end(),
            },
        }
    }

    /// Same as `stmt`, for expressions.
    pub(crate) fn expr(&self, kind: ExprKind, start: Position) -> Expr {
        Expr {
            kind,
            span: Span {
                start,
                end: self.previous_end(),
            },
        }
    }

    /// Consumes any doc comment immediately preceding the current position -
    /// call this before advancing past the first token of a declaration.
    pub(crate) fn take_doc_comment(&mut self) -> Option<String> {
        self.doc_comments.remove(&self.pos)
    }

    /// Bumps the expression-recursion depth counter, panicking (a normal,
    /// catchable error) once it passes MAX_EXPR_DEPTH instead of letting
    /// the recursion keep going until it segfaults. Callers must pair this
    /// with a matching decrement once their own recursive call returns -
    /// see the three call sites for the exact pattern.
    pub(crate) fn enter_expr(&mut self) {
        self.expr_depth += 1;
        if self.expr_depth > MAX_EXPR_DEPTH {
            panic!("parse error: expression nested too deeply (over {MAX_EXPR_DEPTH} levels)");
        }
    }

    pub(crate) fn leave_expr(&mut self) {
        self.expr_depth -= 1;
    }

    #[hotpath::measure]
    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        while !self.at_eof() {
            // See parse_statement's own Namespace arm for why the
            // backslash check matters: `namespace\foo();` at the top
            // level is a relative-name call, not a declaration.
            if matches!(self.peek(), Token::Keyword(Keyword::Namespace))
                && !matches!(self.peek_at(1), Token::Backslash)
            {
                stmts.extend(self.parse_namespace_stmt()?);
            } else {
                stmts.push(self.parse_statement()?);
            }
        }
        Ok(stmts)
    }

    /// The older, braced namespace form - `namespace Foo\Bar { ... }`, or
    /// bare `namespace { ... }` for the global namespace - as opposed to
    /// the common `namespace Foo\Bar;` statement form (still handled
    /// generically in parse_statement). Only legal at the top level of a
    /// file, so this is special-cased in parse_program rather than making
    /// every parse_statement caller handle getting more than one Stmt
    /// back. Namespaces aren't modeled beyond the Stmt::Namespace marker,
    /// so the block's body is spliced into the surrounding statement list
    /// instead of nested - that's also what keeps its declarations visible
    /// to Registry::index, which only scans the top-level Vec<Stmt>.
    pub(crate) fn parse_namespace_stmt(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let start = self.current_pos();
        self.advance(); // `namespace`
        let name = self.parse_qualified_name();
        if self.check(&Token::LBrace) {
            let mut stmts = vec![self.stmt(StmtKind::Namespace(name), start)];
            stmts.extend(self.parse_stmt_list_block()?);
            Ok(stmts)
        } else {
            self.expect_semicolon()?;
            Ok(vec![self.stmt(StmtKind::Namespace(name), start)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Vec<Stmt> {
        Parser::new(Lexer::new(src).tokenize_with_spans())
            .parse_program()
            .unwrap()
    }

    #[test]
    fn doc_comment_attaches_to_top_level_function() {
        let program = parse(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double(int $x): int { return $x * 2; }
        "#,
        );
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(f.doc_comment.as_ref().unwrap().contains("@return int"));
    }

    #[test]
    fn doc_comment_attaches_to_method_not_the_previous_one() {
        let program = parse(
            r#"<?php
            class Box {
                /** @return int */
                public function a(): int { return 1; }

                public function b(): int { return 2; }
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert!(c.methods[0].function.doc_comment.is_some());
        assert!(c.methods[1].function.doc_comment.is_none());
    }

    #[test]
    fn doc_comment_attaches_to_property() {
        let program = parse(
            r#"<?php
            class Box {
                /** @var int */
                public $value;
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert!(
            c.properties[0]
                .doc_comment
                .as_ref()
                .unwrap()
                .contains("@var int")
        );
    }

    #[test]
    fn empty_slash_star_star_slash_is_not_treated_as_a_doc_comment() {
        // `/**/` is the classic ambiguous case: it looks like the start of
        // a doc comment but is conventionally just an empty regular one.
        let program = parse(
            r#"<?php
            /**/
            function f(): int { return 1; }
        "#,
        );
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(f.doc_comment.is_none());
    }

    #[test]
    fn regular_comment_is_not_captured_as_a_doc_comment() {
        let program = parse(
            r#"<?php
            // just a regular comment
            function f(): int { return 1; }
        "#,
        );
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(f.doc_comment.is_none());
    }

    #[test]
    fn top_level_const_declaration_parses() {
        let program = parse("<?php const FOO = 1, BAR = 2;");
        let StmtKind::GlobalConst(consts) = &program[0].kind else {
            panic!("expected GlobalConst");
        };
        assert_eq!(consts.len(), 2);
        assert_eq!(consts[0].name, "FOO");
        assert_eq!(consts[1].name, "BAR");
    }

    #[test]
    fn true_and_false_are_valid_return_types() {
        // PHP 8.2+ stand-alone true/false types, as used throughout
        // php-src's own stub files (e.g. `string|false`).
        let program = parse("<?php function f(): string|false { return false; }");
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.return_type.as_deref(), Some("string|false"));
    }

    #[test]
    fn pure_enum_parses_with_unbacked_cases() {
        let program = parse(
            r#"<?php
            enum SortDirection {
                case Ascending;
                case Descending;
            }
        "#,
        );
        let StmtKind::EnumDecl(e) = &program[0].kind else {
            panic!("expected EnumDecl");
        };
        assert_eq!(e.name, "SortDirection");
        assert!(e.backing_type.is_none());
        assert_eq!(e.cases.len(), 2);
        assert_eq!(e.cases[0].name, "Ascending");
        assert!(e.cases[0].value.is_none());
    }

    #[test]
    fn backed_enum_with_interface_and_method_parses() {
        let program = parse(
            r#"<?php
            enum Suit: string implements \JsonSerializable
            {
                case Hearts = 'H';

                public function label(): string
                {
                    return match ($this) {
                        Suit::Hearts => 'Hearts',
                    };
                }
            }
        "#,
        );
        let StmtKind::EnumDecl(e) = &program[0].kind else {
            panic!("expected EnumDecl");
        };
        assert_eq!(e.backing_type.as_deref(), Some("string"));
        assert_eq!(e.implements, vec!["\\JsonSerializable".to_string()]);
        assert!(matches!(
            &e.cases[0].value,
            Some(Expr {
                kind: ExprKind::Literal(_),
                ..
            })
        ));
        assert_eq!(e.methods.len(), 1);
        assert_eq!(e.methods[0].function.name, "label");
    }

    #[test]
    fn word_operators_bind_looser_than_assignment() {
        // The classic PHP gotcha: `$a = $b or $c;` is `($a = $b) or $c;`,
        // not `$a = ($b or $c);` - unlike &&/||.
        let program = parse("<?php $x = 5 or die();");
        let StmtKind::Expression(Expr {
            kind: ExprKind::Binary(BinOp::Or, lhs, _),
            ..
        }) = &program[0].kind
        else {
            panic!("expected a top-level Or binary expression");
        };
        assert!(matches!(&lhs.kind, ExprKind::Assign(..)));
    }

    #[test]
    fn readonly_class_parses() {
        let program = parse("<?php readonly class Point { public int $x; }");
        assert!(matches!(&program[0].kind, StmtKind::ClassDecl(_)));
    }

    #[test]
    fn class_interface_and_trait_declarations_are_tagged_with_their_own_kind() {
        let program = parse("<?php class C {} interface I {} trait T {}");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.kind, ClassKind::Class);
        let StmtKind::ClassDecl(i) = &program[1].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(i.kind, ClassKind::Interface);
        let StmtKind::ClassDecl(t) = &program[2].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(t.kind, ClassKind::Trait);
    }

    #[test]
    fn a_final_method_is_recorded_as_final_and_a_non_final_one_is_not() {
        let program = parse(
            "<?php class C { final public function locked(): void {} public function open(): void {} }",
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let locked = c
            .methods
            .iter()
            .find(|m| m.function.name == "locked")
            .unwrap();
        assert!(locked.is_final);
        let open = c
            .methods
            .iter()
            .find(|m| m.function.name == "open")
            .unwrap();
        assert!(!open.is_final);
    }

    #[test]
    fn a_readonly_property_modifier_is_recorded_per_property() {
        let program = parse("<?php class C { public readonly int $x; public int $y; }");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let x = c.properties.iter().find(|p| p.name == "x").unwrap();
        assert!(x.is_readonly);
        let y = c.properties.iter().find(|p| p.name == "y").unwrap();
        assert!(!y.is_readonly);
    }

    #[test]
    fn readonly_class_shorthand_marks_every_property_readonly() {
        let program = parse("<?php readonly class C { public int $x; public int $y; }");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert!(c.properties.iter().all(|p| p.is_readonly));
    }

    #[test]
    fn a_promoted_constructor_parameter_captures_its_own_readonly_modifier() {
        let program =
            parse("<?php class C { public function __construct(public readonly int $id) {} }");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let ctor = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct")
            .unwrap();
        assert!(ctor.function.params[0].promoted_readonly);
    }

    #[test]
    fn a_trait_as_alias_clause_is_captured_in_the_ast() {
        let program = parse(
            r#"<?php
            class C {
                use A, B {
                    A::hello insteadof B;
                    B::hello as helloB;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.trait_aliases.len(), 1);
        assert_eq!(c.trait_aliases[0].trait_name.as_deref(), Some("B"));
        assert_eq!(c.trait_aliases[0].method, "hello");
        assert_eq!(c.trait_aliases[0].alias, "helloB");
    }

    #[test]
    fn a_bare_as_alias_clause_with_no_trait_qualifier_is_captured() {
        let program = parse(
            r#"<?php
            class C {
                use X {
                    greet as greetAlias;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.trait_aliases.len(), 1);
        assert_eq!(c.trait_aliases[0].trait_name, None);
        assert_eq!(c.trait_aliases[0].method, "greet");
        assert_eq!(c.trait_aliases[0].alias, "greetAlias");
    }

    #[test]
    fn an_as_clause_that_only_changes_visibility_captures_no_alias() {
        let program = parse(
            r#"<?php
            class C {
                use X {
                    greet as protected;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert!(c.trait_aliases.is_empty());
        assert_eq!(c.resolved_trait_conflicts, vec!["greet".to_string()]);
    }

    #[test]
    fn an_insteadof_clause_records_its_method_as_a_resolved_conflict() {
        let program = parse(
            r#"<?php
            class C {
                use A, B {
                    A::hello insteadof B;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.resolved_trait_conflicts, vec!["hello".to_string()]);
    }

    #[test]
    fn an_insteadof_clause_with_a_fully_qualified_trait_name_is_still_parsed() {
        // A trait name before `::` in a trait-adaptation clause can be
        // fully qualified (`\Some\Trait::method insteadof ...`) - found
        // via bulk-running this project's checker against the vendored
        // phpstan-src corpus (nsrt/traits/traits.php uses exactly this
        // shape and previously failed to parse at all).
        let program = parse(
            r#"<?php
            class C {
                use \NsA\A, \NsB\B {
                    \NsA\A::hello insteadof \NsB\B;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.resolved_trait_conflicts, vec!["hello".to_string()]);
    }

    #[test]
    fn a_bare_anonymous_function_statement_parses_as_an_expression_not_a_declaration() {
        // `function () { ... };` with no assignment at all - a real, if
        // unusual in production code, idiom that's extremely common in
        // real PHPStan's own test fixtures (an isolated global-scope body
        // with no named function needed). Found via bulk-running this
        // project's checker against the vendored phpstan-src corpus.
        let program = parse("<?php function () { echo 1; };");
        assert_eq!(program.len(), 1);
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!(
                "expected an Expression statement, got {:?}",
                program[0].kind
            );
        };
        assert!(matches!(e.kind, ExprKind::Closure { .. }));
    }

    #[test]
    fn a_bare_anonymous_by_ref_function_statement_still_parses_as_an_expression() {
        let program = parse("<?php function &() { static $x = 1; return $x; };");
        assert_eq!(program.len(), 1);
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!(
                "expected an Expression statement, got {:?}",
                program[0].kind
            );
        };
        let ExprKind::Closure { by_ref, .. } = &e.kind else {
            panic!("expected a Closure, got {:?}", e.kind);
        };
        assert!(by_ref);
    }

    #[test]
    fn a_named_function_declaration_is_unaffected_by_the_anonymous_statement_lookahead() {
        let program = parse("<?php function foo(int $x): int { return $x; }");
        assert_eq!(program.len(), 1);
        assert!(matches!(program[0].kind, StmtKind::FunctionDecl(_)));
    }

    #[test]
    fn a_named_by_ref_function_declaration_is_unaffected_by_the_anonymous_statement_lookahead() {
        let program = parse("<?php function &bar(): int { static $x = 1; return $x; }");
        assert_eq!(program.len(), 1);
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected a FunctionDecl, got {:?}", program[0].kind);
        };
        assert_eq!(f.name, "bar");
    }

    #[test]
    fn a_class_named_enum_is_accepted_as_a_parameter_type_hint() {
        // `enum` is only reserved as a declaration keyword at statement
        // position - as a type hint it's an ordinary (if unusually named)
        // class reference. Found via bulk-running this project's checker
        // against the vendored phpstan-src corpus (nsrt/bug-4213.php uses
        // exactly this shape).
        let program = parse("<?php function f(Enum ...$enums): void {}");
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected a FunctionDecl, got {:?}", program[0].kind);
        };
        assert_eq!(f.params[0].type_hint.as_deref(), Some("enum"));
        assert!(f.params[0].variadic);
    }

    #[test]
    fn a_spread_argument_sets_spread_on_its_own_arg() {
        let program = parse("<?php foo(...$args);");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::Call(_, args) = &e.kind else {
            panic!("expected Call, got {:?}", e.kind);
        };
        assert_eq!(args.len(), 1);
        assert!(args[0].spread);
    }

    #[test]
    fn a_plain_call_with_no_spread_leaves_spread_false() {
        let program = parse("<?php foo($a, $b);");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::Call(_, args) = &e.kind else {
            panic!("expected Call, got {:?}", e.kind);
        };
        assert_eq!(args.len(), 2);
        assert!(args.iter().all(|a| !a.spread));
    }

    #[test]
    fn a_spread_argument_sets_spread_on_a_new_call() {
        let program = parse("<?php new Foo(...$args);");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::New(_, args) = &e.kind else {
            panic!("expected New, got {:?}", e.kind);
        };
        assert!(args[0].spread);
    }

    #[test]
    fn exponent_assign_parses_as_pow_not_mul() {
        let program = parse("<?php $x **= 3;");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::CompoundAssign(op, ..) = &e.kind else {
            panic!("expected CompoundAssign, got {:?}", e.kind);
        };
        assert_eq!(*op, BinOp::Pow);
    }

    #[test]
    fn a_spread_element_inside_an_array_literal_sets_its_own_spread_flag() {
        let program = parse("<?php $x = [1, ...$other, 3];");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::Assign(_, rhs) = &e.kind else {
            panic!("expected Assign, got {:?}", e.kind);
        };
        let ExprKind::ArrayLiteral(items) = &rhs.kind else {
            panic!("expected ArrayLiteral, got {:?}", rhs.kind);
        };
        assert_eq!(items.len(), 3);
        assert!(!items[0].spread);
        assert!(items[1].spread);
        assert!(!items[2].spread);
    }

    #[test]
    fn qualified_name_with_keyword_segment_parses() {
        // `Enum` here is a namespace segment, not the `enum` declaration
        // keyword - PHP's keywords are mostly "semi-reserved". The main
        // assertion is just that this doesn't panic (it used to). Known,
        // low-impact precision loss: Keyword::as_str() only has the
        // canonical lowercase spelling, not this source's original
        // capitalization, so the reconstructed segment comes back as
        // "enum" - inert for a `use` target specifically, since a bare
        // class-name reference to it (the only thing checker.rs's use-map
        // resolution consumes) is looked up by its final segment's short
        // name, which PHP identifiers treat case-insensitively for this
        // purpose anyway.
        let program = parse("<?php use Piwigo\\Common\\Enum\\AlbumSortOrder;");
        let StmtKind::Use(imports) = &program[0].kind else {
            panic!("expected Use");
        };
        assert_eq!(imports[0].path, "Piwigo\\Common\\enum\\AlbumSortOrder");
    }

    #[test]
    fn keyword_as_method_and_static_member_name_parses() {
        let program = parse(
            r#"<?php
            $x = $obj->match();
            $y = Foo::new();
            $z = Foo::list();
        "#,
        );
        assert_eq!(program.len(), 3);
    }

    #[test]
    fn keyword_as_named_argument_name_parses() {
        let program = parse("<?php f(namespace: 'x', default: 1);");
        assert!(matches!(&program[0].kind, StmtKind::Expression(_)));
    }

    #[test]
    fn first_class_callable_parses_distinctly_from_a_real_call() {
        let program = parse("<?php $f = strlen(...);");
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected an assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::FirstClassCallable(_)));
    }

    #[test]
    fn include_and_require_parse_without_parens() {
        let program = parse("<?php include $file; require_once $other;");
        assert_eq!(program.len(), 2);
    }

    #[test]
    fn static_local_variable_declaration_parses() {
        let program = parse("<?php function f() { static $count = 0; return $count; } ");
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(matches!(&f.body[0].kind, StmtKind::StaticVar(_)));
    }

    #[test]
    fn bitwise_compound_assignment_operators_parse() {
        let program = parse(
            r#"<?php
            function f(int $v) {
                $v |= 1;
                $v &= 2;
                $v ^= 3;
                $v <<= 1;
                $v >>= 1;
            }
        "#,
        );
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.body.len(), 5);
        for stmt in &f.body {
            assert!(matches!(
                &stmt.kind,
                StmtKind::Expression(Expr {
                    kind: ExprKind::CompoundAssign(..),
                    ..
                })
            ));
        }
    }

    #[test]
    fn anonymous_class_parses_with_interface_and_constructor_args() {
        let program = parse(
            r#"<?php
            $x = new readonly class(1) implements Countable {
                public function __construct(private int $n) {}
                public function count(): int { return $this->n; }
            };
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected an assignment");
        };
        let ExprKind::NewAnonymousClass(decl, args) = &rhs.kind else {
            panic!("expected NewAnonymousClass");
        };
        assert_eq!(args.len(), 1);
        assert_eq!(decl.implements, vec!["Countable".to_string()]);
        assert_eq!(decl.methods.len(), 2);
    }

    #[test]
    fn yield_forms_all_parse() {
        let program = parse(
            r#"<?php
            function gen() {
                yield 1;
                yield 'key' => 2;
                yield from [3, 4];
                yield;
            }
        "#,
        );
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.body.len(), 4);

        let StmtKind::Expression(Expr {
            kind: ExprKind::Yield(None, Some(_)),
            ..
        }) = &f.body[0].kind
        else {
            panic!("expected bare-value yield");
        };
        let StmtKind::Expression(Expr {
            kind: ExprKind::Yield(Some(_), Some(_)),
            ..
        }) = &f.body[1].kind
        else {
            panic!("expected key => value yield");
        };
        assert!(matches!(
            &f.body[2].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::YieldFrom(_),
                ..
            })
        ));
        assert!(matches!(
            &f.body[3].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Yield(None, None),
                ..
            })
        ));
    }

    #[test]
    fn asymmetric_visibility_property_parses() {
        let program = parse(
            r#"<?php
            class Foo {
                public private(set) string $name = 'a';
                private(set) int $count = 0;
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties.len(), 2);
        assert_eq!(c.properties[0].name, "name");
        assert_eq!(c.properties[0].visibility, Visibility::Public);
        assert_eq!(c.properties[1].name, "count");
        assert_eq!(c.properties[1].visibility, Visibility::Public);
    }

    #[test]
    fn property_hooks_parse() {
        let program = parse(
            r#"<?php
            class Foo {
                public string $upper {
                    get => strtoupper($this->raw);
                    set(string $value) {
                        $this->raw = strtolower($value);
                    }
                }
                public int $bare {
                    get;
                    set;
                }
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties.len(), 2);

        let hooks = c.properties[0].hooks.as_ref().expect("expected hooks");
        assert!(matches!(hooks.get, Some(PropertyHookBody::Arrow(_))));
        let (param, body) = hooks.set.as_ref().expect("expected set hook");
        assert_eq!(param.as_ref().unwrap().name, "value");
        assert!(matches!(body, PropertyHookBody::Block(_)));

        let bare_hooks = c.properties[1].hooks.as_ref().expect("expected hooks");
        assert!(bare_hooks.get.is_none());
        assert!(bare_hooks.set.is_none());
    }

    #[test]
    fn binary_and_explicit_octal_literals_parse() {
        let program = parse(
            r#"<?php
            $a = 0b101;
            $b = 0o775;
            $c = 0x1F;
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Literal(Literal::Int(5))));
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Literal(Literal::Int(509))));
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[2].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Literal(Literal::Int(31))));
    }

    #[test]
    fn instanceof_with_dynamic_class_name_parses() {
        let program = parse(
            r#"<?php
            assert($repository instanceof $repositoryClass);
            $ok = $x instanceof Foo;
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assert() call");
        };
        let ExprKind::Instanceof(_, target) = &args[0].value.kind else {
            panic!("expected Instanceof");
        };
        assert!(matches!(target, InstanceofTarget::Expr(_)));

        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Instanceof(_, target) = &rhs.kind else {
            panic!("expected Instanceof");
        };
        assert!(matches!(target, InstanceofTarget::Name(n) if n == "Foo"));
    }

    #[test]
    fn not_binds_looser_than_instanceof() {
        // `! $x instanceof Y` means `!($x instanceof Y)` in real PHP -
        // `instanceof` binds tighter than `!`. A real guard clause
        // (`if (! $x instanceof Y) { ... }`) depended on this for
        // narrowing to work at all; getting it backwards silently
        // produced `(!$x) instanceof Y` instead, which never matches the
        // `Unary(Not, Instanceof(..))` shape narrowing looks for.
        let program = parse("<?php $ok = ! $x instanceof Foo;");
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Unary(UnaryOp::Not, inner) = &rhs.kind else {
            panic!("expected the Not to be the outermost node, got: {rhs:?}");
        };
        assert!(matches!(&inner.kind, ExprKind::Instanceof(_, _)));
    }

    #[test]
    fn reference_assignment_parses() {
        let program = parse(
            r#"<?php
            $a = &$b;
            $c =& $d;
        "#,
        );
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Assign(_, _),
                ..
            })
        ));
        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Assign(_, _),
                ..
            })
        ));
    }

    #[test]
    fn destructuring_with_skipped_slot_parses() {
        let program = parse(
            r#"<?php
            [, $extension] = explode('-', $s);
            [$a, , $c] = $t;
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(lhs, _),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::ArrayLiteral(items) = &lhs.kind else {
            panic!("expected ArrayLiteral target");
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(
            &items[0].value.kind,
            ExprKind::Literal(Literal::Null)
        ));
        assert!(matches!(&items[1].value.kind, ExprKind::Variable(_)));

        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(lhs, _),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::ArrayLiteral(items) = &lhs.kind else {
            panic!("expected ArrayLiteral target");
        };
        assert_eq!(items.len(), 3);
        assert!(matches!(
            &items[1].value.kind,
            ExprKind::Literal(Literal::Null)
        ));
    }

    #[test]
    fn moderately_nested_unary_still_parses() {
        let nested = "~".repeat(500);
        parse(&format!("<?php $x = {nested}1;"));
    }

    #[test]
    #[should_panic(expected = "expression nested too deeply")]
    fn pathologically_nested_unary_fails_cleanly_instead_of_overflowing_the_stack() {
        // php-src's Zend/tests/stack_limit/stack_limit_013.phpt is exactly
        // this shape (~7700 levels) - it's designed to test the real
        // engine's own stack-depth guard, and used to take this parser's
        // process down with an uncatchable stack overflow instead of a
        // normal parse error.
        let nested = "~".repeat(8000);
        parse(&format!("<?php $x = {nested}1;"));
    }

    #[test]
    fn braced_namespace_declarations_splice_their_body_in() {
        let program = parse(
            r#"<?php
            namespace Foo\Bar {
                class A {}
                function f() {}
            }
            namespace {
                class B {}
            }
        "#,
        );
        let class_names: Vec<&str> = program
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::ClassDecl(c) => Some(c.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(class_names, vec!["A", "B"]);
        assert!(
            program
                .iter()
                .any(|s| matches!(&s.kind, StmtKind::FunctionDecl(f) if f.name == "f"))
        );
        assert!(matches!(&program[0].kind, StmtKind::Namespace(n) if n == "Foo\\Bar"));
    }

    #[test]
    fn pipe_operator_parses_left_associative_and_at_the_right_precedence() {
        // Precedence checked empirically against a real PHP 8.5 build:
        // tighter than relational, looser than additive/concat.
        let program = parse(r#"<?php $r = 1 |> 'inc' |> 'dbl' < 3;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        // `<` is outermost: (1 |> 'inc' |> 'dbl') < 3
        let ExprKind::Binary(BinOp::Lt, lhs, _) = &rhs.kind else {
            panic!("expected relational at the top");
        };
        // pipe is left-associative: (1 |> 'inc') |> 'dbl'
        let ExprKind::Binary(BinOp::Pipe, inner, _) = &lhs.kind else {
            panic!("expected Pipe");
        };
        assert!(matches!(&inner.kind, ExprKind::Binary(BinOp::Pipe, _, _)));
    }

    #[test]
    fn dynamic_property_and_method_access_parse() {
        let program = parse(
            r#"<?php
            $x = $obj->{'RealVar' . '4'};
            $obj->{$name}('a');
            Foo::{$name}();
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::DynamicPropertyAccess(_, _)));

        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::DynamicMethodCall(_, _, _),
                ..
            })
        ));

        let StmtKind::Expression(Expr {
            kind: ExprKind::StaticCall(class, member, _),
            ..
        }) = &program[2].kind
        else {
            panic!("expected StaticCall");
        };
        assert_eq!(class, "Foo");
        assert_eq!(member, "{dynamic}");
    }

    #[test]
    fn dnf_type_hint_parses() {
        let program = parse(
            r#"<?php
            class Test {
                public (A&B)|X $prop;
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties[0].type_hint.as_deref(), Some("(A&B)|X"));
    }

    #[test]
    fn namespace_relative_name_parses_as_expression_and_as_type() {
        let program = parse(
            r#"<?php
            function f($a = namespace\unknown) {}
            function g(namespace\integer $x) {}
        "#,
        );
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(matches!(
            &f.params[0].default,
            Some(Expr { kind: ExprKind::Name(n), .. }) if n == "namespace\\unknown"
        ));

        let StmtKind::FunctionDecl(g) = &program[1].kind else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(g.params[0].type_hint.as_deref(), Some("namespace\\integer"));
    }

    #[test]
    fn dollar_brace_variable_variable_forms_parse() {
        let program = parse(
            r#"<?php
            ${f()} = g();
            $oa->${f()} = g();
            C::${f()} = g();
        "#,
        );
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Assign(lhs, _), .. }) if matches!(&lhs.kind, ExprKind::DynamicVariable(_))
        ));
        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Assign(lhs, _), .. }) if matches!(&lhs.kind, ExprKind::DynamicPropertyAccess(_, _))
        ));
        assert!(matches!(
            &program[2].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Assign(lhs, _), .. }) if matches!(&lhs.kind, ExprKind::StaticAccess(c, m) if c == "C" && m == "{dynamic}")
        ));
    }

    #[test]
    fn by_reference_property_hook_parses() {
        let program = parse(
            r#"<?php
            class Foo {
                public $prop {
                    &get => $this->_prop;
                }
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let hooks = c.properties[0].hooks.as_ref().expect("expected hooks");
        assert!(matches!(hooks.get, Some(PropertyHookBody::Arrow(_))));
    }

    #[test]
    fn clone_with_overrides_and_classic_forms_both_parse() {
        let program = parse(
            r#"<?php
            $x = clone($a, ['x' => 1]);
            $y = clone $a->b;
            $z = clone($a)->b;
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Clone(_, Some(_))));

        // `clone $a->b` clones the *result* of `$a->b`, not `$a` itself -
        // postfix binds inside clone's operand (verified against a real
        // PHP 8.5 build).
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Clone(inner, None) = &rhs.kind else {
            panic!("expected Clone with no overrides");
        };
        assert!(matches!(&inner.kind, ExprKind::PropertyAccess(..)));

        // Same rule applies even with explicit parens and no comma:
        // `clone($a)->b` clones `$a->b`, not `$a`.
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[2].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Clone(inner, None) = &rhs.kind else {
            panic!("expected Clone with no overrides");
        };
        assert!(matches!(&inner.kind, ExprKind::PropertyAccess(..)));
    }

    #[test]
    fn goto_and_labels_parse() {
        let program = parse(
            r#"<?php
            goto label;
            echo "skipped";
            label:
            echo "here";
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::Goto(n) if n == "label"));
        assert!(matches!(&program[2].kind, StmtKind::Label(n) if n == "label"));
    }

    #[test]
    fn declare_simple_and_block_forms_parse() {
        let program = parse(
            r#"<?php
            declare(strict_types=1);
            declare(ticks=1) {
                echo "in block";
            }
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::Block(stmts) if stmts.is_empty()));
        assert!(matches!(&program[1].kind, StmtKind::Block(stmts) if !stmts.is_empty()));
    }

    #[test]
    fn group_use_with_trailing_comma_parses() {
        parse(
            r#"<?php
            use Foo\{
                const FOO_CONST,
                function foo_func,
                FooClass as BarClass,
            };
        "#,
        );
    }

    #[test]
    fn public_set_and_final_promoted_params_parse() {
        let program = parse(
            r#"<?php
            class Foo {
                public function __construct(
                    public public(set) string $a,
                    final $b,
                ) {}
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let ctor = &c.methods[0].function;
        assert_eq!(ctor.params[0].promoted_visibility, Some(Visibility::Public));
        assert_eq!(ctor.params[1].name, "b");
    }

    #[test]
    fn legacy_var_property_declaration_parses() {
        let program = parse(
            r#"<?php
            class Dnf {
                var A|(B&C) $a;
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties[0].name, "a");
        assert_eq!(c.properties[0].visibility, Visibility::Public);
        assert_eq!(c.properties[0].type_hint.as_deref(), Some("A|(B&C)"));
    }

    #[test]
    fn global_with_dynamic_target_parses() {
        let program = parse(r#"<?php global ${sf("a")};"#);
        assert!(matches!(&program[0].kind, StmtKind::Global(names) if names == &["{dynamic}"]));
    }

    #[test]
    fn use_with_comma_separated_list_parses() {
        let program = parse(r#"<?php use function foo\bar\baz, foo\bar\qux;"#);
        let StmtKind::Use(imports) = &program[0].kind else {
            panic!("expected Use");
        };
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].path, "foo\\bar\\baz");
        assert_eq!(imports[1].path, "foo\\bar\\qux");
    }

    #[test]
    fn use_group_expands_each_member_with_the_shared_prefix() {
        let program = parse(r#"<?php use Foo\Bar\{Baz, Qux as Quux};"#);
        let StmtKind::Use(imports) = &program[0].kind else {
            panic!("expected Use");
        };
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].path, "Foo\\Bar\\Baz");
        assert_eq!(imports[0].alias, None);
        assert_eq!(imports[1].path, "Foo\\Bar\\Qux");
        assert_eq!(imports[1].alias, Some("Quux".to_string()));
    }

    #[test]
    fn hooks_on_promoted_constructor_property_parse() {
        let program = parse(
            r#"<?php
            class Test {
                public function __construct(
                    public $prop {
                        get { echo "get\n"; }
                        set { echo "set($value)\n"; }
                    },
                ) {}
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let ctor = &c.methods[0].function;
        let hooks = ctor.params[0].hooks.as_ref().expect("expected hooks");
        assert!(matches!(hooks.get, Some(PropertyHookBody::Block(_))));
        assert!(hooks.set.is_some());
    }

    #[test]
    fn halt_compiler_stops_parsing_cleanly() {
        let program = parse(
            r#"<?php
            print "yo!\n";
            __HALT_COMPILER();
            none of this should be parsed at all! {{{ ???
        "#,
        );
        assert_eq!(program.len(), 2);
    }

    #[test]
    fn void_and_binary_casts_parse() {
        let program = parse(r#"<?php (void) foo(); $x = (binary) 42;"#);
        assert!(
            matches!(&program[0].kind, StmtKind::Expression(Expr { kind: ExprKind::Cast(ty, _), .. }) if ty == "void")
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Cast(ty, _) if ty == "string"));
    }

    #[test]
    fn match_default_arm_tolerates_trailing_comma() {
        parse(
            r#"<?php
            $x = match (1) {
                default,
                    => "x",
            };
        "#,
        );
    }

    #[test]
    fn readonly_abstract_final_usable_as_plain_names() {
        let program = parse(
            r#"<?php
            function readonly() {}
            readonly();
            abstract();
            final();
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::FunctionDecl(f) if f.name == "readonly"));
        for stmt in &program[1..] {
            assert!(matches!(
                &stmt.kind,
                StmtKind::Expression(Expr {
                    kind: ExprKind::Call(_, _),
                    ..
                })
            ));
        }
    }

    #[test]
    fn readonly_class_still_parses_as_a_class() {
        let program = parse(r#"<?php readonly class Foo {}"#);
        assert!(matches!(&program[0].kind, StmtKind::ClassDecl(c) if c.name == "Foo"));
    }

    #[test]
    fn yield_followed_by_pure_binary_operator_is_bare() {
        let program = parse(r#"<?php function gen() { var_dump(yield * -1); }"#);
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &f.body[0].kind
        else {
            panic!("expected var_dump() call");
        };
        // (yield) * (-1), not yield applied to some malformed operand.
        let ExprKind::Binary(BinOp::Mul, lhs, _) = &args[0].value.kind else {
            panic!("expected a multiplication at the top");
        };
        assert!(matches!(&lhs.kind, ExprKind::Yield(None, None)));
    }

    #[test]
    fn clone_call_form_variants_parse() {
        parse(
            r#"<?php
            $a = clone($x, );
            $b = clone($x, $y, $z, $w, );
            $c = clone(object: $x, withProperties: []);
            $d = clone($x, withProperties: []);
            $e = clone(...["object" => $x]);
        "#,
        );
    }

    #[test]
    fn clone_as_first_class_callable_parses() {
        let program = parse(r#"<?php $f = clone(...);"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::FirstClassCallable(_)));
    }

    #[test]
    fn enum_usable_as_plain_function_and_constant_name() {
        let program = parse(
            r#"<?php
            function enum() { return 1; }
            var_dump(enum());
            var_dump(enum);
            enum Suit { case Hearts; }
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::FunctionDecl(f) if f.name == "enum"));
        assert!(matches!(&program[3].kind, StmtKind::EnumDecl(e) if e.name == "Suit"));
    }

    #[test]
    fn fn_usable_as_namespace_segment_when_not_a_signature() {
        let program = parse(r#"<?php fn\test();"#);
        assert!(
            matches!(&program[0].kind, StmtKind::Expression(Expr { kind: ExprKind::Call(callee, _), .. })
            if matches!(&callee.kind, ExprKind::Name(n) if n == "fn\\test"))
        );
    }

    #[test]
    fn by_reference_arrow_function_still_parses_as_arrow_fn() {
        let program = parse(r#"<?php $id = fn&(&$x) => $x;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::ArrowFn { .. }));
    }

    #[test]
    fn trailing_dot_float_literal_parses() {
        let program = parse(r#"<?php $x = [5., 6.];"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::ArrayLiteral(items) = &rhs.kind else {
            panic!("expected array literal");
        };
        assert!(matches!(&items[0].value.kind, ExprKind::Literal(Literal::Float(f)) if *f == 5.0));
    }

    #[test]
    fn dynamic_class_expr_after_new_and_instanceof_parses() {
        let program = parse(
            r#"<?php
            $class = 'class';
            var_dump(new ('std'.$class));
            var_dump(new ('std'.$class)());
            var_dump($obj instanceof ('std'.$class));
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &program[1].kind
        else {
            panic!("expected var_dump() call");
        };
        let ExprKind::New(class_expr, _) = &args[0].value.kind else {
            panic!("expected New");
        };
        assert!(matches!(&class_expr.kind, ExprKind::Grouping(_)));

        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &program[2].kind
        else {
            panic!("expected var_dump() call");
        };
        let ExprKind::New(_, ctor_args) = &args[0].value.kind else {
            panic!("expected New");
        };
        assert_eq!(ctor_args.len(), 0);
    }

    #[test]
    fn brace_less_double_dollar_variable_variable_parses() {
        let program = parse(r#"<?php global $a, $$b;"#);
        assert!(
            matches!(&program[0].kind, StmtKind::Global(names) if names == &["a", "{dynamic}"])
        );
    }

    #[test]
    fn shell_exec_backtick_string_parses() {
        let program = parse(r#"<?php $x = `echo -n ""`;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Call(callee, _) = &rhs.kind else {
            panic!("expected a call");
        };
        assert!(matches!(&callee.kind, ExprKind::Name(n) if n == "shell_exec"));
    }

    // Finding 11 from the architecture audit - every diagnostic inside a
    // {$...} string-interpolation expression reported the wrong source
    // position (the throwaway sub-parse's own fresh line-1-col-1
    // coordinates, never biased into the real file's coordinates).

    #[test]
    fn complex_interpolation_expression_spans_are_biased_to_real_file_positions() {
        let program = parse("<?php\nfunction f() {\n    return \"value: {$undefinedThing}\";\n}\n");
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected a function declaration, got {:?}", program[0].kind);
        };
        let StmtKind::Return(Some(ret_expr)) = &f.body[0].kind else {
            panic!("expected a return statement, got {:?}", f.body[0].kind);
        };
        let ExprKind::Interpolated(parts) = &ret_expr.kind else {
            panic!("expected an interpolated string, got {:?}", ret_expr.kind);
        };
        let var_part = parts
            .iter()
            .find(|p| matches!(&p.kind, ExprKind::Variable(_)))
            .expect("expected a Variable part for $undefinedThing");
        // Line 3, where the real interpolation is - not line 1 col 1,
        // the throwaway re-lex's own fresh coordinates.
        assert_eq!(var_part.span.start.line, 3, "got: {:?}", var_part.span);
        assert!(var_part.span.start.col > 1, "got: {:?}", var_part.span);
    }

    #[test]
    fn quote_inside_complex_interpolation_does_not_end_the_string() {
        let program = parse(r#"<?php $x = "before {$data["key"]} after";"#);
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Assign(_, _),
                ..
            })
        ));
    }

    #[test]
    fn namespace_relative_call_statement_is_not_a_declaration() {
        let program = parse(r#"<?php namespace\foo();"#);
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Call(callee, _), .. })
                if matches!(&callee.kind, ExprKind::Name(n) if n == "namespace\\foo")
        ));
    }

    #[test]
    fn double_dollar_variable_variable_in_member_position_parses() {
        let program = parse(
            r#"<?php
            $obj->$$name;
            Foo::$$name;
            $obj->$$name();
        "#,
        );
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::DynamicPropertyAccess(_, _),
                ..
            })
        ));
        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr { kind: ExprKind::StaticAccess(c, m), .. }) if c == "Foo" && m == "{dynamic}"
        ));
        assert!(matches!(
            &program[2].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::DynamicMethodCall(_, _, _),
                ..
            })
        ));
    }

    #[test]
    fn array_cast_parses() {
        let program = parse(r#"<?php $tmp = (array)$var;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Cast(ty, _) if ty == "array"));
    }

    // Coverage measurement (cargo llvm-cov) found parse_switch,
    // parse_do_while, parse_if_alt, parse_alt_block, and
    // parse_optional_level all had zero test executions - real,
    // reachable parser functions for common PHP syntax (switch, do-while,
    // the `if (...): ... endif;` alternative syntax used throughout
    // template-heavy code, and `break N;`/`continue N;`) that happened to
    // never come up in any existing test's PHP source. Manually verified
    // via the full pipeline that all five currently produce correct
    // results; these lock that in at the AST level.

    #[test]
    fn switch_with_brace_syntax_parses_cases_default_and_fallthrough() {
        let program = parse(
            r#"<?php
            switch ($x) {
                case 1:
                case 2:
                    doA();
                    break;
                default:
                    doB();
            }
        "#,
        );
        let StmtKind::Switch { cases, .. } = &program[0].kind else {
            panic!("expected Switch");
        };
        assert_eq!(cases.len(), 3);
        // `case 1:` falls through with no statements of its own - they
        // all attach to the following `case 2:` label, matching real
        // PHP's fallthrough semantics.
        assert!(cases[0].body.is_empty());
        assert_eq!(cases[1].body.len(), 2);
        assert!(cases[2].test.is_none(), "expected the default case");
        assert_eq!(cases[2].body.len(), 1);
    }

    #[test]
    fn switch_alt_syntax_parses_the_same_shape_as_brace_syntax() {
        let program = parse(
            r#"<?php
            switch ($x):
                case 1:
                    doA();
            endswitch;
        "#,
        );
        let StmtKind::Switch { cases, .. } = &program[0].kind else {
            panic!("expected Switch");
        };
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].body.len(), 1);
    }

    #[test]
    fn do_while_parses_its_body_and_trailing_condition() {
        let program = parse(
            r#"<?php
            do {
                doSomething();
            } while ($i < 10);
        "#,
        );
        let StmtKind::DoWhile { body, cond } = &program[0].kind else {
            panic!("expected DoWhile");
        };
        assert!(matches!(&body.kind, StmtKind::Block(stmts) if stmts.len() == 1));
        assert!(matches!(&cond.kind, ExprKind::Binary(BinOp::Lt, ..)));
    }

    #[test]
    fn if_alt_syntax_with_elseif_and_else_folds_into_nested_if() {
        let program = parse(
            r#"<?php
            if ($a):
                doA();
            elseif ($b):
                doB();
            else:
                doC();
            endif;
        "#,
        );
        let StmtKind::If {
            then_branch,
            else_branch,
            ..
        } = &program[0].kind
        else {
            panic!("expected If");
        };
        assert!(matches!(&then_branch.kind, StmtKind::Block(s) if s.len() == 1));
        let Some(else_branch) = else_branch else {
            panic!("expected an elseif chain, got no else branch at all");
        };
        // The `elseif` re-dispatches as a nested `if`, exactly like the
        // brace syntax's own elseif handling - one `endif;` still closes
        // the whole chain, not just the innermost `if`.
        let StmtKind::If {
            then_branch: elseif_then,
            else_branch: elseif_else,
            ..
        } = &else_branch.kind
        else {
            panic!("expected a nested If for the elseif branch");
        };
        assert!(matches!(&elseif_then.kind, StmtKind::Block(s) if s.len() == 1));
        assert!(
            matches!(elseif_else.as_deref(), Some(Stmt { kind: StmtKind::Block(s), .. }) if s.len() == 1)
        );
    }

    #[test]
    fn break_and_continue_parse_their_optional_numeric_level() {
        let program = parse(
            r#"<?php
            break;
            break 2;
            continue;
            continue 3;
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::Break(None)));
        assert!(matches!(&program[1].kind, StmtKind::Break(Some(2))));
        assert!(matches!(&program[2].kind, StmtKind::Continue(None)));
        assert!(matches!(&program[3].kind, StmtKind::Continue(Some(3))));
    }

    #[test]
    fn foreach_alt_syntax_parses_its_body_via_parse_alt_block() {
        // parse_if_alt and parse_switch each parse their own alt-syntax
        // body inline - parse_alt_block is the shared path only
        // while/for/foreach's alt syntax (`endwhile;`/`endfor;`/
        // `endforeach;`) goes through, and none of the tests above
        // happen to exercise it.
        let program = parse(
            r#"<?php
            foreach ($items as $item):
                doA();
                doB();
            endforeach;
        "#,
        );
        let StmtKind::Foreach { body, .. } = &program[0].kind else {
            panic!("expected Foreach");
        };
        assert!(matches!(&body.kind, StmtKind::Block(s) if s.len() == 2));
    }
}
