<?php
$x = null;
$fiber = new Fiber(function (): void {
    global $x;
    Fiber::suspend();
    strlen($x);
});
$fiber->start();
$x = 'str';
$fiber->resume();
