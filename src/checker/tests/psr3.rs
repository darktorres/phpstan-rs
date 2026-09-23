//! PSR-3 logger call hygiene.

use super::helpers::*;

const LOGGER: &str = r#"<?php
namespace Psr\Log {
    interface LoggerInterface {
        public function log($level, string|\Stringable $message, array $context = []): void;
        public function error(string|\Stringable $message, array $context = []): void;
        public function info(string|\Stringable $message, array $context = []): void;
    }
}
"#;

fn found(body: &str, set: impl Fn(&mut crate::registry::Registry)) -> Vec<String> {
    let src = format!("{LOGGER}namespace App {{\nuse Psr\\Log\\LoggerInterface;\n{body}\n}}\n");
    let mut v: Vec<String> = diagnostics_for_with(&src, set)
        .into_iter()
        .filter(|d| d.identifier.starts_with("psr3."))
        .map(|d| format!("{} {}", d.span.start.line - 10, d.identifier))
        .collect();
    v.sort_by_key(|l| l.split(' ').next().unwrap().parse::<i64>().unwrap());
    v
}

#[test]
fn logging_hygiene() {
    let body = r#"enum Kind { case A; }
function f(LoggerInterface $logger, string $name, int $id, Kind $kind, array $data, string $level): void {
    $logger->info('User {name} logged in', ['name' => $name]);
    $logger->info('User {name} logged in');
    $logger->info('User {name} logged in', ['other' => $name]);
    $logger->info("User $name logged in");
    $logger->info('User ' . $id . ' logged in');
    $logger->info('Kind ' . $kind->name);
    $logger->info($name);
    $logger->info('Hello {{name}}', ['name' => $name]);
    $logger->info('Bad {na me}');
    $logger->log('warning', 'ok');
    $logger->log('fatal', 'bad level');
    $logger->log($level, 'dynamic level');
    $logger->info('ctx', ['' => 1, 5 => 2]);
    $logger->error('failed', ['exception' => 'text']);
    $logger->info('Data {data}', ['data' => $data]);
}"#;
    assert_eq!(
        found(body, |r| r.check_psr3_logging = true),
        vec![
            "4 psr3.placeholderMissingContext",
            "5 psr3.placeholderMissingKey",
            "6 psr3.interpolated",
            "6 psr3.messageNotStatic",
            "7 psr3.messageNotStatic",
            "8 psr3.messageNotStatic",
            "9 psr3.messageNotStatic",
            "10 psr3.placeholderDoubleBraces",
            "11 psr3.placeholderInvalidChar",
            "13 psr3.invalidLevel",
            "14 psr3.invalidLevel",
            "15 psr3.contextKeyNotString",
            "15 psr3.contextKeyNotString",
            "16 psr3.contextType",
            "17 psr3.placeholderNotStringable",
        ]
    );
    assert_eq!(found(body, |_| {}), Vec::<String>::new());
}

#[test]
fn exception_context() {
    let body = r#"function maybe_throws(): void {}
function f(LoggerInterface $logger): void {
    try {
        maybe_throws();
    } catch (\RuntimeException $e) {
        $logger->error('failed');
        $logger->error('failed', ['exception' => $e]);
        $logger->error('failed', ['other' => 1]);
    }
    $logger->error('outside');
}"#;
    assert_eq!(
        found(body, |r| r.check_psr3_exception_context = true),
        vec![
            "6 psr3.contextMissingException",
            "8 psr3.contextMissingException"
        ]
    );
}

#[test]
fn context_key_pattern() {
    let body = r#"function f(LoggerInterface $logger): void {
    $logger->info('ctx', ['user_id' => 1, 'userId' => 2]);
}"#;
    assert_eq!(
        found(body, |r| {
            r.check_psr3_logging = true;
            r.psr3_context_key_pattern = Some(regex::Regex::new("^[a-z][a-z0-9_]*$").unwrap());
        }),
        vec!["2 psr3.contextKeyPattern"]
    );
}
