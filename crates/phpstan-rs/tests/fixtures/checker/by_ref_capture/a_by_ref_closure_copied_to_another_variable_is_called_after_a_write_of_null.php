<?php
$x = 'a';
$f = function () use (&$x): void { strlen($x); };
$g = $f;
$x = null;
$g();
