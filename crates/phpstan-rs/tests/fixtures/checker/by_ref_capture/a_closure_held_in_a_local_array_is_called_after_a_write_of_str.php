<?php
function heldStr(): void {
    $x = null;
    $f = function () use (&$x): void {
        strlen($x);
    };
    $fs = [$f];
    $x = 'str';
    $fs[0]();
}
