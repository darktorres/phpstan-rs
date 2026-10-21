<?php
function run(callable $c): void {
    if (!$c) {
    }
    $c();
}
$x = 'a';
$w = function () use (&$x): void {
    strlen($x);
};
$x = null;
run($w);
