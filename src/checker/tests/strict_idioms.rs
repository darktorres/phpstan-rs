//! Opt-in null-coercion and enum-exhaustiveness idioms.

use super::helpers::*;

fn ids(src: &str, null: bool, enums: bool) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, |r| {
        r.check_null_coercion = null;
        r.check_enum_exhaustiveness = enums;
    })
    .into_iter()
    .filter(|d| {
        [
            "isset.object",
            "empty.object",
            "interpolation.nullable",
            "concat.nullable",
            "binaryOp.nullable",
            "assignOp.nullable",
            "match.enumDefault",
            "enum.switch",
            "enum.ifChain",
        ]
        .contains(&d.identifier.as_str())
    })
    .map(|d| format!("{} {}", d.span.start.line, d.identifier))
    .collect();
    v.sort();
    v
}

#[test]
fn null_coercions_and_object_probes() {
    let src = r#"<?php
final class A {}
function f(?string $s, ?int $n, ?A $a, A $b, string $plain): void {
    echo "x{$s}y";
    echo $s . $plain;
    $total = $n + 1;
    $plain .= $s;
    isset($a);
    empty($b);
    echo "{$plain}" . $plain;
    isset($plain);
    $list = [new A()];
    isset($list[0]);
}
"#;
    assert_eq!(
        ids(src, true, false),
        vec![
            "4 interpolation.nullable",
            "5 concat.nullable",
            "6 binaryOp.nullable",
            "7 assignOp.nullable",
            "8 isset.object",
            "9 empty.object",
        ]
    );
    assert!(ids(src, false, false).is_empty());
}

#[test]
fn enum_branching_that_defeats_exhaustiveness() {
    let src = r#"<?php
enum Suit { case Hearts; case Spades; case Clubs; }
function f(Suit $s): string {
    $a = match ($s) { Suit::Hearts => 'h', default => 'other' };
    $b = match ($s) { Suit::Hearts => 'h', Suit::Spades => 's', Suit::Clubs => 'c' };
    switch ($s) { case Suit::Hearts: return 'h'; default: return 'x'; }
}
function g(Suit $s): string {
    if ($s === Suit::Hearts) {
        return 'h';
    } elseif ($s === Suit::Spades) {
        return 's';
    } elseif ($s === Suit::Clubs) {
        return 'c';
    }
    if ($s === Suit::Hearts) { return 'only'; }
    return '';
}
"#;
    assert_eq!(
        ids(src, false, true),
        vec!["4 match.enumDefault", "6 enum.switch", "9 enum.ifChain"]
    );
}
