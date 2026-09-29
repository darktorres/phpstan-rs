<?php

declare(strict_types=1);

if (!function_exists('mb_convert_encoding')) {
    function mb_convert_encoding(array|string|null $string, ?string $to_encoding, array|string|null $from_encoding = null): array|string|false
    {
        return false;
    }
}

function f(string $text): string|false
{
    \PHPStan\Testing\assertType('string|false', mb_convert_encoding($text, 'UTF-8', 'ISO-8859-1'));

    return mb_convert_encoding($text, 'UTF-8', 'ISO-8859-1');
}
