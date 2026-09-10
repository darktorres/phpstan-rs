//! Phase 70.C: `foreach` over a `Traversable`/`Iterator`/`Generator`/
//! `IteratorAggregate`-typed object infers real key/value types instead
//! of always falling back to `Mixed`.

use super::helpers::*;

#[test]
fn foreach_over_a_generic_iterator_infers_real_key_and_value_types() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @implements \Iterator<int, string> */
            class StringIterator implements \Iterator {
                private array $items = ['a', 'b'];
                private int $pos = 0;
                public function current(): mixed { return $this->items[$this->pos]; }
                public function next(): void { $this->pos++; }
                public function key(): mixed { return $this->pos; }
                public function valid(): bool { return isset($this->items[$this->pos]); }
                public function rewind(): void { $this->pos = 0; }
            }
            function f(StringIterator $it): void {
                foreach ($it as $k => $v) {
                    assertType('int', $k);
                    assertType('string', $v);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn foreach_over_a_generator_typed_parameter_infers_real_key_and_value_types() {
    // Generator is itself a real `Iterator` descendant (`Generator<TKey,
    // TValue, TSend, TReturn> extends Iterator<TKey, TValue>` in the
    // real phpstorm-stubs) - the same `Iterator`-targeted template walk
    // this phase adds naturally covers it too, with no Generator-specific
    // special case needed.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param \Generator<int, string, mixed, void> $gen */
            function f($gen): void {
                foreach ($gen as $k => $v) {
                    assertType('int', $k);
                    assertType('string', $v);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn foreach_over_an_iterator_aggregate_resolves_through_its_get_iterator_return_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @implements \Iterator<int, string> */
            class StringIterator implements \Iterator {
                private array $items = ['a', 'b'];
                private int $pos = 0;
                public function current(): mixed { return $this->items[$this->pos]; }
                public function next(): void { $this->pos++; }
                public function key(): mixed { return $this->pos; }
                public function valid(): bool { return isset($this->items[$this->pos]); }
                public function rewind(): void { $this->pos = 0; }
            }
            class StringCollection implements \IteratorAggregate {
                public function getIterator(): StringIterator {
                    return new StringIterator();
                }
            }
            function f(StringCollection $c): void {
                foreach ($c as $k => $v) {
                    assertType('int', $k);
                    assertType('string', $v);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn foreach_over_an_iterator_aggregate_falls_back_to_its_own_generics_when_get_iterator_is_bare() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @implements \IteratorAggregate<int, string> */
            class StringCollection implements \IteratorAggregate {
                public function getIterator(): \Iterator {
                    throw new \RuntimeException('not implemented in this test');
                }
            }
            function f(StringCollection $c): void {
                foreach ($c as $k => $v) {
                    assertType('int', $k);
                    assertType('string', $v);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn foreach_over_a_bare_non_generic_iterator_infers_mixed_key_and_value() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class UntypedIterator implements \Iterator {
                public function current(): mixed { return null; }
                public function next(): void {}
                public function key(): mixed { return null; }
                public function valid(): bool { return false; }
                public function rewind(): void {}
            }
            function f(UntypedIterator $it): void {
                foreach ($it as $k => $v) {
                    assertType('mixed', $k);
                    assertType('mixed', $v);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn foreach_over_an_array_subject_is_unaffected_by_this_phase() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $items */
            function f(array $items): void {
                foreach ($items as $k => $v) {
                    assertType('string', $k);
                    assertType('int', $v);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn foreach_key_variable_from_a_generic_iterator_is_used_correctly_afterward() {
    // A real, if small, regression guard: before this phase the key
    // variable was unconditionally `Mixed`, so calling a string-only
    // function on it would never have been flagged either way - this
    // confirms the *value* variable side (which never was `Mixed`-only)
    // still behaves, and that iterating doesn't itself introduce a false
    // positive on ordinary use of the bound key.
    let d = diagnostics_for(
        r#"<?php
            /** @implements \Iterator<int, string> */
            class StringIterator implements \Iterator {
                private array $items = ['a', 'b'];
                private int $pos = 0;
                public function current(): mixed { return $this->items[$this->pos]; }
                public function next(): void { $this->pos++; }
                public function key(): mixed { return $this->pos; }
                public function valid(): bool { return isset($this->items[$this->pos]); }
                public function rewind(): void { $this->pos = 0; }
            }
            function f(StringIterator $it): void {
                foreach ($it as $k => $v) {
                    $sum = $k + 1;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
