<?php
function gd(string $n): void {
    global $$n;
    $$n = 'str';
}
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
gd('x');
$f();
