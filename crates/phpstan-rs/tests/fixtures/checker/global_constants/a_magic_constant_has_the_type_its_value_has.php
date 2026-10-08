<?php

use function PHPStan\Testing\assertType;

function probe(): void
{
    assertType('non-empty-string', __DIR__);
    assertType('non-empty-string', __FILE__);
    assertType('int<1, max>', __LINE__);
    assertType('string', __FUNCTION__);
}
