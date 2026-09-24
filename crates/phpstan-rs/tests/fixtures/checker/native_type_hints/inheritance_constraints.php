<?php
class Base {
    /**
     * @param int $x
     * @return int
     */
    public function untypedParam($x) { return 1; }
    public function typedParam(array $x): array { return []; }
    /** @return string */
    public function overriddenUntyped() { return ''; }
    /** @return string */
    public function overriddenTyped() { return ''; }
    /** @return static */
    public function fluent() { return $this; }
    /** @return string */
    public function __toString() { return ''; }
}
class Child extends Base {
    /** @param int $x */
    public function untypedParam($x): int { return 1; }
    /** @param list<int> $x */
    public function typedParam($x): array { return []; }
    public function overriddenUntyped() { return ''; }
    public function overriddenTyped(): string { return ''; }
}
