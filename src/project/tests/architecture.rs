//! `Project::check_architecture` with `[[architecture]]` rules.

use std::path::PathBuf;

use crate::architecture::ArchitectureRule;

use super::super::*;

fn rules(toml_text: &str) -> Vec<ArchitectureRule> {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        architecture: Vec<ArchitectureRule>,
    }
    let wrapper: Wrapper = toml::from_str(toml_text).expect("rules parse");
    for rule in &wrapper.architecture {
        rule.validate().expect("rule validates");
    }
    wrapper.architecture
}

fn check(files: &[(&str, &str)], rules_toml: &str) -> Vec<String> {
    let mut project = Project::new();
    for (path, source) in files {
        project
            .load_analyzed_file(PathBuf::from(path), source)
            .unwrap();
    }
    project.finish_loading();
    let mut out: Vec<String> = project
        .check_architecture(&rules(rules_toml))
        .into_iter()
        .flat_map(|(path, diagnostics)| {
            diagnostics.into_iter().map(move |d| {
                format!(
                    "{}:{} {}: {}",
                    path.display(),
                    d.span.start.line,
                    d.identifier,
                    d.message
                )
            })
        })
        .collect();
    out.sort();
    out
}

const LAYERS: &[(&str, &str)] = &[
    (
        "domain.php",
        "<?php\nnamespace App\\Domain;\nuse App\\Infra\\Db;\nfinal class User {\n    public function __construct(private Db $db) {}\n    public function save(): \\App\\Infra\\Mailer { return new \\App\\Infra\\Mailer(); }\n    /** @return list<\\App\\Infra\\Cache> */\n    public function caches(): array { return []; }\n    public function now(): \\DateTimeImmutable { return new \\DateTimeImmutable(); }\n}\n",
    ),
    (
        "infra.php",
        "<?php\nnamespace App\\Infra;\nclass Db {}\nclass Mailer {}\nclass Cache {}\n",
    ),
];

#[test]
fn a_forbidden_dependency_direction_is_reported_at_each_use() {
    let found = check(
        LAYERS,
        r#"
[[architecture]]
name = "domain is infrastructure-free"
classes = [{ namespace = "App\\Domain" }]
should_not_depend_on = [{ namespace = "App\\Infra" }]
"#,
    );
    assert_eq!(
        found,
        vec![
            "domain.php:4 architecture.dependency: Class App\\Domain\\User depends on App\\Infra\\Cache, which architecture rule \"domain is infrastructure-free\" forbids.",
            "domain.php:5 architecture.dependency: Class App\\Domain\\User depends on App\\Infra\\Db, which architecture rule \"domain is infrastructure-free\" forbids.",
            "domain.php:6 architecture.dependency: Class App\\Domain\\User depends on App\\Infra\\Mailer, which architecture rule \"domain is infrastructure-free\" forbids.",
        ]
    );
}

#[test]
fn only_allowed_dependencies_and_constructions_are_checked() {
    let found = check(
        LAYERS,
        r#"
[[architecture]]
classes = [{ namespace = "App\\Domain" }]
can_only_depend_on = [{ classname = "App\\Infra\\Db" }, { classname = "/Cache$/" }]
should_not_construct = [{ namespace = "App\\Infra" }]
"#,
    );
    assert_eq!(
        found,
        vec![
            "domain.php:6 architecture.construction: Class App\\Domain\\User constructs App\\Infra\\Mailer, which architecture rule \"#1\" forbids.",
            "domain.php:6 architecture.dependency: Class App\\Domain\\User depends on App\\Infra\\Mailer, but architecture rule \"#1\" doesn't allow it.",
        ]
    );
}

#[test]
fn shape_relations_are_checked() {
    let files = [(
        "c.php",
        "<?php\nnamespace App\\Http;\ninterface Action {}\ntrait Loggable {}\nabstract class Base {}\n#[\\Attribute]\nfinal class Route {}\nclass HomeController extends Base implements Action {\n    use Loggable;\n    public function index(): void {}\n    public function show(): void {}\n}\n#[Route]\nfinal class AboutController implements Action {\n    public function __invoke(): void {}\n}\n",
    )];
    let found = check(
        &files,
        r#"
[[architecture]]
classes = [{ classname = "/Controller$/" }]
should_be_final = true
should_not_extend = [{ is_abstract = true }]
should_implement = [{ classname = "App\\Http\\Action" }]
should_not_use_trait = [{ is_trait = true }]
should_apply_attribute = "App\\Http\\Route"
should_have_only_one_public_method = true

[[architecture]]
classes = [{ is_attribute = true }]
should_be_named = "/^App\\\\Http\\\\[A-Z]\\w+$/"
excluding = [{ namespace = "App\\Other" }]
"#,
    );
    assert_eq!(
        found,
        vec![
            "c.php:8 architecture.attribute: Class App\\Http\\HomeController should apply attribute App\\Http\\Route (architecture rule \"#1\").",
            "c.php:8 architecture.extends: Class App\\Http\\HomeController should not extend App\\Http\\Base (architecture rule \"#1\").",
            "c.php:8 architecture.final: Class App\\Http\\HomeController should be final (architecture rule \"#1\").",
            "c.php:8 architecture.publicMethods: Class App\\Http\\HomeController should have exactly one public method, but has 2 (architecture rule \"#1\").",
            "c.php:8 architecture.usesTrait: Class App\\Http\\HomeController should not use App\\Http\\Loggable (architecture rule \"#1\").",
        ]
    );
}

#[test]
fn an_invalid_pattern_fails_validation() {
    let rule: ArchitectureRule = toml::from_str("classes = [{ namespace = \"/(/\" }]").unwrap();
    assert!(rule.validate().is_err());
}
