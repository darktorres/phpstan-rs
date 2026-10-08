<?php

use function PHPStan\Testing\assertType;

class Box
{
    public ?int $v = null;
    public ?int $w = null;
}

function stamp(Box $box): void
{
    $box->w = 2;
}

function kept(Box $box): void
{
    if ($box->v === null) {
        return;
    }
    require __DIR__ . '/stamp.php';
    assertType('int', $box->v);
}

function written(Box $box): void
{
    if ($box->w === null) {
        return;
    }
    require __DIR__ . '/stamp.php';
    assertType('int|null', $box->w);
}
