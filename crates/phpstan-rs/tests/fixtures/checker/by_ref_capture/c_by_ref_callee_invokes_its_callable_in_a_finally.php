<?php
function fin(callable $c): void {
    try {
        return;
    } finally {
        $c();
    }
}
$x = null;
$w = function () use (&$x): void {
    $x = 'str';
};
$r = function () use (&$x): void {
    strlen($x);
};
fin($w);
$r();
