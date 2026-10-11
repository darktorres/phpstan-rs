<?php
declare(strict_types=1);
function f(): void {
    $fn = 'strlen';
    assertType('3', $fn('abc'));
    try {
        $fn(12345);
    } catch (TypeError) {
    }
}
