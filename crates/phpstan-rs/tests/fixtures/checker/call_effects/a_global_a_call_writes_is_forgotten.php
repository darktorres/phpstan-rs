<?php
namespace CallEffects\GlobalVariable;

use function PHPStan\Testing\assertType;

function writeCounter(): void
{
    global $counter;
    $counter = 7;
}

function check(): void
{
    global $counter;
    $counter = null;
    assertType('null', $counter);
    writeCounter();
    assertType('7|null', $counter);
}

check();
