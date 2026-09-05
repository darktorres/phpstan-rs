//! Parsing of expressions and operators: precedence, associativity, casts, closures, and other expression-level constructs.

use super::super::*;
use super::helpers::*;

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

// Phase 85: an assignment nested (unparenthesized) as a comparison
// operator's own RHS. Real PHP's own grammar lets `variable '=' expr`
// appear at this operand position - a real, common guard-clause idiom
// (`if (false === $x = fn()) {...}`), found via a real Piwigo-corpus
// false positive (RedisTrait::createConnection): `$params` never got
// bound to `parse_url()`'s own return type because `false === $params
// = parse_url($params)` used to parse as `(false === $params) =
// parse_url($params)` - a malformed assignment target this project's
// parser silently tolerated - instead of real PHP's own `false ===
// ($params = parse_url($params))`.
#[test]
fn an_assignment_nested_as_an_identical_comparisons_rhs_parses_as_real_php_does() {
    let program = parse("<?php if (false === $params = parse_url($x)) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::Identical, lhs, rhs) = &cond.kind else {
        panic!("expected a top-level Identical binary expression, got {cond:?}");
    };
    assert!(matches!(&lhs.kind, ExprKind::Literal(Literal::Bool(false))));
    let ExprKind::Assign(target, value) = &rhs.kind else {
        panic!("expected the RHS to be an Assign, got {:?}", rhs.kind);
    };
    assert!(matches!(&target.kind, ExprKind::Variable(name) if name == "params"));
    assert!(matches!(&value.kind, ExprKind::Call(..)));
}

#[test]
fn a_compound_assignment_nested_as_a_not_equal_comparisons_rhs_parses_as_real_php_does() {
    let program = parse("<?php if (null !== $total += $x) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::NotIdentical, _, rhs) = &cond.kind else {
        panic!("expected a top-level NotIdentical binary expression, got {cond:?}");
    };
    assert!(matches!(
        &rhs.kind,
        ExprKind::CompoundAssign(BinOp::Add, ..)
    ));
}

#[test]
fn an_assignment_nested_as_a_relational_comparisons_rhs_parses_as_real_php_does() {
    let program = parse("<?php if (0 > $pos = strrpos($x, ':')) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::Gt, _, rhs) = &cond.kind else {
        panic!("expected a top-level Gt binary expression, got {cond:?}");
    };
    assert!(matches!(&rhs.kind, ExprKind::Assign(..)));
}

// A plain comparison with no trailing `=` (the overwhelmingly common
// case) must still parse exactly as before - `parse_assignment_tail`
// only fires when an assignment operator token genuinely follows.
#[test]
fn an_ordinary_comparison_with_no_trailing_assignment_is_unaffected() {
    let program = parse("<?php if (false === $params) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::Identical, _, rhs) = &cond.kind else {
        panic!("expected a top-level Identical binary expression, got {cond:?}");
    };
    assert!(matches!(&rhs.kind, ExprKind::Variable(name) if name == "params"));
}

// Phase 85 follow-up: an assignment nested (unparenthesized) as `&&`'s/
// `||`'s own RHS - the identical real idiom the comparison-operator
// fixes above cover, just one operand position looser in the precedence
// chain. Real PHP's own grammar allows this here too (confirmed directly:
// `php -r '$e=true; if ($e && $x = "hi") { echo $x; }'` prints `hi`, and
// `$x` stays genuinely unset when `$e` is falsy - real short-circuiting,
// not eager evaluation), found via a real corpus false positive
// (symfony/console's own `Application.php`: `$e instanceof
// CommandNotFoundException && $namespace = $this->findNamespace($name)`
// used to parse as `($e instanceof CommandNotFoundException &&
// $namespace) = $this->findNamespace($name)`, leaving `$namespace`
// looking undefined to every reference inside the `if` body).
#[test]
fn an_assignment_nested_as_a_logical_and_rhs_parses_as_real_php_does() {
    let program = parse("<?php if ($e instanceof Foo && $x = find($e)) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::And, lhs, rhs) = &cond.kind else {
        panic!("expected a top-level And binary expression, got {cond:?}");
    };
    assert!(matches!(&lhs.kind, ExprKind::Instanceof(..)));
    let ExprKind::Assign(target, value) = &rhs.kind else {
        panic!("expected the RHS to be an Assign, got {:?}", rhs.kind);
    };
    assert!(matches!(&target.kind, ExprKind::Variable(name) if name == "x"));
    assert!(matches!(&value.kind, ExprKind::Call(..)));
}

#[test]
fn an_assignment_nested_as_a_logical_or_rhs_parses_as_real_php_does() {
    let program = parse("<?php if ($e === null || $x = find($e)) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::Or, _, rhs) = &cond.kind else {
        panic!("expected a top-level Or binary expression, got {cond:?}");
    };
    assert!(matches!(&rhs.kind, ExprKind::Assign(..)));
}

#[test]
fn an_ordinary_logical_and_with_no_trailing_assignment_is_unaffected() {
    let program = parse("<?php if ($e instanceof Foo && $x) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::And, _, rhs) = &cond.kind else {
        panic!("expected a top-level And binary expression, got {cond:?}");
    };
    assert!(matches!(&rhs.kind, ExprKind::Variable(name) if name == "x"));
}

// `!$var = expr` parses as `!($var = expr)`, not `(!$var) = expr` -
// confirmed via `php -r '$x=0; $r=!$x=5; var_dump($r,$x);'` printing
// `bool(false) int(5)`. Corpus-found real idiom: plupload's own
// `upload.php`, `if (!is_dir($targetDir) || !$dir = opendir($targetDir))
// { die(...); }`, left `$dir` looking undefined at every later
// `readdir($dir)` without this - `!`'s own fix has to run at the point
// `!` actually binds (inside `parse_not`, tighter than `&&`/`||`), not
// rely on the enclosing `||`'s own assignment-tail fix: by the time
// that runs, `!` has already wrapped the bare variable, and misassigning
// into `Unary(Not, $var)` as if it were an lvalue wouldn't help either.
#[test]
fn a_bang_negated_assignment_parses_as_negating_the_assignment_result() {
    let program = parse("<?php if (!$dir = opendir($path)) { return; }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Unary(UnaryOp::Not, inner) = &cond.kind else {
        panic!("expected a top-level Not unary expression, got {cond:?}");
    };
    let ExprKind::Assign(target, value) = &inner.kind else {
        panic!(
            "expected the Not's operand to be an Assign, got {:?}",
            inner.kind
        );
    };
    assert!(matches!(&target.kind, ExprKind::Variable(name) if name == "dir"));
    assert!(matches!(&value.kind, ExprKind::Call(..)));
}

#[test]
fn a_bang_negated_assignment_nested_in_a_logical_or_still_assigns() {
    let program = parse("<?php if (!is_dir($path) || !$dir = opendir($path)) { readdir($dir); }");
    let StmtKind::If { cond, .. } = &program[0].kind else {
        panic!("expected an If statement, got {:?}", program[0].kind);
    };
    let ExprKind::Binary(BinOp::Or, _, rhs) = &cond.kind else {
        panic!("expected a top-level Or binary expression, got {cond:?}");
    };
    let ExprKind::Unary(UnaryOp::Not, inner) = &rhs.kind else {
        panic!(
            "expected the Or's RHS to be a Not unary expression, got {:?}",
            rhs.kind
        );
    };
    assert!(matches!(&inner.kind, ExprKind::Assign(..)));
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

// Phase 37: `#[Attr] function () {}` / `#[Attr] static fn() => ...` - the
// only expression-position shape real PHP allows an attribute group on.

#[test]
fn an_attributed_closure_expression_carries_its_own_attrs() {
    let program = parse(r#"<?php $f = #[Pure] function () { return 1; };"#);
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected assignment");
    };
    let ExprKind::Closure { attrs, .. } = &rhs.kind else {
        panic!("expected a Closure, got {:?}", rhs.kind);
    };
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].name, "Pure");
}

#[test]
fn an_attributed_static_arrow_function_carries_its_own_attrs() {
    let program = parse(r#"<?php $f = #[Pure] static fn($x) => $x;"#);
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected assignment");
    };
    let ExprKind::ArrowFn {
        attrs, is_static, ..
    } = &rhs.kind
    else {
        panic!("expected an ArrowFn, got {:?}", rhs.kind);
    };
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].name, "Pure");
    assert!(is_static);
}

#[test]
fn a_plain_closure_with_no_attributes_has_an_empty_attrs_list() {
    let program = parse(r#"<?php $f = function () { return 1; };"#);
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected assignment");
    };
    let ExprKind::Closure { attrs, .. } = &rhs.kind else {
        panic!("expected a Closure, got {:?}", rhs.kind);
    };
    assert!(attrs.is_empty());
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
    assert!(matches!(&program[0].kind, StmtKind::Global(names) if names == &["a", "{dynamic}"]));
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
