<?php
use function PHPStan\Testing\assertType;
function f(bool $c): void {
    if ($c) { $v = 1; }
    assertType('1|2', $v ?? 2);
    if (isset($v)) { assertType('1', $v); }
}
