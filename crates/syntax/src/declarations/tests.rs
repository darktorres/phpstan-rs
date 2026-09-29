use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

/// Each declaration in `src` as `(name, conditional)`, in order.
fn declarations(src: &str) -> Vec<(String, bool)> {
    let stmts = Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .expect("parses");
    let mut out = Vec::new();
    for_each_declaration(&stmts, |declaration, _| {
        out.push((declaration.name(), declaration.conditional));
    });
    out
}

fn named(list: &[(&str, bool)]) -> Vec<(String, bool)> {
    list.iter().map(|(n, c)| (n.to_string(), *c)).collect()
}

#[test]
fn braced_namespaces_and_declare_blocks_declare_unconditionally() {
    assert_eq!(
        declarations(
            "<?php namespace A { class C {} function f() {} const K = 1; }
             namespace B { declare(ticks=1) { interface I {} } }"
        ),
        named(&[
            ("A\\C", false),
            ("A\\f", false),
            ("A\\K", false),
            ("B\\I", false)
        ]),
    );
}

#[test]
fn a_declaration_reached_only_by_running_code_is_conditional() {
    assert_eq!(
        declarations(
            "<?php namespace N;
             if (!function_exists('N\\g')) { function g() {} }
             function outer() { class Inner {} }
             try { enum E {} } finally {}
             $f = function () { function h() {} };"
        ),
        named(&[
            ("N\\g", true),
            ("N\\outer", false),
            ("N\\Inner", true),
            ("N\\E", true),
            ("N\\h", true),
        ]),
    );
}

#[test]
fn a_define_call_declares_its_name_as_written() {
    assert_eq!(
        declarations(
            "<?php namespace N;
             define('A\\B', 1);
             \\define('C', 2);
             if (true) { define('D', 3); }
             N\\define('E', 4);
             define($name, 5);"
        ),
        named(&[("A\\B", false), ("C", false), ("D", true)]),
    );
}
