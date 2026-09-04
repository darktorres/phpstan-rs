//! Array literal typing, shapes, list-ness, index assignment, destructuring.

use super::super::*;
use super::helpers::*;

#[test]
fn a_wrong_typed_array_literal_element_is_flagged_regardless_of_strict_mixed_arrays() {
    // Finding 48: an array literal's own element types are now
    // inferred precisely (a real ArrayShape, not Array(Mixed)), so
    // real information flows through unconditionally - unlike before
    // Finding 48, `strict_mixed_arrays` isn't what makes this catch a
    // genuinely wrong literal element; ordinary, non-strict checking
    // already does.
    let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            needsInts([1, "two", 3]);
        "#;
    let d = diagnostics_for(src);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("expects array<int>"), "got: {d:?}");
    // strict_mixed_arrays doesn't change this verdict either way.
    let d = diagnostics_for_with(src, |r| r.strict_mixed_arrays = true);
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn a_correctly_typed_array_literal_is_not_flagged_with_or_without_strict_mixed_arrays() {
    // The other half of the same fix: before Finding 48, a *correct*
    // literal was indistinguishable from an incorrect one once
    // strict_mixed_arrays removed the Array(Mixed) leniency, since
    // neither carried any real element-type information. Now that a
    // literal carries a real shape, the genuinely correct case must
    // stay silent.
    let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            needsInts([1, 2, 3]);
        "#;
    assert!(diagnostics_for(src).is_empty());
    assert!(diagnostics_for_with(src, |r| r.strict_mixed_arrays = true).is_empty());
}

#[test]
fn strict_mixed_arrays_still_flags_a_genuinely_unknown_mixed_typed_array() {
    // strict_mixed_arrays' own remaining, real purpose: a value whose
    // element type is genuinely Mixed for a reason other than being
    // an array literal (here, read back out of an untyped array
    // parameter) - not affected by Finding 48's own literal-specific
    // fix at all.
    let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function test(array $untyped): void { needsInts($untyped); }
        "#;
    assert!(diagnostics_for(src).is_empty());
    let d = diagnostics_for_with(src, |r| r.strict_mixed_arrays = true);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("expects array<int>"), "got: {d:?}");
}

#[test]
fn a_literal_array_shape_mismatch_is_flagged_against_a_docblock_shape() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{0: int, 1: string} $pair */
            function needsPair(array $pair): void {}
            function test(): void { needsPair(["backwards", 5]); }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("needsPair()")),
        "expected a shape-mismatch diagnostic, got: {d:?}"
    );
}

#[test]
fn a_correctly_shaped_literal_is_not_flagged_against_a_docblock_shape() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{0: int, 1: string} $pair */
            function needsPair(array $pair): void {}
            function test(): void { needsPair([1, "a"]); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn destructuring_a_swap_from_an_inline_array_literal_gets_real_per_position_types() {
    // Finding 48's shared root cause with Finding 42: before this
    // fix, [$a, $b] = [$b, $a] silently retyped both sides to Mixed,
    // since the literal's own element types were never inferred -
    // masking a real type mismatch the swap introduces.
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function test(): void {
                $a = 1;
                $b = "x";
                [$a, $b] = [$b, $a];
                needsString($b);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("needsString() argument #1 ($s) expects string, 1 given")),
        "expected the post-swap type mismatch to be caught, got: {d:?}"
    );
}

#[test]
fn building_an_array_progressively_via_index_assignment_is_not_flagged() {
    // A real regression found while verifying Finding 48 against the
    // full vendor/phpstorm-stubs corpus: $x = []; $x['a'] = ...; is
    // one of the most common ways to build an array in real PHP -
    // check_shape_key_access must not treat the literal's own
    // starting shape as a sealed contract the array can never grow
    // past, either at the write site itself or at a later read.
    let d = diagnostics_for(
        r#"<?php
            function test(): string {
                $x = [];
                $x['a'] = 'hello';
                $x['b'] = 42;
                return $x['a'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reading_a_key_never_assigned_after_building_an_array_progressively_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $x = [];
                $x['a'] = 'hello';
                echo $x['b'];
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("array{...} has no key 'b'")),
        "expected a missing-key diagnostic, got: {d:?}"
    );
}

#[test]
fn appending_to_an_array_in_a_loop_then_reading_a_literal_index_is_not_flagged() {
    // Phase 22 Finding 2, real shape: RegenerateFixtureTest.php's own
    // `$tagIds = []; foreach ([...] as $name) { $tagIds[] = ...; }`
    // then `$tagIds[0]`/`[1]`/`[2]` read afterward.
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $tagIds = [];
                foreach (['a', 'b', 'c'] as $name) {
                    $tagIds[] = strlen($name);
                }
                echo $tagIds[0];
                echo $tagIds[1];
                echo $tagIds[2];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assigning_a_dynamic_key_in_a_loop_then_reading_a_literal_key_is_not_flagged() {
    // Phase 22 Finding 2, real shape: SearchFilterRenderer.php's own
    // `$displayFilters[$filtName] = ...;` inside a `foreach`, then
    // `$displayFilters['words']` read elsewhere.
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $displayFilters = [];
                foreach (['words' => true, 'tags' => false] as $filtName => $filtConf) {
                    $displayFilters[$filtName] = $filtConf;
                }
                echo $displayFilters['words'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dynamic_write_disables_typo_catching_for_the_rest_of_that_variable() {
    // The real, deliberate cost of Finding 2's fix, pinned down as a
    // test rather than left as prose: once `$x` is widened away from
    // `Type::ArrayShape` by an append/dynamic-key write anywhere in
    // its lifetime, `check_shape_key_access` (which only fires on
    // `Type::ArrayShape`) stops catching a genuine typo on *any* key
    // of that variable from that point on - not just the new one.
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $x = [];
                $x['a'] = 1;
                $x[] = 2;
                echo $x['typo'];
            }
        "#,
    );
    assert!(
        d.is_empty(),
        "expected the typo to go uncaught (the fix's known trade-off), got: {d:?}"
    );
}

// A round of Piwigo-corpus bucketing found the array-append-widening
// path (Phase 22) built a plain `Array`, never a real `list<T>`, even
// though real PHP's own append semantics always assign the next
// sequential int key - verified via dumpType(): a single append onto
// an already-list-like container (including a freshly-empty `[]`)
// stays a real, now-provably-non-empty list.
#[test]
fn appending_onto_a_freshly_empty_array_produces_a_non_empty_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-list<string> $x */
            function sink(array $x): void {}
            function f(string $a): void {
                $content = [];
                $content[] = $a;
                sink($content);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn appending_onto_an_already_typed_list_stays_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $x
             *  @return non-empty-list<string>
             */
            function f(array $x): array {
                $x[] = 'y';
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dynamic_key_write_does_not_claim_the_result_is_still_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $x
             *  @return list<string>
             */
            function f(array $x, string $key): array {
                $x[$key] = 'y';
                return $x;
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the list-ness mismatch to still be flagged: {d:?}"
    );
}

#[test]
fn a_nested_write_through_a_dynamically_keyed_array_grows_its_element_shape() {
    // Real shape: CalendarMonthly::buildMonthCalendar builds $items
    // via a dynamic-key write ($items[(int) $row->period] = [...]),
    // then adds more fields per-element via a *nested* write
    // ($items[$day]['derivative'] = ...) - discovered as a genuine
    // new false positive while verifying Finding 2's own fix:
    // bind_assignment_target's Index arm required a bare Variable
    // base, so the nested write (base is itself $items[$day], an
    // Index, not a Variable) was silently ignored, leaving
    // $items[$day]'s inferred shape frozen at its initial literal -
    // missing 'derivative' entirely.
    let d = diagnostics_for(
        r#"<?php
            function test(int $period, string $file): void {
                $items = [];
                $items[$period] = ['nb_images' => 0];
                $items[$period]['derivative'] = $file;
                echo $items[$period]['derivative'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 12 Finding E: a real, pre-existing gap found by making
// check_shape_key_access Union-aware and re-checking the real
// corpus - PiwigoInfosSender::send's own $piwigoInfos becomes a real
// Union of two differently-shaped ArrayShapes across an if/else (one
// branch's own nested field differs from the other's), and a later
// flat write into that Union silently updated no member at all,
// leaving a key written one line earlier still reported missing by a
// very next nested write's own "does this key exist" check.
// Positive precision check (not just "no false positive"): `Mixed` is
// never flagged against anything (this project's own "don't guess"
// leniency), so a test only asserting an empty diagnostic list here
// would pass whether or not `$value` was ever given its real, precise
// `string` type. Passing it to an `int`-only parameter instead makes
// the fix's own effect directly observable: a real mismatch is only
// reported once `$value` is actually inferred as `string`.
#[test]
fn foreach_over_a_homogeneous_array_shape_infers_the_shared_field_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): int { return $n; }
            function test(): void {
                $versions = ['current' => 'a', 'latest' => 'b'];
                foreach ($versions as $key => $value) {
                    needsInt($value);
                }
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected $value to be inferred as string (and flagged against needsInt): {d:?}"
    );
}

#[test]
fn a_flat_write_into_a_union_of_array_shapes_updates_every_member() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $cond): void {
                $info = ['a' => 1];
                if ($cond) {
                    $info['b'] = 'x';
                } else {
                    $info['c'] = 'y';
                }
                $info['file_extensions'] = [];
                $info['file_extensions']['jpg'] = 3;
                echo $info['file_extensions']['jpg'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 12 (unplanned follow-on): the real `PiwigoInfosSender::send`
// shape - a `Union`-typed container (built across an `if`/`else`) whose
// very first write to a brand-new key is itself a *nested* append
// (`$info['updates'][] = ...`), never a flat `$info['updates'] = [];`
// first. `check_assignment_target`'s `Index` arm used to route the
// inner `$info['updates']` sub-expression through `check_expr`, which
// wrongly checked it as a genuine read via `check_shape_key_access` -
// masked before Finding E made that function actually inspect a
// `Union` instead of bailing on it outright.
#[test]
fn a_nested_append_creating_a_brand_new_key_on_a_union_typed_container_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $cond): void {
                if ($cond) {
                    $info = ['a' => 1];
                } else {
                    $info = ['b' => 2];
                }
                $info['updates'][] = 'x';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn overwriting_an_existing_key_via_index_assignment_updates_its_tracked_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function test(): int {
                $x = ['a' => 1];
                $x['a'] = 5;
                return needsInt($x['a']);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_spread_element_inside_a_literal_defeats_precise_shape_inference() {
    // A spread element's own contents aren't known statically (real
    // PHP inserts a variable number of the spread value's own
    // elements, shifting every later positional key by an amount
    // this checker can't know) - must degrade to the honest
    // Array(Mixed) for the whole literal rather than mis-keying the
    // spread expression itself as if it occupied one fixed slot.
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $nums */
            function needsInts(array $nums): void {}
            function test(array $other): void {
                needsInts([1, ...$other, "not an int"]);
            }
        "#,
    );
    assert!(
        d.is_empty(),
        "a spread element should defeat shape inference, not produce a wrong verdict: {d:?}"
    );
}

// Task 2: array-shape generalization, tried before the hard
// MAX_UNION_MEMBERS collapse - 13 shapes that all share the *same*
// key set merge into one shape with unioned field types instead of
// collapsing to `Mixed`. Tested directly against `generalize_by_key_
// set` itself, not via sequential `union_types` calls: `.reduce()`
// builds the union one pairwise call at a time, which triggers
// generalization repeatedly at different *partial* states (and can
// itself separately trip the per-field union's own `MAX_UNION_
// MEMBERS` cap when the field values are too varied) - a real
// property of how `union_types` is actually invoked, but not what
// this test is trying to isolate, which is `generalize_by_key_set`'s
// own one-shot merge behavior over an already-complete member list.
#[test]
fn generalize_by_key_set_merges_same_key_set_shapes_instead_of_collapsing() {
    // Only 2 distinct field values (alternating), so the per-field
    // union stays well under its own cap and the result is a clean,
    // precise `bool`, not `Mixed` - proving the merge actually folds
    // real field types together, not just picks one arbitrarily.
    let members: Vec<Type> = (0..13)
        .map(|i| {
            let value = if i % 2 == 0 { Type::True } else { Type::False };
            Type::ArrayShape(vec![("x".to_string(), value, false)])
        })
        .collect();
    let result = generalize_by_key_set(members);
    assert_eq!(
        result.len(),
        1,
        "expected every same-key-set shape to merge into one"
    );
    match &result[0] {
        Type::ArrayShape(fields) => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "x");
            assert!(!fields[0].2, "the merged field must not be optional");
            assert_eq!(
                fields[0].1,
                Type::Union(vec![Type::True, Type::False]),
                "expected the alternating True/False values to fold into a real Union"
            );
        }
        other => panic!("expected a single merged ArrayShape, got {other:?}"),
    }
}

// A mix of same- and different-key-set shapes only merges the
// matching subset - the different-key-set members pass through
// untouched, exactly as many members as went in.
#[test]
fn generalize_by_key_set_only_merges_the_matching_subset() {
    let mut members: Vec<Type> = (0..13)
        .map(|i| {
            let value = if i % 2 == 0 { Type::True } else { Type::False };
            Type::ArrayShape(vec![("x".to_string(), value, false)])
        })
        .collect();
    members.extend((0..5).map(|i| Type::ArrayShape(vec![(format!("y{i}"), Type::Int, false)])));
    let result = generalize_by_key_set(members);
    // The 13 same-key-set ("x") shapes merge into 1; the 5
    // different-key-set ("y0".."y4") shapes each stay distinct and
    // untouched.
    assert_eq!(result.len(), 6, "expected 1 merged + 5 untouched members");
    let untouched_y_shapes = result
            .iter()
            .filter(|m| matches!(m, Type::ArrayShape(fields) if fields.len() == 1 && fields[0].0.starts_with('y')))
            .count();
    assert_eq!(
        untouched_y_shapes, 5,
        "expected all 5 y-shapes to pass through unmerged"
    );
}

// Phase 14 Task 7: the array-destructuring regression risk gets its
// own direct, dedicated coverage - the live repro this round's own
// review found broken (foreach over a list<T>-typed variable
// silently inferring Mixed) and its siblings.
#[test]
fn foreach_over_a_list_typed_variable_still_infers_the_real_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function f($x): void {
                foreach ($x as $item) {
                    needsInt($item);
                }
            }
            function needsInt(int $x): int { return $x; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_two_list_typed_arrays_still_uses_array_union_not_numeric_addition() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $a
             *  @param list<int> $b
             *  @return array<int>
             */
            function f($a, $b): array {
                return $a + $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_element_type_callers_still_work_given_a_list_typed_argument() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function f($x): void {
                foreach (array_map(fn($v) => $v + 1, $x) as $v) {
                    needsInt($v);
                }
                foreach (array_chunk($x, 2) as $chunk) {
                    foreach ($chunk as $v) {
                        needsInt($v);
                    }
                }
                needsInt(array_reduce($x, fn($carry, $v) => $carry + $v, 0));
            }
            function needsInt(int $x): int { return $x; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_percent_d_is_still_flagged_for_a_list_typed_argument() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function f($x): void {
                sprintf('%d', $x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("list<int>"), "got: {d:?}");
}

#[test]
fn a_by_ref_call_with_a_list_typed_declared_parameter_still_widens_precisely() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function mutates(array &$x): void {}
            function f(): void {
                $items = [];
                mutates($items);
                foreach ($items as $item) {
                    needsInt($item);
                }
            }
            function needsInt(int $x): int { return $x; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A new round of Piwigo-corpus bucketing surfaced `infer_array_literal_type`
// giving up entirely (falling to bare `Array(Mixed)`) on any array
// literal containing a spread element - the real, common shape being
// `[...findA(), ...findB()]`, combining several `list<T>`-returning
// helpers into one. Verified against real PHPStan's own dumpType():
// spreading two `list<T>`s stays a `list<T>`; spreading a `list<T>`
// alongside a plain `array<T>` degrades to plain `array<T>` (not a
// list); a leading literal element makes the whole thing non-empty.
#[test]
fn spreading_two_lists_into_an_array_literal_still_produces_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $a
             *  @param list<int> $b
             *  @return list<int>
             */
            function f($a, $b): array {
                return [...$a, ...$b];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn spreading_a_list_and_a_plain_array_degrades_to_a_plain_array_not_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $a
             *  @param array<int> $b
             *  @return list<int>
             */
            function f($a, $b): array {
                return [...$a, ...$b];
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the list-ness mismatch to still be flagged: {d:?}"
    );
}

// Found by the corpus recheck after the fix above landed: real Piwigo
// code (`UserInfoWithThemeName::toArray()`) spreads another object's
// own precisely-known `toArray()` shape and adds one more field -
// `[...$this->userInfo->toArray(), 'theme_name' => $x]`. Verified
// against real PHPStan's own dumpType(): spreading a shape with only
// non-canonical (string) keys merges those keys in verbatim, keeping
// the whole literal a precise shape rather than degrading to a
// generic array.
#[test]
fn spreading_a_known_shape_and_adding_a_field_still_produces_a_precise_shape() {
    let d = diagnostics_for(
        r#"<?php
            /** @return array{a: int, b: string} */
            function inner(): array {
                return ['a' => 1, 'b' => 'x'];
            }
            /** @return array{a: int, b: string, c: bool} */
            function outer(): array {
                return [...inner(), 'c' => true];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_element_type_union_arm_does_not_collapse_to_mixed_via_an_empty_branch() {
    // The regression this fix could easily introduce: folding an
    // empty ArrayShape branch's own degenerate `Mixed` element type
    // into the union would collapse the *whole* merged element type
    // down to `Mixed`, discarding the real element type the
    // non-empty branch actually has.
    let d = diagnostics_for(
        r#"<?php
            /** @return list<int> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    $x[] = 1;
                }
                sort($x);
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_union_with_a_non_array_member_does_not_get_wrongly_upgraded_to_a_list() {
    // `array_element_type`'s new Union arm must require *every*
    // member to be array-like - a `list<int>|null` has no sensible
    // single "element type," so sort() falls back to its own
    // ordinary (imprecise, but safe) declared-parameter-type
    // widening instead of wrongly claiming the result is a list.
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int>|null $x
             *  @return list<int>
             */
            function f($x): array {
                sort($x);
                return $x;
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the fallback array(mixed) to still mismatch list<int>: {d:?}"
    );
}

#[test]
fn a_literal_element_before_a_spread_list_makes_the_result_a_non_empty_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $a
             *  @return non-empty-list<int>
             */
            function f($a): array {
                return [1, ...$a];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn homogeneous_array_index_infers_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $words */
            function f(array $words) {
                return needsInt($words[0]);
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn homogeneous_array_index_of_matching_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param int[] $nums */
            function f(array $nums) {
                return needsInt($nums[0]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn shape_key_access_infers_the_field_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param array{name: string, age: int} $p */
            function f(array $p) {
                return needsInt($p['age']);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn shape_key_typo_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{name: string, age: int} $p */
            function f(array $p) {
                return $p['agee'];
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("'agee'"));
    assert_eq!(d[0].level, 2);
}

#[test]
fn shape_key_access_with_dynamic_index_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{name: string, age: int} $p */
            function f(array $p, string $key) {
                return $p[$key];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn shape_optional_field_access_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{name: string, age?: int} $p */
            function f(array $p) {
                return $p['age'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn destructuring_assignment_binds_element_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $pair */
            function f(array $pair) {
                [$a, $b] = $pair;
                return needsInt($a);
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn list_style_destructuring_also_binds_element_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $pair */
            function f(array $pair) {
                list($a, $b) = $pair;
                return needsInt($a);
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn keyed_destructuring_binds_its_target() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $data) {
                ['name' => $n] = $data;
                return $n;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_plus_array_infers_as_array_not_arithmetic() {
    let d = diagnostics_for(
        r#"<?php
            function merge(array $config, array $defaults): int {
                $merged = $config + $defaults;
                return count($merged);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 42 from the architecture audit - destructuring from an
// array{...} shape didn't extract each position/key's own precise
// type, falling back to Mixed.

#[test]
fn positional_destructuring_extracts_the_shapes_real_per_position_types() {
    let d = diagnostics_for(
        r#"<?php
            /** @return array{0: int, 1: string} */
            function pair(): array { return [1, "a"]; }
            function test(): string {
                [$num, $str] = pair();
                return $num;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expected string, got int"),
        "got: {d:?}"
    );
}

#[test]
fn keyed_destructuring_extracts_the_shapes_real_per_key_types() {
    let d = diagnostics_for(
        r#"<?php
            /** @return array{name: string, age: int} */
            function person(): array { return ["name" => "Al", "age" => 30]; }
            function test(): int {
                ["name" => $name, "age" => $age] = person();
                return $name;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expected int, got string"),
        "got: {d:?}"
    );
}

#[test]
fn correctly_typed_shape_destructuring_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @return array{0: int, 1: string} */
            function pair(): array { return [1, "a"]; }
            function test(): int {
                [$num, $str] = pair();
                return $num;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 50: InvalidKeyInArrayItemRule - an array literal key must be a
// type PHP itself accepts as an array key at runtime.

#[test]
fn an_array_literal_key_typed_as_an_object_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $obj): array {
                return [$obj => 1];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "array.invalidKey");
    assert!(
        d[0].message.contains("Invalid array key type Foo"),
        "got: {d:?}"
    );
}

#[test]
fn an_array_literal_key_typed_as_an_array_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function test(array $xs): array {
                return [$xs => 1];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "array.invalidKey");
}

#[test]
fn ordinary_int_and_string_array_literal_keys_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $i, string $s): array {
                return [$i => 1, $s => 2, 3 => 'a', 'k' => 'b'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bool_float_and_null_array_literal_keys_are_not_flagged_php_coerces_them() {
    // PHP silently coerces these at runtime (matches real PHPStan's own
    // default, non-strict AllowedArrayKeysTypes) - not a hard TypeError
    // the way an array/object key is.
    let d = diagnostics_for(
        r#"<?php
            function test(bool $b, float $f, ?string $n): array {
                return [$b => 1, $f => 2, $n => 3];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_mixed_typed_array_literal_key_is_not_flagged_dont_guess() {
    let d = diagnostics_for(
        r#"<?php
            function test(mixed $m): array {
                return [$m => 1];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_union_key_type_mixing_a_legal_and_illegal_member_is_not_flagged() {
    // This project has no tri-state "possibly invalid" - only a key
    // type that's confidently, entirely illegal is flagged.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(int|Foo $k): array {
                return [$k => 1];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_refined_string_array_literal_key_is_not_flagged() {
    // An accessory-refined string (literal-string, non-empty-string,
    // ...) is still a plain legal string key - only an Array-shaped
    // base type is illegal.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $k */
            function test(string $k): array {
                return [$k => 1];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
