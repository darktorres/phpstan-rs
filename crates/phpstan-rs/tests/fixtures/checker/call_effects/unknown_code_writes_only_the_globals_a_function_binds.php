<?php
namespace CallEffects\GlobalsFunctionsBind;

use function PHPStan\Testing\assertType;

function wild(string $name): void
{
    $name();
}

function writesTheBoundGlobal(): void
{
    global $bound;
    $bound = $_GET['bound'] ?? null;
}

$never = 1;
$bound = 1;
$never = 5;
$bound = 5;
wild('anything');
assertType('5', $never);
assertType('mixed', $bound);
