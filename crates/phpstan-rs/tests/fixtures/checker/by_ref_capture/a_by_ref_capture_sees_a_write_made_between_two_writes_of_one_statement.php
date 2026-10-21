<?php
function show(mixed ...$args): void {
}
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
show($x = 1, $f(), $x = null);
