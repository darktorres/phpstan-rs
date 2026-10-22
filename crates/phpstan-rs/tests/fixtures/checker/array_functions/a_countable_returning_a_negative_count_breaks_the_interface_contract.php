<?php
use function PHPStan\Testing\assertType;

final class Debt implements Countable
{
    public function count(): int
    {
        return -5;
    }
}

function f(Debt $debt): void {
    assertType('int<0, max>', count($debt));
}
