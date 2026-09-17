//! Array literal typing, shapes, list-ness, index assignment, destructuring.

use super::super::*;
use super::helpers::*;

#[test]
fn a_wrong_typed_array_literal_element_is_flagged_regardless_of_strict_mixed_arrays() {
    // An array literal's own element types are inferred precisely (a
    // real ArrayShape, not Array(Mixed)), so real information flows
    // through unconditionally - `strict_mixed_arrays` isn't what makes
    // this catch a genuinely wrong literal element; ordinary,
    // non-strict checking already does.
    let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            needsInts([1, "two", 3]);
        "#;
    let d = diagnostics_for(src);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expects array<(int|string), int>"),
        "got: {d:?}"
    );
    // strict_mixed_arrays doesn't change this verdict either way.
    let d = diagnostics_for_with(src, |r| r.strict_mixed_arrays = true);
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn a_correctly_typed_array_literal_is_not_flagged_with_or_without_strict_mixed_arrays() {
    // The other half of the same coverage: without a real element-type
    // shape, a correct literal would be indistinguishable from an
    // incorrect one once strict_mixed_arrays removed the Array(Mixed)
    // leniency. Since a literal carries a real shape, the genuinely
    // correct case must stay silent.
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
    // element type is genuinely Mixed for a reason other than being an
    // array literal (here, read back out of an untyped array
    // parameter).
    let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function test(array $untyped): void { needsInts($untyped); }
        "#;
    assert!(diagnostics_for(src).is_empty());
    let d = diagnostics_for_with(src, |r| r.strict_mixed_arrays = true);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expects array<(int|string), int>"),
        "got: {d:?}"
    );
}

// An array-literal shape whose individual fields are genuinely Mixed
// (e.g. from unresolved global constants, not the whole array) must
// get the identical `Array(Mixed)` leniency above, or each field is
// checked strictly instead, wrongly flagging a shape literal built
// from unmodeled constants against an ordinary `array<V>`-typed
// parameter.
#[test]
fn an_array_literal_with_a_mixed_typed_field_from_an_unknown_constant_is_not_flagged() {
    let src = r#"<?php
            /** @param array<int|string> $xs */
            function needsIntOrString($xs): void {}
            needsIntOrString([SOME_UNKNOWN_CONST_A, SOME_UNKNOWN_CONST_B]);
        "#;
    assert!(diagnostics_for(src).is_empty(), "unexpected diagnostics");
    // strict_mixed_arrays removes this leniency too, the same as the
    // whole-array case above.
    let d = diagnostics_for_with(src, |r| r.strict_mixed_arrays = true);
    assert_eq!(d.len(), 1, "got: {d:?}");
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
    // `[$a, $b] = [$b, $a]` must not silently retype both sides to
    // Mixed by failing to infer the literal's own element types -
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
    // `$x = []; $x['a'] = ...;` is one of the most common ways to
    // build an array in real PHP - check_shape_key_access must not
    // treat the literal's own
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

// A genuinely empty shape has zero possible keys, so a dynamic-key
// read is unconditionally real PHP's own "undefined array key"
// warning, returning `null` - `index_result_type` must not answer bare
// `Mixed` here regardless, discarding real, certain information.
// Deliberately not extended to a literal key too (`preg_match`'s own
// `$matches` shape relies on that case staying lenient - see
// `index_result_type`'s own doc comment for why).
#[test]
fn reading_a_dynamic_key_from_a_freshly_empty_array_infers_null() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int $x */
            function needsInt($x): void {}
            function f(string $k): void {
                $x = [];
                needsInt($x[$k]);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type"
                && x.message.contains("expects int, null given")),
        "got: {d:?}"
    );
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
    // A real, deliberate cost of this design, pinned down as a test
    // rather than left as prose: once `$x` is widened away from
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

// The array-append-widening path must build a real `list<T>`, not a
// plain `Array`, since real PHP's own append semantics always assign
// the next sequential int key - verified via dumpType(): a single
// append onto
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
    // A dynamic-key write ($items[$period] = [...]), followed by more
    // fields added per-element via a nested write ($items[$day]
    // ['derivative'] = ...): `bind_assignment_target`'s Index arm must
    // not only accept a bare Variable base, since the nested write's
    // base is itself $items[$day], an Index, not a Variable - otherwise
    // it's silently ignored, leaving $items[$day]'s inferred shape
    // frozen at its initial literal, missing 'derivative' entirely.
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

// The same shape, one level closer to the actual site: the nested
// write happens inside a `foreach` that iterates the very same array
// it mutates - `check_foreach`'s own doc comment discloses this exact
// trade-off (merging the loop's exit state with its own pre-loop
// snapshot would be wrong, since every key really iterated over gets
// the loop's own treatment, never the stale pre-loop one). Guards
// `Checker::converge_loop_body`'s own "drop only names introduced
// inside the loop, never re-widen one that already existed" rule from
// the other direction: a name already tracked before the loop must
// keep exactly its post-loop type, not get unioned back with what it
// was before.
#[test]
fn a_foreach_mutating_its_own_subject_array_keeps_the_post_loop_shape_precisely() {
    let d = diagnostics_for(
        r#"<?php
            function test(array $items, string $file): void {
                foreach ($items as $day => $data) {
                    $items[$day]['derivative'] = $file;
                }
                foreach ($items as $day => $data) {
                    echo $data['derivative'];
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `$items` starts as a literal `[]`, gets built up by a first foreach
// (over a separate array, so it may run zero times), and only then is
// mutated in place by a second foreach over itself. `LoopZeroIterations`
// correctly makes the first foreach's own exit state a real `Union`
// including the empty-shape alternative (the loop might never run) -
// which means the second foreach's own entry for `$items` is that same
// `Union`, not a bare `ArrayShape`/`Array` anymore. `write_index_into`'s
// `Type::Union` arm requires every member to accept the write or the
// whole write is silently dropped - and its own empty-shape member
// reads back as `Type::Null` at a dynamic key (`index_result_type`'s
// own "empty shape, dynamic key reads null" arm), so `write_index_into`
// needs its own arm for that case, or it bails the entire nested write
// and freezes `$items`'s tracked shape at its pre-second-loop state.
#[test]
fn a_foreach_mutating_its_own_subject_array_that_a_prior_maybe_empty_loop_built_keeps_the_new_fields()
 {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $rows */
            function test(array $rows, string $file): void {
                $items = [];
                foreach ($rows as $period) {
                    $items[$period] = ['nb_images' => 0];
                }
                foreach ($items as $day => $data) {
                    $items[$day]['derivative'] = $file;
                }
                foreach ($items as $day => $data) {
                    echo $data['derivative'];
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "shapeAccess.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

// Structurally distinct from the two tests just above: those mutate
// the subject directly keyed by its own loop key
// (`$items[$day][...] = ...`); this one mutates the subject only
// indirectly, through a by-ref value variable that aliases into it
// (`foreach ($infosOfImage as &$info) { $info['src_image'] = ...; }`).
// `by_ref` must not be dropped entirely by `check_foreach`, or a field
// the loop body writes into the by-ref alias never widens
// `$infosOfImage`'s own tracked element type, so a later dynamic-key
// read (`$infosOfImage[$id]['src_image']`) would report the key
// missing even though every element unconditionally gets it.
#[test]
fn a_foreach_writing_into_a_by_ref_value_variable_widens_the_subjects_own_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<int, array{id: int}> $infos
             */
            function test(array $infos): void {
                foreach ($infos as &$info) {
                    $info['src_image'] = 'x';
                }
                unset($info);

                $rep = $infos[1] ?? null;
                $x = $rep === null ? null : $rep['src_image'];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "shapeAccess.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

// The negative case pinning why `by_ref` specifically has to gate this: a
// plain (by-value) loop variable is its own independent copy - writing
// into it can never affect the subject array's own real elements, so the
// same key access past the loop must still be flagged.
#[test]
fn a_foreach_writing_into_a_plain_by_value_variable_does_not_widen_the_subject() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<int, array{id: int}> $infos
             */
            function test(array $infos): void {
                foreach ($infos as $info) {
                    $info['src_image'] = 'x';
                }

                $rep = $infos[1] ?? null;
                $x = $rep === null ? null : $rep['src_image'];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "shapeAccess.notFound"),
        "expected shapeAccess.notFound, got: {d:?}"
    );
}

// A variable becoming a real Union of two differently-shaped
// ArrayShapes across an if/else (one branch's own nested field differs
// from the other's) must have a later flat write into that Union
// update every member, or a key written one line earlier is still
// reported missing by a very next nested write's own "does this key
// exist" check.
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

// A `Union`-typed container (built across an `if`/`else`) whose very
// first write to a brand-new key is itself a nested append
// (`$info['updates'][] = ...`), never a flat `$info['updates'] = [];`
// first. `check_assignment_target`'s `Index` arm must not route the
// inner `$info['updates']` sub-expression through `check_expr`, which
// would wrongly check it as a genuine read via `check_shape_key_access`.
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

// Array-shape generalization, tried before the hard
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

// Direct coverage for a foreach over a list<T>-typed variable, which
// must not silently infer Mixed for its element type, and its siblings.
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

// `+` on two arrays must not collapse straight to `array<array-key,
// mixed>` unconditionally, discarding both operands' own key type -
// `$filters + $functions` on two `array<string, callable>`-typed
// values would make every `foreach ($merged as $name => ...)`'s `$name`
// infer as `int|string`, cascading into false positives on
// `strtolower($name)`/`strtoupper($name)`/any other string-only callee
// project-wide. Fixed to union both
// operands' real key/value types (mirroring real PHPStan's own
// `InitializerExprTypeResolver::getPlusType`'s general-array-case
// formula), collapsing back to the shared key type when both operands
// already agree, exactly as here.
#[test]
fn adding_two_string_keyed_arrays_keeps_the_shared_string_key_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, callable> $a
             *  @param array<string, callable> $b
             */
            function f(array $a, array $b): void {
                $merged = $a + $b;
                assertType('array<string, callable>', $merged);
                foreach ($merged as $name => $callable) {
                    assertType('string', $name);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// When the two operands' key types genuinely differ, the result is the
// real union of both - not silently widened all the way to the generic
// `array-key`.
#[test]
fn adding_arrays_with_different_key_types_unions_them() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<int, string> $a
             *  @param array<string, int> $b
             */
            function f(array $a, array $b): void {
                // Key union is `int|string` - exactly `unknown_array_key()`
                // - so the shorthand single-param `array<V>` display kicks
                // in (matching `Type::Array`'s own existing display rule),
                // same as any other array whose key type isn't known to be
                // narrower than the default.
                assertType('array<string|int>', $a + $b);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// `list<T>`-ness survives `+` when both operands are provably lists -
// mirrors real PHPStan's own `isList()->yes() && isList()->yes()`
// accessory-preservation rule.
#[test]
fn adding_two_lists_preserves_list_ness() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<int> $a
             *  @param list<int> $b
             */
            function f(array $a, array $b): void {
                assertType('list<int>', $a + $b);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Disclosed, accepted precision gap (not a soundness bug): two literal
// `array{...}` shapes merged via `+` fall back to the general
// `Array(key, val)` union case rather than a precise merged shape
// preserving each individual key - real PHPStan's own constant-array
// merge branch (`ConstantArrayTypeBuilder`-based, with real optional-key
// nuance around when a left-hand value gets unioned with the right
// side's) is a separate, structurally different feature, not ported
// here. Still a sound, real supertype - just not maximally precise.
#[test]
fn adding_two_literal_shapes_loses_precise_keys_a_known_disclosed_gap() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = ['a' => 1];
                $b = ['b' => 'x'];
                // The key generalizes from the two distinct literal keys
                // down to plain `string` (the real, disclosed precision
                // loss this test pins) - but the *value* union still keeps
                // each side's own literal type rather than widening it,
                // since nothing here forces a wider value type.
                assertType("array<string, 1|'x'>", $a + $b);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
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

// `infer_array_literal_type` must not give up entirely (falling to
// bare `Array(Mixed)`) on any array literal containing a spread
// element - a common shape is `[...findA(), ...findB()]`, combining
// several `list<T>`-returning helpers into one: spreading two
// `list<T>`s stays a `list<T>`; spreading a `list<T>` alongside a
// plain `array<T>` degrades to plain `array<T>` (not a list); a
// leading literal element makes the whole thing non-empty.
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

// Real PHP spread semantics always renumber a source's own *integer*
// keys sequentially into the result - only a possible *string* key
// actually breaks list-ness. A plain `array<int, T>` source (list-ness
// itself unproven - it's not tagged `list<T>`) still spreads list-safe,
// confirmed via `dumpType()` against real PHPStan: swapping the source's
// own declared type to `array<string, int>` (a genuinely possible string
// key) is what correctly still produces a mismatch, not a merely-unproven
// `array<int, int>`.
#[test]
fn spreading_a_plain_int_keyed_array_still_produces_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<int, int> $a
             *  @return list<int>
             */
            function f($a): array {
                return [...$a];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn spreading_a_string_keyed_array_still_breaks_list_ness() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, int> $a
             *  @return list<int>
             */
            function f($a): array {
                return [...$a];
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the list-ness mismatch to still be flagged for a genuinely string-keyed source: {d:?}"
    );
}

// Spreading another object's own precisely-known shape and adding one
// more field - `[...$this->userInfo->toArray(), 'theme_name' => $x]`.
// Spreading a shape with only non-canonical (string) keys merges those
// keys in verbatim, keeping the whole literal a precise shape rather
// than degrading to a generic array.
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

// Destructuring from an array{...} shape must extract each position/
// key's own precise type, not fall back to Mixed.

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

// Unsealed-shape tracking (`array{a: int, ...}`) and quoted-shape-key
// parsing.

#[test]
fn isset_on_an_undeclared_key_of_an_unsealed_shape_is_never_flagged_as_absent() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{a: int, ...} $x */
            function f(array $x): void {
                if (isset($x['b'])) {
                    echo 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "isset.offset"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn isset_on_an_undeclared_key_of_a_sealed_shape_is_still_flagged_as_absent() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{a: int} $x */
            function f(array $x): void {
                if (isset($x['b'])) {
                    echo 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "isset.offset"),
        "got: {d:?}"
    );
}

#[test]
fn accessing_an_undeclared_key_of_an_unsealed_shape_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{a: int, ...} $x */
            function f(array $x): void {
                $y = $x['b'];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "shapeAccess.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_quoted_shape_key_with_a_hyphen_is_parsed_and_looked_up_correctly() {
    // A quoted key's surrounding quote characters must be stripped, or
    // the field is stored as the literal text `'content-type'` (quotes
    // included) instead of the string PHP itself uses as the array key -
    // silently breaking every lookup against it.
    let d = diagnostics_for(
        r#"<?php
            /** @param array{'content-type': string} $x */
            function f(array $x): void {
                $y = $x['content-type'];
                $z = $x['nonexistent-key'];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "shapeAccess.notFound");
}

#[test]
fn a_shape_with_a_hyphenated_key_displays_it_bare() {
    // A hyphen is part of this project's own "bare shape key" grammar
    // (`looks_like_shape_key`'s own doc comment - e.g. gettext's
    // `plural-forms` key) - round-trips unquoted.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): void {}
            /** @param array{'content-type': string} $x */
            function f(array $x): void {
                needsInt($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("array{content-type: string}"),
        "got: {d:?}"
    );
}

#[test]
fn a_shape_with_a_key_needing_quotes_displays_it_quoted() {
    // A space isn't part of the bare-key grammar at all - must round-
    // trip quoted.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): void {}
            /** @param array{'foo bar': string} $x */
            function f(array $x): void {
                needsInt($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("array{'foo bar': string}"),
        "got: {d:?}"
    );
}

// A dynamic-key array write's own key type must be narrowed by a
// preceding `is_int()`/`is_string()`-family guard on the key expression
// itself, the same benefit an ordinary variable read already gets via
// `infer_expr`: `if (is_int($row['id'])) { $arr[$row['id']] = ...; }`
// must infer `$arr`'s key type as narrowed `int`, not `$row['id']`'s
// unnarrowed type.
#[test]
fn a_dynamic_key_write_narrowed_by_a_preceding_is_int_guard_infers_an_int_key() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $row */
            function f(array $row, string $val): void {
                $arr = [];
                if (is_int($row['id'])) {
                    $arr[$row['id']] = $val;
                    assertType('array<int, string>', $arr);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dynamic_key_write_narrowed_by_a_preceding_is_string_guard_infers_a_string_key() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $row */
            function f(array $row, string $val): void {
                $arr = [];
                if (is_string($row['id'])) {
                    $arr[$row['id']] = $val;
                    assertType('array<string, string>', $arr);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Negative control: without a narrowing guard, a dynamic-key write still
// falls back to the unknown-int-or-string key default, proving the fix
// above is additive (only narrows when the key expression's own type is
// already confidently known), not a blanket assumption. A key exactly
// equal to `unknown_array_key()` (`int|string`) displays in its own
// single-type-param shorthand (`array<V>`, not `array<int|string, V>`) -
// `types/display.rs`'s own established convention for the "any legal
// key" default, unrelated to this fix.
#[test]
fn an_unguarded_dynamic_key_write_still_infers_an_unknown_key_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $row */
            function f(array $row, string $val): void {
                $arr = [];
                $arr[$row['id']] = $val;
                assertType('array<string>', $arr);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Two successive narrowed dynamic-key writes (once with a confidently
// `int` key, once with a confidently `string` key) union into
// `array<int|string, V>` on the already-widened `Type::Array` path -
// confirms the "union in, don't just keep unchanged" generalization
// doesn't regress the already-tested nested-write behavior. Displays in
// the same single-type-param shorthand as the unguarded case above,
// since `int|string` is exactly `unknown_array_key()`.
//
// Each `if` has no `else`, so the "neither write happened yet"
// empty-shape branch is genuinely part of the merge - but `union_types`
// drops a redundant empty `ArrayShape` once a general `Array(key, val)`
// member is also present in the same union (`array{}` is a subtype of
// any `Array(key, val)` - vacuously, since an empty array satisfies
// "every key/value is `key`/`val`-typed" for any `key`/`val` at all),
// matching real PHPStan's own `dumpType()` output for the identical
// shape - see `union_types`'s own doc comment for the full mechanism.
#[test]
fn successive_narrowed_dynamic_key_writes_union_the_container_s_key_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $a
             *  @param array<string, mixed> $b
             */
            function f(array $a, array $b, string $val): void {
                $arr = [];
                if (is_int($a['id'])) {
                    $arr[$a['id']] = $val;
                }
                if (is_string($b['id'])) {
                    $arr[$b['id']] = $val;
                }
                assertType('array<string>', $arr);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
