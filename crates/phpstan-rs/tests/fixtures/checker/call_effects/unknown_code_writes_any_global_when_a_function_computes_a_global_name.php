<?php
namespace CallEffects\GlobalsComputed;

use function PHPStan\Testing\assertType;

function wild(string $name): void
{
    $name();
}

function writesAComputedGlobal(string $key): void
{
    $GLOBALS[$key] = $_GET['value'] ?? null;
}

$any = 1;
$any = 5;
wild($_GET['f']);
assertType('mixed', $any);
