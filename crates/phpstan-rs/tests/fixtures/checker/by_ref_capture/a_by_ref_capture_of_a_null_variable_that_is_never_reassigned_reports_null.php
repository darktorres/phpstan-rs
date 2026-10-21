<?php
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
$f();
