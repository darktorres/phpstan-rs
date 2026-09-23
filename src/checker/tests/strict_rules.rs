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
