<?php
function run(bool $flag): void {
    $x = null;
    $f = function () use (&$x): void {
        strlen($x);
    };
    $f = $flag ? function () use (&$x): void {
        strlen($x);
    } : $f;
    $x = 'str';
    $f();
}
