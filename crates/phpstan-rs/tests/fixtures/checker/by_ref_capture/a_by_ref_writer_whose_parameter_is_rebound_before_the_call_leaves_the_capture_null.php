<?php
function rebind(callable $c): void { $c = function (): void {}; $c(); }
$x = null;
$w = function () use (&$x): void { $x = 'str'; };
$r = function () use (&$x): void { strlen($x); };
rebind($w);
$r();
