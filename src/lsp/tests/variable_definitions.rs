//! Local-variable-declaration navigation: go-to-definition for a variable read
//! resolving back to its defining write/binding site (parameter, assignment,
//! foreach/catch/global binding, byref call arg, array auto-vivification,
//! preg_match population, destructuring target), including control-flow-aware
//! resolution across branches and loops.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

// Phase 17 follow-up: local-variable-declaration navigation - a read
// resolves to its own nearest-preceding declaration in the same scope
// (a parameter, an assignment, a `foreach` binding, or a `catch (...) $e`
// binding), a plain textual-order heuristic, not full control-flow
// analysis (see `Resolved::Variable`'s own doc comment).

#[test]
fn definition_on_a_variable_read_resolves_to_its_own_parameter() {
    let dir = TempDir::new("var_param");
    let src = "<?php function f(int $x) { return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$x) {");
    assert_eq!(result["range"]["start"]["line"], expected["line"]);
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_variable_read_resolves_to_its_nearest_preceding_assignment() {
    let dir = TempDir::new("var_reassign");
    let src = "<?php function f() { $x = 1; $x = 2; return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    // The *second* assignment, not the first.
    let expected = lsp_pos(src, "$x = 2");
    assert_eq!(result["range"]["start"]["line"], expected["line"]);
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_variable_read_resolves_to_its_foreach_binding() {
    let dir = TempDir::new("var_foreach");
    let src =
        "<?php function f(array $items) { foreach ($items as $k => $v) { echo $k; echo $v; } }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let k_pos = lsp_pos(src, "$k;");
    let v_pos = lsp_pos(src, "$v;");

    let messages = run(
        &dir,
        &[
            definition_request(2, &uri, k_pos),
            definition_request(3, &uri, v_pos),
        ],
    );
    let k_result = response_result(&messages, 2);
    let v_result = response_result(&messages, 3);
    let expected_k = lsp_pos(src, "$k =>");
    let expected_v = lsp_pos(src, "$v)");
    assert_eq!(
        k_result["range"]["start"]["character"],
        expected_k["character"]
    );
    assert_eq!(
        v_result["range"]["start"]["character"],
        expected_v["character"]
    );
}

#[test]
fn definition_on_a_variable_read_resolves_to_its_catch_binding() {
    let dir = TempDir::new("var_catch");
    let src = "<?php function f() { try { doStuff(); } catch (\\Exception $e) { echo $e; } }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$e;");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$e) {");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn a_variable_declared_in_one_function_does_not_leak_into_another() {
    let dir = TempDir::new("var_scope");
    let src = "<?php function f() { $x = 1; } function g() { return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2), Value::Null);
}

// Phase 17 follow-up: top-level script-scope navigation. `ProbeScope::
// TopLevel` re-checks the whole file's own top-level statements via
// `Checker::check_program` instead of `check_function` - real top-level
// PHP code is exactly as checkable as a function body.

#[test]
fn definition_on_a_top_level_variable_read_resolves_via_check_program() {
    let dir = TempDir::new("var_top_level");
    let src = "<?php $x = 1; echo $x;";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x;");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$x = 1");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn hover_and_definition_on_a_top_level_instance_call_resolve_via_the_receivers_type() {
    let dir = TempDir::new("top_level_instance");
    let src = "<?php class Widget { public function greet(): string { return 'hi'; } } $w = new Widget(); $w->greet();";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos.clone()),
            definition_request(3, &uri, pos),
        ],
    );
    assert_eq!(
        response_result(&messages, 2)["contents"],
        "Widget::greet(): string"
    );
    let def = response_result(&messages, 3);
    assert_eq!(def["uri"], uri);
}

// Phase 17 follow-up: hover now shows an inferred type for a local
// variable too, not just a class/function/member - both a read and the
// variable's own declaration site, each via the real checker probe
// (`Checker::probe_hit_at`), not a hand-rolled approximation.

#[test]
fn hover_on_a_variable_read_shows_its_inferred_type() {
    let dir = TempDir::new("hover_var_read");
    let src = "<?php function f(int $x) { echo $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2)["contents"], "int");
}

#[test]
fn hover_on_a_closure_use_capture_shows_its_inferred_type() {
    // The capture token itself, inside `use (...)`, is a real reference
    // to the outer scope's own `$x` - previously only wired for go-to-
    // definition's own separate `probe_var_provenance` probe, never for
    // `probe_hit`/hover (`ExprKind::Closure`'s own `uses` loop never
    // called `probe_hit_at` at all).
    let dir = TempDir::new("hover_var_closure_use");
    let src = "<?php function g(int $x) { $f = function () use ($x) { return $x; }; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x) {");

    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2)["contents"], "int");
}

#[test]
fn hover_on_a_parameter_declaration_shows_its_own_type() {
    let dir = TempDir::new("hover_var_param_self");
    let src = "<?php function f(int $x) { echo $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x) {");

    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2)["contents"], "int");
}

#[test]
fn hover_on_a_foreach_bindings_own_declaration_shows_its_element_type() {
    let dir = TempDir::new("hover_var_foreach_self");
    let src = "<?php function f(array $items) { foreach ($items as $v) { echo $v; } }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$v) {");

    let messages = run(&dir, &[hover_request(2, &uri, pos)]);
    assert_eq!(response_result(&messages, 2)["contents"], "mixed");
}

// Phase 17 follow-up: auto-vivification. A by-ref call argument, an
// array index-write, or preg_match's own $matches out-parameter is a
// variable's real first appearance when there's nothing declared for it
// yet - `declare`d there, not left with no provenance at all.

#[test]
fn definition_on_a_read_resolves_to_a_byref_call_argument_that_first_defined_it() {
    let dir = TempDir::new("var_byref_arg");
    let src = "<?php function f(array &$a) {} function g() { f($items); return $items; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$items; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$items);");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_read_after_a_closure_resolves_to_its_own_use_by_ref_capture() {
    let dir = TempDir::new("var_closure_use_byref");
    let src = "<?php function g() { $f = function () use (&$x) { $x = 1; }; $f(); return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$x) {");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

// Phase 17: the capture token itself, inside `use (...)`, is now a real,
// navigable reference to the outer scope's own variable - previously
// unreachable for both hover and go-to-definition (`Walker::expr`'s own
// `ExprKind::Closure` arm never visited `uses` at all).

#[test]
fn definition_on_a_use_capture_by_value_resolves_to_its_outer_assignment() {
    let dir = TempDir::new("var_closure_use_click_byval");
    let src = "<?php function g() { $x = 1; $f = function () use ($x) { return $x; }; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x) {");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$x = 1");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_use_capture_by_ref_resolves_to_its_pre_existing_outer_assignment() {
    let dir = TempDir::new("var_closure_use_click_byref_existing");
    let src = "<?php function g() { $x = 1; $f = function () use (&$x) { $x = 2; }; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x) {");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$x = 1");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_use_capture_by_ref_that_auto_vivifies_resolves_to_itself() {
    let dir = TempDir::new("var_closure_use_click_byref_new");
    let src = "<?php function g() { $f = function () use (&$x) { $x = 1; }; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x) {");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    // No pre-existing outer declaration - real PHP auto-vivifies `$x` in
    // the declaring scope right here, so this capture site is its own
    // definition.
    let expected = lsp_pos(src, "$x) {");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_read_resolves_to_its_own_array_auto_vivification_write() {
    let dir = TempDir::new("var_index_write");
    let src = "<?php function f() { $x['key'] = 1; return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$x['key']");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_read_resolves_to_the_preg_match_call_that_first_defined_matches() {
    let dir = TempDir::new("var_preg_match");
    let src = "<?php function f($s) { preg_match('/x/', $s, $matches); return $matches; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$matches; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$matches);");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_read_after_an_existing_variable_is_widened_by_ref_keeps_its_real_declaration() {
    // Regression guard for the "only when genuinely new" half: an
    // already-declared variable passed by-ref must keep resolving to its
    // own real declaration, not the incidental by-ref call site.
    let dir = TempDir::new("var_byref_existing");
    let src =
        "<?php function f(array &$a) {} function g() { $items = []; f($items); return $items; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$items; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "$items = []");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

// Phase 17 follow-up: branch-aware variable navigation. A read only
// reachable through mutually exclusive arms that assign the name
// differently offers every real candidate (an LSP `Array` of locations),
// rather than silently picking the textually-last one.

#[test]
fn definition_on_a_variable_read_after_if_else_offers_both_branches() {
    let dir = TempDir::new("var_if_else");
    let src = "<?php function f($cond) { if ($cond) { $x = 1; } else { $x = 2; } return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    let locations = result.as_array().expect("expected an array of locations");
    assert_eq!(locations.len(), 2);
}

#[test]
fn definition_on_a_variable_read_after_a_one_sided_if_offers_the_original_too() {
    let dir = TempDir::new("var_if_no_else");
    let src = "<?php function f($cond) { $x = 0; if ($cond) { $x = 1; } return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    // The `if` might not have run at all - the pre-`if` `$x = 0` is a
    // real, live alternative to `$x = 1`, not superseded by it.
    let locations = result.as_array().expect("expected an array of locations");
    assert_eq!(locations.len(), 2);
}

#[test]
fn definition_on_a_variable_read_after_a_guard_clause_excludes_the_terminated_branch() {
    // The real payoff of routing through the checker's own flow-sensitive
    // analysis instead of a hand-rolled approximation: the `else` branch
    // here always `return`s, so real PHPStan-matching termination-pruning
    // (`always_terminates`, `checker/stmt.rs`'s own `If` handling)
    // excludes its own `$x = 2` from ever reaching the merge point at
    // all - only `$x = 1` is a live candidate, a plain `Scalar`, not an
    // over-cautious `Array` offering an alternative that could never
    // actually be reached.
    let dir = TempDir::new("var_guard_clause");
    let src =
        "<?php function f($cond) { if ($cond) { $x = 1; } else { $x = 2; return; } echo $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert!(
        result.is_object(),
        "expected a single Location object (the terminated else branch excluded), got {result:?}"
    );
    let expected = lsp_pos(src, "$x = 1");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_variable_read_after_a_straight_line_reassignment_stays_a_scalar() {
    // Regression guard: sequential, unconditional reassignment (no
    // branch/loop involved at all) must still resolve to exactly one
    // location, not get swept up into the new branch-merge machinery.
    let dir = TempDir::new("var_straight_line");
    let src = "<?php function f() { $x = 1; $x = 2; return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert!(
        result.is_object(),
        "expected a single Location object, got {result:?}"
    );
}

#[test]
fn definition_on_a_variable_read_after_a_while_loop_offers_the_pre_loop_value_too() {
    let dir = TempDir::new("var_loop");
    let src = "<?php function f($cond) { $x = 0; while ($cond) { $x = 1; } return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    // The loop might run zero times - `$x = 0` (before the loop) is a
    // real alternative to the loop body's own `$x = 1`.
    let locations = result.as_array().expect("expected an array of locations");
    assert_eq!(locations.len(), 2);
}

#[test]
fn definition_on_a_variable_read_after_a_foreach_stays_a_scalar() {
    // Real PHPStan-matching, corpus-verified checker behavior (Phase 29,
    // `checker/stmt.rs`): unlike `while`/`for`, a `foreach`'s own body is
    // deliberately *not* merged with the pre-loop state - a common real
    // idiom relies on the loop variable's own post-loop type being
    // trusted as if the loop body definitely ran at least once. This
    // follow-up's own provenance tracking rides along the real checker,
    // so it correctly inherits that same one-candidate answer rather
    // than a hand-rolled heuristic's own (wrong, relative to the real
    // checker) "every loop might run zero times" assumption.
    let dir = TempDir::new("var_foreach_no_merge");
    let src = "<?php function f(array $items) { $x = 0; foreach ($items as $item) { $x = $item; } return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert!(
        result.is_object(),
        "expected a single Location object, got {result:?}"
    );
    let expected = lsp_pos(src, "$x = $item");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn definition_on_a_variable_read_after_a_do_while_stays_a_scalar() {
    // Unlike every other loop shape, `do { ... } while (...);` always
    // runs its body at least once - a read after it should resolve to
    // exactly one location, not offer the pre-loop value as a spurious
    // alternative.
    let dir = TempDir::new("var_do_while");
    let src = "<?php function f($cond) { $x = 0; do { $x = 1; } while ($cond); return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert!(
        result.is_object(),
        "expected a single Location object, got {result:?}"
    );
    let expected = lsp_pos(src, "$x = 1");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

// Phase 17 follow-up: list-destructuring targets are now tracked as
// declarations too.

#[test]
fn definition_on_a_read_resolves_to_its_destructuring_assignment_target() {
    let dir = TempDir::new("var_destructure");
    let src = "<?php function f() { [$a, $b] = [1, 2]; echo $a; echo $b; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let a_pos = lsp_pos(src, "$a;");
    let b_pos = lsp_pos(src, "$b;");

    let messages = run(
        &dir,
        &[
            definition_request(2, &uri, a_pos),
            definition_request(3, &uri, b_pos),
        ],
    );
    let a_result = response_result(&messages, 2);
    let b_result = response_result(&messages, 3);
    let expected_a = lsp_pos(src, "$a, $b");
    let expected_b = lsp_pos(src, "$b] =");
    assert_eq!(
        a_result["range"]["start"]["character"],
        expected_a["character"]
    );
    assert_eq!(
        b_result["range"]["start"]["character"],
        expected_b["character"]
    );
}

#[test]
fn definition_on_a_read_resolves_to_its_foreach_destructuring_target() {
    let dir = TempDir::new("var_destructure_foreach");
    let src = "<?php function f(array $rows) { foreach ($rows as [$a, $b]) { echo $a; } }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$a;");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
}

// Phase 17 follow-up: `global $x;` links back to a top-level declaration.

#[test]
fn definition_on_a_global_variable_read_resolves_to_the_global_statement_itself() {
    // The real `Checker` deliberately never links `global $x;` back to
    // any specific top-level assignment (`checker/stmt.rs`'s own doc
    // comment on `StmtKind::Global`: real PHP's own global scope isn't
    // tracked at all, `Mixed` is the honest answer) - since this
    // follow-up's own provenance tracking rides along the real checker
    // rather than a separate, less-principled heuristic, a read resolves
    // to the `global $x;` statement itself, not a guess at which
    // top-level write "really" produced the value.
    let dir = TempDir::new("var_global");
    let src = "<?php $x = 1; function f() { global $x; return $x; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "$x; }");

    let messages = run(&dir, &[definition_request(2, &uri, pos)]);
    let result = response_result(&messages, 2);
    assert_eq!(result["uri"], uri);
    let expected = lsp_pos(src, "global $x;");
    assert_eq!(result["range"]["start"]["character"], expected["character"]);
}

#[test]
fn the_probe_hook_does_not_perturb_ordinary_diagnostics() {
    // Regression guard that Checker::probe_span/probe_hit (Stage B) is a
    // true no-op outside an active probe: a hover request against an
    // instance-receiver call (which internally builds its own throwaway,
    // probe-armed Checker) followed by a real didOpen on the same file
    // must still publish exactly the diagnostics ordinary checking always
    // produces - proving the probe never leaks into the shared Project's
    // own checking path.
    let dir = TempDir::new("probe_no_perturb");
    let src = "<?php class Widget { public function greet(): string { return 'hi'; } } function useWidget(Widget $w): string { return $w->greet(); } function f(): int { return 'not an int'; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "greet();");

    let messages = run(
        &dir,
        &[
            hover_request(2, &uri, pos),
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": src}}),
            ),
        ],
    );
    let diag_notification = messages
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("expected a publishDiagnostics notification");
    let diagnostics = diag_notification["params"]["diagnostics"]
        .as_array()
        .unwrap();
    assert!(
        diagnostics.iter().any(|d| d["code"] == "return.type"),
        "expected f()'s own real return.type mismatch to still be reported, got: {diagnostics:?}"
    );
}
