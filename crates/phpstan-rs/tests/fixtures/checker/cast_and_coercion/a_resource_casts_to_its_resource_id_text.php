<?php
/** @param resource $h */
function f($h): string
{
    $s = (string) $h;
    \PHPStan\Testing\assertType('string', $s);
    echo $h;
    return "handle {$h}";
}
