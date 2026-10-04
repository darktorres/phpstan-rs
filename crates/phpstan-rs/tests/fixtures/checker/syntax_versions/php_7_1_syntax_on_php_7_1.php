<?php
class A
{
    private const X = 1;

    /** @param iterable<int> $i */
    public function f(?int $x, iterable $i): void
    {
        [$a, $b] = [1, 2];
        ["k" => $c] = ["k" => 3];
        try {
        } catch (InvalidArgumentException | RuntimeException $e) {
        }
        echo self::X, $x, $a, $b, $c, count([$i]);
    }
}
