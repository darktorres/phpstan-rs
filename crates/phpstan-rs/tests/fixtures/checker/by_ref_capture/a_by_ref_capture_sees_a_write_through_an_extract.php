<?php
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
extract(['x' => 'str']);
$f();
