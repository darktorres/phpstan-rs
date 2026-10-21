<?php
$x = null;
$a = function (int $n) use (&$x): void {
    if ($n > 0) {
        echo "pos";
    } else {
        strlen($x);
    }
};
$a(2);
