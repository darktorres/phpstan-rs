<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

/** @param array<string, mixed> $row */
function takesRow(array $row): void {}

/** @param array<string, mixed> $comment */
function f(array $comment, string $author): void
{
    $merged = array_merge($comment, ['author' => $author]);
    assertType('array{author: string, ...<string, mixed>}', $merged);
    takesRow($merged);
}
