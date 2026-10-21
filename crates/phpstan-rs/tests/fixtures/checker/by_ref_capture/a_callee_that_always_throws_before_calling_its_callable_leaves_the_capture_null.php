<?php
function spin(callable $c, int $n): void {
    if ($n === 0) {
        throw new \Exception('done');
    }
    spin($c, $n - 1);
}
$x = null;
$w = function () use (&$x): void { $x = 'str'; };
$r = function () use (&$x): void { strlen($x); };
try { spin($w, 3); } catch (\Exception $e) {}
$r();
