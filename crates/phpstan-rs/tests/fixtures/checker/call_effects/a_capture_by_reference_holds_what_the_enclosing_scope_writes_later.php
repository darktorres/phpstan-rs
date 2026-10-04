<?php
namespace CallEffects\CaptureLater;

use function PHPStan\Testing\assertType;

function check(): void
{
    $state = null;
    $read = function () use (&$state): void {
        assertType('null|5', $state);
    };
    $state = 5;
    $read();
}

check();
