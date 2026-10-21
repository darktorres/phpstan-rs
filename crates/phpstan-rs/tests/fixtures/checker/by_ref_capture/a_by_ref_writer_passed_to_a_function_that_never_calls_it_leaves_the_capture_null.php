<?php
function runIf(callable $c, bool $b): void { if ($b) { $c(); } }
$x = null;
$w = function () use (&$x): void { $x = 'str'; };
$r = function () use (&$x): void { strlen($x); };
runIf($w, false);
$r();
