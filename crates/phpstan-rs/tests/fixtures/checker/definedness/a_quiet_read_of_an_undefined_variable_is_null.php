<?php
use function PHPStan\Testing\assertType;
function f(): void {
    $y = $undef ?? 'a';
    assertType("'a'", $y);
}
