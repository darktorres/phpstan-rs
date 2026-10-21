<?php
function run(callable $c): void {
    if ($u ?? false) {
    }
    $c();
}
$x = 'a';
$w = function () use (&$x): void {
    strlen($x);
};
$x = null;
run($w);
