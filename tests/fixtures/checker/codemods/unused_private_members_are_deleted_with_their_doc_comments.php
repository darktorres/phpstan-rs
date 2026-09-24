<?php
final class A {
    /** Never used. */
    private const UNUSED = 1;
    private const A = 1, B = 2;

    #[\Deprecated]
    private ?int $gone;

    /**
     * Helper.
     */
    private function helper(): void {}

    public function used(): int { return self::A; }
}
