<?php
function heldNull(): void {
    $x = 'a';
    $f = function () use (&$x): void {
        strlen($x);
    };
    $fs = [$f];
    $x = null;
    $fs[0]();
}
