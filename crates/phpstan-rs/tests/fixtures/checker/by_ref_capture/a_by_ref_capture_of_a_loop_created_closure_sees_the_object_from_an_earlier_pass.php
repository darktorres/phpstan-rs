<?php
$x = null;
for ($i = 0; $i < 2; $i++) {
    if ($i === 1) {
        $f();
    }
    $x = new ArrayObject();
    $f = function () use (&$x): void {
        strlen($x);
    };
}
