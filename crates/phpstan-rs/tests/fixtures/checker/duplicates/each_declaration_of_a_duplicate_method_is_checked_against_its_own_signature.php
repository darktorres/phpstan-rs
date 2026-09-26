<?php
class A
{
    public function f(int $a): int
    {
        return $a;
    }

    public function f(string $b, string $c): string
    {
        return $b . $c;
    }
}
