<?php
function callGlobalF(): void {
    global $f;
    $f();
}
function h(string $n): void {
    $n();
}
$x = 'a';
$f = function () use (&$x): void {
    strlen($x);
};
$x = null;
callGlobalF();
$name = substr('xcallGlobalF', 1);
h($name);
