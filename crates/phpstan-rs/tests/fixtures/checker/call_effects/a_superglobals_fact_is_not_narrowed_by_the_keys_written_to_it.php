<?php
namespace CallEffects\SuperglobalFact;

use function PHPStan\Testing\assertType;

function aKeyWrittenInOneFileNarrowsNoRead(callable $callback): void
{
    $callback();
    assertType('array<(int|string), mixed>', $_SERVER);
}

$_SERVER['HTTP_X_TEST'] = 'test';
