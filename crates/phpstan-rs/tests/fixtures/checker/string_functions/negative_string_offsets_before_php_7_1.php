<?php
use function PHPStan\Testing\assertType;

function read(): void {
    $abc = 'abc';
    assertType('string', $abc[-1]);
}
