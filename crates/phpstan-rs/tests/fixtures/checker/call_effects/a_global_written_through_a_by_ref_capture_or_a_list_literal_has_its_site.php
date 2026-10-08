<?php
namespace CallEffects\GlobalCaptures;

use function PHPStan\Testing\assertType;

$counted = 0;
$listed = [0];
$keyed = ['a' => 1, 5];
$spread = [...[1, 2]];

$bump = function () use (&$counted) {
    $counted = 'many';
};
$arrow = fn() => $counted = null;
$kept = 1;
$byValue = function () use ($kept) {
    $kept = 'local';
};

function reads(): void
{
    global $kept, $counted, $listed, $keyed, $spread;
    assertType("0|'many'", $counted);
    assertType('1', $kept);
    assertType('array{0?: 0}', $listed);
    assertType('array{a?: 1, 0?: 5}', $keyed);
}
