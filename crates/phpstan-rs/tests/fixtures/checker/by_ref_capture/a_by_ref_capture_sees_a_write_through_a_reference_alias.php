<?php
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
$g = &$x;
$g = 5;
$f();
