<?php
$x = null;
$writer = function () use (&$x): void {
    $x = 'str';
};
$f = function () use (&$x): void {
    strlen($x);
};
$writer();
$f();
