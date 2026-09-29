<?php
define('MY_HEX_TAG', 1);
define('MY_HEX_APOS', 4);
function f(int $flags): int { return $flags; }
function g(bool $both): int {
    $flags = $both ? MY_HEX_TAG | MY_HEX_APOS : MY_HEX_APOS;
    $other = $both ? MY_HEX_TAG | MY_HEX_APOS : MY_HEX_TAG;
    return f($flags) + f($other);
}
