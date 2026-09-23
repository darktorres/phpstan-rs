//! Opt-in `phpstan-strict-rules` ports.

use super::helpers::*;

fn ids(src: &str, set: impl Fn(&mut crate::registry::Registry)) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, set)
        .into_iter()
        .filter(|d| {
            [
                "constructor.call",
                "staticMethod.dynamicCall",
                "foreach.keyOverwrite",
                "foreach.valueOverwrite",
                "for.variableOverwrite",
                "switch.type",
                "function.strict",
            ]
            .contains(&d.identifier.as_str())
        })
        .map(|d| format!("{} {}", d.span.start.line, d.identifier))
        .collect();
    v.sort();
    v
}

#[test]
fn constructor_calls() {
    let src = r#"<?php
class Base { public function __construct() {} }
class Child extends Base {
    public function __construct() {
        parent::__construct();
        Base::__construct();
        self::__construct();
    }
    public function reset(): void {
        parent::__construct();
        $this->__construct();
    }
}
trait Init {
    public function __construct() { parent::__construct(); }
    public function boot(): void { parent::__construct(); }
    public function other(): void { parent::__construct(); }
}
class User extends Base { use Init { Init::boot as __construct; } }
function f(Child $c): void { $c->__construct(); }
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_illegal_constructor_calls = true;
    assert_eq!(
        ids(src, enabled),
        vec![
            "10 constructor.call",
            "11 constructor.call",
            "17 constructor.call",
            "20 constructor.call",
            "7 constructor.call",
        ]
    );
    assert_eq!(ids(src, |_| {}), Vec::<String>::new());
}

#[test]
fn dynamic_calls_to_static_methods() {
    let src = r#"<?php
class Base { public static function make(): static { return new static(); } public function run(): void {} }
class Child extends Base {
    public function go(?Child $other): void {
        $this->make();
        $this->run();
        static::make();
        $other?->make();
        $f = $this->make(...);
    }
}
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_dynamic_static_calls = true;
    assert_eq!(
        ids(src, enabled),
        vec![
            "5 staticMethod.dynamicCall",
            "8 staticMethod.dynamicCall",
            "9 staticMethod.dynamicCall",
        ]
    );
    let messages: Vec<String> = diagnostics_for_with(src, enabled)
        .into_iter()
        .filter(|d| d.identifier == "staticMethod.dynamicCall")
        .map(|d| d.message)
        .collect();
    assert_eq!(messages[0], "Dynamic call to static method Base::make().");
    assert_eq!(ids(src, |_| {}), Vec::<String>::new());
}

#[test]
fn loop_variables_overwriting_defined_ones() {
    let src = r#"<?php
function f(array $rows, int $i, bool $b): void {
    $row = null;
    foreach ($rows as $key => $row) {}
    foreach ($rows as $k => [$i, $fresh]) {}
    for ($i = 0, $j = 0; $i < 3; $i++) {}
    if ($b) { $maybe = 1; }
    foreach ($rows as $maybe) {}
    foreach ($rows as $b => $value) {}
}
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_loop_variable_overwrite = true;
    assert_eq!(
        ids(src, enabled),
        vec![
            "4 foreach.valueOverwrite",
            "5 foreach.valueOverwrite",
            "6 for.variableOverwrite",
            "9 foreach.keyOverwrite",
        ]
    );
    assert_eq!(ids(src, |_| {}), Vec::<String>::new());
}

#[test]
fn switch_case_types() {
    let src = r#"<?php
function f(int $n, string $s, mixed $m): void {
    switch ($n) {
        case 1: break;
        case 'one': break;
        case null: break;
        default: break;
    }
    switch ($s) { case 'a': case 2: break; }
    switch ($m) { case 'a': case 2: break; }
    switch (true) { case $n > 1: break; }
}
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_switch_case_types = true;
    assert_eq!(
        ids(src, enabled),
        vec!["5 switch.type", "6 switch.type", "9 switch.type"]
    );
    let messages: Vec<String> = diagnostics_for_with(src, enabled)
        .into_iter()
        .filter(|d| d.identifier == "switch.type")
        .map(|d| d.message)
        .collect();
    assert_eq!(
        messages[0],
        "Switch condition type (int) does not match case condition 'one' (string)."
    );
    assert_eq!(ids(src, |_| {}), Vec::<String>::new());
}

#[test]
fn strict_function_calls() {
    let src = r#"<?php
function f(array $a, bool $b, \Generator $g): void {
    in_array(1, $a);
    in_array(1, $a, true);
    in_array(1, $a, $b);
    array_search(1, $a, strict: true);
    array_keys($a);
    array_keys($a, 1);
    base64_decode('x');
    base64_decode('x', true);
    iterator_to_array($g);
    iterator_to_array($g, false);
    iterator_to_array($g, preserve_keys: true);
    in_array(...[1, $a]);
}
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_strict_function_calls = true;
    assert_eq!(
        ids(src, enabled),
        vec![
            "11 function.strict",
            "3 function.strict",
            "5 function.strict",
            "8 function.strict",
            "9 function.strict",
        ]
    );
    let messages: Vec<String> = diagnostics_for_with(src, enabled)
        .into_iter()
        .filter(|d| d.identifier == "function.strict")
        .map(|d| d.message)
        .collect();
    assert_eq!(
        messages[..2],
        [
            "Call to function in_array() requires parameter #3 to be set.",
            "Call to function in_array() requires parameter #3 to be true.",
        ]
    );
    assert_eq!(ids(src, |_| {}), Vec::<String>::new());
}

#[test]
fn loose_haystack_comparisons() {
    let src = r#"<?php
/**
 * @param list<string> $names
 * @param list<int> $ids
 * @param list<'a'|'b'> $letters
 * @param list<numeric-string> $numbers
 * @param list<?int> $maybe
 */
function f(array $names, array $ids, array $letters, array $numbers, array $maybe, int $n, string $s, mixed $m): void {
    in_array($s, $names);
    in_array($n, $ids);
    in_array($n, $letters);
    in_array($n, $names);
    in_array($s, $numbers);
    in_array($n, $maybe);
    in_array($n, $names, true);
    in_array($m, $names);
    array_search(1.5, $ids);
    array_keys($ids, '1');
}
"#;
    let run = |version: Option<(u8, u8)>| {
        let mut v: Vec<String> = diagnostics_for_with(src, |r| {
            r.check_loose_haystack_comparison = true;
            r.php_version = version;
        })
        .into_iter()
        .filter(|d| d.identifier == "function.looseComparison")
        .map(|d| d.span.start.line.to_string())
        .collect();
        v.sort_by_key(|l| l.parse::<u32>().unwrap());
        v
    };
    assert_eq!(run(Some((8, 3))), vec!["13", "14", "15", "18", "19"]);
    assert_eq!(run(Some((7, 4))), vec!["12", "13", "14", "15", "18", "19"]);
}

#[test]
fn arithmetic_operands() {
    let src = r#"<?php
/** @param numeric-string $num */
function f(int $i, float $f, string $s, ?int $maybe, bool $b, array $a, mixed $m, string $num, \GMP $g): void {
    $x = $i + $f;
    $x = $i + $s;
    $x = $maybe * 2;
    $x = $num / 2;
    $x = $a + $a;
    $x = $a + 1;
    $x = $m - 1;
    $x = -$b;
    $s++;
    $s--;
    $maybe++;
    $g++;
    $i **= $s;
    $x = '5' % 2;
}
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_arithmetic_operands = true;
    assert_eq!(
        ids_with(src, enabled, &["NonNumeric", "nonNumeric"]),
        vec![
            "11 unaryMinus.nonNumeric",
            "13 postDec.nonNumeric",
            "14 postInc.nonNumeric",
            "16 pow.rightNonNumeric",
            "5 plus.rightNonNumeric",
            "6 mul.leftNonNumeric",
        ]
    );
    let messages: Vec<String> = diagnostics_for_with(src, enabled)
        .into_iter()
        .filter(|d| d.identifier == "plus.rightNonNumeric")
        .map(|d| d.message)
        .collect();
    assert_eq!(
        messages,
        vec!["Only numeric types are allowed in +, string given on the right side."]
    );
}

fn ids_with(
    src: &str,
    set: impl Fn(&mut crate::registry::Registry),
    suffixes: &[&str],
) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, set)
        .into_iter()
        .filter(|d| suffixes.iter().any(|s| d.identifier.ends_with(s)))
        .map(|d| format!("{} {}", d.span.start.line, d.identifier))
        .collect();
    v.sort();
    v
}

#[test]
fn comparable_types() {
    let src = r#"<?php
function f(int $i, float $f, string $s, ?int $maybe, bool $b, \DateTimeImmutable $d, \DateTime $e, mixed $m): void {
    var_dump($i < $f);
    var_dump($s >= 'a');
    var_dump($d < $e);
    var_dump($i < $s);
    var_dump($maybe > 1);
    var_dump($b <=> true);
    var_dump($m < 1);
    var_dump([$i, $s] < [$f, 'x']);
    var_dump([$i, $s] < [$s, 'x']);
    var_dump($d > 5);
}
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_comparable_types = true;
    assert_eq!(
        ids_with(src, enabled, &["nonComparable", "differentTypes"]),
        vec![
            "11 comparison.differentTypes",
            "12 comparison.differentTypes",
            "6 comparison.differentTypes",
            "7 comparison.nonComparable",
            "8 comparison.nonComparable",
            "9 comparison.nonComparable",
        ]
    );
    let messages: Vec<String> = diagnostics_for_with(src, enabled)
        .into_iter()
        .filter(|d| d.span.start.line == 6 || d.span.start.line == 7)
        .map(|d| d.message)
        .collect();
    assert_eq!(
        messages,
        vec![
            "Cannot compare different types in int < string.",
            "Comparison int|null > int contains non-comparable type, only int|float|string|DateTimeInterface or comparable tuple is allowed.",
        ]
    );
}

#[test]
fn mixed_type_operands() {
    let src = r#"<?php
final class Name { public function __toString(): string { return ''; } }
function f(string $s, ?string $ns, int $i, array $a, Name $n, \stdClass $o, mixed $m): void {
    $x = $s . $i;
    $x = $s . $n;
    $x = $s . $o;
    $x = $s + $i;
    $x = $s == $i;
    $x = $s < $i;
    $x = $s === $i;
    $x = $s . $ns;
    $x = $a + $i;
    $x = $a + $a;
    $x = $s . $m;
    $s .= $i;
    $s += $i;
    $x = '' . $i;
}
"#;
    let enabled = |r: &mut crate::registry::Registry| r.check_mixed_type_operands = true;
    assert_eq!(
        ids_with(src, enabled, &["nonStringOperand", "nonArrayOperand"]),
        vec![
            "12 binaryOp.nonArrayOperand",
            "16 assignOp.nonStringOperand",
            "17 binaryOp.nonStringOperand",
            "6 binaryOp.nonStringOperand",
            "7 binaryOp.nonStringOperand",
            "9 binaryOp.nonStringOperand",
        ]
    );
    let messages: Vec<String> = diagnostics_for_with(src, enabled)
        .into_iter()
        .filter(|d| d.span.start.line == 7 && d.identifier.ends_with("Operand"))
        .map(|d| d.message)
        .collect();
    assert_eq!(
        messages,
        vec!["string (string) in combination with non-string (int) is not allowed."]
    );
}
