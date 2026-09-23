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
