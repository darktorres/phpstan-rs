mod ast;
mod lexer;
mod parser;
mod token;

use std::env;
use std::fs;

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
}
