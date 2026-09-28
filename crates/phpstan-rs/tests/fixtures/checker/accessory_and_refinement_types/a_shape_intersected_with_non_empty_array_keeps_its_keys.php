<?php
use function PHPStan\Testing\assertType;
/** @param array{foo?: string, bar?: string}&non-empty-array $x */
function f(array $x): void {
    assertType('non-empty-array{foo?: string, bar?: string}', $x);
}
