<?php
$x = 'a';
$f = function () use (&$x): void { strlen($x); };
$f();
$x = null;
$n = 'f';
$$n();
