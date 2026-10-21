<?php
$x = 0;
$walk = function (int $n) use (&$walk, &$x): void {
    $x = $x + 1;
    if ($n > 0) {
        $walk($n - 1);
    }
};
$walk(rand(0, 5));
