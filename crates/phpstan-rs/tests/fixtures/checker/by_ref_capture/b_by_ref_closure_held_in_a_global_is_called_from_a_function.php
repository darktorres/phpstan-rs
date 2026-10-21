<?php
function callGlobalF(): void {
    global $f;
    $f();
}
$x = 'a';
$f = function () use (&$x): void {
    strlen($x);
};
$f();
$x = null;
callGlobalF();
