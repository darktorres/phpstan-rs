//! `checker::noop`: NoopRule - a bare expression statement whose result
//! is discarded and which has no side effect.

use super::helpers::*;

#[test]
fn a_bare_xor_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a xor $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "logicalXor.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_boolean_and_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a && $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_boolean_or_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a || $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanOr.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_word_and_statement_is_flagged_under_the_logical_identifier() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a and $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "logicalAnd.resultUnused"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "word spelling must not also report under the symbol identifier: {d:?}"
    );
}

#[test]
fn a_bare_word_or_statement_is_flagged_under_the_logical_identifier() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a or $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "logicalOr.resultUnused"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanOr.resultUnused"),
        "word spelling must not also report under the symbol identifier: {d:?}"
    );
}

#[test]
fn a_bare_ternary_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, int $b, int $c): void {
                $a ? $b : $c;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "ternary.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_comparison_chain_joined_by_and_is_flagged() {
    // The real-world shape this rule mostly exists to catch: a
    // forgotten `if`, leaving a comparison's own result discarded.
    let d = diagnostics_for(
        r#"<?php
            function test(int $a, int $b, int $c, int $d): void {
                ($a === $b) && ($c !== $d);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn guard_or_die_idiom_is_never_flagged() {
    // A legitimate idiom (`checker/stmt.rs` has narrowing support for
    // exactly this shape) - the right side has a real side effect (a
    // method call), so this must never be treated as a no-op, even
    // though its own top-level shape (a bare `||` statement) otherwise
    // looks identical to one.
    let d = diagnostics_for(
        r#"<?php
            class Guard {
                public function die_(string $msg): void {
                    exit($msg);
                }
            }
            function test(bool $cond, Guard $g): void {
                $cond || $g->die_('nope');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanOr.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_a_function_call_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function sideEffect(): bool { echo 'x'; return true; }
            function test(bool $a): void {
                $a && sideEffect();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_instantiation_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(bool $a): void {
                $a || new Foo();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanOr.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_an_assignment_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                ($a = $b) && $a;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_an_increment_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $i, bool $a): void {
                $i++ && $a;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_assignment_of_a_boolean_and_expression_is_never_flagged() {
    // The statement's own top-level shape is Assign, not Binary - never
    // even reaches the match arms that could flag it.
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $x = $a && $b;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

fn noop_messages(src: &str) -> Vec<String> {
    let mut v: Vec<String> = all_diagnostics_for(src)
        .into_iter()
        .filter(|d| d.identifier.ends_with(".resultUnused"))
        .map(|d| format!("{}: {}", d.identifier, d.message))
        .collect();
    v.sort();
    v
}

#[test]
fn a_bare_side_effect_free_expression_is_named() {
    let d = noop_messages(
        r#"<?php
            final class Point { public int $x = 0; }
            function test(int $a, array $list, string $s, Point $p): void {
                $a;
                $a + 1;
                $a === 2;
                $list['key'];
                $s[0];
                $p->x;
                isset($list['k']);
                empty($s);
                [$a, 'b' => $s];
                (int) $s;
                -$a;
                $p instanceof Point;
                PHP_EOL;
                Point::class;
                fn() => 1;
                "x{$a}y";
                $list ?? null;
            }
        "#,
    );
    assert_eq!(
        d,
        vec![
            "expr.resultUnused: Expression \"\"x{$a}y\"\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$a + 1\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$a === 2\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$a\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$list ?? null\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$list['key']\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$p instanceof Point\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$p->x\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"$s[0]\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"(int) $s\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"-$a\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"PHP_EOL\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"Point::class\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"[$a, 'b' => $s]\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"empty($s)\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"fn() => 1\" on a separate line does not do anything.",
            "expr.resultUnused: Expression \"isset($list['k'])\" on a separate line does not do anything.",
        ]
    );
}

#[test]
fn effects_revealed_by_types_are_respected() {
    let d = noop_messages(
        r#"<?php
            class Magic { public function __get(string $n): mixed { return 1; } }
            class Stringy { public function __toString(): string { return ''; } }
            /** @implements ArrayAccess<string, int> */
            abstract class Bag implements ArrayAccess {}
            function test(Magic $m, Stringy $s, Bag $b, mixed $x, object $o): void {
                $m->anything;
                $b['key'];
                (string) $s;
                'a' . $s;
                "{$s}";
                $x->prop;
                $x['k'];
                $o->prop;
            }
        "#,
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn exit_calls_closures_and_writes_are_not_noops() {
    let d = noop_messages(
        r#"<?php
            function test(array $a, int $i, object $o): void {
                exit;
                die('x');
                function () {};
                $i++;
                $a[] = 1;
                $o->p = 1;
                print 'x';
                $$i;
                `ls`;
            }
        "#,
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn word_operators_are_reported_despite_a_local_assignment() {
    let d = noop_messages(
        r#"<?php
            function test(bool $b): void {
                $x = true and $b;
                $y = false or $b;
                $z = $b && true;
                $w = $b ? 1 : 2;
                $p = f() and $b;
            }
            function f(): bool { return true; }
        "#,
    );
    assert_eq!(
        d,
        vec![
            "logicalAnd.resultUnused: Unused result of \"and\" operator.",
            "logicalOr.resultUnused: Unused result of \"or\" operator.",
        ]
    );
}

#[test]
fn word_operators_carry_a_precedence_tip() {
    let d = diagnostics_for("<?php\nfunction t(bool $a, bool $b): void { $a xor $b; }\n");
    let xor = d
        .iter()
        .find(|x| x.identifier == "logicalXor.resultUnused")
        .expect("xor reported");
    assert!(
        xor.tip
            .as_deref()
            .is_some_and(|t| t.contains("unexpected precedence")),
        "{xor:?}"
    );
}

#[test]
fn long_expressions_are_cut_to_their_first_line() {
    let d = noop_messages(
        "<?php\nfunction t(int $a): void { match ($a) { 1 => 'one', default => 'other' }; }\n",
    );
    assert_eq!(
        d,
        vec![
            "expr.resultUnused: Expression \"match ($a) {…\" on a separate line does not do anything."
        ]
    );
}
