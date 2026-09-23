//! Opt-in exception hygiene in `catch` blocks.

use super::helpers::*;

fn ids(src: &str) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, |r| r.check_exception_hygiene = true)
        .into_iter()
        .filter(|d| d.identifier.starts_with("catch."))
        .map(|d| format!("{} {}", d.span.start.line, d.identifier))
        .collect();
    v.sort();
    v
}

#[test]
fn a_wrapping_exception_must_chain_the_caught_one() {
    let found = ids(r#"<?php
final class DomainError extends \RuntimeException {}
final class NoPrevious extends \Exception { public function __construct(string $m) { parent::__construct($m); } }
function f(): void {
    try { g(); }
    catch (\LogicException $e) { throw new DomainError('lost'); }
    try { g(); }
    catch (\LogicException $e) { throw new DomainError('kept', 0, $e); }
    try { g(); }
    catch (\LogicException $e) { throw new NoPrevious('cannot chain'); }
    try { g(); }
    catch (\LogicException $e) { if (h()) { throw new DomainError('nested'); } throw $e; }
}
function g(): void {}
function h(): bool { return true; }
"#);
    assert_eq!(
        found,
        vec!["12 catch.previousNotPassed", "6 catch.previousNotPassed"]
    );
}

#[test]
fn a_base_exception_catch_must_rethrow_unconditionally() {
    let found = ids(r#"<?php
function f(): void {
    try { g(); } catch (\Throwable $e) { log_it($e); }
    try { g(); } catch (\Exception $e) { log_it($e); throw $e; }
    try { g(); } catch (\Exception $e) { if (h()) { throw $e; } }
    try { g(); } catch (\InvalidArgumentException $e) { log_it($e); }
}
function g(): void {}
function h(): bool { return true; }
function log_it(\Throwable $e): void {}
"#);
    assert_eq!(
        found,
        vec!["3 catch.baseWithoutRethrow", "5 catch.baseWithoutRethrow"]
    );
}

#[test]
fn off_by_default() {
    let d =
        diagnostics_for("<?php\nfunction f(): void { try { f(); } catch (\\Throwable $e) {} }\n");
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("catch.base")),
        "{d:?}"
    );
}
