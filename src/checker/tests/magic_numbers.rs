//! Magic numbers outside constant declarations.

use super::helpers::*;

fn found(src: &str, set: impl Fn(&mut crate::registry::Registry)) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, |r| {
        r.check_magic_numbers = true;
        set(r);
    })
    .into_iter()
    .filter(|d| d.identifier.starts_with("magicNumber."))
    .map(|d| format!("{} {}", d.span.start.line, d.identifier))
    .collect();
    v.sort_by_key(|l| l.split(' ').next().unwrap().parse::<u32>().unwrap());
    v
}

const SRC: &str = r#"<?php
const LIMIT = 10;
define('OTHER', 20);
class Box {
    public const SIZE = 3;
    public int $count = 5;
    public int $zero = 0;
    public function __construct(private int $max = 7) {}
    public function run(int $n, int $flags): int {
        $x = 42;
        $y = $n * 2;
        $z = $flags & 0x10;
        $cmp = $n > 100;
        $t = $n ? 3 : self::SIZE;
        $s = '12';
        $neg = -1;
        $grouped = (5);
        strlen('abc', 8);
        $m = match ($n) { 4 => 'four', default => 'other' };
        switch ($n) { case 9: break; }
        $ok = $n + 1;
        return 60;
    }
}
if (true) {
    class Nested { public int $limit = 99; }
}
"#;

#[test]
fn every_position() {
    assert_eq!(
        found(SRC, |_| {}),
        vec![
            "6 magicNumber.property",
            "8 magicNumber.defaultParameter",
            "10 magicNumber.assignment",
            "11 magicNumber.arithmetic",
            "12 magicNumber.bitwise",
            "13 magicNumber.comparison",
            "14 magicNumber.ternary",
            "15 magicNumber.assignment",
            "16 magicNumber.assignment",
            "17 magicNumber.assignment",
            "18 magicNumber.argument",
            "19 magicNumber.matchArm",
            "20 magicNumber.switchCase",
            "22 magicNumber.return",
            "26 magicNumber.property",
        ]
    );
}

#[test]
fn exemptions_and_numeric_strings() {
    let found = found(SRC, |r| {
        r.magic_number_exemptions = Some(vec![-1.0, 42.0, 60.0]);
        r.magic_numbers_ignore_numeric_strings = true;
    });
    assert!(!found.iter().any(|l| l.starts_with("10 ")
        || l.starts_with("15 ")
        || l.starts_with("16 ")
        || l.starts_with("22 ")));
    assert!(found.contains(&"21 magicNumber.arithmetic".to_string()));
}
