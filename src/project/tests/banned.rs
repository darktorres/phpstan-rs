//! `Project::check_banned` and the `restricted_symbols` presets.

use std::path::PathBuf;

use crate::banned::{BannedConstruct, restricted_symbol_preset};

use super::super::*;

fn check(source: &str, constructs: &[&str], types: &[&str]) -> Vec<String> {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("a.php"), source)
        .unwrap();
    project.finish_loading();
    let constructs: Vec<BannedConstruct> = constructs
        .iter()
        .map(|c| BannedConstruct::parse(c).unwrap())
        .collect();
    let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    let mut out: Vec<String> = project
        .check_banned(&constructs, &types)
        .into_values()
        .flatten()
        .map(|d| format!("{} {}: {}", d.span.start.line, d.identifier, d.message))
        .collect();
    out.sort();
    out
}

#[test]
fn every_banned_construct_is_reported_where_it_appears() {
    let source = "<?php\nfunction f(array &$list, string ...$rest): void {\n    eval('1;');\n    isset($list[0]);\n    empty($list);\n    compact('list');\n    $x = @file('a');\n    switch ($x) { default: break; }\n    $y = $x ?: 1;\n    $z = $x == 1;\n    $list[] = 1;\n    $o = `ls`;\n    $g = $_GET['q'];\n    $v = $$x;\n    $m = $x->$y;\n    $n = &$x;\n    f(list: $list);\n    exit;\n}\n";
    let all: Vec<&str> = BannedConstruct::ALL.iter().map(|(n, _)| *n).collect();
    let found = check(source, &all, &[]);
    let ids: Vec<&str> = found
        .iter()
        .map(|f| f.split(' ').nth(1).unwrap().trim_end_matches(':'))
        .collect();
    for expected in [
        "banned.byReferenceParameters",
        "banned.eval",
        "banned.isset",
        "banned.empty",
        "banned.compact",
        "banned.errorSuppression",
        "banned.switch",
        "banned.shortTernary",
        "banned.looseComparison",
        "banned.arrayAppend",
        "banned.backticks",
        "banned.superglobals",
        "banned.variableVariables",
        "banned.dynamicMemberAccess",
        "banned.assignByReference",
        "banned.namedArguments",
        "banned.exit",
    ] {
        assert!(ids.contains(&expected), "{expected} missing from {found:?}");
    }
    assert!(found.contains(&"7 banned.errorSuppression: The @ error-suppression operator is banned by this project's configuration.".to_string()), "{found:?}");
}

#[test]
fn only_configured_constructs_are_reported() {
    let found = check("<?php\n$a = 1 == 2;\neval('1;');\n", &["eval"], &[]);
    assert_eq!(
        found,
        vec!["3 banned.eval: eval() is banned by this project's configuration."]
    );
}

#[test]
fn a_banned_type_is_found_in_signatures_properties_and_float_literals() {
    let found = check(
        "<?php\nfinal class Money {\n    /** @var float */\n    public $amount = 0;\n    public function __construct(public float $rate = 1.5) {}\n    /** @return list<float> */\n    public function parts(): array { return [(float) '1']; }\n}\n",
        &[],
        &["float"],
    );
    assert_eq!(
        found,
        vec![
            "4 banned.type: Property Money::$amount uses banned type float.",
            "5 banned.type: Float literal 1.5 uses banned type float.",
            "5 banned.type: Parameter $rate of Money::__construct() uses banned type float.",
            "7 banned.type: A (float) cast uses banned type float.",
            "7 banned.type: The return type of Money::parts() uses banned type float.",
        ]
    );
}

#[test]
fn an_unknown_construct_or_preset_is_rejected() {
    assert!(BannedConstruct::parse("gotos").is_err());
    assert!(restricted_symbol_preset("nope").is_err());
    let entries = restricted_symbol_preset("multibyte-strings").unwrap();
    assert_eq!(entries.len(), 12);
    assert!(
        entries
            .iter()
            .any(|e| e.name == "strlen" && e.message.contains("mb_strlen()"))
    );
}
