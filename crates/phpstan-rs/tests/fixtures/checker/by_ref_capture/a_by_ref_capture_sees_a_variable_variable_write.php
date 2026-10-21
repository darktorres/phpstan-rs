<?php
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
$n = 'x';
$$n = 'str';
$f();
