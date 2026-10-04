<?php
namespace CallEffects\StaticLocal;

use function PHPStan\Testing\assertType;

function depth(int $level): ?int
{
    static $deepest = null;
    if ($level === 0) {
        $deepest = 3;
        return $deepest;
    }
    $deepest = null;
    depth($level - 1);
    assertType('mixed', $deepest);
    return 1;
}

depth(2);
