use std::fs;
use std::path::Path;

use checker::Checker;
use lexer::Lexer;
use parser::Parser;
use phpstan_rs::{checker, output, php_stubs, registry};
use registry::{Registry, format_signature};
use syntax::{lexer, parser};

const SAMPLE: &str = r#"<?php

namespace App;

class Greeter
{
    private string $prefix;

    public function __construct(string $prefix = "Hello")
    {
        $this->prefix = $prefix;
    }

    public function greet(string $name): string
    {
        if ($name === "") {
            return $this->prefix . ", stranger!";
        }
        return $this->prefix . ", " . $name . "!";
    }
}

/**
 * @param int[] $numbers
 */
function sum(array $numbers): int
{
    $total = 0;
    foreach ($numbers as $n) {
        $total += $n;
    }
    return $total;
}

$greeter = new Greeter();
$names = ["Ada", "Grace", "Alan"];
foreach ($names as $i => $name) {
    echo $greeter->greet($name), "\n";
}

echo sum([1, 2, 3]) ?? 0;
"#;

pub(crate) fn run_demo(file: Option<&Path>) {
    let source = if let Some(path) = file {
        // See check_chunk's comment on why this is a lossy conversion, not
        // a strict one.
        let bytes =
            fs::read(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        println!("(no file given, parsing built-in sample)\n");
        SAMPLE.to_string()
    };

    let tokens = Lexer::new(&source).tokenize_with_spans();
    println!("-- {} tokens --", tokens.len());

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program().unwrap();

    println!("-- {} top-level statements --", program.len());
    for stmt in &program {
        println!("{:#?}", stmt);
    }

    let mut registry = Registry::build(&program);
    // Generated (real php-src source) takes priority over the hand-curated
    // approximations - merge_stubs only fills in names not already present.
    registry.merge_stubs(php_stubs::generated_stubs());
    registry.merge_stub_classes(php_stubs::generated_stub_classes());
    registry.merge_stub_constants(php_stubs::generated_stub_constants());
    // `merge_stub_classes` can add extends/implements edges after
    // `Registry::build`'s own internal precompute already ran - rebuild
    // `ancestor_names`/member-owner maps once more now that the registry
    // is settled, so the "class hierarchy" section below (`is_subclass_
    // of`) sees every merged-in stub class too.
    registry.precompute_ancestor_names();
    registry.precompute_member_owners();

    println!("\n-- resolved signatures (from source) --");
    let mut fn_names: Vec<&std::sync::Arc<str>> = registry.functions.keys().collect();
    fn_names.sort();
    for name in &fn_names {
        println!("{}", format_signature(name, &registry.functions[*name]));
    }

    let mut class_names: Vec<&std::sync::Arc<str>> = registry.classes.keys().collect();
    class_names.sort();
    for class_name in &class_names {
        let info = &registry.classes[*class_name];
        let mut method_names: Vec<&std::sync::Arc<str>> = info.methods.keys().collect();
        method_names.sort();
        for method_name in method_names {
            let qualified = format!("{class_name}::{method_name}");
            println!(
                "{}",
                format_signature(&qualified, &info.methods[method_name])
            );
        }
    }

    println!("\n-- class hierarchy --");
    for class_name in &class_names {
        let ancestors: Vec<&std::sync::Arc<str>> = class_names
            .iter()
            .filter(|other| *other != class_name && registry.is_subclass_of(class_name, other))
            .copied()
            .collect();
        if ancestors.is_empty() {
            println!("{class_name}: (no declared ancestors in this file)");
        } else {
            let joined: Vec<&str> = ancestors.iter().map(|s| s.as_ref()).collect();
            println!("{class_name} <: {}", joined.join(", "));
        }
    }

    if let Some(sig) = registry.lookup_function("strlen") {
        println!("\n-- built-in stub example --");
        println!("{}", format_signature("strlen", sig));
    }

    let mut checker = Checker::new(&registry);
    checker.check_program(&program);
    let diagnostics = checker.into_diagnostics();

    println!("\n-- diagnostics --");
    if diagnostics.is_empty() {
        println!("(none)");
    } else {
        for d in &diagnostics {
            println!("{}", output::diagnostic_line(d));
            if let Some(tip) = d.tip_text() {
                println!("    💡 {tip}");
            }
        }
    }
}
