<?php
/** @param resource $h */
function f($h): string
{
    $s = (string) $h;
    \PHPStan\Testing\assertType('non-empty-string', $s);
    echo $h;
    return "handle {$h}";
}
