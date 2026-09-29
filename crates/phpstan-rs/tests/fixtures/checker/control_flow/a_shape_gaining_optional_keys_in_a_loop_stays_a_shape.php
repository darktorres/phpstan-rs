<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

/** @param list<string> $tokens */
function parse(array $tokens): void
{
    $page = [];
    $i = 0;
    while (isset($tokens[$i])) {
        if ($tokens[$i] === 'flat') {
            $page['flat'] = true;
        } elseif (str_starts_with($tokens[$i], 'created-')) {
            $page['field'] = $tokens[$i];
        } else {
            $page['start'] = strlen($tokens[$i]);
        }
        $i++;
    }
    assertType('array{flat?: true, field?: non-falsy-string, start?: int<0, max>}', $page);
}

/** @param list<string> $tokens */
function collect(array $tokens): void
{
    $items = [];
    foreach ($tokens as $token) {
        $items[] = $token;
    }
    assertType('list<string>', $items);
}
