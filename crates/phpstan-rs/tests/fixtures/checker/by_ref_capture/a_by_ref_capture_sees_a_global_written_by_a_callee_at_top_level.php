<?php
function reset_it(): void {
    global $x;
    $x = 'str';
}
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
reset_it();
$f();
