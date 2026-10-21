<?php
function s(): void { global $x; $x = 'str'; $x = null; }
$x = 'a';
$f = function () use (&$x): void { strlen($x); };
s();
$f();
