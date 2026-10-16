use std::fs;
use std::path::{Path, PathBuf};
use types::Name;

use phpstan_rs::project::session::{OverlayFile, Session};
use phpstan_rs::project::worker_pool;
use phpstan_rs::registry::stub_set::{StubSource, Stubs};
use phpstan_rs::{output, registry};
use registry::format_signature;

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
    let (path, source) = if let Some(path) = file {
        let source =
            fs::read(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        (path.to_path_buf(), source)
    } else {
        println!("(no file given, parsing built-in sample)\n");
        (PathBuf::from("sample.php"), SAMPLE.as_bytes().to_vec())
    };

    let parsed = syntax::parse(&source).unwrap();
    println!("-- {} ignore marks --", parsed.ignore_marks.len());
    let program = parsed.stmts;

    println!("-- {} top-level statements --", program.len());
    for stmt in &program {
        println!("{:#?}", stmt);
    }

    // The file is an analysed file of a project on the built-in stubs,
    // loaded and checked as the project loads and checks any other.
    let stubs = Stubs::new(StubSource::from_environment(), worker_pool());
    let project = Session::for_overlay(
        vec![OverlayFile {
            path: path.clone(),
            bytes: source.clone(),
            analysed: true,
            stub: false,
        }],
        stubs.clone(),
    )
    .load()
    .unwrap_or_else(|e| panic!("failed to load {}: {e}", path.display()))
    .project;
    let registry = project.registry();

    println!("\n-- resolved signatures (from source) --");
    let mut fn_names: Vec<&Name> = registry.functions.keys().collect();
    fn_names.sort();
    for name in &fn_names {
        println!("{}", format_signature(name, &registry.functions[*name]));
    }

    let mut class_names: Vec<&Name> = registry.classes.keys().collect();
    class_names.sort();
    for class_name in &class_names {
        let info = &registry.classes[*class_name];
        let mut method_names: Vec<&Name> = info.methods().keys().collect();
        method_names.sort();
        for method_name in method_names {
            let qualified = format!("{class_name}::{method_name}");
            println!(
                "{}",
                format_signature(&qualified, &info.methods()[method_name])
            );
        }
    }

    println!("\n-- class hierarchy --");
    for class_name in &class_names {
        let ancestors: Vec<&Name> = class_names
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

    let diagnostics = project.check_file(&path);

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
