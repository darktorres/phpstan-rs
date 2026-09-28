<?php

use function PHPStan\Testing\assertType;

function run(string $dir): void
{
    assertType('string|false|null', `ls $dir`);
}

/** @phpstan-pure */
function listing(): string|false|null
{
    return `ls`;
}
