<?php
class Cases {
    /** @return ($x is null ? mixed : array) */
    public function conditional_else_branch($x) { return []; }
    /** @param array|mixed[] $row */
    public function bare_array_joined_with_a_written_one(array $row): void {}
    /** @param mixed[]|array $row */
    public function written_one_joined_with_a_bare_array(array $row): void {}
    /** @return array<string, mixed> */
    public function written_value_type(): array { return []; }
    /** @return ($keys is array ? array<string, mixed> : string) */
    public function condition_is_not_a_declared_type(?array $keys = null): array|string { return ''; }
    /** @param (\Closure(static): mixed)|string|array $column */
    public function bare_array_beside_a_closure_returning_mixed($column): void {}
    /** @var (\Closure(string, array): mixed)[] */
    protected array $callbacks = [];
    /** @return iterable|mixed[] */
    public function native_iterable_narrowed_to_written(): iterable { return []; }
    /** @return \Traversable<list<mixed>> */
    public function native_iterable_still_admits_an_array_of_unknown_values(): iterable { yield []; }
}
class Counter implements Countable {
    public function count() { return 0; }
}
interface Source {
    public function key(): mixed;
}
class Reader implements Source {
    public function key() { return 1; }
}
