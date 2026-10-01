<?php

declare(strict_types=1);

if (!function_exists('mb_str_pad')) {
    function mb_str_pad(string $s, int $l): string
    {
        return $s . $l;
    }
}
