mod ast;
mod lexer;
mod parser;
mod token;
mod types;

use std::env;
use std::fs;

use ast::{FunctionDecl, Param, Stmt};
use lexer::Lexer;
use parser::Parser;

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

    println!("\n-- resolved signatures --");
    for stmt in &program {
        print_signatures_in_stmt(stmt, None);
    }
}

fn print_signatures_in_stmt(stmt: &Stmt, class_name: Option<&str>) {
    match stmt {
        Stmt::FunctionDecl(f) => print_signature(class_name, &f.name, f),
        Stmt::ClassDecl(c) => {
            for method in &c.methods {
                print_signature(Some(&c.name), &method.function.name, &method.function);
            }
        }
        Stmt::Namespace(_) | Stmt::Use(_) => {}
        _ => {}
    }
}

fn print_signature(class_name: Option<&str>, name: &str, f: &FunctionDecl) {
    let params: Vec<String> = f.params.iter().map(format_param).collect();
    let return_ty = f
        .return_type
        .as_deref()
        .map(types::parse_type_string)
        .unwrap_or(types::Type::Mixed);

    let qualified = match class_name {
        Some(c) => format!("{c}::{name}"),
        None => name.to_string(),
    };
    println!("{qualified}({}): {return_ty}", params.join(", "));
}

fn format_param(p: &Param) -> String {
    let ty = p
        .type_hint
        .as_deref()
        .map(types::parse_type_string)
        .unwrap_or(types::Type::Mixed);
    let variadic = if p.variadic { "..." } else { "" };
    format!("{ty} {variadic}${}", p.name)
}
