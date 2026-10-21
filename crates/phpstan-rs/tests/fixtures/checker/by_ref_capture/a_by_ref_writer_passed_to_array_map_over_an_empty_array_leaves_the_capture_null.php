<?php
/** @param array<mixed> $items */
function viaMap(array $items, callable $c): void { array_map($c, $items); }
$x = null;
$w = function ($i) use (&$x): void { $x = 'str'; };
$r = function () use (&$x): void { strlen($x); };
viaMap([], $w);
$r();
