<?php
function runThrow(callable $c): void { throw new \Exception('e'); $c(); }
$x = null;
$w = function () use (&$x): void { $x = 'str'; };
$r = function () use (&$x): void { strlen($x); };
try { runThrow($w); } catch (\Exception $e) {}
$r();
