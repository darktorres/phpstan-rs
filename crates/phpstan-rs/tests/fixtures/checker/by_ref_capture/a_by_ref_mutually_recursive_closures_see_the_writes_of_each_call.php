<?php
$x = null;
$a = null;
$b = null;
$a = function (int $n) use (&$b, &$x): void {
    if ($n > 0) {
        $b($n - 1);
    } else {
        strlen($x);
    }
};
$b = function (int $n) use (&$a, &$x): void {
    $x = 'str';
    $a($n);
};
$a(2);
