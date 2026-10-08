<?php

use function PHPStan\Testing\assertType;

class Box
{
    public ?int $v = null;
}

function load(Box $box): void
{
    if ($box->v === null) {
        return;
    }
    require __DIR__ . '/quiet.php';
    assertType('int', $box->v);
}
