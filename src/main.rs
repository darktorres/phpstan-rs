mod ast;
mod lexer;
mod parser;
mod registry;
mod stubs;
mod token;
mod types;

use std::env;
use std::fs;

use lexer::Lexer;
use parser::Parser;
use registry::{FunctionSig, Registry};

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

fn main() {
    let args: Vec<String> = env::args().collect();

    let source = if let Some(path) = args.get(1) {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
    } else {
        println!("(no file given, parsing built-in sample)\n");
        SAMPLE.to_string()
    };

    let tokens = Lexer::new(&source).tokenize();
    println!("-- {} tokens --", tokens.len());

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();

    println!("-- {} top-level statements --", program.len());
    for stmt in &program {
        println!("{:#?}", stmt);
    }

    let mut registry = Registry::build(&program);
    registry.merge_stubs(stubs::builtin_stubs());

    println!("\n-- resolved signatures (from source) --");
    let mut fn_names: Vec<&String> = registry.functions.keys().collect();
    fn_names.sort();
    for name in &fn_names {
        println!("{}", format_signature(name, &registry.functions[*name]));
    }

    let mut class_names: Vec<&String> = registry.classes.keys().collect();
    class_names.sort();
    for class_name in &class_names {
        let info = &registry.classes[*class_name];
        let mut method_names: Vec<&String> = info.methods.keys().collect();
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
        let ancestors: Vec<&String> = class_names
            .iter()
            .filter(|other| *other != class_name && registry.is_subclass_of(class_name, other))
            .copied()
            .collect();
        if ancestors.is_empty() {
            println!("{class_name}: (no declared ancestors in this file)");
        } else {
            let joined: Vec<&str> = ancestors.iter().map(|s| s.as_str()).collect();
            println!("{class_name} <: {}", joined.join(", "));
        }
    }

    if let Some(sig) = registry.lookup_function("strlen") {
        println!("\n-- built-in stub example --");
        println!("{}", format_signature("strlen", sig));
    }
}

fn format_signature(name: &str, sig: &FunctionSig) -> String {
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|(name, ty, variadic)| {
            let dots = if *variadic { "..." } else { "" };
            format!("{ty} {dots}${name}")
        })
        .collect();
    format!("{name}({}): {}", params.join(", "), sig.return_type)
}
