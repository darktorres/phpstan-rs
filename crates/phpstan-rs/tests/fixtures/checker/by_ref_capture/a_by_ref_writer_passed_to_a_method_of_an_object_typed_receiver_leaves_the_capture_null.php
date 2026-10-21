<?php
class Quiet {
    public function run(callable $c): void {}
}
function go(object $o, callable $c): void {
    $o->run($c);
}
$x = null;
$w = function () use (&$x): void { $x = 'str'; };
$r = function () use (&$x): void { strlen($x); };
go(new Quiet(), $w);
$r();
