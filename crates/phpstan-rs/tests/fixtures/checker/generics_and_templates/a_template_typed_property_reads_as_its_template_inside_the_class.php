<?php
use function PHPStan\Testing\assertType;
interface P {}
final class Q implements P {}
/** @template T of P|null = null */
class Builder {
    /** @var T */
    protected ?P $parent = null;
    public function f(): void {
        assertType('T|null', $this->parent);
        if ($this->parent instanceof Q) {
            echo 1;
        }
    }
}
