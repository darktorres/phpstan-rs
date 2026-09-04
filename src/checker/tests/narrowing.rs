//! Type narrowing from conditionals, instanceof, is_*, isset, assert, etc.

use super::super::*;
use super::helpers::*;

// Phase 32 Finding K, real shape: `ImageDerivativeController::
// parseRequest`'s `$pos = strrpos($req, '.'); $pos !== false ||
// $this->ierror('Missing .', 400); $ext = substr($req, $pos);` -
// `ierror(): never` is a real PHP `never` return type.
#[test]
fn a_bare_or_statement_with_a_never_returning_right_side_narrows_the_left() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                function ierror(string $msg): never {
                    throw new Exception($msg);
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->ierror('Missing .');
                    return substr($req, $pos);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_or_statement_does_not_narrow_when_the_right_side_isnt_never() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                function regular(): bool {
                    return true;
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->regular();
                    return substr($req, $pos);
                }
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected $pos to stay unnarrowed (still int|false): {d:?}"
    );
}

// Real shape: `Piwigo\Tag\TagService::getTagIds(string|array<string>
// $rawTags, ...)`: `if (! is_array($rawTags)) { $rawTags =
// explode(',', $rawTags); }` - `is_array()`'s registered target is
// generic `Type::Array(Mixed)`; the specific-element `Array(String)`
// member used to survive the exclusion untouched (`Array(String) !=
// Array(Mixed)`), keeping the full `string|array<string>` type going
// into `explode()`'s plain-`string` argument.
#[test]
fn is_array_false_branch_excludes_a_specific_element_array_member_too() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string|array<string> $rawTags */
            function f($rawTags): int {
                if (! is_array($rawTags)) {
                    $rawTags = explode(',', $rawTags);
                }
                return count($rawTags);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real shape: `Piwigo\Section\SectionInitializer::parse()`'s
// `$rewritten = is_string($rewritten) ? $rewritten : '';` then
// `str_replace('//', '/', $rewritten)` - the ternary's own
// `union_types(String, StringLiteral(""))` used to keep both members,
// so `$rewritten` never matched `str_replace`'s subject-narrowing
// `== Type::String` check again.
#[test]
fn str_replace_narrows_a_subject_built_from_a_ternarys_string_and_string_literal_branches() {
    let d = diagnostics_for(
        r#"<?php
            function f($rewritten): string {
                $rewritten = is_string($rewritten) ? $rewritten : '';
                $rewritten = str_replace('//', '/', $rewritten);
                return strtoupper($rewritten);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The false-branch side of Phase 28's `is_scalar` special case was
// outright broken with no diagnostic in this corpus surfacing it:
// `narrow_type_excluding(current, is_scalar's 4-member target)`
// filtered via `*m != excluded`, and no real member is ever
// structurally equal to the whole 4-member union, so `!is_scalar($x)`
// never excluded anything, silently.
#[test]
fn is_scalar_false_branch_actually_excludes_the_scalar_members() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|array $x */
            function f($x): array {
                if (! is_scalar($x)) {
                    return $x;
                }
                return [];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `apply_phpunit_assertion_narrowing` (Phase 27) shares the exact same
// bug as `is_array()` above, found via this finding's own caller
// audit: `assertIsArray($x)` with `$x: array<string>|int` used to
// fall back to the imprecise generic `array` for the same reason.
#[test]
fn assert_is_array_keeps_a_specific_element_type_not_the_generic_array() {
    let d = diagnostics_for(
        r#"<?php
            class T {
                /** @param array<string>|int $x */
                function f($x): array {
                    self::assertIsArray($x);
                    return $x;
                }
                static function assertIsArray($x): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn guard_clause_narrows_nullable_param_after_early_return() {
    // The single most common narrowing idiom in real PHP: without
    // recognizing that the `if` branch terminates, the narrowed-to-null
    // state from that branch would incorrectly leak past the guard.
    let d = diagnostics_for(
        r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    return "stranger";
                }
                return $name;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn guard_clause_narrows_in_else_branch_too() {
    let d = diagnostics_for(
        r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    $name = "stranger";
                } else {
                    // $name narrowed to string here already, this is a no-op
                }
                return $name;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_string_narrowing_catches_misuse_inside_the_guarded_branch() {
    // Before narrowing, an untyped param stays Mixed forever and this
    // bug would have been silently skipped rather than caught.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function process($value) {
                if (is_string($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn is_callable_narrowing_catches_misuse_inside_the_guarded_branch() {
    // Phase 12 item 6: widened narrowing_type_for_predicate's table
    // to is_callable, the one predicate from the roadmap's own list
    // that actually fits this mechanism (a single-arg call narrowing
    // a bare variable to one concrete Type). The others on that list
    // (class_exists/function_exists/in_array/array_key_exists/
    // method_exists/property_exists) are all real PHP predicates but
    // don't fit this table at all - they're either 2-arg calls (this
    // project's narrowing only matches a single-arg predicate call)
    // or don't narrow a *value's type* the way is_string/is_array/etc.
    // do, so they were deliberately not force-fit here.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function process($value) {
                if (is_callable($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsInt"), "got: {d:?}");
}

// Phase 28, real shape: `proc_open()`/`fsockopen()` return
// `resource|false`, and this corpus's test helpers guard every one of
// them with `if (! is_resource($x)) { throw ...; }` before
// `fclose`/`fwrite`/`fread`/`proc_close`/etc. - `is_resource` was
// simply missing from `narrowing_type_for_predicate`'s table.
#[test]
fn is_resource_narrowing_excludes_false() {
    let d = diagnostics_for(
        r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $proc = open();
                if (! is_resource($proc)) {
                    throw new Exception('failed');
                }
                fclose($proc);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_scalar_narrows_to_the_four_scalar_types() {
    // Unlike is_numeric (deliberately not narrowed at all - real
    // PHPStan's own int|float|numeric-string result has no
    // representation here), is_scalar's real set is exactly
    // int|float|string|bool, with no such approximation gap.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function process(mixed $value) {
                if (is_scalar($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsInt"), "got: {d:?}");
}

// Phase 13 Finding B: real shape from
// `SectionPopulator::computeMetaRobots`'s own
// `is_countable($page_tag_ids) and count($page_tag_ids) > 1` -
// `count()` on anything but `array|Countable` is a real PHP 8+
// `TypeError`, and `is_countable` wasn't in
// `narrowing_type_for_predicate` at all.
#[test]
fn is_countable_narrows_a_possibly_null_value_before_count() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                $tags = null;
                if (is_countable($tags) and count($tags) > 1) {
                    return count($tags);
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_countable_false_branch_excludes_array_and_countable() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|array<int> $x */
            function f($x): int {
                if (! is_countable($x)) {
                    return $x;
                }
                return count($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 14 Task 6: narrowing sources for the new refinement types,
// each verified against real PHPStan's own dumpType().
#[test]
fn not_equal_empty_string_narrows_to_non_empty_string() {
    let d = diagnostics_for(
        r#"<?php
            function needsNonEmpty(): void {}
            /** @param non-empty-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if ($x !== '') {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A round of Piwigo-corpus bucketing found `$x !== []` (the array
// equivalent of the string idiom just above) was never handled at
// all - `is_known_single_value` didn't recognize an empty array
// literal as exhaustively single-valued the way it already does a
// literal string/int - verified against real PHPStan's own
// dumpType() for both a plain array and an already-list-typed value.
#[test]
fn not_equal_empty_array_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink(array $x): void {}
            /** @param array<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn not_equal_empty_array_narrows_an_already_typed_list_to_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-list<int> $x */
            function sink(array $x): void {}
            /** @param list<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn not_empty_on_a_string_narrows_to_non_falsy_string_not_merely_non_empty() {
    // empty() is also false for '0' - real PHPStan narrows !empty($x)
    // to non-falsy-string, not just non-empty-string.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn not_empty_on_an_array_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn count_greater_than_zero_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (count($x) > 0) {
                    sink($x);
                }
            }
            function g(array $x): void {
                if (count($x) >= 1) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_numeric_narrows_a_plain_string_to_numeric_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (is_numeric($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_numeric_false_branch_leaves_a_string_unchanged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!is_numeric($x)) {
                    return;
                }
                sink($x);
            }
            function g(string $x): void {
                if (is_numeric($x)) {
                    return;
                }
                sink($x);
            }
        "#,
    );
    // g() must still be flagged: the false branch is plain string,
    // not numeric-string.
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn instanceof_narrows_to_the_checked_class() {
    let d = diagnostics_for(
        r#"<?php
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if ($animal instanceof Cat) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_narrows_to_the_checked_class_in_a_namespaced_file() {
    // The bare-name twin above passes trivially even if namespace
    // resolution were completely broken, since an empty namespace
    // makes FQN == bare name. This is the version that actually
    // exercises it: `useCat(Cat $c)`'s param type is resolved to an
    // FQN at *registry-build* time (resolve_names_in_type), while the
    // `instanceof Cat` narrowing resolves it separately at *check*
    // time (name_ctx.resolve) - both need to land on the exact same
    // string for is_assignable's `a == b` class-name comparison to
    // ever succeed.
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if ($animal instanceof Cat) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A round of Piwigo-corpus bucketing found instanceof's own true
// branch unconditionally replaced the checked variable's type with
// just the target class/interface, discarding whatever it was
// already known to be - correct for the common down-cast (a known
// parent narrowed to a known child) or nullable-narrowing (?Foo
// narrowed to Foo) shape, but wrong when the target is an unrelated
// *sibling* interface: real Piwigo code (Template\Renderer::render)
// takes a single `View $view` parameter through three independent,
// sequential `if ($view instanceof SomeOtherInterface)` checks -
// each one was discarding the fact that `$view` is still a `View`,
// so a later use of `$view` as a plain `View` (unconditionally,
// after all three checks) saw a nonsensical accumulated union of
// bare, unrelated interfaces instead.
#[test]
fn instanceof_against_an_unrelated_interface_keeps_the_original_type_too() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            interface HasPageAssets {}
            function needsView(View $v): void {}
            function f(View $view): void {
                if ($view instanceof HasPageAssets) {
                    // no-op
                }
                needsView($view);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_against_an_unrelated_interface_also_narrows_to_the_new_interface() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            interface HasPageAssets { public function pageAssets(): array; }
            function f(View $view): void {
                if ($view instanceof HasPageAssets) {
                    $view->pageAssets();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Regression test for a real bug caught while implementing the fix
// above: when the pre-narrowing type is something that could never
// actually be `instanceof` anything at all (`Null`, a scalar, ... -
// itself a sign of a real, separate upstream inference gap, not
// narrowed here), building a real Intersection out of it produces
// nonsense like `null&ImageId` - worse than the old, simpler
// behavior. Falls back to trusting the checked class alone whenever
// the pre-narrowing type isn't a plain, single already-known class/
// interface.
#[test]
fn instanceof_true_on_a_provably_non_object_type_does_not_build_a_nonsense_intersection() {
    let d = diagnostics_for(
        r#"<?php
            class Widget15 { public int $value = 0; }
            /** @param null $x */
            function f($x): int {
                if ($x instanceof Widget15) {
                    return $x->value;
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real shape: Template\Renderer::render's own three independent,
// sequential instanceof checks on the same View $view parameter -
// after the first check's own if/no-else merge turns $view into a
// real Union of object-like branches, a *second* instanceof check
// against an unrelated interface must still combine correctly, not
// fall back to bare-trust-the-target the moment current stops being
// a single plain Object.
#[test]
fn a_second_independent_instanceof_check_still_combines_after_the_first_ones_own_merge() {
    let d = diagnostics_for(
        r#"<?php
            interface View2 {}
            interface HasPageAssets2 {}
            interface ExposesPageData2 { public function exposedPageData(): array; }
            function needsView(View2 $v): void {}
            function f(View2 $view): void {
                if ($view instanceof HasPageAssets2) {
                    // no-op
                }
                if ($view instanceof ExposesPageData2) {
                    $view->exposedPageData();
                }
                needsView($view);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_narrowing_down_a_class_hierarchy_still_replaces_the_type_entirely() {
    // The common down-cast case must keep working exactly as before -
    // no spurious Cat&Animal intersection when Cat already IS-A
    // Animal.
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal { public function meow(): string { return "meow"; } }
            function f(Animal $animal): string {
                if ($animal instanceof Cat) {
                    return $animal->meow();
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_false_branch_drops_the_checked_class_from_a_union() {
    // Phase 33 Finding 1: BrowserTestHelpers::dbEscape's real shape.
    let d = diagnostics_for(
        r#"<?php
            class Mysqli {}
            class PgConnection {}
            function needsPgConnection(PgConnection $c): void {}
            function dbEscape(Mysqli|PgConnection $db): void {
                if ($db instanceof Mysqli) {
                    return;
                }
                needsPgConnection($db);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_false_branch_of_the_exact_declared_class_is_dead_code() {
    let d = diagnostics_for(
        r#"<?php
            class Cat {}
            function needsInt(int $n): void {}
            function handle(Cat $c): void {
                if (! ($c instanceof Cat)) {
                    needsInt($c);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_false_branch_against_an_unrelated_class_leaves_the_type_untouched() {
    let d = diagnostics_for(
        r#"<?php
            class Cat {}
            class Dog {}
            function needsCat(Cat $c): void {}
            function handle(Cat|Dog $animal): void {
                if ($animal instanceof Dog) {
                    return;
                }
                needsCat($animal);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn in_array_strict_narrows_the_true_branch_to_the_haystacks_literal_union() {
    // Phase 33 Finding 2: CatPermSubmitRequest::fromArray's real
    // shape - $post['status'] ?? null infers Mixed (an unshaped
    // `array`'s own element type; see widen_coalesce_result's own
    // `?? null` no-op case), so this exercises narrow_type_to's
    // non-Union fallback too.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<array-key, mixed> $post
             */
            function fromArray(array $post): string {
                $status_raw = $post['status'] ?? null;
                $status = in_array($status_raw, ['public', 'private'], true) ? $status_raw : '';
                return $status;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real corpus shape (BatchManagerGlobalRequest::fromArrays):
// `$x ?? null` is a real no-op - if `$x` is null the result is null
// either way, and otherwise it's exactly `$x` - so its own type must
// be exactly `$x`'s own (unnarrowed) type, not the literal `Type::Null`
// `widen_coalesce_result` used to trust as the fallback's own type
// whenever the right side was a bare `null`. That silently collapsed
// an unshaped array read (`$post['selection'] ?? null`, `$post: array`
// with no shape, hence `Mixed`) down to provably-null, so a later
// `is_array(...)` check on it - genuinely checking a real `Mixed`
// value - never had a chance to actually narrow anything.
#[test]
fn coalesce_with_a_null_fallback_keeps_the_left_sides_own_type() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<array-key, mixed> $post
             * @param array<mixed> $default
             */
            function f(array $post, array $default): array {
                $selection_raw = $post['selection'] ?? null;
                return is_array($selection_raw) ? $selection_raw : $default;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn in_array_without_the_strict_flag_narrows_nothing() {
    let d = diagnostics_for(
        r#"<?php
            function fromArray(?string $status_raw): string {
                $status = in_array($status_raw, ['public', 'private']) ? $status_raw : '';
                return $status;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("null"), "got: {d:?}");
}

#[test]
fn in_array_strict_with_a_non_literal_haystack_element_narrows_nothing() {
    let d = diagnostics_for(
        r#"<?php
            function fromArray(?string $status_raw, string $other): string {
                $status = in_array($status_raw, [$other, 'public'], true) ? $status_raw : '';
                return $status;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("null"), "got: {d:?}");
}

#[test]
fn array_key_exists_true_branch_keeps_only_shapes_declaring_the_key() {
    let d = diagnostics_for(
        r#"<?php
            function needsBValue(string $b): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (array_key_exists('b', $x)) {
                    needsBValue($x['b']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_exists_false_branch_keeps_only_shapes_lacking_the_key() {
    let d = diagnostics_for(
        r#"<?php
            function needsAValue(int $a): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (! array_key_exists('b', $x)) {
                    needsAValue($x['a']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_exists_on_a_merely_optional_key_narrows_neither_branch() {
    let shape = Type::ArrayShape(vec![("a".to_string(), Type::Int, true)]);
    assert_eq!(
        narrowing::narrow_array_shape_by_key_presence(&shape, "a", true),
        shape
    );
    assert_eq!(
        narrowing::narrow_array_shape_by_key_presence(&shape, "a", false),
        shape
    );
}

#[test]
fn array_key_exists_on_a_plain_array_leaves_the_type_untouched() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<string, int> $x
             */
            function handle(array $x): void {
                if (array_key_exists('a', $x)) {
                    $y = $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_string_narrows_a_variable_subscript_array_index_expression() {
    // Phase 33 Finding 5: Translator::translate's real shape.
    let d = diagnostics_for(
        r#"<?php
            class Translator {
                /** @var array<string, string|array<int, string>> */
                private array $mirror = [];

                public function translate(string $key): string {
                    $val = $key;
                    if (is_string($this->mirror[$key])) {
                        $val = $this->mirror[$key];
                    }
                    return $val;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_the_subscript_variable_invalidates_the_narrowed_entry() {
    // The soundness case this finding exists for. `is_int(...)`'s own
    // non-`Union` narrowing fallback (`narrow_type_to`'s `_ =>
    // target.clone()`) makes the *stale* entry a confidently-wrong
    // `int` rather than the correct, unprovable `mixed` a fresh read
    // would give - `needsString` (not `needsInt`) exposes the
    // difference: a surviving stale `int` would wrongly fail it,
    // while a correctly-purged `mixed` is leniently never flagged at
    // all (this project's own "don't guess" rule for `Mixed`
    // arguments), so the fix is confirmed by the diagnostic
    // *disappearing*, not by one appearing.
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key, string $other): void {
                if (is_int($arr[$key])) {
                    $key = $other;
                    needsString($arr[$key]);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_an_intermediate_property_link_invalidates_a_deeper_narrowed_entry() {
    // Reassigning $foo->bar to a fresh literal that *keeps* key 'k'
    // (just with a different value type) sidesteps the unrelated
    // "array{...} has no key" check (`check_shape_key_access` runs
    // unconditionally, independent of narrowing) while still
    // exercising this fix directly: without it, the stale
    // `int`-narrowed `"foo->bar[k]"` entry would still win over the
    // real, freshly-reassigned `'newstring'`, wrongly reporting no
    // mismatch at all against `needsInt`.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @var array<string, mixed> */
                public array $bar = [];
            }
            function needsInt(int $n): void {}
            function handle(Foo $foo): void {
                if (is_int($foo->bar['k'])) {
                    $foo->bar = ['k' => 'newstring'];
                    needsInt($foo->bar['k']);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("newstring"), "got: {d:?}");
}

#[test]
fn a_by_ref_call_argument_invalidates_a_stale_narrowed_index_entry() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<string, mixed> $arr
             */
            function widenIt(array &$arr): void {}
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                if (is_int($arr['k'])) {
                    widenIt($arr);
                    needsString($arr['k']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_an_unrelated_variable_does_not_purge_narrowing() {
    // The mirror-image control for the three tests above: since a
    // correctly-purged `mixed` is leniently never flagged, "no
    // diagnostic" alone wouldn't prove anything was preserved -
    // asserting the mismatch *still fires* is what proves the real
    // `int` narrowing survived the unrelated assignment intact.
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key): void {
                if (is_int($arr[$key])) {
                    $unrelated = 5;
                    needsString($arr[$key]);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int"), "got: {d:?}");
}

#[test]
fn or_of_same_key_predicates_narrows_the_true_branch_to_their_union() {
    // Phase 34 Finding B: CalendarMonthly::buildMonthCalendar's real
    // shape - $year infers Type::Null before this fix
    // (widen_coalesce_result's own fallback for a Mixed left side).
    let d = diagnostics_for(
        r#"<?php
            function needsIntOrString(int|string $x): void {}
            /**
             * @param array<string, mixed> $chronology_date
             */
            function handle(array $chronology_date): void {
                $year = $chronology_date['year'] ?? null;
                $year = is_int($year) || is_string($year) ? $year : 0;
                needsIntOrString($year);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_or_of_instanceof_checks_on_the_same_variable_narrows_to_their_union() {
    // Phase 51's own cast-validity check surfaced this real gap
    // (latte/latte's own PrintContext::objectProperty/memberAsString,
    // `$node instanceof A || $node instanceof B ? (string) $node :
    // ...`) - or_chain_narrowing_target had no leaf for `instanceof` at
    // all, so this fell all the way through to "no narrowing," and the
    // cast was checked against the un-narrowed base class instead of
    // the two Stringable subclasses proven by the condition.
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Named extends Base { public function __toString(): string { return "n"; } }
            class Ident extends Base { public function __toString(): string { return "i"; } }
            function test(Base $node): string {
                return $node instanceof Named || $node instanceof Ident
                    ? (string) $node
                    : "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_three_way_or_of_same_key_predicates_still_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function needsIntOrStringOrBool(int|string|bool $x): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                $x = $arr['x'] ?? null;
                $x = is_int($x) || is_string($x) || is_bool($x) ? $x : 0;
                needsIntOrStringOrBool($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn or_of_predicates_on_different_keys_narrows_neither() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function handle(?int $x, ?string $y): void {
                $x = is_int($x) || is_string($y) ? $x : 0;
                needsString($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn or_of_and_of_negated_predicates_on_different_keys_narrows_each_independently() {
    // Real shape: CommentRepository::findAllWithConditions/findList (7
    // sites) - previously undiagnosed as needing "its own sizable
    // narrowing-engine feature" (multi-variable compound-boolean
    // narrowing). Each `||` leaf is `!isA($v) && !isB($v)` for a
    // *different* $v - De Morgan on the outer `||`'s false branch
    // (already handled) decomposes to one independent `&&`-false-branch
    // narrowing per leaf (newly handled), each touching only its own
    // key, so no cross-key reasoning is actually needed here at all.
    let d = diagnostics_for(
        r#"<?php
            function wantsThree(int|string $a, int|string $b, bool|int $c): void {}
            function handle(mixed $commentId, mixed $imageId, mixed $validated): void {
                if ((! is_int($commentId) && ! is_string($commentId))
                    || (! is_int($imageId) && ! is_string($imageId))
                    || (! is_bool($validated) && ! is_int($validated))) {
                    return;
                }
                wantsThree($commentId, $imageId, $validated);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_instanceof_narrows_for_the_rest_of_the_block() {
    // Real shape: vendor/pestphp/pest-plugin-mutate's own
    // AbstractFunctionCallUnwrapMutator::mutate() does exactly this -
    // `assert($node instanceof FuncCall); $node->args` - real PHPStan
    // documents support for narrowing on assert() the same as `if`.
    let d = diagnostics_for(
        r#"<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            function mutate(Node $node) {
                assert($node instanceof FuncCall);
                return $node->args;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_without_a_matching_narrowing_shape_is_a_harmless_no_op() {
    // apply_narrowing already no-ops on condition shapes it doesn't
    // understand - this just confirms assert()'s own new call site
    // doesn't crash or misbehave for one (a plain boolean variable).
    let d = diagnostics_for(
        r#"<?php
            function check(bool $ok) {
                assert($ok);
                return $ok;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 27, real shape: real PHPUnit tests throughout this corpus
// narrow via assertion calls instead of an `if`-guard - `$output =
// ob_get_clean(); self::assertIsString($output);
// self::assertStringContainsString(...);` - real PHPStan documents
// support for exactly this via its own `phpstan/phpstan-phpunit`
// extension, which this project doesn't depend on, so it's
// replicated directly. Matches the same "statement narrows
// unconditionally" shape `assert($cond)` already gets above.
#[test]
fn phpunit_assert_is_string_narrows_for_the_rest_of_the_block() {
    let d = diagnostics_for(
        r#"<?php
            class TestCase {
                public static function assertIsString($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $output = fetch();
                    self::assertIsString($output);
                    return $output;
                }
            }
            /** @return string|false */
            function fetch() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpunit_assert_not_false_narrows_a_resource_for_the_rest_of_the_block() {
    let d = diagnostics_for(
        r#"<?php
            class TestCase {
                public static function assertNotFalse($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): void {
                    $handle = open();
                    self::assertNotFalse($handle);
                    fclose($handle);
                }
            }
            /** @return resource|false */
            function open() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpunit_assert_not_null_narrows_via_an_instance_method_call_too() {
    // Same idiom via `$this->assertNotNull(...)` (PHPUnit's methods
    // are static, but PHP allows calling a static method through an
    // instance too, and real test code does this constantly).
    let d = diagnostics_for(
        r#"<?php
            class TestCase {
                public static function assertNotNull($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $value = fetch();
                    $this->assertNotNull($value);
                    return $value;
                }
            }
            /** @return ?string */
            function fetch() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpunit_assert_instance_of_narrows_to_the_expected_class() {
    let d = diagnostics_for(
        r#"<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            class TestCase {
                public static function assertInstanceOf($expected, $actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(Node $node): array {
                    self::assertInstanceOf(FuncCall::class, $node);
                    return $node->args;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn narrowing_a_generated_stub_union_return_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): string { return $s; }
            function f() {
                $encoded = json_encode(["a" => 1]);
                if ($encoded !== false) {
                    return needsString($encoded);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn short_circuit_and_narrows_its_right_operand_and_the_enclosing_if_body() {
    // A real, common AST-visitor idiom (found via a false positive on
    // this project's own tools/ directory once undefined-property
    // detection actually exercised it): narrow a property chain via
    // `instanceof`, then read a property only the narrowed-to
    // subtype has - both within the same `&&` (needs check_expr's
    // short-circuit-aware traversal) and in the enclosing `if`'s body
    // (needs apply_narrowing to compose `&&`'s operands, not just
    // handle a single top-level condition shape).
    let and_operand = diagnostics_for(
        r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                return $arg->value instanceof Variable && $arg->value->name;
            }
        "#,
    );
    assert!(and_operand.is_empty(), "unexpected: {and_operand:?}");

    let if_body = diagnostics_for(
        r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                if ($arg->value instanceof Variable) {
                    echo $arg->value->name;
                    echo $arg->value->totallyMadeUp;
                }
            }
        "#,
    );
    assert!(
        if_body
            .iter()
            .any(|x| x.message.contains("Variable::$totallyMadeUp")),
        "expected the genuinely-undefined property to still be caught: {if_body:?}"
    );
}

#[test]
fn or_short_circuit_narrows_its_right_operand_on_the_false_branch() {
    // `||`'s right side only runs when the left is false - the mirror
    // image of `&&`'s "both true" composition.
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x) {
                return $x === null || strlen($x) > 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_by_ref_out_param_defined_on_the_right_of_and_is_readable_afterward() {
    // Phase 22 Finding 6, real shape: Piwigo\Admin\AdminShell::
    // runDispatch's `if (isset(...) and is_string(...) and (bool)
    // preg_match(..., $matches)) { ... reads $matches ... }` -
    // `$matches` is `preg_match`'s by-ref out-param, defined as a
    // side effect of evaluating the right operand of a chained
    // `and`/`&&`. `check_expr`'s `Binary(And, l, r)` arm used to
    // check `r` against a *clone* of `locals` that got discarded
    // once the expression finished, so this definition never
    // reached the real `locals` the enclosing `if`'s body sees.
    let d = diagnostics_for(
        r#"<?php
            function f(string $subject): void {
                if (is_string($subject) and (bool) preg_match('/(\d+)/', $subject, $matches)) {
                    echo $matches[0];
                    echo $matches[1];
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_narrowing_inside_and_does_not_leak_into_the_else_branch() {
    // The critical regression test for Finding 6's own fix: a naive
    // "merge every newly-inserted key back into locals" (rather than
    // only real variable names) would insert `apply_narrowing`'s own
    // synthetic `"arg->value"` narrowing key into the real `locals`
    // *before* the `if`'s then/else split even runs - contaminating
    // the `else` branch (where the condition was false, so
    // `$arg->value` is NOT known to be a `Variable`) with the
    // narrowed-to type. A test that only covers the by-ref-in-`&&`
    // shape above would still pass under that broken version and
    // would not catch this.
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                if ($arg->value instanceof Variable && $arg->value->name !== '') {
                    echo 'narrowed';
                } else {
                    echo $arg->value->name;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("Base::$name")),
        "expected the else branch to still see $arg->value as Base (undefined $name), got: {d:?}"
    );
}

#[test]
fn is_string_narrows_a_literal_array_index_expression() {
    // Phase 22, real shape: Piwigo\Template\Template::__construct's
    // `if (isset($lang_info['jquery_code']) and is_string($lang_info
    // ['jquery_code']) and ...) { ... str_replace('-', '_',
    // $lang_info['jquery_code']) ... }` - `narrowing_key` (and every
    // `apply_narrowing` call site consuming it) used to require a
    // bare `ExprKind::Variable` directly, so `is_string(...)` never
    // narrowed a literal-key array-index expression the way it
    // already narrowed a property chain.
    let d = diagnostics_for(
        r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if (is_string($info['code'])) {
                    return strtoupper($info['code']);
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Found while implementing/testing Finding F above: `check_expr`'s own
// `Index` arm never consulted the narrowed `locals` state at all -
// unlike `infer_expr`'s `Index` arm, and unlike `check_expr`'s own
// `PropertyAccess` arm just above it in the same match. A narrowed
// array-index expression passed as a *function argument* (checked via
// `infer_expr` for the argument-type comparison, the shape the test
// just above this one exercises) already worked; a bare `return
// $x['k'];` reading the same narrowed expression directly did not.
#[test]
fn is_string_narrows_a_literal_array_index_expression_read_back_via_a_bare_return() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if (is_string($info['code'])) {
                    return $info['code'];
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 32 Finding G, real shape: `PasswordController::resetPassword`'s
// `$user_id = (bool) $reset_password_key_result ? ... :
// $this->resetPasswordCode();` (both branches `false|int`) then `if (!
// is_numeric($user_id)) { return false; }` before `UserId::from
// ($user_id)` (expects plain `int`).
#[test]
fn is_numeric_narrows_the_true_branch_by_excluding_bool_and_null() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|false $x */
            function f($x): int {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The unsoundness a shared-target table entry would have introduced
// (see this arm's own doc comment): `is_numeric`'s true branch must
// *not* drop `String`, since a numeric-looking string still satisfies
// it.
#[test]
fn is_numeric_leaves_string_untouched_in_the_true_branch() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|string $x */
            function f($x): int|string {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_numeric_false_branch_narrows_by_excluding_int_and_float() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|string $x */
            function f($x): string {
                if (! is_numeric($x)) {
                    return $x;
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn equality_narrows_a_literal_array_index_expression() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if ($info['code'] !== null) {
                    return strtoupper($info['code']);
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_string_narrows_an_assignment_nested_in_its_own_argument() {
    // Phase 23, real shape: Piwigo\Admin\Extensions\
    // CoreUpdateService::checkPiwigoUpgrade's
    // `is_string($result = @HttpClientService::fetch(...))` -
    // `narrowing_key` used to return None for an Assign expression, so
    // this extremely common "assign-then-check" idiom was never
    // narrowed at all, even though the assignment itself already
    // defined `$result` correctly.
    let d = diagnostics_for(
        r#"<?php
            function fetch(): string|false { return 'x'; }
            function f(): string {
                if (is_string($result = fetch())) {
                    return $result;
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 26, real shape: Piwigo\Admin\Extensions\
// ExtensionScanner::scanDirectoryEntries's `while (($file =
// readdir($dir)) !== false) { ... preg_match(..., $file) ... }` -
// real PHP *requires* the parens here (`=` binds looser than `!==`,
// so `$file = readdir($dir) !== false` would parse as `$file =
// (readdir($dir) !== false)` instead), so the extremely common
// "assign-then-check" idiom is almost always wrapped in a `Grouping`
// node - `narrowing_key` never saw through it to reach the `Assign`
// arm Phase 23 added underneath.
#[test]
fn is_string_narrows_a_parenthesized_assignment_in_a_while_condition() {
    let d = diagnostics_for(
        r#"<?php
            function readdir_(): string|false { return 'x'; }
            function f(): void {
                while (($file = readdir_()) !== false) {
                    preg_match('/x/', $file);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 26, real shape: Piwigo\Admin\Image\ImageBackend::webpInfo's
// `$fp = fopen(...); if (! (bool) $fp) { throw ...; } fread($fp,
// 25);` - a common, explicit style for a truthiness guard. `(bool)`
// doesn't change which values are truthy/falsy, but `apply_narrowing`
// had no arm for `Cast` at all, so the guard was never recognized.
#[test]
fn a_bool_cast_truthiness_guard_narrows_like_a_bare_variable_check() {
    let d = diagnostics_for(
        r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $fp = open();
                if (! (bool) $fp) {
                    throw new Exception('failed');
                }
                fclose($fp);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 32 Finding F, real shape: `BrowserTestHelpers::apiFetch`'s
// `is_int($decoded['status'] ?? null)` - `narrowing_key` had no arm
// for a `??`-wrapped expression at all, so this common
// "narrow-an-array-element-that-might-be-absent" idiom never narrowed
// `$decoded['status']` itself.
#[test]
fn is_int_narrows_through_a_null_coalesce_fallback() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{status?: int|null} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? null)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The unsoundness this fix's own scoping avoids: unlike `?? null`,
// `is_int($decoded['status'] ?? 0)` can be true purely because the
// fallback `0` itself is an int, even when `$decoded['status']` is
// absent/`null` - narrowing it to `int` there would be a real, new
// false negative, so the coalesce arm must not fire for this
// fallback.
#[test]
fn is_int_does_not_narrow_through_a_non_null_coalesce_fallback() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? 0)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected $decoded['status'] to stay unnarrowed (still int|string): {d:?}"
    );
}

// The same boundary, checked against a non-literal fallback too - not
// just "a fallback that happens to be non-zero."
#[test]
fn is_int_does_not_narrow_through_a_variable_coalesce_fallback() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded, int $default): int {
                if (is_int($decoded['status'] ?? $default)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected $decoded['status'] to stay unnarrowed (still int|string): {d:?}"
    );
}

// Real, general unsoundness bug found via a Phase 12 corpus re-check
// (`CategoryUpdateController::__invoke`, minimized here) that first
// looked like a cross-file registry-timing mystery but turned out to
// be a plain, always-reproducible narrowing bug: `apply_equality_
// narrowing`'s exclusion branch was using the *other side's own
// static type* (`Bool`, a general type covering both `true`/`false`,
// not one specific value) the same way it uses a genuine single-value
// exclusion (`Null`/`True`/`False`/a literal) - wrongly collapsing an
// already-known `bool` to `Type::Never` the moment it was compared
// `!==` against another `bool`-typed (but not literally known) value.
#[test]
fn not_identical_to_a_non_literal_bool_does_not_collapse_the_narrowed_type_to_never() {
    let d = diagnostics_for(
        r#"<?php
            function test(?bool $a, bool $b): void {
                $update = [];
                if ($a !== null && $a !== $b) {
                    $update['flag'] = $a;
                }
                echo $update['flag'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Round 14 of the Piwigo corpus cleanup: `narrow_type_to`'s own
// non-Union fallback unconditionally returned the comparison
// target's type, discarding a narrower `original` - a real
// unsoundness bug on the *true* branch of `===`, the opposite
// direction from the `!==`-exclusion bug above. Real corpus shape
// (`PictureController::__invoke`, minimized here): `$image_id`
// (a definite `int`) compared `===` against `getRefererImageId()`
// (`int|null`) inside an unrelated `if`/`else` that never reassigns
// `$image_id` - the pre-existing code widened `$image_id` itself to
// `int|null` on the true branch, corrupting every downstream use for
// the rest of the function even though neither branch ever assigns
// to it.
#[test]
fn identical_to_a_broader_typed_value_does_not_widen_the_narrower_side() {
    let d = diagnostics_for(
        r#"<?php
            class ImageId {
                public static function from(int $value): self { return new self(); }
            }
            function getRefererImageId(): ?int { return null; }
            function f(int $image_id): void {
                if (getRefererImageId() === $image_id) {
                    // no-op - neither branch reassigns $image_id
                }
                ImageId::from($image_id);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real, general bug found via a Phase 12 corpus re-check (minimized
// from `HtmlServiceTestFatalErrorCaller::call`): `check_expr`'s own
// `Ternary` arm wrote `merge_scopes(then_locals, else_locals)` back
// into the *outer* locals, unlike a `Ternary` used as a value inside
// a larger expression ever should - a ternary's own condition
// narrowing a *param* (`$collected` here) down to the literal empty
// shape on its true branch then got permanently unioned into
// `$collected`'s real, tracked type for everything checked
// afterward, including a second, independent evaluation of the exact
// same sub-expression - here, `preg_replace`'s own subject-type
// check saw the corrupted union instead of `$collected[0]`'s real
// `string` type, losing its precise return type.
#[test]
fn a_ternarys_own_condition_narrowing_does_not_leak_past_the_ternary() {
    let d = diagnostics_for(
        r#"<?php
            class Holder {
                public ?string $captured = null;

                /** @param list<string> $collected */
                public function run(array $collected): void {
                    $this->captured = $collected === [] ? null : preg_replace('/^x/', '', $collected[0]);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_a_property_after_narrowing_it_does_not_leave_the_stale_narrowed_type_behind() {
    // Phase 22, real shape: a lazy-init idiom
    // (Piwigo\Asset\ViteManifest::entries) - `if ($this->entries ===
    // null) { $this->entries = [...]; } return $this->entries;`.
    // `apply_equality_narrowing` correctly narrows `$this->entries`
    // to `null` for the *condition's own* branch, but
    // `bind_assignment_target` had no `PropertyAccess` arm at all, so
    // the reassignment inside the `if` body never updated that same
    // synthetic narrowing-key entry - `infer_expr`'s own "check
    // narrowed key first" fast path kept reading back the stale
    // `null` instead of the real, newly-assigned array, so the
    // return type looked nullable when it never could be.
    let d = diagnostics_for(
        r#"<?php
            class Cache {
                /** @var array<string>|null */
                private ?array $entries = null;

                /** @return array<string> */
                private function entries(): array {
                    if ($this->entries === null) {
                        $this->entries = ['a', 'b'];
                    }
                    return $this->entries;
                }

                /** @return array<string> */
                public function get(): array {
                    return $this->entries();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_self_narrows_to_the_real_enclosing_class_not_a_literal_self() {
    // Real, common pattern (amphp/amp's Future::iterate does exactly
    // this: `if ($future instanceof self)`) - apply_narrowing's
    // instanceof handling used to treat "self"/"parent"/"static" as
    // ordinary bare class names to resolve via name_ctx.resolve
    // directly, instead of through resolve_pseudo against the
    // enclosing class the way every other self/parent/static site
    // already does. In a namespaced file this narrowed to a
    // nonsensical class name ("App\self") instead of the real one
    // ("App\Box") - which would have looked like *no* diagnostics at
    // all below (a bogus, not-in-the-registry class silently
    // suppresses the undefined-method check), not a wrong one, so
    // this asserts a diagnostic *does* appear, to actually
    // distinguish correct narrowing from the old silent failure.
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            class Box {
                public function useSame($other): string {
                    if ($other instanceof self) {
                        return $other->totallyMadeUp();
                    }
                    return "";
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("App\\Box::totallyMadeUp")),
        "expected narrowing to resolve to the real class App\\Box, got: {d:?}"
    );
}

#[test]
fn negated_instanceof_guard_clause_narrows_past_the_if() {
    // `if (! $x instanceof Y) { throw ...; }` must narrow $x to Y for
    // the code after the guard - depends on `!` correctly binding
    // looser than `instanceof` at the parser level (a real bug this
    // session found and fixed: it used to parse as `(!$x) instanceof
    // Y`, which silently narrowed nothing).
    let d = diagnostics_for(
        r#"<?php
            interface Base {}
            interface Specific extends Base {
                public function specificMethod(): int;
            }
            function make(): Base { return new class implements Specific {
                public function specificMethod(): int { return 1; }
            }; }
            function f() {
                $x = make();
                if (! $x instanceof Specific) {
                    throw new \Exception('nope');
                }
                return $x->specificMethod();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_inline_var_annotation_on_a_later_statement_narrows_a_generically_typed_read() {
    // Phase 35 Finding F: the real `StructuralTest::findCallSites`
    // shape, reduced - `$file` comes back from `current()`'s own
    // generic (untyped, so `mixed`) return, an inline `/** @var
    // FakeFileInfo9921 $file */` sits on an unrelated `if` statement that
    // merely reads it, and code after that `if` calls a method only
    // `FakeFileInfo9921` has. Without the annotation, `$file` stays
    // `mixed` and this method call is never checked at all (this
    // project's usual mixed-argument/mixed-receiver leniency) - so a
    // real "undefined method" diagnostic here proves the annotation
    // actually took effect, not just that nothing was flagged.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): void {
                $file = $it->current();
                /** @var FakeFileInfo9921 $file */
                if (true) {
                }
                $file->missingMethod();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined method")),
        "expected an undefined-method diagnostic, got: {d:?}"
    );
}

#[test]
fn a_bare_inline_var_annotation_overrides_the_immediately_following_assignment() {
    // The bug found during adversarial validation: applying the
    // assertion only *before* dispatch would have this immediately
    // clobbered by the assignment's own natural (mixed) inference,
    // since `bind_assignment_target` runs inside the very same
    // `check_stmt` call. A `return type mismatch` naming FakeFileInfo9921
    // proves the after-dispatch reapplication actually won.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): string {
                /** @var FakeFileInfo9921 */
                $file = $it->current();
                return $file;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")
            && x.message.contains("FakeFileInfo9921")),
        "expected a return-type mismatch naming FakeFileInfo9921, got: {d:?}"
    );
}

#[test]
fn a_named_inline_var_annotation_on_its_own_assignment_also_overrides_it() {
    // Same bug, explicitly-named form - proving the fix isn't
    // bare-form-specific.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): string {
                /** @var FakeFileInfo9921 $file */
                $file = $it->current();
                return $file;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")
            && x.message.contains("FakeFileInfo9921")),
        "expected a return-type mismatch naming FakeFileInfo9921, got: {d:?}"
    );
}

#[test]
fn an_inline_var_override_is_invalidated_by_a_later_real_reassignment() {
    // The override correctly gets invalidated by a later real
    // reassignment of the same variable - Phase 33 Finding 5's own
    // purge-on-reassignment, exercised through this new call site.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            function needsInt(int $n): void {}
            function f($it): void {
                $file = $it;
                /** @var FakeFileInfo9921 $file */
                if (true) {
                }
                $file = 5;
                needsInt($file);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_inline_var_annotation_preceding_a_foreach_binding_two_names_is_a_no_op() {
    // Scope guard for `statement_defined_variable_names`'s own
    // restriction: a bare (nameless) tag can only infer its target
    // when the statement it precedes has exactly one candidate -
    // a `foreach` binding both a key and a value has two, so this is
    // left unguessed rather than assuming either one.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            function f(array $items): string {
                /** @var FakeFileInfo9921 */
                foreach ($items as $k => $v) {
                    return $v;
                }
                return "fallback";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_doc_comment_with_no_var_tag_is_a_no_op() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                /** Just a description, no @var tag here. */
                $x = "not an int";
                return 1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Findings 54/56 from the architecture audit - a bare truthiness
// check (`if ($x)` / `if (!$x)`) and `empty($x)` both applied no
// narrowing at all.

#[test]
fn negated_truthy_guard_narrows_past_the_if() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if (!$x) {
                    return "empty";
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn positive_truthy_guard_narrows_inside_the_if() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if ($x) {
                    return $x;
                }
                return "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn positive_truthy_guard_does_not_narrow_the_false_branch() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if ($x) {
                    return "yes";
                }
                return $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn empty_guard_narrows_past_the_if() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if (empty($x)) {
                    return "empty";
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 23 Finding 1: `isset($x)`'s truthy branch narrows away `Null`,
// the single most common PHP nullability idiom - previously a no-op.
#[test]
fn isset_guard_narrows_null_away_in_the_truthy_branch() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return strlen($x);
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_guard_narrows_a_property_access_too() {
    let d = diagnostics_for(
        r#"<?php
            class Box { public ?string $value = null; }
            function f(Box $box): int {
                if (isset($box->value)) {
                    return strlen($box->value);
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A round of Piwigo-corpus bucketing found a static property
// (`self::$prop !== null`) had no narrowing key at all - only an
// instance property's `->` chain was handled. Real Piwigo test code
// (BrowserTestHelpers::adminSessionCookies) relies on exactly this.
#[test]
fn a_static_property_is_narrowed_by_an_equality_check_too() {
    let d = diagnostics_for(
        r#"<?php
            class Cache1 {
                private static ?string $value = null;
                public static function get(): string {
                    if (self::$value !== null) {
                        return self::$value;
                    }
                    return self::$value = 'computed';
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_a_static_property_after_narrowing_it_does_not_leave_the_stale_narrowed_type_behind()
{
    let d = diagnostics_for(
        r#"<?php
            class Cache2 {
                private static ?string $value = null;
                public static function reset(): int {
                    if (self::$value !== null) {
                        self::$value = null;
                        return strlen(self::$value);
                    }
                    return 0;
                }
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the stale narrowed type to be purged: {d:?}"
    );
}

#[test]
fn isset_false_branch_is_not_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return 0;
                }
                return strlen($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Finding 55 from the architecture audit - ternary/switch/match never
// applied condition-based narrowing at all, for any condition shape.

#[test]
fn ternary_narrows_both_branches() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return $x === null ? "default" : $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return $x !== null ? $x : "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let wrong = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return $x === null ? $x : "default";
            }
        "#,
    );
    assert_eq!(wrong.len(), 1, "got: {wrong:?}");
}

// Phase 23 Finding 4, real shape: `$handle = $filename !== null ?
// @fopen($filename, 'wb') : false;` - fopen()'s own type is already a
// Union (`resource|false`); combining it with the ternary's other
// branch via a raw `Type::Union(vec![then_ty, else_ty])` nested that
// whole union unflattened, so a later `!== false` narrowing on the
// outer union couldn't reach the `false` still hiding inside it.
#[test]
fn ternary_flattens_a_branch_that_is_itself_already_a_union() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $filename): void {
                $handle = $filename !== null ? @fopen($filename, 'wb') : false;
                if ($handle !== false) {
                    fclose($handle);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn switch_default_narrows_past_a_terminating_case() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        return "was-null";
                    default:
                        return $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let wrong = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        return $x;
                    default:
                        return "not null";
                }
            }
        "#,
    );
    assert_eq!(wrong.len(), 1, "got: {wrong:?}");
}

#[test]
fn switch_fallthrough_case_does_not_leak_its_own_narrowing_past_a_later_terminating_case() {
    // Regression test for a real bug caught while implementing this
    // fix: an empty `case null: case "": ...` fallthrough group's own
    // optimistic narrowing (subject === null) must not leak past the
    // *second* case (which itself terminates) into `default`.
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                    case "":
                        return "empty-ish";
                    default:
                        return $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn match_true_pattern_narrows_each_arm_and_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return match (true) {
                    $x === null => "default",
                    default => $x,
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let wrong = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return match (true) {
                    $x === null => $x,
                    default => "not null",
                };
            }
        "#,
    );
    assert_eq!(wrong.len(), 1, "got: {wrong:?}");
}

#[test]
fn match_on_a_real_subject_narrows_via_implicit_equality() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return match ($x) {
                    null => "default",
                    default => $x,
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 26 from the architecture audit - `??`/`??=`'s left side was
// checked for "undefined variable"/"undefined property" like an
// ordinary read, even though both are specifically designed to be
// warning-free for a variable/property that isn't set at all.

#[test]
fn coalesce_does_not_flag_a_conditionally_assigned_variable_as_undefined() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): string {
                if ($c) { $result = "computed"; }
                return $result ?? "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_does_not_flag_an_undefined_array_key_root_variable() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                return $config['key'] ?? "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_widens_to_the_left_side_s_array_type_when_the_fallback_is_a_narrower_shape() {
    // Phase 22 Finding 4, real shape: Piwigo\Activity\Projection\
    // SystemActivityLogEntry::$details is `public ?array $details`;
    // `$details = $row->details ?? [];` used to collapse the whole
    // expression's type to the literal empty `[]` shape from the
    // fallback alone, discarding that `$row->details`'s own real,
    // non-empty-capable `?array` type could hold arbitrary keys -
    // every later `$details['key']` read then looked like a typo.
    let d = diagnostics_for(
        r#"<?php
            class Row { public ?array $details; }
            function f(Row $row): void {
                $details = $row->details ?? [];
                echo $details['config_section'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_still_collapses_to_the_fallback_for_an_ordinary_mixed_left_side() {
    // The common, currently-precise case Finding 4's own fix must
    // not regress: when the left side's real type isn't an
    // informative `Type::Array`, `??`'s result stays exactly the
    // fallback's own (here, literal string) type - not blanket
    // `Mixed` from unioning in `$x`'s own type.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function f($x): int {
                return needsInt($x ?? 0);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_does_not_flag_an_undeclared_instance_property() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f(Foo $x): string { return $x->missing ?? "default"; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_still_flags_an_undeclared_static_property() {
    // isset()/?? silence a suppressible *notice* - an undeclared
    // *static* property is a real, non-suppressible PHP fatal error,
    // and must still be flagged under `??`.
    let d = diagnostics_for(
        r#"<?php
            class Bar {}
            function f(): string { return Bar::$missing ?? "default"; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn coalesce_assign_does_not_flag_its_own_left_side_as_undefined() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                $x ??= "default";
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 17 from the architecture audit - loose equality (`==`) was
// narrowed exactly like strict equality (`===`), discarding every
// other value PHP's own type-juggling rules also match.

#[test]
fn loose_equality_does_not_narrow_to_the_literal_compared_to_value() {
    // `$x == 0` also matches the real bool `false` (PHP's own
    // `false == 0` rule) - narrowing exactly to the literal `0`
    // would incorrectly discard that possibility. This project's own
    // separate, deliberate choice to analyze every file as if
    // strict_types were always active means passing the
    // *unnarrowed* `bool|int` union to a `bool`-only parameter can
    // still legitimately be flagged on its own terms - what this
    // test actually guards against is over-narrowing to the wrong,
    // overly-specific literal value.
    let d = diagnostics_for(
        r#"<?php
            function g(bool $flag): void {}
            function f(bool|int $x): void {
                if ($x == 0) {
                    g($x);
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.message.contains("'0' given") || x.message.contains(", 0 given")),
        "still over-narrowing to the literal compared-to value: {d:?}"
    );
}

#[test]
fn strict_equality_still_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function h(int $y): void {}
            function f(?int $x): void {
                if ($x === null) {
                    return;
                }
                h($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Findings 52/53 from the architecture audit - a `while`/`do-while`
// loop never applied its negated condition as narrowing after exit,
// and a `for` loop's body was checked as if it always executes
// exactly once, with no "may run zero times" merge at all.

#[test]
fn while_loop_narrows_the_negated_condition_after_exit() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                $x = null;
                while ($x === null) {
                    $x = 5;
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn do_while_loop_narrows_the_negated_condition_after_exit() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                $x = null;
                do {
                    $x = 5;
                } while ($x === null);
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn for_loop_narrows_the_negated_condition_after_exit() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                for ($x = null; $x === null;) {
                    $x = 5;
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 15 from the architecture audit - the nullsafe operator
// (?->) and plain -> produced identical AST nodes, so infer_expr
// never knew a nullsafe access's result could be null.

#[test]
fn nullsafe_method_call_result_is_implicitly_nullable() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(?Owner $obj): void { needsFoo($obj?->getFoo()); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("Foo|null"), "got: {d:?}");
}

#[test]
fn nullsafe_property_access_result_is_implicitly_nullable() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public ?Foo $foo = null; }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(?Owner $obj): void { needsFoo($obj?->foo); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn nullsafe_result_accepted_by_a_nullable_parameter_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsMaybeFoo(?Foo $x): void {}
            function test(?Owner $obj): void { needsMaybeFoo($obj?->getFoo()); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn plain_arrow_on_a_non_nullable_receiver_is_unaffected() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(Owner $obj): void { needsFoo($obj->getFoo()); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
